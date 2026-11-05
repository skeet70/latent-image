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
  , HullScan (..)
  , TableSide (..)
  , TableWord (..)
  , TablePatch (..)
  , PlacementError (..)
  , getterTableBase
  , setterTableBase
  , setterFallback
  , carrierBytes
  , inCarrier
  , scanDepth
  , setterHull
  , carrierIntervals
  , checkPreceding
  , scanReferences
  , hitTarget
  , referencesInto
  , chooseCarrier
  , scanHull
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
import Data.Maybe (listToMaybe)
import Data.Ord (Down (..))
import qualified Data.Set as S
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)
import PropertyFields (PropertyId (..))
import Replacement.PropertyService.Classify
import ServiceInventory (Image (..), imageWord)

-- | A half-open run [start, end) of stock setter arms.
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

-- | The scan of the setter arms' hull: the hits no data site accounts for, the accepted ones, and
-- the carrier those hits leave.
data HullScan = HullScan
  { hullRegion :: Carrier
  , hullHits :: [ScanHit]
  , hullExempted :: [ExemptedHit]
  , hullCarrier :: Maybe Carrier
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
  | CarrierNotCandidate GuestAddress
  | LargerCarrierAdmissible Carrier
  | TableShape
  | TableWordDisagrees TableSide PropertyId
  | CarrierWordNotOwned PropertyId
  | PrecedingNotTransfer Word32
  | PrecedingMissing
  | CarrierReferenced [ScanHit]
  deriving (Eq, Show)

getterTableBase, setterTableBase, setterFallback :: GuestAddress
getterTableBase = GuestAddress 0x61e12c
setterTableBase = GuestAddress 0x6246cc
setterFallback = GuestAddress 0x6301fc

carrierBytes :: Carrier -> Int
carrierBytes (Carrier (GuestAddress start) (GuestAddress end)) = fromIntegral (end - start)

inCarrier :: Carrier -> GuestAddress -> Bool
inCarrier (Carrier start end) address = start <= address && address < end

scanDepth :: ScanImage -> ScanDepth
scanDepth label = case label of
  ScanImage2 -> WordsOnly
  ScanImage3 -> WordsOnly
  _ -> InstructionsAndWords

-- | Distinct setter arm targets below the setter fallback, ascending, each with whether every
-- property using it is owned.
armTargets :: [Property] -> [(GuestAddress, Bool)]
armTargets properties = M.toAscList (M.fromListWith (&&) [(propertySetterArm p, owns (propertyClass p)) | p <- properties, propertySetterArm p < setterFallback])
  where
    owns verdict = case verdict of
      Owned OwnedUnassigned -> False
      Owned _ -> True
      Delegated _ -> False

-- | [lowest setter arm, setter fallback).
setterHull :: [Property] -> Maybe Carrier
setterHull properties = case armTargets properties of
  (low, _) : _ -> Just (Carrier low setterFallback)
  [] -> Nothing

-- | Each maximal run of consecutive owned arm targets as its arm starts followed by its end: the next
-- distinct target, else the setter fallback.
ownedRuns :: [Property] -> [[GuestAddress]]
ownedRuns properties = collect (zip targets (map (Just . fst) (drop 1 targets) ++ [Nothing]))
  where
    targets = armTargets properties
    collect entries = case dropWhile (not . snd . fst) entries of
      [] -> []
      started ->
        let (run, rest) = span (snd . fst) started
            end = maybe setterFallback id (snd (last run))
        in (map (fst . fst) run ++ [end]) : collect rest

-- | Every interval of an owned run that starts and ends at arm boundaries, largest first, lowest
-- address on a tie.
carrierIntervals :: [Property] -> [Carrier]
carrierIntervals properties =
  sortOn (\(Carrier (GuestAddress start) (GuestAddress end)) -> (Down (end - start), start))
    [Carrier start end | boundaries <- ownedRuns properties, (index, start) <- zip [1 :: Int ..] boundaries, end <- drop index boundaries]

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

-- | Every aligned word of the images that equals an address in `region`. For images scanned as
-- instructions, also every word that branches, loads PC-relative or forms an ADR into it, and every
-- MOVW whose MOVT of the same register within the next 8 words builds an address in it. Sites inside
-- the region are kept. `exempt` lists image4 word addresses whose value is accounted for.
scanReferences :: Carrier -> [(ScanImage, Image)] -> S.Set GuestAddress -> [ScanHit]
scanReferences region images exempt = concatMap scan images
  where
    scan (label, Image _ _ base bytes) =
      let count = B.length bytes `div` 4
          raw index = U.word32At bytes (4 * index)
          site index = base + 4 * fromIntegral index
          instructions = scanDepth label == InstructionsAndWords
      in concat [hits label instructions (site index) (raw index) [raw later | later <- [index + 1 .. min (count - 1) (index + 8)]] | index <- [0 .. count - 1]]
    into = inCarrier region . GuestAddress
    hits label instructions site word following =
      [ScanHit label (GuestAddress site) word WordInto | into word, not (label == ScanImage4 && S.member (GuestAddress site) exempt)]
        ++ if not instructions
          then []
          else
            [ScanHit label (GuestAddress site) word (BranchInto (GuestAddress target)) | Just target <- [branchTarget site word], into target]
              ++ [ScanHit label (GuestAddress site) word (PcRelativeInto (GuestAddress target)) | target <- pcRelativeTargets site word, into target]
              ++ [ScanHit label (GuestAddress site) word (MovPairInto (GuestAddress built)) | Just built <- [movPair word following], into built]

-- | The address a hit refers to.
hitTarget :: ScanHit -> GuestAddress
hitTarget (ScanHit _ _ word reference) = case reference of
  BranchInto target -> target
  WordInto -> GuestAddress word
  MovPairInto target -> target
  PcRelativeInto target -> target

-- | The hits that refer into `interval` from a site outside it; only image4 sites can lie inside.
referencesInto :: Carrier -> [ScanHit] -> [ScanHit]
referencesInto interval = filter (\hit -> inCarrier interval (hitTarget hit) && not (hitImage hit == ScanImage4 && inCarrier interval (hitSite hit)))

-- | The first of `carrierIntervals` whose preceding word transfers unconditionally and that no hit
-- from outside it reaches.
chooseCarrier :: Image -> [Property] -> [ScanHit] -> Maybe Carrier
chooseCarrier image properties hits = listToMaybe [interval | interval <- carrierIntervals properties, admissible interval]
  where
    byTarget = M.fromListWith (++) [(target, [hit]) | hit <- hits, let GuestAddress target = hitTarget hit]
    admissible interval@(Carrier (GuestAddress low) (GuestAddress high)) =
      either (const False) (const True) (checkPreceding image interval)
        && null (referencesInto interval (concat (M.elems (M.takeWhileAntitone (< high) (M.dropWhileAntitone (< low) byTarget)))))

-- | The hull scan of `scanned` for the classified properties. The repointed setter table words of
-- owned properties are exempt.
scanHull :: Image -> [(ScanImage, Image)] -> [Property] -> Either PlacementError HullScan
scanHull image4 scanned properties = do
  hull <- maybe (Left NoOwnedRun) Right (setterHull properties)
  let GuestAddress setterTable = setterTableBase
      exempt = S.fromList [GuestAddress (setterTable + 4 * (n - 1)) | Property (PropertyId n) _ _ (Owned _) <- properties]
      (refused, exempted) = partitionHits scanned dataSites (scanReferences hull scanned exempt)
  pure (HullScan hull refused exempted (chooseCarrier image4 properties refused))

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
