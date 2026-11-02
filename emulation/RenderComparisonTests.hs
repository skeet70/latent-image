{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (IOException, bracket, try)
import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), object, toJSON, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.Text as T
import qualified Data.Vector as V
import Emulation.RenderComparison (compareRenderReceipts)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.JSON (decodeStrictIntegers, encodeCanonical)
import Numeric (readHex)
import System.Directory (createDirectory, doesPathExist, getTemporaryDirectory, removeDirectoryRecursive, removeFile)
import System.FilePath (takeDirectory, (</>))
import System.IO (hClose, openTempFile)

check :: String -> Bool -> IO ()
check name condition = unless condition (ioError (userError name))

withTemporary :: (FilePath -> IO a) -> IO a
withTemporary action = bracket acquire removeDirectoryRecursive action
  where
    acquire = do
      base <- getTemporaryDirectory
      (path, handle) <- openTempFile base "render-comparison-"
      hClose handle
      removeFile path
      createDirectory path
      pure path

pngBytes :: B.ByteString
pngBytes = B.pack (hex "89504e470d0a1a0a0000000d4948445200000001000000010804000000b51c0c020000000b4944415478da6364f80f00010501012718e3660000000049454e44ae426082")
  where
    hex [] = []
    hex (a:b:rest) = case readHex [a,b] of
      [(byte, "")] -> byte : hex rest
      _ -> error "invalid fixture hex"
    hex _ = error "incomplete fixture hex"

planeBytes :: B.ByteString
planeBytes = B.pack [1,2,3,4]

binding :: FilePath -> B.ByteString -> Value
binding path bytes = object
  ["path" .= path, "sha256" .= sha256Hex (sha256 bytes), "bytes" .= B.length bytes]

set :: K.Key -> Value -> Value -> Value
set key value (Object fields) = Object (KM.insert key value fields)
set _ _ _ = error "fixture object required"

remove :: K.Key -> Value -> Value
remove key (Object fields) = Object (KM.delete key fields)
remove _ _ = error "fixture object required"

modify :: K.Key -> (Value -> Value) -> Value -> Value
modify key change value@(Object fields) = case KM.lookup key fields of
  Just previous -> set key (change previous) value
  Nothing -> error "fixture field required"
modify _ _ _ = error "fixture object required"

at :: [K.Key] -> Value -> Value
at [] value = value
at (key:rest) (Object fields) = maybe Null (at rest) (KM.lookup key fields)
at _ _ = Null

captures :: Value -> [Value]
captures result = case at ["captures"] result of
  Array entries -> V.toList entries
  _ -> []

metadata :: B.ByteString -> B.ByteString -> Integer -> Value
metadata png source clock = object
  ["schema" .= (1 :: Int), "node" .= ("scanout" :: T.Text), "instance" .= (1 :: Int), "instructions" .= clock
  ,"pixels" .= object ["width" .= (1 :: Int), "height" .= (1 :: Int), "rgb_bytes" .= (3 :: Int), "rgb_sha256" .= sha256Hex (sha256 (B.pack [1,2,3]))]
  ,"png" .= binding "capture.png" png, "source_bytes" .= binding "capture.planes.bin" source
  ,"source_planes" .= [object ["kind" .= ("main" :: T.Text), "native_source_address" .= (4096 :: Int), "offset" .= (0 :: Int), "bytes" .= B.length source]]
  ,"native_frame_state" .= object ["main" .= object ["source" .= ("0x1000" :: T.Text)], "ordered" .= [1,2 :: Int]]
  ,"native_scanout_state" .= object ["instances" .= [object ["instance" .= (1 :: Int), "register" .= (7 :: Int)]]]
  ]

fixture :: FilePath -> B.ByteString -> B.ByteString -> Integer -> (Value -> Value) -> ([Value] -> [Value]) -> IO FilePath
fixture directory png source clock changeMetadata changeCaptures = do
  createDirectory directory
  B.writeFile (directory </> "capture.png") png
  B.writeFile (directory </> "capture.planes.bin") source
  let rawMetadata = encodeCanonical (changeMetadata (metadata png source clock))
      capture label = object
        ["label" .= (label :: T.Text), "path" .= ("capture.png" :: FilePath), "sha256" .= sha256Hex (sha256 png)
        ,"scanout_evidence" .= object ["metadata" .= binding "capture.scanout.json" rawMetadata, "source_bytes" .= binding "capture.planes.bin" source]]
      receipt = object ["schema" .= (1 :: Int), "menu_receipts" .= changeCaptures [capture "first", capture "second"]]
      path = directory </> "e2e-result.json"
  B.writeFile (directory </> "capture.scanout.json") rawMetadata
  B.writeFile path (encodeCanonical receipt)
  pure path

readJSON :: FilePath -> IO Value
readJSON path = B.readFile path >>= either (ioError . userError) pure . decodeStrictIntegers

writeJSON :: FilePath -> Value -> IO ()
writeJSON path = B.writeFile path . encodeCanonical

withPair :: (FilePath -> FilePath -> FilePath -> IO a) -> IO a
withPair action = withTemporary $ \directory -> do
  before <- fixture (directory </> "before") pngBytes planeBytes 100 id id
  after <- fixture (directory </> "after") pngBytes planeBytes 125 id reverse
  action before after (directory </> "comparison.json")

refuses :: String -> IO Bool -> IO ()
refuses name action = do
  outcome <- try action :: IO (Either IOException Bool)
  check name (case outcome of Left _ -> True; Right _ -> False)

cleanCase :: IO ()
cleanCase = withPair $ \before after output -> do
  equal <- compareRenderReceipts before after output
  result <- readJSON output
  check "clean capture comparison" (equal && at ["equal"] result == Bool True && length (captures result) == 2)
  check "capture labels pair independently of receipt order" (map (at ["capture"]) (captures result) == map String ["first", "second"])
  forM_ (captures result) $ \entry -> do
    check "instruction clocks retained without equality gate" (at ["instructions"] entry == object ["reference" .= (100 :: Int), "candidate" .= (125 :: Int), "delta" .= (25 :: Int)])
    forM_ ["png_bytewise_equal", "source_planes_bytewise_equal", "native_frame_state_equal", "native_scanout_state_equal", "source_planes_equal", "pixels_equal", "node_equal", "instance_equal"] $ \key ->
      check "all clean equality results retained" (at [key] entry == Bool True)
    forM_ [("reference", before), ("candidate", after)] $ \(side, receipt) ->
      forM_ [("png", "capture.png"), ("source_bytes", "capture.planes.bin"), ("metadata", "capture.scanout.json")] $ \(kind, filename) -> do
        let path = takeDirectory receipt </> filename
        bytes <- B.readFile path
        check "every compared artifact has its exact binding" (at ["artifacts", side, kind] entry == binding path bytes)
  forM_ [("reference", before), ("candidate", after)] $ \(side, path) -> do
    bytes <- B.readFile path
    check "both receipts have their exact bindings" (at ["receipts", side] result == binding path bytes)

mismatchCase :: String -> K.Key -> B.ByteString -> B.ByteString -> (Value -> Value) -> IO ()
mismatchCase name key png source changeMetadata = withTemporary $ \directory -> do
  before <- fixture (directory </> "before") pngBytes planeBytes 100 id id
  after <- fixture (directory </> "after") png source 90 changeMetadata id
  let output = directory </> "comparison.json"
  equal <- compareRenderReceipts before after output
  result <- readJSON output
  check name (not equal && at ["equal"] result == Bool False && all ((== Bool False) . at [key]) (captures result) && length (captures result) == 2)
  check "mismatch result retains negative clock delta" (all ((== toJSON (-10 :: Int)) . at ["instructions", "delta"]) (captures result))

receiptMutation :: String -> (Value -> Value) -> IO ()
receiptMutation name change = withPair $ \before after output -> do
  readJSON after >>= writeJSON after . change
  refuses name (compareRenderReceipts before after output)
  doesPathExist output >>= check "invalid receipt does not publish a result" . not

metadataRefusal :: String -> (Value -> Value) -> IO ()
metadataRefusal name change = withTemporary $ \directory -> do
  before <- fixture (directory </> "before") pngBytes planeBytes 100 id id
  after <- fixture (directory </> "after") pngBytes planeBytes 100 change id
  let output = directory </> "comparison.json"
  refuses name (compareRenderReceipts before after output)
  doesPathExist output >>= check "invalid metadata does not publish a result" . not

mapReceipts :: (Value -> Value) -> Value -> Value
mapReceipts change = modify "menu_receipts" $ \value -> case value of
  Array entries -> Array (V.map change entries)
  _ -> error "fixture capture array required"

main :: IO ()
main = do
  cleanCase
  mismatchCase "PNG mutation" "png_bytewise_equal" (pngBytes <> B.singleton 0) planeBytes id
  mismatchCase "plane byte mutation" "source_planes_bytewise_equal" pngBytes (B.pack [1,2,3,5]) id
  mismatchCase "complete native frame state mutation" "native_frame_state_equal" pngBytes planeBytes
    (modify "native_frame_state" (set "extra_vendor_field" (Bool True)))
  mismatchCase "native frame array ordering mutation" "native_frame_state_equal" pngBytes planeBytes
    (modify "native_frame_state" (set "ordered" (toJSON [2,1 :: Int])))
  mismatchCase "complete native scanout state mutation" "native_scanout_state_equal" pngBytes planeBytes
    (modify "native_scanout_state" (set "extra_vendor_field" (Bool True)))
  mismatchCase "source span mutation" "source_planes_equal" pngBytes planeBytes
    (set "source_planes" (toJSON [object ["kind" .= ("main" :: T.Text), "native_source_address" .= (8192 :: Int), "offset" .= (0 :: Int), "bytes" .= (4 :: Int)]]))
  mismatchCase "pixel metadata mutation" "pixels_equal" pngBytes planeBytes
    (modify "pixels" (set "rgb_sha256" (toJSON (replicate 64 'a'))))
  mismatchCase "node metadata mutation" "node_equal" pngBytes planeBytes (set "node" (String "other-scanout"))
  mismatchCase "instance metadata mutation" "instance_equal" pngBytes planeBytes (set "instance" (toJSON (2 :: Int)))
  receiptMutation "missing capture" (modify "menu_receipts" (\value -> case value of
    Array entries -> Array (V.take 1 entries)
    _ -> error "fixture capture array required"))
  receiptMutation "no capture pairs" (set "menu_receipts" (toJSON ([] :: [Value])) )
  receiptMutation "duplicate label" (mapReceipts (set "label" (String "same")))
  receiptMutation "unmatched label" (mapReceipts (\entry -> if at ["label"] entry == String "first" then set "label" (String "unmatched") entry else entry))
  receiptMutation "missing capture list" (remove "menu_receipts")
  receiptMutation "bad PNG binding" (mapReceipts (set "sha256" (toJSON (replicate 64 '0'))))
  receiptMutation "bad PNG size" (mapReceipts (set "bytes" (toJSON (999 :: Int))))
  receiptMutation "bad metadata binding" (mapReceipts (modify "scanout_evidence" (modify "metadata" (set "sha256" (toJSON (replicate 64 '0'))))))
  receiptMutation "bad metadata size" (mapReceipts (modify "scanout_evidence" (modify "metadata" (set "bytes" (toJSON (1 :: Int))))))
  receiptMutation "bad source binding" (mapReceipts (modify "scanout_evidence" (modify "source_bytes" (set "sha256" (toJSON (replicate 64 '0'))))))
  receiptMutation "bad source size" (mapReceipts (modify "scanout_evidence" (modify "source_bytes" (set "bytes" (toJSON (1 :: Int))))))
  receiptMutation "null size is not an absent size" (mapReceipts (set "bytes" Null))
  receiptMutation "missing PNG digest" (mapReceipts (remove "sha256"))
  receiptMutation "missing scanout evidence" (mapReceipts (remove "scanout_evidence"))
  metadataRefusal "unsupported scanout schema" (set "schema" (toJSON (2 :: Int)))
  forM_ ["schema", "node", "instance", "instructions", "pixels", "png", "source_bytes", "source_planes", "native_frame_state", "native_scanout_state"] $ \key ->
    metadataRefusal ("missing required scanout field " ++ show key) (remove key)
  forM_ ["width", "height", "rgb_bytes", "rgb_sha256"] $ \key ->
    metadataRefusal ("missing required pixel field " ++ show key) (modify "pixels" (remove key))
  forM_ ["native_frame_state", "native_scanout_state"] $ \key -> forM_ [Null, object [], toJSON ([] :: [Value]), toJSON [1 :: Int]] $ \value ->
    metadataRefusal "native state requires nonempty object envelope" (set key value)
  metadataRefusal "empty source planes" (set "source_planes" (toJSON ([] :: [Value])))
  forM_ ["kind", "native_source_address", "offset", "bytes"] $ \key ->
    metadataRefusal ("missing required source plane field " ++ show key)
      (modify "source_planes" (\value -> case value of
        Array entries -> Array (V.map (remove key) entries)
        _ -> error "fixture source plane array required"))
  metadataRefusal "bad nested PNG binding" (modify "png" (set "sha256" (toJSON (replicate 64 '0'))))
  metadataRefusal "bad nested source binding" (modify "source_bytes" (set "bytes" (toJSON (99 :: Int))))
  metadataRefusal "invalid pixel extent" (modify "pixels" (set "rgb_bytes" (toJSON (2 :: Int))))
  metadataRefusal "incomplete source plane extent" (set "source_planes" (toJSON [object ["kind" .= ("main" :: T.Text), "native_source_address" .= (4096 :: Int), "offset" .= (0 :: Int), "bytes" .= (3 :: Int)]]))
  withPair $ \before after output -> do
    B.writeFile output "preserve existing output\n"
    refuses "preexisting output is refused" (compareRenderReceipts before after output)
    B.readFile output >>= check "preexisting output preserved bytewise" . (== "preserve existing output\n")
  withPair $ \before after output -> do
    _ <- compareRenderReceipts before after output
    original <- B.readFile output
    refuses "comparison output cannot be overwritten" (compareRenderReceipts before after output)
    B.readFile output >>= check "published result preserved" . (== original)
  withPair $ \before after output -> do
    original <- B.readFile after
    B.writeFile after ("{\"schema\":1," <> B.drop 1 original)
    refuses "duplicate JSON keys refused" (compareRenderReceipts before after output)
  withPair $ \before after output -> do
    removeFile (takeDirectory after </> "capture.png")
    refuses "missing referenced artifact refused" (compareRenderReceipts before after output)
  putStrLn "render comparison behavioral cases passed"
