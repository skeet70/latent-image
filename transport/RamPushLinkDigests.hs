-- Compares payloads' reference linked images with their review records, or
-- rewrites the records' `linked` digests from them.
--   ram-push-link-digests check RECORD BASE=LINKED BASE=LINKED
--   ram-push-link-digests repin (RECORD_NAME BASE=LINKED BASE=LINKED)...
-- `repin` rewrites transport/RECORD_NAME in the workspace; `bazel run` only.
module Main where

import Control.Monad (forM, forM_, unless)
import qualified Crypto.Hash.SHA256 as SHA256
import qualified Data.ByteString as B
import Data.Word (Word32)
import Numeric (readHex, showHex)
import System.Environment (getArgs, lookupEnv)
import System.Exit (die)
import System.FilePath ((</>))
import FujiAF.RamPushReview

-- | A reference base and the linked image built at it.
data LinkedFile = LinkedFile Word32 FilePath

linkedFile :: String -> Either String LinkedFile
linkedFile argument = case break (== '=') argument of
  ('0' : 'x' : digits, '=' : path) | not (null path), [(base, "")] <- readHex digits -> Right (LinkedFile base path)
  _ -> Left ("expected BASE=LINKED, not " ++ argument)

digestOf :: LinkedFile -> IO LinkedOutput
digestOf (LinkedFile base path) = do
  bytes <- B.readFile path
  let hex = concatMap (\byte -> let digits = showHex byte "" in replicate (2 - length digits) '0' ++ digits) (B.unpack (SHA256.hash bytes))
  maybe (die (path ++ ": digest is not SHA-256")) (pure . LinkedOutput base) (sha256Hex hex)

readRecord :: FilePath -> IO ReviewRecord
readRecord path = either (die . ((path ++ ": ") ++)) pure . parseReviewRecord =<< B.readFile path

builtOutputs :: [String] -> IO [LinkedOutput]
builtOutputs arguments = either die (mapM digestOf) (mapM linkedFile arguments)

groups :: [String] -> Either String [(String, [String])]
groups [] = Right []
groups (name : first : second : rest) = ((name, [first, second]) :) <$> groups rest
groups _ = Left "repin takes groups of RECORD_NAME BASE=LINKED BASE=LINKED"

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    "check" : record : linked -> do
      reviewRecord <- readRecord record
      built <- builtOutputs linked
      let findings = linkedFindings reviewRecord built
      forM_ findings putStrLn
      unless (null findings) $
        die "the reviewed payload's linked bytes changed: run `bazel run //transport:ram-push-link-repin` and commit the record; the changed payload needs a review before a push"
    "repin" : rest -> do
      workspace <- lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= maybe (die "run with bazel run") pure
      records <- either die pure (groups rest)
      updates <- forM records $ \(name, linked) -> do
        let path = workspace </> "transport" </> name
        reviewRecord <- readRecord path
        built <- builtOutputs linked
        updated <- either (die . ((path ++ ": ") ++)) pure (relinkOutputs built reviewRecord)
        pure (path, reviewRecord, updated)
      forM_ updates $ \(path, before, after) ->
        if recordLinked before == recordLinked after
          then putStrLn (path ++ ": unchanged")
          else B.writeFile path (renderReviewRecord after) >> putStrLn (path ++ ": linked digests rewritten")
    _ -> die "usage: ram-push-link-digests check RECORD BASE=LINKED BASE=LINKED | repin (RECORD_NAME BASE=LINKED BASE=LINKED)..."
