{-# LANGUAGE OverloadedStrings #-}
module Replacement.Task1Actor.Artifact
  ( Artifact, ArtifactError (..), Carrier (..), CodeAddress (..), Service (..), CallSite (..)
  , deriveArtifact, carriers, serviceBindings, semanticMap, artifactCarriers, emittedCarriers
  , canonicalSource, CanonicalSource, stageImage5, artifactIdentity, semanticSites
  ) where

import qualified A32 as A
import Control.Monad (unless)
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import Data.List (nub, sort)
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)

newtype CodeAddress = CodeAddress Word32 deriving (Eq, Ord, Show)
data Carrier = Carrier { carrierStart :: CodeAddress, carrierEnd :: CodeAddress } deriving (Eq, Show)
data Service = Setup | ControllerSetup | LoopService | Execution | Receive | LowCommand | CommandService
  | PredicateA | PredicateB | Suppression | HeldCommand | HeldClear | PendingFirst | PendingSecond
  | Image4Pending | PendingPredicate | PendingThird | PendingPriority | DeferredFirst | DeferredSecond
  | AfterFirst | AfterSecond | AfterThird | Diagnostic | Pump | Allocate | Register | RegistrationError
  | SetupService | SetupZero deriving (Eq, Ord, Show, Enum, Bounded)
data CallSite = CallSite { callPc :: CodeAddress, callService :: Service, callReturn :: CodeAddress } deriving (Eq, Show)
data ArtifactError = ExtentRejected | SymbolsRejected | PaddingRejected | TransferRejected | MissingService | SourceRejected | StockFallbackRejected deriving (Eq, Show)
data Artifact = Artifact B.ByteString [(Carrier, B.ByteString)] [CallSite] [(Word32,Word32)]
newtype CanonicalSource = CanonicalSource B.ByteString

carriers :: [Carrier]
carriers = [Carrier (CodeAddress 0xbfb124) (CodeAddress 0xbfb1b8), Carrier (CodeAddress 0xc22f60) (CodeAddress 0xc2327c)]

serviceBindings :: [(Service, String, CodeAddress)]
serviceBindings = zipWith (\service (name,address) -> (service,name,CodeAddress address)) [minBound..maxBound]
  [ ("setup",0xbfb124), ("controller_setup",0xc22bd0), ("loop_service",0xd3cbd4), ("execution",0x63270c)
  , ("receive",0xdb5124), ("low_command",0xd0b318), ("command_service",0xd0b438), ("predicate_a",0xd0c954)
  , ("predicate_b",0xd35f3c), ("suppression",0xd5cf50), ("held_command",0xd3ad34), ("held_clear",0xd3ad10)
  , ("pending_first",0xd3b5ec), ("pending_second",0xd19d18), ("image4_pending",0x636004), ("pending_predicate",0xd1734c)
  , ("pending_third",0xd17370), ("pending_priority",0xd3b0dc), ("deferred_first",0xd3ab00), ("deferred_second",0xd3ad5c)
  , ("after_first",0xd36ea4), ("after_second",0xd36e60), ("after_third",0xd35e14), ("diagnostic",0xd0cd10)
  , ("pump",0xbfbd20), ("allocate",0xd36310), ("register",0xdb4b34), ("registration_error",0x633594)
  , ("setup_service",0xd48240), ("setup_zero",0xd38130) ]

require :: Bool -> ArtifactError -> Either ArtifactError ()
require condition problem = unless condition (Left problem)

artifactIdentity :: Artifact -> String
artifactIdentity (Artifact bytes _ _ _) = SHA.sha256Hex (SHA.sha256 bytes)

semanticMap :: Artifact -> [CallSite]
semanticMap (Artifact _ _ sites _) = sites

emittedCarriers :: Artifact -> [(Carrier,B.ByteString)]
emittedCarriers (Artifact _ parts _ _) = parts

artifactCarriers :: Artifact -> [(Carrier,B.ByteString)]
artifactCarriers = map pad . emittedCarriers
  where
    pad (carrier@(Carrier (CodeAddress low) (CodeAddress high)),bytes) =
      (carrier, bytes <> B.concat (replicate ((fromIntegral (high-low) - B.length bytes) `div` 4) (U.putWord32 0xe320f000)))

semanticSites :: Artifact -> [(Word32,Word32)]
semanticSites (Artifact _ _ _ sites) = sites

markerPcs :: [Word32]
markerPcs = [0xc22f64,0xc22f74,0xc22f84,0xc22fb4,0xc22fd8,0xc23094,0xc230a0,0xc230b4,0xc230c0,0xc230d8,0xc230f4,0xc23104,0xc23114,0xc2311c,0xc2312c,0xc23150,0xc231ec,0xc231fc,0xc23278]

deriveArtifact :: B.ByteString -> B.ByteString -> Either ArtifactError Artifact
deriveArtifact bytes symbolsBytes = do
  symbols <- either (const (Left SymbolsRejected)) Right (F.parseSymbols symbolsBytes)
  let names = map F.symbolName symbols
      find name = either (const (Left SymbolsRejected)) Right (F.lookupSymbol symbols name)
      at symbol = F.symbolAddress symbol
      extent symbol = at symbol + F.symbolSize symbol
  require (length names == length (nub names)) SymbolsRejected
  once <- find "xm1_task1_once"
  actor <- find "xm1_task1_actor"
  require (at once == 0xbfb124 && at actor == 0xc22f60) SymbolsRejected
  require (F.symbolSize once > 0 && extent once <= 0xbfb1b8 && F.symbolSize actor > 0 && extent actor <= 0xc2327c
    && all (\symbol -> at symbol .&. 3 == 0 && F.symbolSize symbol .&. 3 == 0) [once,actor]) ExtentRejected
  require (B.length bytes == fromIntegral (extent actor - 0xbfb124)) ExtentRejected
  let parts = zip carriers [B.take (fromIntegral (F.symbolSize once)) bytes,
        B.take (fromIntegral (F.symbolSize actor)) (B.drop (0xc22f60-0xbfb124) bytes)]
      hole = B.take (fromIntegral (0xc22f60 - extent once)) (B.drop (fromIntegral (F.symbolSize once)) bytes)
      within address = any (\symbol -> address >= at symbol && address < extent symbol) [once,actor]
  require (B.all (==0) hole) PaddingRejected
  mapM_ (\(_,name,CodeAddress address) -> find name >>= \symbol -> require (at symbol == address && F.symbolSize symbol == 0) SymbolsRejected) serviceBindings
  require (all (\symbol -> F.symbolName symbol `elem` (map (\(_,name,_) -> name) serviceBindings ++ ["xm1_task1_once","xm1_task1_actor"])
    || (F.symbolSize symbol == 0 && within (at symbol)
      && (take 2 (F.symbolName symbol) == "$a" || F.symbolName symbol `elem` ["j11_" ++ showHex pc "" | pc <- markerPcs]))) symbols) SymbolsRejected
  sites <- concat <$> mapM (scan within) parts
  require (sort (nub (map callService sites)) == [minBound..maxBound]) MissingService
  marked <- mapM (\pc -> do
    symbol <- find ("j11_" ++ showHex pc "")
    require (F.symbolSize symbol == 0 && at symbol >= at actor && at symbol < extent actor && at symbol .&. 3 == 0) SymbolsRejected
    pure (pc,at symbol)) markerPcs
  let unique service = case [pc | CallSite (CodeAddress pc) bound _ <- sites, bound == service] of
        [pc] -> Right pc
        _ -> Left SymbolsRejected
  controller <- unique ControllerSetup
  setup <- unique Setup
  require (setup == 0xc22f6c && controller == 0xc22f70
    && U.word32At bytes (0xc22f60-0xbfb124) == 0xe92d4fe0) SymbolsRejected
  let predecessor stock service = do
        mapped <- maybe (Left SymbolsRejected) Right (lookup stock marked)
        require (any (\site -> callPc site == CodeAddress (mapped-4) && callService site == service) sites) SymbolsRejected
        pure (stock-4,mapped-4)
  calls <- sequence [predecessor 0xc22fd8 Receive, predecessor 0xc23150 Receive, predecessor 0xc23278 Pump]
  mapped <- mapM (\(stock,emitted) -> if stock `elem` [0xc230a0,0xc230c0,0xc230f4]
    then do
      require (fromIntegral (emitted-0xbfb124) + 12 <= B.length bytes) SymbolsRejected
      let cmp = U.word32At bytes (fromIntegral (emitted+4-0xbfb124))
          branch = U.word32At bytes (fromIntegral (emitted+8-0xbfb124))
      require (cmp == if stock == 0xc230f4 then 0xe3500000 else 0xe3500001) SymbolsRejected
      case A.decode (emitted+8) branch of
        A.Branch (A.Cond condition) False _ | condition == (if stock == 0xc230f4 then 0 else 1) -> pure (stock,emitted+8)
        _ -> Left SymbolsRejected
    else do
      whenDeferred stock emitted
      pure (stock,emitted)) marked
  pure (Artifact bytes parts sites (mapped ++ calls ++ [(0xc22f60,0xc22f60),(0xbfb124,0xbfb124),(0xc22bd0,0xc22bd0),(0xc22f6c,setup),(0xc22f70,controller)]))
  where
    whenDeferred stock emitted
      | stock `elem` [0xc23114,0xc2312c] = do
          require (emitted >= 0xc22f70) SymbolsRejected
          let word delta = U.word32At bytes (fromIntegral (emitted-0xbfb124-delta))
          require (word 16 == 0xe3500001 && word 12 == 0xe320f000
            && word 4 == if stock == 0xc23114 then 0xe3a00002 else 0xe3a00001) SymbolsRejected
          case A.decode (emitted-8) (word 8) of
            A.Branch (A.Cond 1) False target | target > emitted -> pure ()
            _ -> Left SymbolsRejected
      | otherwise = pure ()
    scan within (Carrier (CodeAddress low) _,code) = walk [] [low]
      where
        end = low + fromIntegral (B.length code)
        walk _ [] = pure []
        walk seen (pc:rest)
          | pc `elem` seen = walk seen rest
          | otherwise = do
              require (pc >= low && pc < end && pc .&. 3 == 0) TransferRejected
              let offset = fromIntegral (pc-low)
                  decoded = A.decode pc (U.word32At code offset)
                  successors = case decoded of
                    A.Branch (A.Cond condition) False target -> target : [pc+4 | condition /= 14]
                    A.BranchExchange _ False _ -> []
                    A.LoadMultiple _ _ _ _ restored | A.pc `elem` restored -> []
                    _ -> [pc+4]
              calls <- instruction within code low offset
              remaining <- walk (pc:seen) (successors ++ rest)
              pure (calls ++ remaining)
    instruction within code low offset =
      let pc = low + fromIntegral offset
          word = U.word32At code offset
      in case A.decode pc word of
        A.Branch _ True target -> case [service | (service,_,CodeAddress address) <- serviceBindings, address == target] of
          [service] -> pure [CallSite (CodeAddress pc) service (CodeAddress (pc+4))]
          _ -> Left TransferRejected
        A.Branch _ False target -> [] <$ require (within target) StockFallbackRejected
        A.BranchExchange condition False register -> [] <$ require (register == A.lr && condition == A.Cond 14 && pc < 0xbfb1b8) TransferRejected
        A.BranchExchange {} -> Left TransferRejected
        A.LoadMultiple condition base mode updates restored -> [] <$ require
          (base /= A.pc && (A.pc `notElem` restored || (pc < 0xbfb1b8 && base == A.sp && mode == A.IncrementAfter && updates && condition == A.Cond 14))) TransferRejected
        A.Load _ _ _ destination base _ indexing -> [] <$ require (destination /= A.pc && (base /= A.pc || indexing == A.OffsetOnly)) TransferRejected
        A.Store _ _ _ base _ indexing -> [] <$ require (base /= A.pc || indexing == A.OffsetOnly) TransferRejected
        A.StoreMultiple _ base _ _ saved -> [] <$ require (base /= A.pc && A.pc `notElem` saved) TransferRejected
        A.MoveWide _ destination _ -> [] <$ require (destination /= A.pc) TransferRejected
        A.MoveTop _ destination _ -> [] <$ require (destination /= A.pc) TransferRejected
        A.ReadStatus _ destination -> [] <$ require (destination /= A.pc) TransferRejected
        A.ZeroExtend _ destination _ _ _ -> [] <$ require (destination /= A.pc) TransferRejected
        A.Multiply _ destination _ _ _ -> [] <$ require (destination /= A.pc) TransferRejected
        A.Compute _ destinations -> [] <$ require (A.pc `notElem` destinations) TransferRejected
        A.DataProcessing _ _ _ destination _ _ -> [] <$ require (destination /= A.pc) TransferRejected
        A.Rejected {} -> Left TransferRejected
        _ -> pure []

canonicalSource :: B.ByteString -> B.ByteString -> Either ArtifactError CanonicalSource
canonicalSource dat image = do
  require (SHA.sha256Hex (SHA.sha256 dat) == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"
    && SHA.sha256Hex (SHA.sha256 image) == "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a") SourceRejected
  pure (CanonicalSource image)

stageImage5 :: CanonicalSource -> Artifact -> Either ArtifactError B.ByteString
stageImage5 (CanonicalSource original) artifact = do
  require (all (\(Carrier (CodeAddress low) _,code) -> code /= B.take (B.length code) (B.drop (fromIntegral (low-0xbf1a00)) original)) (emittedCarriers artifact)) StockFallbackRejected
  pure (foldl patch original (artifactCarriers artifact))
  where
    patch image (Carrier (CodeAddress low) (CodeAddress high),code) =
      B.take (fromIntegral (low-0xbf1a00)) image <> code <> B.drop (fromIntegral (high-0xbf1a00)) image
