{-# LANGUAGE GHC2021 #-}

-- | Where the owned property service sits in image4: the carrier, the stock references that would
-- reach it, and the getter and setter table words repointed at the owned stubs.
module Replacement.PropertyService.Placement
  ( GuestAddress (..)
  , Carrier (..)
  , StubAddresses (..)
  , ScanImage (..)
  , ScanDepth (..)
  , Reference (..)
  , ScanHit (..)
  , DataReason (..)
  , DataSite (..)
  , ExemptedHit (..)
  , TableSide (..)
  , TableWord (..)
  , TablePatch (..)
  , PlacementError (..)
  , getterTableBase
  , setterTableBase
  , setterFallback
  , getterExit
  , setterExit
  , carrierBytes
  , inCarrier
  , scanDepth
  , deriveCarrier
  , checkPreceding
  , scanReferences
  , hitListing
  , dataSites
  , siteWindow
  , partitionHits
  , derivePatch
  , patchCount
  ) where

import qualified A32 as A
import Data.Bits (shiftL, shiftR, testBit, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (sortOn)
import qualified Data.Map.Strict as M
import Data.Ord (Down (..))
import qualified Data.Set as S
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)
import PropertyFields (PropertyId (..))
import Replacement.PropertyService.Classify
import ServiceInventory (Image (..), imageWord)

-- | The half-open run [start, end) of stock setter arms the owned code replaces.
data Carrier = Carrier { carrierStart :: GuestAddress, carrierEnd :: GuestAddress } deriving (Eq, Show)

data StubAddresses = StubAddresses { getterStub :: GuestAddress, setterStub :: GuestAddress } deriving (Eq, Show)

data ScanImage = ScanImage4 | ScanImage5 | ScanPlain | ScanLoaderA | ScanLoaderB | ScanImage2 | ScanImage3
  deriving (Eq, Ord, Show, Enum, Bounded)

-- | Whether an image's words are also read as instructions.
data ScanDepth = InstructionsAndWords | WordsOnly deriving (Eq, Show)

data Reference
  = BranchInto GuestAddress
  | WordInto
  | MovPairInto GuestAddress
  | PcRelativeInto GuestAddress
  deriving (Eq, Show)

data ScanHit = ScanHit
  { hitImage :: ScanImage
  , hitSite :: GuestAddress
  , hitWord :: Word32
  , hitReference :: Reference
  }
  deriving (Eq, Show)

data DataReason = HighEntropyData deriving (Eq, Show)

-- | A reviewed scan site holding data: the hit is accepted only when its image, address and word
-- match and the 64 bytes at [address - 32, address + 32) hash to `siteWindowSha256`.
data DataSite = DataSite
  { siteImage :: ScanImage
  , siteAddress :: GuestAddress
  , siteWord :: Word32
  , siteWindowSha256 :: SHA.Sha256
  , siteReason :: DataReason
  }

-- | An accepted hit with the count of words in its site window the decoder rejects.
data ExemptedHit = ExemptedHit
  { exemptedSite :: DataSite
  , exemptedHit :: ScanHit
  , exemptedRejectedWords :: Int
  }

data TableSide = GetterTable | SetterTable deriving (Eq, Ord, Show)

data TableWord = TableWord
  { wordSide :: TableSide
  , wordProperty :: PropertyId
  , wordAddress :: GuestAddress
  , wordOriginal :: GuestAddress
  , wordPatched :: GuestAddress
  }
  deriving (Eq, Show)

newtype TablePatch = TablePatch [TableWord] deriving (Eq, Show)

data PlacementError
  = NoOwnedRun
  | TableShape
  | TableWordDisagrees TableSide PropertyId
  | CarrierWordNotOwned PropertyId
  | PrecedingNotTransfer Word32
  | PrecedingMissing
  | CarrierReferenced [ScanHit]
  deriving (Eq, Show)

getterTableBase, setterTableBase, setterFallback, getterExit, setterExit :: GuestAddress
getterTableBase = GuestAddress 0x61e12c
setterTableBase = GuestAddress 0x6246cc
setterFallback = GuestAddress 0x6301fc
getterExit = GuestAddress 0x624400
setterExit = GuestAddress 0x630200

carrierBytes :: Carrier -> Int
carrierBytes (Carrier (GuestAddress start) (GuestAddress end)) = fromIntegral (end - start)

inCarrier :: Carrier -> GuestAddress -> Bool
inCarrier (Carrier start end) address = start <= address && address < end

scanDepth :: ScanImage -> ScanDepth
scanDepth label = case label of
  ScanImage2 -> WordsOnly
  ScanImage3 -> WordsOnly
  _ -> InstructionsAndWords

-- | The largest maximal run of consecutive distinct setter arm targets whose properties are all owned
-- fields or slot fields, lowest address on a tie; a run ends at the next distinct target, else at the setter fallback.
deriveCarrier :: [Property] -> Either PlacementError Carrier
deriveCarrier properties = case sortOn (\(GuestAddress start, GuestAddress end) -> (Down (end - start), start)) runs of
  (start, end) : _ -> Right (Carrier start end)
  [] -> Left NoOwnedRun
  where
    targets = M.toAscList (M.fromListWith (&&) [(propertySetterArm p, ownsArm (propertyClass p)) | p <- properties])
    bounded = zip targets (map (Just . fst) (drop 1 targets) ++ [Nothing])
    runs = collect bounded
    collect entries = case dropWhile (not . snd . fst) entries of
      [] -> []
      started@(((start, _), _) : _) ->
        let (run, rest) = span (snd . fst) started
            end = case snd (last run) of
              Just next -> Just next
              Nothing | setterFallback > fst (fst (last run)) -> Just setterFallback
              Nothing -> Nothing
        in maybe id (\e -> ((start, e) :)) end (collect rest)
    ownsArm verdict = case verdict of
      Owned (OwnedField _) -> True
      Owned (OwnedSlotField _) -> True
      _ -> False

-- | The stock word before the carrier when it transfers unconditionally: B with condition AL or
-- `pop {..., pc}`.
checkPreceding :: Image -> Carrier -> Either PlacementError Word32
checkPreceding image (Carrier (GuestAddress start) _) = case imageWord image (start - 4) of
  Just word | transfers (A.decode (start - 4) word) -> Right word
  Just word -> Left (PrecedingNotTransfer word)
  Nothing -> Left PrecedingMissing
  where
    transfers instruction = case instruction of
      A.Branch (A.Cond 14) False _ -> True
      A.LoadMultiple (A.Cond 14) base A.IncrementAfter True registers -> base == A.sp && A.pc `elem` registers
      _ -> False

-- | Every aligned word of the images that equals a carrier address. For images scanned as
-- instructions, also every word outside the carrier that branches, loads PC-relative or forms an ADR
-- into it, and every MOVW whose MOVT of the same register within the next 8 words builds a carrier
-- address. `exempt` lists image4 word addresses whose carrier value is accounted for.
scanReferences :: Carrier -> [(ScanImage, Image)] -> S.Set GuestAddress -> [ScanHit]
scanReferences carrier images exempt = concatMap scan images
  where
    scan (label, Image _ _ base bytes) =
      let count = B.length bytes `div` 4
          raw index = U.word32At bytes (4 * index)
          site index = base + 4 * fromIntegral index
          instructions = scanDepth label == InstructionsAndWords
      in concat [hits label instructions (site index) (raw index) [raw later | later <- [index + 1 .. min (count - 1) (index + 8)]] | index <- [0 .. count - 1]]
    into = inCarrier carrier . GuestAddress
    hits label instructions site word following =
      [ScanHit label (GuestAddress site) word WordInto | into word, not (label == ScanImage4 && S.member (GuestAddress site) exempt)]
        ++ if not instructions
          then []
          else
            [ScanHit label (GuestAddress site) word (BranchInto (GuestAddress target)) | not (into site), Just target <- [branchTarget site word], into target]
              ++ [ScanHit label (GuestAddress site) word (PcRelativeInto (GuestAddress target)) | not (into site), target <- pcRelativeTargets site word, into target]
              ++ [ScanHit label (GuestAddress site) word (MovPairInto (GuestAddress built)) | Just built <- [movPair word following], into built]

-- | The target of B, BL or BLX (immediate).
branchTarget :: Word32 -> Word32 -> Maybe Word32
branchTarget site word
  | word .&. 0xfe000000 == 0xfa000000 = Just (site + 8 + offset + (if testBit word 24 then 2 else 0))
  | otherwise = case A.decode site word of
      A.Branch _ _ target -> Just target
      _ -> Nothing
  where
    imm = word .&. 0x00ffffff
    offset = (if testBit imm 23 then imm .|. 0xff000000 else imm) `shiftL` 2

-- | Addresses a PC-relative immediate load reads or an ADD/SUB from pc forms.
pcRelativeTargets :: Word32 -> Word32 -> [Word32]
pcRelativeTargets site word = case A.decode site word of
  A.Load _ width _ _ base (A.OffsetImmediate offset) A.OffsetOnly | base == A.pc ->
    let target = fromIntegral (toInteger site + 8 + toInteger offset)
    in target : [target + 4 | width == A.Dual]
  A.DataProcessing _ A.ADD _ _ base (A.Immediate amount) | base == A.pc -> [site + 8 + amount]
  A.DataProcessing _ A.SUB _ _ base (A.Immediate amount) | base == A.pc -> [site + 8 - amount]
  _ -> []

movPair :: Word32 -> [Word32] -> Maybe Word32
movPair word following = case decodeWide word of
  Just (False, register, low) -> case [high | Just (True, other, high) <- map decodeWide following, other == register] of
    high : _ -> Just ((high `shiftL` 16) .|. low)
    [] -> Nothing
  _ -> Nothing
  where
    decodeWide value
      | value `shiftR` 28 == 15 = Nothing
      | value .&. 0x0ff00000 == 0x03000000 = Just (False, (value `shiftR` 12) .&. 15, wide value)
      | value .&. 0x0ff00000 == 0x03400000 = Just (True, (value `shiftR` 12) .&. 15, wide value)
      | otherwise = Nothing
    wide value = ((value `shiftR` 4) .&. 0xf000) .|. (value .&. 0xfff)

hitListing :: ScanHit -> String
hitListing (ScanHit label (GuestAddress site) word reference) =
  show label ++ " 0x" ++ showHex site "" ++ ": " ++ pad (showHex word "") ++ "  " ++ show (A.decode site word) ++ "  " ++ show reference
  where
    pad text = replicate (8 - length text) '0' ++ text

dataSites :: [DataSite]
dataSites =
  [ DataSite ScanPlain (GuestAddress 0x12ba48c) 0xcacdc8b4 (reviewed "4cfc92baaac6d3aa300630892294888e194d4e2d188ba40808af0c94f4bffbc8") HighEntropyData
  ]
  where
    reviewed text = either (error . ("data site digest: " ++)) id (SHA.parseSha256Hex text)

-- | The 64 bytes at [address - 32, address + 32) when the image holds them.
siteWindow :: Image -> GuestAddress -> Maybe B.ByteString
siteWindow (Image _ _ base bytes) (GuestAddress address)
  | address < base + 32 || toInteger address + 32 > toInteger base + toInteger (B.length bytes) = Nothing
  | otherwise = Just (B.take 64 (B.drop (fromIntegral (address - base) - 32) bytes))

-- | Hits no data site accounts for, and the accepted ones.
partitionHits :: [(ScanImage, Image)] -> [DataSite] -> [ScanHit] -> ([ScanHit], [ExemptedHit])
partitionHits images sites = foldr place ([], [])
  where
    place hit (refused, accepted) = case [(site, window) | site <- sites, Just window <- [accounts site hit]] of
      (site, window) : _ -> (refused, ExemptedHit site hit (rejectedWords (siteAddress site) window) : accepted)
      [] -> (hit : refused, accepted)
    accounts site (ScanHit label address word _)
      | siteImage site == label && siteAddress site == address && siteWord site == word =
          case lookup label images >>= \image -> siteWindow image address of
            Just window | SHA.sha256 window == siteWindowSha256 site -> Just window
            _ -> Nothing
      | otherwise = Nothing
    rejectedWords (GuestAddress address) window =
      length [() | index <- [0 .. 15], A.Rejected _ <- [A.decode (address - 32 + 4 * fromIntegral index) (U.word32At window (4 * index))]]

-- | Every owned property's getter and setter word repointed at the stubs, after checking each stock
-- word against its classified arm and every carrier-reaching setter word against ownership.
derivePatch :: Image -> [Property] -> Carrier -> StubAddresses -> Either PlacementError TablePatch
derivePatch image properties carrier (StubAddresses getter setter) = do
  if map propertyId properties == [PropertyId n | n <- [1 .. 645]] then Right () else Left TableShape
  TablePatch . concat <$> mapM one properties
  where
    one (Property identifier@(PropertyId n) getterArm setterArm verdict) = do
      let at (GuestAddress base) = GuestAddress (base + 4 * (n - 1))
          check side (GuestAddress address) (GuestAddress expected) = case imageWord image address of
            Just actual | actual == expected -> Right ()
            _ -> Left (TableWordDisagrees side identifier)
          owned = case verdict of
            Owned _ -> True
            Delegated _ -> False
      check GetterTable (at getterTableBase) getterArm
      check SetterTable (at setterTableBase) setterArm
      if inCarrier carrier setterArm && not owned then Left (CarrierWordNotOwned identifier) else Right ()
      pure
        [ word
        | owned
        , word <-
            [ TableWord GetterTable identifier (at getterTableBase) getterArm getter
            , TableWord SetterTable identifier (at setterTableBase) setterArm setter
            ]
        ]

patchCount :: TableSide -> TablePatch -> Int
patchCount side (TablePatch words') = length (filter ((== side) . wordSide) words')
