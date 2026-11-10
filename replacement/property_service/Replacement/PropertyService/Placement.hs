{-# LANGUAGE GHC2021 #-}

-- | Entry adapters, retained islands and reference closure for owned dispatch.
module Replacement.PropertyService.Placement
  ( GuestAddress (..)
  , Carrier (..)
  , EntryAddresses (..)
  , ScanImage (..)
  , ScanDepth (..)
  , Reference (..)
  , ScanHit (..)
  , DataReason (..)
  , DataSite (..)
  , ExemptedHit (..)
  , HullScan (..)
  , PlacementError (..)
  , retiredHull
  , retainedWrappers
  , stage4Carrier
  , getterStockEntry
  , setterStockEntry
  , retainedRegions
  , carrierBytes
  , inCarrier
  , scanDepth
  , scanReferences
  , hitTarget
  , referencesInto
  , chooseCarrier
  , scanHull
  , hitListing
  , dataSites
  , siteWindow
  , partitionHits
  ) where

import qualified A32 as A
import Data.Bits (shiftL, shiftR, testBit, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.Set as S
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)
import Replacement.PropertyService.Classify
import ServiceInventory (Image (..))

-- | A half-open guest address interval.
data Carrier = Carrier { carrierStart :: GuestAddress, carrierEnd :: GuestAddress } deriving (Eq, Show)

data EntryAddresses = EntryAddresses { getterEntry :: GuestAddress, setterEntry :: GuestAddress } deriving (Eq, Show)

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

data PlacementError
  = CarrierNotCandidate GuestAddress
  | CarrierReferenced [ScanHit]
  | ExecutableReference ScanHit
  deriving (Eq, Show)

retiredHull, retainedWrappers, stage4Carrier :: Carrier
retiredHull = Carrier (GuestAddress 0x61e110) (GuestAddress 0x6309c4)
retainedWrappers = Carrier (GuestAddress 0x6245f8) (GuestAddress 0x624694)
stage4Carrier = Carrier (GuestAddress 0x62c120) (GuestAddress 0x62fbfc)

getterStockEntry, setterStockEntry :: GuestAddress
getterStockEntry = GuestAddress 0x61e110
setterStockEntry = GuestAddress 0x624694

carrierBytes :: Carrier -> Int
carrierBytes (Carrier (GuestAddress start) (GuestAddress end)) = fromIntegral (end - start)

inCarrier :: Carrier -> GuestAddress -> Bool
inCarrier (Carrier start end) address = start <= address && address < end

scanDepth :: ScanImage -> ScanDepth
scanDepth label = case label of
  ScanImage2 -> WordsOnly
  ScanImage3 -> WordsOnly
  _ -> InstructionsAndWords

-- | Unresolved external data references reserve two aligned words.
retainedRegions :: HullScan -> [Carrier]
retainedRegions scan = retainedWrappers :
  [ Carrier (GuestAddress low) (GuestAddress (low + 8))
  | hit <- referencesInto retiredHull (hullHits scan)
  , hitTarget hit `notElem` [getterStockEntry, setterStockEntry]
  , not (inCarrier retainedWrappers (hitTarget hit))
  , let GuestAddress target = hitTarget hit
  , let low = target .&. 0xfffffffc
  ]

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

chooseCarrier :: Image -> [Property] -> [ScanHit] -> Maybe Carrier
chooseCarrier _ _ hits
  | any overlaps reservations = Nothing
  | otherwise = Just stage4Carrier
  where
    reservations = retainedRegions (HullScan retiredHull hits [] Nothing)
    overlaps (Carrier low high) = low < carrierEnd stage4Carrier && carrierStart stage4Carrier < high

-- | Scan the whole hull without exempting tables or dead instructions.
scanHull :: Image -> [(ScanImage, Image)] -> [Property] -> Either PlacementError HullScan
scanHull image4 scanned properties = do
  let (refused, exempted) = partitionHits scanned dataSites (scanReferences retiredHull scanned S.empty)
  pure (HullScan retiredHull refused exempted (chooseCarrier image4 properties refused))

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
  , DataSite ScanImage4 (GuestAddress 0x7e3be0) 0x0bf90bc3 (reviewed "3e20ec40e999ccbcdaafc7204fcd3224289970bae3b8c46d281dc67cd6da3139") HighEntropyData
  , DataSite ScanImage5 (GuestAddress 0x101f378) 0x0bd80bd7 (reviewed "7ff8574a10b28ad47727da1a2a2d18d884e1a5e18a6282abae5215b0c9ec886b") HighEntropyData
  , DataSite ScanImage5 (GuestAddress 0x10210dc) 0x1ad81ad7 (reviewed "34c62daa86dc8ec655391abff310e66b5dd48af915f3fce6f1fbc7411fe569fc") HighEntropyData
  , DataSite ScanImage5 (GuestAddress 0x1092bb0) 0xdbd67414 (reviewed "dd02d733eb10199502204258d936a4a9345c3a89486d6aed0e163a42c8c5ee9d") HighEntropyData
  , DataSite ScanImage5 (GuestAddress 0x109fcbc) 0xfbd60cda (reviewed "18656f6cee168419580024c493fbe110f29a4aa50a67d30c8085cdbf0cb53402") HighEntropyData
  , DataSite ScanImage5 (GuestAddress 0x10a3e8c) 0x2bd5fff3 (reviewed "76b947f09460180f28040d7dc774443ea7f3a3235c60719d36afcf22a6b47c08") HighEntropyData
  , DataSite ScanImage5 (GuestAddress 0x10a41f8) 0x2bd5f607 (reviewed "b29db04c776ef3bc20671edd0608c54bbfe487f6917a5c305b09aa8c0b7ac1e5") HighEntropyData
  , DataSite ScanImage5 (GuestAddress 0x10bfafc) 0x8bd59396 (reviewed "ed94ad49ce53673b3bec53b6ce370180fbbbd8d71d90450116535df78ee687d7") HighEntropyData
  , DataSite ScanImage5 (GuestAddress 0x10c4e90) 0x9ad58486 (reviewed "2e952f4bbf4f327954b6684d49229823d594c85b7656d06ea2665b708971733d") HighEntropyData
  , DataSite ScanImage5 (GuestAddress 0x10dee14) 0x6ad51452 (reviewed "19c6ac59db48e8393103eef2275dab3980a2098d20dfb45bff44761d7e8d0216") HighEntropyData
  , DataSite ScanPlain (GuestAddress 0x12b7004) 0xbacda046 (reviewed "236c4ed14aca7f50f34df17f5a2316688fe29259686c9e85be0ea2479e658844") HighEntropyData
  , DataSite ScanPlain (GuestAddress 0x12bd64c) 0xfacdae43 (reviewed "71694b90376f5442ad2e1b5a5b6c8385239b64b13a0ec3787f4539eabc6bf32c") HighEntropyData
  , DataSite ScanPlain (GuestAddress 0x12c0860) 0x2bcd81a1 (reviewed "5fb2fd62901fd5e6062725bcc621d18eeee470a463a3ae1aceb6d51eef97c31f") HighEntropyData
  , DataSite ScanPlain (GuestAddress 0x12c43dc) 0x0acd76bb (reviewed "a03ee8098bebd6eb2aba9ee601b27aad9208c95df61bcb79d23cef395335ddce") HighEntropyData
  , DataSite ScanPlain (GuestAddress 0x12c5dd0) 0x1bcd7865 (reviewed "5e9593ee693bf5d01837571a9748e5cc94d818d96690086086a8b01a0a697db0") HighEntropyData
  , DataSite ScanPlain (GuestAddress 0x12cba78) 0x7bcd506b (reviewed "29913b96e1c293b9d07ae5af977b99120ed5acdbfc78bdb488a5bef6fdd79496") HighEntropyData
  , DataSite ScanPlain (GuestAddress 0x12d1b88) 0x5acd3750 (reviewed "8ed771c2fdc3542da55525fba82b7548513fff3b794355e48cdd18945e2767f6") HighEntropyData
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

