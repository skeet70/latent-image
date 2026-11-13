{-# LANGUAGE GHC2021 #-}

-- | Image layout, definite retained references and direct-call stack composition.
-- Owned memory safety rests on the compiler, bounded types, source boundaries and subsystem comparisons.
module Replacement.PropertyService.Admission
  ( FunctionName (..), FunctionExtent (..), CodeLayout (..), MappedSpan (..)
  , AddressSpan (..), StackOffset (..), CallKind (..), CallEdge (..)
  , EntryFrame (..), CodeProof (..), Violation (..), AdmissionError (..)
  , admitCode, admitCodeWithCallbacks
  ) where

import qualified A32 as A
import qualified ArmElf as ELF
import Control.Monad (foldM, forM_, unless, when)
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Replacement.PropertyService.Placement (GuestAddress (..))

newtype FunctionName = FunctionName String deriving (Eq, Ord, Show)
data FunctionExtent = FunctionExtent
  { functionName :: FunctionName, functionStart :: GuestAddress, functionEnd :: GuestAddress }
  deriving (Eq, Show)
data AddressSpan = AddressSpan GuestAddress GuestAddress deriving (Eq, Show)
data MappedSpan = ArmSpan AddressSpan | DataSpan AddressSpan deriving (Eq, Show)
newtype StackOffset = StackOffset Int deriving (Eq, Ord, Show)
data CodeLayout = CodeLayout
  { layoutBase :: GuestAddress
  , layoutFunctions :: [FunctionExtent]
  , layoutMappings :: [MappedSpan]
  , layoutData :: [(GuestAddress, GuestAddress)]
  , layoutBoundaryWindows :: [AddressSpan]
  , layoutStockLeaves :: [(GuestAddress, StackOffset)]
  , layoutReferences :: [ELF.AddressRelocation]
  }
data CallKind = Call | TailCall | StockLeafCall | StockLeafTailCall | CallbackCall | CallbackTailCall deriving (Eq, Ord, Show)
data CallEdge = CallEdge
  { edgeSite :: GuestAddress, edgeTarget :: GuestAddress, edgeStack :: StackOffset, edgeKind :: CallKind }
  deriving (Eq, Ord, Show)
data EntryFrame = EntryFrame
  { frameEntry :: GuestAddress, frameLow :: StackOffset, frameCalls :: [CallEdge] }
  deriving (Eq, Show)
data CodeProof = CodeProof
  { proofFrames :: [EntryFrame]
  , proofComposed :: M.Map GuestAddress StackOffset
  , proofInstructions :: S.Set GuestAddress
  , proofLiterals :: S.Set GuestAddress
  , proofReferences :: [ELF.AddressRelocation]
  }
  deriving (Eq, Show)
data Violation = CallOutsideOwned GuestAddress | BranchOutsideOwned GuestAddress
  | StackUnbounded | Recursion | LayoutInvalid | BoundaryInvalid
  | ReferenceOutsideOwned GuestAddress
  deriving (Eq, Show)
data AdmissionError = AdmissionError GuestAddress Violation deriving (Eq, Show)

contains :: Word32 -> AddressSpan -> Bool
contains address (AddressSpan (GuestAddress low) (GuestAddress high)) = low <= address && address < high

admitCode :: CodeLayout -> B.ByteString -> Either AdmissionError CodeProof
admitCode = admitCodeWithCallbacks []

-- | Callback bodies and stack depths remain outside this proof.
admitCodeWithCallbacks :: [FunctionExtent] -> CodeLayout -> B.ByteString -> Either AdmissionError CodeProof
admitCodeWithCallbacks callbacks layout bytes = do
  let validSpan (AddressSpan (GuestAddress low) (GuestAddress high)) =
        low .&. 3 == 0 && high .&. 3 == 0 && base <= low && low < high && toInteger high <= end
      bounds mapping = case mapping of ArmSpan span' -> span'; DataSpan span' -> span'
      ownedWords = take (B.length bytes `div` 4) [base, base + 4 ..]
      dataAt address = any (\mapping -> case mapping of DataSpan span' -> contains address span'; _ -> False) (layoutMappings layout)
  require base LayoutInvalid (not (B.null bytes) && B.length bytes `mod` 4 == 0 && end <= toInteger (maxBound :: Word32)
    && all (validSpan . bounds) (layoutMappings layout)
    && all (\function -> validSpan (AddressSpan (functionStart function) (functionEnd function))) (layoutFunctions layout)
    && all (\address -> length [() | mapping <- layoutMappings layout, contains address (bounds mapping)] == 1) ownedWords
    && all (\(GuestAddress low, GuestAddress high) -> low <= high && low .&. 3 == 0 && high .&. 3 == 0
      && all dataAt (take (fromIntegral ((high - low) `div` 4)) [low, low + 4 ..])) (layoutData layout))
  require base BoundaryInvalid (all (\(AddressSpan (GuestAddress low) (GuestAddress high)) -> low < high) (layoutBoundaryWindows layout))
  frames <- mapM scanFunction (layoutFunctions layout)
  forM_ (filter isCode ownedWords) $ \site -> case decoded site of
    A.Branch _ link target -> require site
      ((if link then CallOutsideOwned else BranchOutsideOwned) (GuestAddress target))
      (owned target || declaredEntry target)
    A.Rejected A.ThumbBranch -> Left (AdmissionError (GuestAddress site)
      (CallOutsideOwned (GuestAddress (maybe site id (A.directTarget site (word site))))))
    _ -> pure ()
  forM_ (layoutReferences layout) $ \(ELF.AddressRelocation site target kind) -> do
    require site LayoutInvalid (site .&. 3 == 0 && owned site && toInteger site + 4 <= end)
    require site (ReferenceOutsideOwned (GuestAddress target)) (owned target || declaredEntry target
      || (kind == ELF.DataReference && any (contains target) (layoutBoundaryWindows layout)))
  let instructions = S.fromList [GuestAddress address | address <- ownedWords, not (dataAt address), inFunction address]
      literals = S.fromList [GuestAddress address | address <- ownedWords, dataAt address, inFunction address]
      frameMap = M.fromList [(frameEntry frame, frame) | frame <- frames]
  composed <- foldM (compose frameMap S.empty) M.empty (map frameEntry frames)
  pure (CodeProof frames composed instructions literals (layoutReferences layout))
  where
    GuestAddress base = layoutBase layout
    end = toInteger base + toInteger (B.length bytes)
    owned address = base <= address && toInteger address < end
    stockFrames = M.fromList (layoutStockLeaves layout)
    declaredEntry address = M.member (GuestAddress address) stockFrames || GuestAddress address `elem` map functionStart callbacks
    word address = U.word32At bytes (fromIntegral (address - base))
    decoded address = A.decode address (word address)
    require address violation condition = unless condition (Left (AdmissionError (GuestAddress address) violation))
    inFunction address = any (\(FunctionExtent _ (GuestAddress low) (GuestAddress high)) -> low <= address && address < high) (layoutFunctions layout)
    isCode address = any (\mapping -> case mapping of ArmSpan span' -> contains address span'; _ -> False) (layoutMappings layout)
    scanFunction function@(FunctionExtent _ (GuestAddress start) (GuestAddress finish)) = do
      let sites = [address | address <- [start, start + 4 .. finish - 4], isCode address]
          -- Summing allocation instructions is a conservative frame bound, including split prologues.
          low = negate (sum (map allocation sites))
      require start StackUnbounded (low >= -4096)
      edges <- concat <$> mapM (scanInstruction function low) sites
      pure (EntryFrame (GuestAddress start) (StackOffset low) edges)
    allocation site = case decoded site of
      A.StoreMultiple _ register A.DecrementBefore True registers | register == A.sp -> 4 * length registers
      A.Store _ _ _ register (A.OffsetImmediate amount) indexing
        | register == A.sp && indexing /= A.OffsetOnly && amount < 0 -> negate (fromIntegral amount)
      A.DataProcessing _ A.SUB _ destination source (A.Immediate amount)
        | destination == A.sp && source == A.sp -> fromIntegral amount
      _ -> 0
    scanInstruction function low site = case decoded site of
      A.Branch _ link target
        | not link && target >= start && target < finish -> pure []
        | GuestAddress target `elem` map functionStart (layoutFunctions layout) -> pure [edge target (if link then Call else TailCall)]
        | M.member (GuestAddress target) stockFrames -> pure [edge target (if link then StockLeafCall else StockLeafTailCall)]
        | GuestAddress target `elem` map functionStart callbacks -> pure [edge target (if link then CallbackCall else CallbackTailCall)]
        | owned target -> pure []
        | otherwise -> Left (AdmissionError (GuestAddress site) ((if link then CallOutsideOwned else BranchOutsideOwned) (GuestAddress target)))
      A.Rejected A.ThumbBranch -> Left (AdmissionError (GuestAddress site)
        (CallOutsideOwned (GuestAddress (maybe site id (A.directTarget site (word site))))))
      _ -> pure []
      where
        GuestAddress start = functionStart function
        GuestAddress finish = functionEnd function
        edge target kind = CallEdge (GuestAddress site) (GuestAddress target) (StackOffset low) kind
    compose frames visiting done entry
      | M.member entry done = Right done
      | S.member entry visiting = Left (AdmissionError entry Recursion)
      | otherwise = do
          let frame = frames M.! entry
              calls = frameCalls frame
          done' <- foldM (compose frames (S.insert entry visiting)) done
            [edgeTarget edge | edge <- calls, edgeKind edge `elem` [Call, TailCall]]
          let depth edge = let StackOffset at = edgeStack edge in case edgeKind edge of
                Call -> at + ownedDepth edge
                TailCall -> at + ownedDepth edge
                StockLeafCall -> at + stockDepth edge
                StockLeafTailCall -> at + stockDepth edge
                _ -> at
              ownedDepth edge = let StackOffset amount = done' M.! edgeTarget edge in amount
              stockDepth edge = let StackOffset amount = stockFrames M.! edgeTarget edge in amount
              StackOffset own = frameLow frame
              low = minimum (own : map depth calls)
          when (low < -4096) (Left (AdmissionError entry StackUnbounded))
          pure (M.insert entry (StackOffset low) done')
