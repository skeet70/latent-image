{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM_, unless, (>=>))
import qualified Crypto.Hash.SHA256 as SHA256
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson.KeyMap as M
import Data.Bits (complement, xor)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (sortOn)
import qualified Data.Vector as V
import Data.Word (Word32)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.JSON as J
import FujiAF.PeerWaitControl
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)
import System.Environment (getArgs)

assert :: String -> Bool -> IO ()
assert label condition = unless condition (fail label)

right :: Either String a -> IO a
right = either fail pure

refuses :: String -> Either String a -> IO ()
refuses label result = case result of Left _ -> pure (); Right _ -> fail label

hash :: B.ByteString -> String
hash = concatMap (\byte -> let text = showHex byte "" in replicate (2 - length text) '0' ++ text) . B.unpack . SHA256.hash

veneerWords :: [Word32]
veneerWords =
  [ 0xe92d500f,0xe10f0000,0xe3021000,0xe3443002,0xe300c22b,0xe5932004
  , 0xe3520000,0x0a00000a,0xe152000c,0x1a000005,0xe3a02064,0xe320f000
  , 0xe2522001,0x1afffffc,0xe2511001,0x1afffff4,0xe128f000,0xe8bd500f
  , 0xeafffffe,0xe3011000,0xe2511001,0x1afffffd,0xe128f000,0xe8bd500f
  , 0xe3443002,0xea0007a3
  ]

fixture :: PeerControl -> Value
fixture control = object
  [ "schema" .= (1 :: Int), "control" .= controlName control, "argument" .= (0x69cdf8 :: Word32)
  , "source_dat_sha256" .= text "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"
  , "source_record1_sha256" .= text "c2da900dcd3f193b05b5ae968dfcb87723571462eafd84c0fed8467f66f7dff3"
  , "output_dat_sha256" .= expectedDatHash control
  , "output_record1_sha256" .= expectedRecordHash control
  , "record2_sha256" .= text "2f1bf3f0e483b892d2890599679c1b515cfc1be8b8c27cbb06d4d913e428abb4"
  , "loader_b_sha256" .= text "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"
  , "original_checksum" .= (2399842703 :: Word32)
  , "output_checksum" .= (case control of HeldSentinel -> 2399842050; DelayedRelease -> 2399857611 :: Word32)
  , "checksum_dat_extent" .= ([524,528] :: [Int]), "all_other_dat_bytes_equal" .= True
  , "patches" .= case control of
      HeldSentinel -> [gate 0xeafffffe "db8c2881cbdfd6291ff7bdd5a0a304a850d07de94cb60566880df7389a38f1d2"]
      DelayedRelease ->
        [ patch 0x100 (replicate 26 0xffffffff) veneerWords
            "f71c209c8df8eeb07005a11966d0300bd86d153006ba2e05302caf822246d90f"
            "0ad6032e1b93cac434a9ae1f502cd7b69b701dcc89dca7dfedb885052c116817"
        , gate 0xeafff841 "c8dfdb85444331878649a47f27fee928b8337ff4ec20a495d9e54166f39ed0bf"
        ]
  , "limits" .= object
      [ "budget" .= n 10000000, "prefix_to_gate" .= n 4000000
      , "held_gate_to_four_loads" .= n 4000000, "delayed_gate_to_clear" .= n 5100000
      , "clear_microsteps" .= n 2048, "clear_to_loader" .= n 20000
      , "wait_stops" .= n 4, "peer_wait_reads" .= n 128, "watch_rows" .= n 12288
      , "collector_rows" .= n 16384, "receipt_bytes" .= n 16777216
      , "host_seconds" .= n 120, "stream_bytes" .= n 1048576, "poll_reads" .= n 8192
      , "backoff_iterations" .= n 100, "release_iterations" .= n 4096
      ]
  ]
  where
    n :: Int -> Int
    n = id
    text :: String -> String
    text = id
    gate word = patch 0x1ff4 [0xe3443002] [word] "e7db7baed22eab40807042850fd46fb733e4bd813d725065948a9cdb0a88aee1"
    patch :: Int -> [Word32] -> [Word32] -> String -> String -> Value
    patch start before after beforeHash afterHash = object
      [ "slot_start" .= start, "slot_end" .= (start + length before * 4)
      , "guest_start" .= start, "guest_end" .= (start + length before * 4)
      , "dat_start" .= (0x4025c + start), "dat_end" .= (0x4025c + start + length before * 4)
      , "expected_media_words" .= before, "replacement_media_words" .= after
      , "expected_media_bytes" .= B.unpack (B.concat (map U.putWord32 before))
      , "replacement_media_bytes" .= B.unpack (B.concat (map U.putWord32 after))
      , "expected_media_sha256" .= beforeHash, "replacement_media_sha256" .= afterHash
      ]

expectedDatHash, expectedRecordHash :: PeerControl -> String
expectedDatHash HeldSentinel = "d5520aaa4842ce4546bbf21e930d83e9ef7a5372535a0f670f09c088c09dfedb"
expectedDatHash DelayedRelease = "e45d49bdbc874084d5bfa39b3c5b619db948309a1cef2acd9ca16283f07c4db9"
expectedRecordHash HeldSentinel = "863ea4eaa00e391bacef2b9d312f61cc2450fb726d7d2856812a432945bebaa0"
expectedRecordHash DelayedRelease = "e90a785cd85c9e3c3dd43593333d1b94c3d49ebe94e0a21ffd4d806464e1eabb"

mutants :: Value -> [Value]
mutants (Object fields) =
  Object (M.insert "unexpected" Null fields)
    : [Object (M.delete key fields) | key <- M.keys fields]
    ++ [Object (M.insert key changed fields) | (key,value) <- M.toList fields, changed <- mutants value]
mutants (Array values) = Array (V.snoc values Null)
  : [Array (values V.// [(index,changed)]) | (index,value) <- zip [0..] (V.toList values), changed <- mutants value]
mutants (Number value) = [Number (value + 1),String "invalid"]
mutants (String _) = [String "altered",Null]
mutants (Bool value) = [Bool (not value)]
mutants Null = [Bool False]

replaceByte :: Int -> B.ByteString -> B.ByteString
replaceByte index bytes = B.take index bytes <> B.singleton (B.index bytes index `xor` 1) <> B.drop (index + 1) bytes

manifestTests :: IO ()
manifestTests = forM_ [HeldSentinel,DelayedRelease] $ \control -> do
  manifest <- right (parseManifest (J.encodeCanonical (fixture control)))
  assert "manifest control roundtrip" (manifestControl manifest == control)
  assert "manifest argument binding" (argumentWord (manifestArgument manifest) == 0x69cdf8)
  assert "manifest canonical roundtrip" (manifestValue manifest == fixture control)
  forM_ (mutants (fixture control)) $ \value -> refuses "altered closed manifest accepted" (parseManifest (J.encodeCanonical value))
  let encoded = J.encodeCanonical (fixture control)
  refuses "duplicate manifest key accepted" (parseManifest ("{\"schema\":1," <> B.drop 1 encoded))
  let (prefix,suffix) = C.breakSubstring "\"schema\":1" encoded
  forM_ ["1.0","1e0"] $ \spelling ->
    refuses "noninteger manifest schema accepted"
      (parseManifest (prefix <> "\"schema\":" <> spelling <> B.drop 10 suffix))
  refuses "oversized manifest accepted" (parseManifest (B.replicate 65537 32))

linkedTests :: B.ByteString -> B.ByteString -> IO ()
linkedTests bytes symbols = do
  right (validateControlVeneer bytes symbols)
  assert "linked exact veneer words" (bytes == B.concat (map U.putWord32 veneerWords))
  assert "linked veneer digest" (hash bytes == "0ad6032e1b93cac434a9ae1f502cd7b69b701dcc89dca7dfedb885052c116817")
  forM_ [0 .. B.length bytes - 1] $ \index -> refuses "altered linked veneer accepted" (validateControlVeneer (replaceByte index bytes) symbols)
  forM_ [B.take 100 bytes,bytes <> B.replicate 4 0,B.replicate 0x100 0 <> bytes] $ \bad ->
    refuses "wrong linked extent accepted" (validateControlVeneer bad symbols)
  rows <- right (F.parseSymbols symbols)
  forM_ ["xm1_peer_wait_control_entry","xm1_peer_wait_control_poll","xm1_peer_wait_control_failure"
        ,"xm1_peer_wait_control_acknowledged","xm1_peer_wait_control_end","xm1_peer_wait_original_clear"] $ \name -> do
    row <- right (F.lookupSymbol rows name)
    let remaining = filter (not . C.isSuffixOf (C.pack name)) (C.lines symbols)
        wrong = C.pack (showHex (F.symbolAddress row + 4) "" ++ " " ++ showHex (F.symbolSize row) "" ++ " T " ++ name)
    refuses "missing linked site accepted" (validateControlVeneer bytes (C.unlines remaining))
    refuses "shifted linked site accepted" (validateControlVeneer bytes (C.unlines (wrong:remaining)))
    refuses "duplicate linked site accepted" (validateControlVeneer bytes (symbols <> C.unlines [wrong]))

artifactTests :: B.ByteString -> IO ()
artifactTests source = do
  sourceParsed <- right (U.parseContainer source)
  forM_ [HeldSentinel,DelayedRelease] $ \control -> do
    artifact <- right (buildControl control source)
    assert "qualified source exposed" (controlSource artifact == sourceParsed)
    let candidate = controlDat artifact
        manifest = controlManifest artifact
        extents = (524,528) : (270928,270932) : case control of HeldSentinel -> []; DelayedRelease -> [(263004,263108)]
    assert "fixed candidate DAT hash" (hash candidate == expectedDatHash control)
    assert "source length preserved" (B.length candidate == B.length source)
    assert "manifest exact artifact binding" (manifestValue manifest == fixture control)
    parsedManifest <- right (parseManifest (J.encodeCanonical (manifestValue manifest)))
    assert "artifact manifest roundtrip" (manifestControl parsedManifest == control)
    parsed <- right (U.parseContainer candidate)
    assert "candidate checks" (all U.checkPassed (U.checks parsed ++ U.carriedChecks parsed))
    slot <- right (F.firstRecordSlotBytes (U.parsedParts parsed))
    assert "fixed candidate record1 hash" (hash (B.map complement (B.take 0x4300 slot)) == expectedRecordHash control)
    let unchanged previous [] = B.drop previous source == B.drop previous candidate
        unchanged previous ((start,end):rest) =
          B.take (start - previous) (B.drop previous source) == B.take (start - previous) (B.drop previous candidate)
            && unchanged end rest
    assert "all other DAT bytes equal" (unchanged 0 (sortOn fst extents))
    assert "all non-pretable parts preserved" (all (\difference -> U.diffPart difference == "main pre-table" || U.diffChangedBytes difference == 0) (U.diffParts sourceParsed parsed))
    refuses "control used as source" (buildControl control candidate)
    forM_ [0,524,263004,270928,0x5025c,B.length source - 1] $ \index ->
      refuses "modified original source accepted" (buildControl control (replaceByte index source))
    refuses "truncated original source accepted" (buildControl control (B.init source))
    refuses "appended original source accepted" (buildControl control (source <> B.singleton 0))

main :: IO ()
main = do
  arguments <- getArgs
  (imagePath,symbolPath,sourcePaths) <- case arguments of
    [image,symbols] -> pure (image,symbols,[])
    [image,symbols,source] -> pure (image,symbols,[source])
    _ -> fail "expected linked image, symbols, and optional original DAT"
  image <- B.readFile imagePath
  symbols <- B.readFile symbolPath
  linkedTests image symbols
  manifestTests
  forM_ [HeldSentinel,DelayedRelease] $ \control -> do
    assert "control name roundtrip" (parseControlName (controlName control) == Right control)
    forM_ [B.empty,B.replicate 0x4300 0xff,B.replicate 0x50000 0] $ \source ->
      refuses "unqualified source accepted" (buildControl control source)
  refuses "unknown control name accepted" (parseControlName "natural")
  forM_ sourcePaths (B.readFile >=> artifactTests)
  putStrLn "peer wait control artifact tests pass"
