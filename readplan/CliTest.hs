{-# LANGUAGE OverloadedStrings #-}
-- Exercises both tools as executed programs and validates every emitted plan against the shared
-- schema. Synthetic inputs only; no camera I/O.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (Value(..))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as M
import qualified Data.ByteString as B
import Data.List (isInfixOf)
import qualified Data.Text as T
import qualified Data.Text.Encoding as T
import System.Directory (createDirectory, createDirectoryIfMissing, doesPathExist, getCurrentDirectory, getTemporaryDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (ExitCode(ExitSuccess))
import System.FilePath ((</>))
import System.Posix.Files (setFileMode)
import System.Posix.Process (getProcessID)
import System.Process (readProcessWithExitCode)
import FujiAF.AdjustmentTables (CodeAddress(..), sha256Of, sha256Text)
import FujiAF.JSON (decodeStrictIntegers)
import FujiAF.ReadPlan
import FujiAF.Synthetic
import FujiAF.WindowCompare

assert :: String -> Bool -> IO ()
assert name condition = unless condition (error ("FAIL: " ++ name))

expect :: Either String a -> IO a
expect = either (error . ("FAIL: " ++)) pure

succeeds :: String -> FilePath -> [String] -> IO String
succeeds name program arguments = do
  (code, out, errors) <- readProcessWithExitCode program arguments ""
  assert (name ++ ": " ++ errors) (code == ExitSuccess)
  pure out

fails :: String -> FilePath -> [String] -> IO ()
fails name program arguments = do
  (code, _, _) <- readProcessWithExitCode program arguments ""
  assert (name ++ " was accepted") (code /= ExitSuccess)

privateDirectory :: FilePath -> IO FilePath
privateDirectory path = do
  createDirectory path
  setFileMode path 0o700
  pure path

writeBytes :: FilePath -> B.ByteString -> IO ()
writeBytes path bytes = B.writeFile path bytes >> setFileMode path 0o600

field :: Value -> String -> Value
field (Object fields) key = maybe (error ("FAIL: missing field " ++ key)) id (M.lookup (K.fromString key) fields)
field _ key = error ("FAIL: field " ++ key ++ " of a non-object")

stringOf :: Value -> String
stringOf (String value) = T.unpack value
stringOf other = error ("FAIL: expected a string, got " ++ show other)

sourceRoot :: IO FilePath
sourceRoot = do
  directory <- lookupEnv "TEST_SRCDIR"
  workspace <- lookupEnv "TEST_WORKSPACE"
  maybe getCurrentDirectory pure ((</>) <$> directory <*> workspace)

flipByte :: Int -> B.ByteString -> B.ByteString
flipByte position bytes =
  B.concat [B.take position bytes, B.singleton (B.index bytes position + 1), B.drop (position + 1) bytes]

main :: IO ()
main = do
  [generator, comparer, checker] <- getArgs
  temporary <- lookupEnv "TEST_TMPDIR" >>= maybe getTemporaryDirectory pure
  pid <- getProcessID
  let root = temporary </> ("readplan-cli-" ++ show pid)
  createDirectoryIfMissing True root
  setFileMode root 0o700
  images <- privateDirectory (root </> "images")
  plans <- privateDirectory (root </> "plans")
  let imagePath = images </> "synthetic-image.bin"
      CodeAddress base = syntheticBase
      imageArgument = imagePath ++ "@0x" ++ drop 2 (showOffset base)
  writeBytes imagePath syntheticImage
  generatorDigest <- sha256Text . sha256Of <$> B.readFile generator

  schemas <- sourceRoot
  let schema = schemas </> "schemas/read-plan.schema.json"
  forM_ [("rc0-bound", 4 :: Int, [imageArgument]), ("rc0-extent", 5, [imageArgument]), ("adjustment-42000", 1309, []), ("gate-byte", 4, [])] $
    \(scope, count, imageArguments) -> do
      let output = plans </> (scope ++ ".json")
      report <- succeeds ("generating " ++ scope) generator ([scope, "read-" ++ scope ++ "-v1", output] ++ imageArguments)
      assert (scope ++ " report states its window count") (("windows " ++ show count ++ " ") `isInfixOf` report)
      assert (scope ++ " report names the generator digest") (("generator sha256 " ++ generatorDigest) `isInfixOf` report)
      planBytes <- B.readFile output
      assert (scope ++ " report names the plan digest") (sha256Text (sha256Of planBytes) `isInfixOf` report)
      document <- expect (decodeStrictIntegers planBytes)
      windows <- expect (parsePlan document)
      assert (scope ++ " plan carries its windows") (length windows == count)
      assert (scope ++ " plan binds the generator executable")
        (stringOf (field (field document "generated_by") "sha256") == generatorDigest)
      unless (null imageArguments) $ do
        assert (scope ++ " report binds the image digest") (sha256Text (sha256Of syntheticImage) `isInfixOf` report)
        assert (scope ++ " plan basis binds the image digest")
          (sha256Text (sha256Of syntheticImage) `isInfixOf` stringOf (field document "firmware_basis"))
      _ <- succeeds (scope ++ " plan conforms to the shared schema") checker ["--schemafile", schema, output]
      pure ()

  fails "unknown scope" generator ["rc0", "read-x-v1", plans </> "unknown.json", imageArgument]
  fails "plan id with a space" generator ["rc0-bound", "read plan", plans </> "bad-id.json", imageArgument]
  fails "existing output" generator ["rc0-bound", "read-again-v1", plans </> "rc0-bound.json", imageArgument]
  fails "relative output" generator ["rc0-bound", "read-relative-v1", "plan-relative.json", imageArgument]
  fails "image without a load address" generator ["rc0-bound", "read-noaddress-v1", plans </> "noaddress.json", imagePath]
  fails "rc0 scope without an image" generator ["rc0-bound", "read-noimage-v1", plans </> "noimage.json"]
  fails "fixed-range scope given an image" generator ["adjustment-42000", "read-image-v1", plans </> "image.json", imageArgument]
  fails "no arguments" generator []
  shared <- privateDirectory (root </> "shared-plans")
  setFileMode shared 0o755
  fails "output directory readable by others" generator ["rc0-bound", "read-shared-v1", shared </> "plan.json", imageArgument]

  worktree <- privateDirectory (root </> "worktree")
  _ <- privateDirectory (worktree </> ".git")
  trackedPlans <- privateDirectory (worktree </> "plans")
  fails "output inside a Git working tree" generator ["rc0-bound", "read-tracked-v1", trackedPlans </> "plan.json", imageArgument]
  privateArea' <- privateDirectory (worktree </> "private-data")
  privatePlans <- privateDirectory (privateArea' </> "plans")
  _ <- succeeds "output under the working tree's private-data" generator
    ["rc0-bound", "read-private-v1", privatePlans </> "plan.json", imageArgument]
  writtenUnder <- doesPathExist (privatePlans </> "plan.json")
  assert "plan written under private-data" writtenUnder

  let plan = plans </> "rc0-bound.json"
      fallbackBytes' = B.pack [fromIntegral (position `mod` 251) | position <- [0 .. 0x3ff :: Int]]
      slice offset = B.take 256 (B.drop offset fallbackBytes')
  firmware <- privateDirectory (root </> "firmware")
  let payloadPath = firmware </> "payload.bin"
      fallbackArgument = payloadPath ++ "@0x10"
  writeBytes payloadPath (B.replicate 0x10 0xee <> fallbackBytes')
  captures <- privateDirectory (root </> "captures")
  writeBytes (captures </> "000100.bin") (slice 0x100)
  writeBytes (captures </> "000200.bin") (B.take 128 (slice 0x200))
  writeBytes (captures </> "000300.bin") (flipByte 0x0c (slice 0x300))
  reports <- privateDirectory (root </> "reports")

  let reportPath = reports </> "rc0-bound.txt"
  report <- succeeds "comparing captured windows" comparer [plan, captures, fallbackArgument, reportPath, imageArgument]
  forM_ [ "window 0x000100 match"
        , "window 0x000200 not-comparable captured window is 128 bytes"
        , "window 0x000300 differ 1 bytes from 0x00030c"
        , "window 0x00c000 not-comparable no capture file 00c000.bin"
        , "4 windows: 1 match, 1 differ, 2 not comparable"
        , "develop codes fed by differing bytes: 6"
        , sha256Text (sha256Of syntheticImage)
        ] $ \line -> assert ("comparison report line " ++ show line) (line `isInfixOf` report)
  written <- B.readFile reportPath
  assert "the report file holds what was printed" (T.decodeUtf8 written == T.pack report)

  fails "existing report" comparer [plan, captures, fallbackArgument, reportPath, imageArgument]
  fails "report inside a Git working tree" comparer
    [plan, captures, fallbackArgument, trackedPlans </> "report.txt", imageArgument]
  trackedCaptures <- privateDirectory (worktree </> "captures")
  writeBytes (trackedCaptures </> "000100.bin") (slice 0x100)
  fails "capture directory inside a Git working tree" comparer
    [plan, trackedCaptures, fallbackArgument, reports </> "tracked.txt", imageArgument]
  privateCaptures <- privateDirectory (privateArea' </> "captures")
  writeBytes (privateCaptures </> "000100.bin") (slice 0x100)
  _ <- succeeds "capture directory under the working tree's private-data" comparer
    [plan, privateCaptures, fallbackArgument, privatePlans </> "report.txt", imageArgument]
  sharedCaptures <- privateDirectory (root </> "shared-captures")
  setFileMode sharedCaptures 0o755
  fails "capture directory readable by others" comparer
    [plan, sharedCaptures, fallbackArgument, reports </> "shared.txt", imageArgument]
  fails "relative capture directory" comparer [plan, "captures", fallbackArgument, reports </> "relative.txt", imageArgument]
  fails "fallback payload without an offset" comparer [plan, captures, payloadPath, reports </> "nooffset.txt", imageArgument]
  fails "comparison without an image" comparer [plan, captures, fallbackArgument, reports </> "noimage.txt"]
  fails "no comparison arguments" comparer []

  putStrLn "PASS: both tools as executed programs, three scopes validated against the shared schema, generator and image binding, argument and private-path refusals, three comparison outcomes; synthetic inputs only, no camera I/O"
