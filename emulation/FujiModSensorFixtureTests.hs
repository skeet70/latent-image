{-# LANGUAGE OverloadedStrings #-}
-- | Behavioural checks for fujimod-xm1-sensor-fixture through its CLI.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), decodeStrict)
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C8
import qualified Data.Text as T
import qualified Data.Vector as V
import System.Directory (createDirectory, doesPathExist, getTemporaryDirectory, makeAbsolute, removeDirectoryRecursive, removeFile)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>))
import System.IO (hClose, openTempFile)
import System.Process (proc, readCreateProcessWithExitCode)

check :: String -> Bool -> IO ()
check label passed = unless passed (putStrLn ("FAIL: " ++ label) >> exitFailure)

require :: String -> Maybe a -> IO a
require label = maybe (putStrLn ("FAIL: " ++ label) >> exitFailure) pure

decodedObject :: String -> B.ByteString -> IO (KM.KeyMap Value)
decodedObject label bytes = require (label ++ " decodes as a JSON object") $ case decodeStrict bytes of
  Just (Object o) -> Just o
  _ -> Nothing

-- | A base run document whose input clock has the given mode; the extra
-- options text, if any, ends with a comma.
runDocument :: B.ByteString -> B.ByteString -> B.ByteString
runDocument clockMode extraOptions =
  "{\"devices\":{\"nodes\":[{\"name\":\"image_input\",\"model\":{\"kind\":\"image-input\",\"sensor\":null,\
  \\"frame_period\":null,\"raw_irq\":{\"core\":0,\"line\":36},\"options\":{"
    <> extraOptions
    <> "\"sensor_period_ticks\":null,\
       \\"clock\":{\"mode\":\""
    <> clockMode
    <> "\",\"source\":{\"kind\":\"periodic\",\"period_ticks\":400000}},\
       \\"cfa\":[0,1,2,0,1,2,0,1,2,0,1,2,0,1,2,0,1,2,0,1,2,0,1,2,0,1,2,0,1,2,0,1,2,0,1,2]}}}]},\
       \\"scene\":{},\"generated_assets\":[]}"

base :: B.ByteString
base = runDocument "input_enable" ""

fixtureDeclaring :: B.ByteString -> Int -> Int -> B.ByteString
fixtureDeclaring schema width height =
  "{" <> schema <> ",\"id\":\"test\",\"image_input\":\"image_input\",\"frames\":\"frames\",\"asset\":\"asset\",\
  \\"raster\":{\"width\":" <> C8.pack (show width) <> ",\"height\":" <> C8.pack (show height) <> "},\
  \\"scene\":\"raw16-index-ramp12\",\"sensor_period_ticks\":400000,\
  \\"response\":{\"black\":0,\"full_scale\":4095,\"reference_white\":[4095,4095,4095]}}"

fixture :: Int -> Int -> B.ByteString
fixture = fixtureDeclaring "\"schema\":1"

readbackFixture :: B.ByteString -> B.ByteString
readbackFixture law = fixtureDeclaring ("\"schema\":2,\"enable_readback\":\"" <> law <> "\"") 4096 2

-- | The options of the single image_input node, and the document with them
-- replaced.
imageInputOptions :: KM.KeyMap Value -> Maybe (KM.KeyMap Value, KM.KeyMap Value -> KM.KeyMap Value)
imageInputOptions document = do
  Object devices <- KM.lookup "devices" document
  Array nodes <- KM.lookup "nodes" devices
  [index] <- Just (V.toList (V.findIndices named nodes))
  Object node <- Just (nodes V.! index)
  Object model <- KM.lookup "model" node
  Object options <- KM.lookup "options" model
  let replace options' =
        let node' = KM.insert "model" (Object (KM.insert "options" (Object options') model)) node
         in KM.insert "devices" (Object (KM.insert "nodes" (Array (nodes V.// [(index, Object node')])) devices)) document
  pure (options, replace)
 where
  named (Object o) = KM.lookup "name" o == Just (String "image_input")
  named _ = False

main :: IO ()
main = do
  [toolArg] <- getArgs
  tool <- makeAbsolute toolArg
  temporary <- getTemporaryDirectory
  (root, handle) <- openTempFile temporary "sensor-fixture-test"
  hClose handle
  removeFile root
  createDirectory root
  let run = root </> "base.json"
      valid = root </> "valid.json"
      oversized = root </> "oversized.json"
      derived = root </> "derived.json"
      altered = root </> "altered.json"
      refused = root </> "refused.json"
      write runPath fixturePath output = readCreateProcessWithExitCode (proc tool ["write", "--run", runPath, "--fixture", fixturePath, "--output", output]) ""
      refuses label runPath fixturePath = do
        (code, out, err) <- write runPath fixturePath refused
        written <- doesPathExist refused
        check (label ++ ": " ++ err) (code /= ExitSuccess && null out && not written)
  B.writeFile run base
  B.writeFile valid (fixture 4096 2)
  B.writeFile oversized (fixture 4611686018427387904 2)
  (code, out, err) <- readCreateProcessWithExitCode (proc tool ["write", "--run", run, "--fixture", valid, "--output", derived]) ""
  check ("a 4096 x 2 raster derives: " ++ err) (code == ExitSuccess)
  (verified, verifiedOut, verifyErr) <- readCreateProcessWithExitCode (proc tool ["verify", "--run", run, "--fixture", valid, "--document", derived]) ""
  check ("the derived document verifies: " ++ verifyErr) (verified == ExitSuccess && verifiedOut == out)
  bytes <- B.readFile derived
  B.writeFile altered (B.snoc bytes 10)
  (mismatch, mismatchOut, mismatchErr) <- readCreateProcessWithExitCode (proc tool ["verify", "--run", run, "--fixture", valid, "--document", altered]) ""
  check ("a document differing by one byte is refused: " ++ mismatchErr) (mismatch /= ExitSuccess && null mismatchOut)
  (overflow, overflowOut, overflowErr) <- readCreateProcessWithExitCode (proc tool ["write", "--run", run, "--fixture", oversized, "--output", refused]) ""
  written <- doesPathExist refused
  check ("a raster whose byte count overflows Int is refused: " ++ overflowErr) (overflow /= ExitSuccess && null overflowOut && not written)

  schema1Document <- decodedObject "the schema-1 document" bytes
  (schema1Options, _) <- require "the schema-1 document has image_input options" (imageInputOptions schema1Document)
  check "the schema-1 options have no enable_readback" (not (KM.member "enable_readback" schema1Options))
  schema1Record <- decodedObject "the schema-1 identity record" (C8.pack out)
  check "the schema-1 identity record has no enable_readback" (not (KM.member "enable_readback" schema1Record))
  forM_ ["identity", "exchanged_source_bits"] $ \law -> do
    let lawFixture = root </> ("readback-" ++ C8.unpack law ++ ".json")
        lawDocument = root </> ("derived-" ++ C8.unpack law ++ ".json")
        lawValue = String (T.pack (C8.unpack law))
        named label = "the " ++ C8.unpack law ++ " " ++ label
    B.writeFile lawFixture (readbackFixture law)
    (lawCode, lawOut, lawErr) <- write run lawFixture lawDocument
    check (named "fixture derives: " ++ lawErr) (lawCode == ExitSuccess)
    document <- B.readFile lawDocument >>= decodedObject (named "document")
    (options, replaceOptions) <- require (named "document has image_input options") (imageInputOptions document)
    check (named "options carry enable_readback") (KM.lookup "enable_readback" options == Just lawValue)
    check (named "document otherwise equals the schema-1 document") (replaceOptions (KM.delete "enable_readback" options) == schema1Document)
    record <- decodedObject (named "identity record") (C8.pack lawOut)
    check (named "identity record carries the law") (KM.lookup "enable_readback" record == Just lawValue)

  let exchanged = root </> "readback-exchanged_source_bits.json"
      unknownLaw = root </> "unknown-law.json"
      missingLaw = root </> "missing-law.json"
      heldKey = root </> "held-key-base.json"
      sourceMask = root </> "source-mask-base.json"
  B.writeFile unknownLaw (readbackFixture "exchanged_all_bits")
  B.writeFile missingLaw (fixtureDeclaring "\"schema\":2" 4096 2)
  B.writeFile heldKey (runDocument "input_enable" "\"enable_readback\":\"identity\",")
  B.writeFile sourceMask (runDocument "source_mask" "")
  refuses "an unknown enable_readback law is refused" run unknownLaw
  refuses "a schema-2 fixture without enable_readback is refused" run missingLaw
  refuses "a base whose options already hold enable_readback is refused" heldKey exchanged
  refuses "a base whose input clock is not input_enable is refused" sourceMask exchanged
  removeDirectoryRecursive root
  putStrLn "fujimod-xm1-sensor-fixture: checks passed"
