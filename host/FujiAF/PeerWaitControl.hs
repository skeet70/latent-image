{-# LANGUAGE OverloadedStrings #-}
module FujiAF.PeerWaitControl
  ( PeerControl (..), controlName, parseControlName
  , ControlArgument, argumentWord
  , ControlArtifact, ControlManifest, buildControl, controlDat, controlManifest, controlSource
  , manifestValue, parseManifest, manifestControl, manifestArgument
  , validateControlVeneer
  ) where

import Control.Monad (forM_, unless)
import qualified Crypto.Hash.SHA256 as SHA256
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson.KeyMap as M
import Data.Bits (complement, (.&.), shiftL)
import qualified Data.ByteString as B
import Data.List (sortOn)
import qualified Data.Text as T
import Data.Word (Word32)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.JSON as J
import qualified FujiAF.PeerWaitControlImage as Image
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)

data PeerControl = HeldSentinel | DelayedRelease deriving (Eq, Show)

controlName :: PeerControl -> String
controlName HeldSentinel = "held-sentinel"
controlName DelayedRelease = "delayed-release"

parseControlName :: String -> Either String PeerControl
parseControlName "held-sentinel" = Right HeldSentinel
parseControlName "delayed-release" = Right DelayedRelease
parseControlName _ = Left "unsupported peer wait control"

newtype ControlArgument = ControlArgument Word32 deriving (Eq, Show)

argumentWord :: ControlArgument -> Word32
argumentWord (ControlArgument value) = value

data ControlManifest = ControlManifest PeerControl [Patch] deriving (Eq, Show)
data ControlArtifact = ControlArtifact B.ByteString ControlManifest U.Parsed deriving (Eq, Show)

controlDat :: ControlArtifact -> B.ByteString
controlDat (ControlArtifact bytes _ _) = bytes

controlManifest :: ControlArtifact -> ControlManifest
controlManifest (ControlArtifact _ manifest _) = manifest

controlSource :: ControlArtifact -> U.Parsed
controlSource (ControlArtifact _ _ source) = source

manifestControl :: ControlManifest -> PeerControl
manifestControl (ControlManifest control _) = control

manifestArgument :: ControlManifest -> ControlArgument
manifestArgument _ = ControlArgument 0x69cdf8

newtype Digest = Digest String deriving (Eq, Show)
newtype GuestAddress = GuestAddress Word32 deriving (Eq, Show)
newtype DatOffset = DatOffset Int deriving (Eq, Show)
data Patch = Patch GuestAddress F.SlotOffset DatOffset U.MediaImage U.MediaImage
  deriving (Eq, Show)

sourceDatDigest, sourceRecord1Digest, record2Digest, loaderDigest, veneerDigest :: Digest
sourceDatDigest = Digest "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"
sourceRecord1Digest = Digest "c2da900dcd3f193b05b5ae968dfcb87723571462eafd84c0fed8467f66f7dff3"
record2Digest = Digest "2f1bf3f0e483b892d2890599679c1b515cfc1be8b8c27cbb06d4d913e428abb4"
loaderDigest = Digest "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"
veneerDigest = Digest "0ad6032e1b93cac434a9ae1f502cd7b69b701dcc89dca7dfedb885052c116817"

digestText :: Digest -> String
digestText (Digest text) = text

digest :: B.ByteString -> Digest
digest = Digest . concatMap hexByte . B.unpack . SHA256.hash
  where hexByte byte = let text = showHex byte "" in replicate (2 - length text) '0' ++ text

outputDatDigest, outputRecord1Digest :: PeerControl -> Digest
outputDatDigest HeldSentinel = Digest "d5520aaa4842ce4546bbf21e930d83e9ef7a5372535a0f670f09c088c09dfedb"
outputDatDigest DelayedRelease = Digest "e45d49bdbc874084d5bfa39b3c5b619db948309a1cef2acd9ca16283f07c4db9"
outputRecord1Digest HeldSentinel = Digest "863ea4eaa00e391bacef2b9d312f61cc2450fb726d7d2856812a432945bebaa0"
outputRecord1Digest DelayedRelease = Digest "e90a785cd85c9e3c3dd43593333d1b94c3d49ebe94e0a21ffd4d806464e1eabb"

outputChecksum :: PeerControl -> Word32
outputChecksum HeldSentinel = 2399842050
outputChecksum DelayedRelease = 2399857611

veneerWords :: [Word32]
veneerWords =
  [ 3912060943,3775856640,3808563200,3812896770,3808477739,3851624452
  , 3813801984,167772170,3780247564,436207621,3818922084,3810586624
  , 3797032961,452984828,3796963329,452984820,3777556480,3904720911
  , 3942645758,3808497664,3796963329,452984829,3777556480,3904720911
  , 3812896770,3925870499
  ]

mediaWords :: [Word32] -> U.MediaImage
mediaWords = U.MediaImage . B.concat . map U.putWord32

validateControlVeneer :: B.ByteString -> B.ByteString -> Either String ()
validateControlVeneer bytes symbols = do
  require (bytes == U.mediaImageBytes (mediaWords veneerWords) && digest bytes == veneerDigest)
    "linked control veneer differs from the reviewed 104 bytes"
  rows <- F.parseSymbols symbols
  forM_ [("xm1_peer_wait_control_entry",0x100),("xm1_peer_wait_control_poll",0x114)
        ,("xm1_peer_wait_control_failure",0x148),("xm1_peer_wait_control_acknowledged",0x14c)
        ,("xm1_peer_wait_control_end",0x168),("xm1_peer_wait_original_clear",0x1ff8)] $ \(name,address) -> do
    symbol <- F.lookupSymbol rows name
    require (F.symbolAddress symbol == address) "linked control symbol address differs"
  entry <- F.lookupSymbol rows "xm1_peer_wait_control_entry"
  require (F.symbolSize entry == 104) "linked control entry size differs"
  forM_ [(0x11c,0x14c),(0x124,0x140),(0x134,0x12c),(0x13c,0x114)
        ,(0x148,0x148),(0x154,0x150),(0x164,0x1ff8)] $ \(site,target) -> do
    let word = U.word32At bytes (site - 0x100)
        raw = toInteger (word .&. 0xffffff)
        displacement = if raw >= 0x800000 then raw - 0x1000000 else raw
    require (word .&. 0x0f000000 == 0x0a000000
      && toInteger site + 8 + (displacement `shiftL` 2) == target) "linked control branch target differs"

patches :: PeerControl -> Either String [Patch]
patches control = do
  gate <- patch 0x1ff4 (mediaWords [0xe3443002])
    (mediaWords [case control of HeldSentinel -> 0xeafffffe; DelayedRelease -> 0xeafff841])
  case control of
    HeldSentinel -> pure [gate]
    DelayedRelease -> do
      veneer <- patch 0x100 (mediaWords (replicate 26 0xffffffff)) (mediaWords veneerWords)
      pure [veneer,gate]
  where
    patch address expected replacement = do
      offset <- F.slotOffset address
      _ <- F.expectedMediaPatch offset expected replacement
      pure (Patch (GuestAddress (fromIntegral address)) offset (DatOffset (0x4025c + address)) expected replacement)

manifestValue :: ControlManifest -> Value
manifestValue manifest@(ControlManifest control planned) = object
  [ "schema" .= (1 :: Int), "control" .= controlName control
  , "argument" .= argumentWord (manifestArgument manifest)
  , "source_dat_sha256" .= digestText sourceDatDigest
  , "source_record1_sha256" .= digestText sourceRecord1Digest
  , "output_dat_sha256" .= digestText (outputDatDigest control)
  , "output_record1_sha256" .= digestText (outputRecord1Digest control)
  , "record2_sha256" .= digestText record2Digest, "loader_b_sha256" .= digestText loaderDigest
  , "original_checksum" .= (2399842703 :: Word32), "output_checksum" .= outputChecksum control
  , "checksum_dat_extent" .= ([524,528] :: [Int])
  , "all_other_dat_bytes_equal" .= True
  , "patches" .= map patchValue planned
  , "limits" .= object
      [ "budget" .= (10000000 :: Int), "prefix_to_gate" .= (4000000 :: Int)
      , "held_gate_to_four_loads" .= (4000000 :: Int), "delayed_gate_to_clear" .= (5100000 :: Int)
      , "clear_microsteps" .= (2048 :: Int), "clear_to_loader" .= (20000 :: Int)
      , "wait_stops" .= (4 :: Int), "peer_wait_reads" .= (128 :: Int)
      , "watch_rows" .= (12288 :: Int), "collector_rows" .= (16384 :: Int)
      , "receipt_bytes" .= (16777216 :: Int), "host_seconds" .= (120 :: Int)
      , "stream_bytes" .= (1048576 :: Int), "poll_reads" .= (8192 :: Int)
      , "backoff_iterations" .= (100 :: Int), "release_iterations" .= (4096 :: Int)
      ]
  ]
  where
    patchValue (Patch (GuestAddress address) offset (DatOffset file) (U.MediaImage expected) (U.MediaImage replacement)) = object
      [ "slot_start" .= F.slotOffsetBytes offset, "slot_end" .= (F.slotOffsetBytes offset + B.length expected)
      , "guest_start" .= address, "guest_end" .= (address + fromIntegral (B.length expected))
      , "dat_start" .= file, "dat_end" .= (file + B.length expected)
      , "expected_media_words" .= wordsOf expected, "replacement_media_words" .= wordsOf replacement
      , "expected_media_bytes" .= B.unpack expected, "replacement_media_bytes" .= B.unpack replacement
      , "expected_media_sha256" .= digestText (digest expected)
      , "replacement_media_sha256" .= digestText (digest replacement)
      ]
    wordsOf bytes = [U.word32At bytes offset | offset <- [0,4 .. B.length bytes - 4]]

parseManifest :: B.ByteString -> Either String ControlManifest
parseManifest bytes = do
  require (B.length bytes <= 65536) "control manifest exceeds size bound"
  value <- J.decodeStrictIntegers bytes
  control <- case value of
    Object fields -> case M.lookup "control" fields of
      Just (String name) -> parseControlName (T.unpack name)
      _ -> Left "control manifest lacks control"
    _ -> Left "control manifest is not an object"
  planned <- patches control
  let manifest = ControlManifest control planned
  require (value == manifestValue manifest) "control manifest differs from the fixed source-bound contract"
  pure manifest

buildControl :: PeerControl -> B.ByteString -> Either String ControlArtifact
buildControl control source = do
  require (B.length source == 38405148) "control requires the original X-M1 1.50 DAT length"
  require (digest source == sourceDatDigest) "control requires the original X-M1 1.50 DAT digest"
  parsed <- U.parseContainer source
  checkAll (U.checks parsed ++ U.carriedChecks parsed ++ U.stockChecks parsed)
  let parts = U.parsedParts parsed
      header = U.parsedHeader parsed
      mainStart = U.stagingLength + fromIntegral (U.hMainOffset header)
  require (mainStart == U.stagingLength && U.partVersionMajor parts == 1 && U.partVersionMinor parts == 0x50)
    "source main offset or version differs"
  require (U.hMainChecksum header == 2399842703) "source main checksum differs"
  slot <- F.firstRecordSlotBytes parts
  require (digest (B.map complement (B.take 0x4300 slot)) == sourceRecord1Digest) "source first record differs"
  qualifyUnchanged parsed source
  validateControlVeneer Image.veneerBytes Image.veneerSymbols
  planned <- patches control
  expected <- mapM (\(Patch _ offset _ before after) -> F.expectedMediaPatch offset before after) planned
  updated <- F.replaceExpectedFirstRecord expected parts
  candidate <- U.rebuildContainer parsed updated
  reparsed <- U.parseContainer candidate
  checkAll (U.checks reparsed ++ U.carriedChecks reparsed ++ U.assemblyChecks updated (U.parsedParts reparsed))
  require (U.hMainChecksum (U.parsedHeader reparsed) == outputChecksum control) "candidate checksum differs"
  require (B.length source == B.length candidate) "candidate DAT length differs"
  outsideEqual source candidate ((U.mainChecksumOffset,4) : [(offset,B.length (U.mediaImageBytes after)) | Patch _ _ (DatOffset offset) _ after <- planned])
  forM_ planned $ \(Patch (GuestAddress address) offset (DatOffset file) _ after) -> do
    require (fromIntegral address == F.slotOffsetBytes offset && file == mainStart + F.firstRecordSlotOffset + F.slotOffsetBytes offset)
      "control patch coordinate binding differs"
    require (slice file (B.length (U.mediaImageBytes after)) candidate == B.map complement (U.mediaImageBytes after))
      "candidate patch bytes differ"
  candidateSlot <- F.firstRecordSlotBytes (U.parsedParts reparsed)
  require (digest (B.map complement (B.take 0x4300 candidateSlot)) == outputRecord1Digest control) "candidate first-record digest differs"
  qualifyUnchanged reparsed candidate
  require (digest candidate == outputDatDigest control) "candidate DAT digest differs"
  pure (ControlArtifact candidate (ControlManifest control planned) parsed)

qualifyUnchanged :: U.Parsed -> B.ByteString -> Either String ()
qualifyUnchanged parsed bytes = do
  let parts = U.parsedParts parsed
      mainStart = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader parsed))
  require (digest (B.map complement (slice (mainStart + 0x50000) 0x4c80 bytes)) == record2Digest)
    "original record 2 differs"
  loader <- case filter ((== 1) . U.segId) (U.partSegments parts) of
    [segment] -> Right segment
    _ -> Left "source loader segment is not unique"
  require (U.segLoad loader == 0xd0000 && U.segSize loader >= 0x24 && U.segSize loader <= 0x20000)
    "original loader extent differs"
  let media = B.map complement (slice (mainStart + fromIntegral (U.segFlash loader)) (fromIntegral (U.segSize loader)) bytes)
  require (digest media == loaderDigest && U.word32At media 4 == 0xd0000 && U.word32At media 0xc == 0xd0020)
    "original loader B differs"

outsideEqual :: B.ByteString -> B.ByteString -> [(Int,Int)] -> Either String ()
outsideEqual source candidate extents = go 0 (sortOn fst extents)
  where
    go previous [] = require (B.drop previous source == B.drop previous candidate) "DAT suffix outside patches differs"
    go previous ((start,size):rest) = do
      require (start >= previous && size > 0 && start <= B.length source - size) "DAT patch extent is invalid"
      require (slice previous (start - previous) source == slice previous (start - previous) candidate)
        "DAT bytes outside patches differ"
      go (start + size) rest

slice :: Int -> Int -> B.ByteString -> B.ByteString
slice start size = B.take size . B.drop start

checkAll :: [U.Check] -> Either String ()
checkAll = mapM_ (\check -> require (U.checkPassed check) ("container contract: " ++ U.checkField check))

require :: Bool -> String -> Either String ()
require condition message = unless condition (Left message)
