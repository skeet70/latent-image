{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (IOException, try)
import Data.Aeson (Value, encode, object, (.=))
import Data.Bits (complement)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import FirstStageMediaReply (ByteCount (..), ModelReply (..), ModelStatus (..), parseModelReply)
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import System.Directory (createDirectory, doesFileExist)
import System.Environment (getArgs)
import System.Exit (ExitCode (..))
import System.FilePath ((</>))

newtype ModelExecutable = ModelExecutable FilePath
newtype FirmwareInput = FirmwareInput FilePath
newtype HeldImage = HeldImage FilePath
newtype OracleDirectory = OracleDirectory FilePath
newtype FileOffset = FileOffset Int

data Inputs = Inputs ModelExecutable FirmwareInput HeldImage OracleDirectory

parseInputs :: [String] -> IO Inputs
parseInputs arguments = case arguments of
  ["--model", model, "--firmware", firmware, "--expected", expected, "--output", directory] ->
    pure (Inputs (ModelExecutable model) (FirmwareInput firmware) (HeldImage expected) (OracleDirectory directory))
  _ -> H.failH "usage: first-stage-media-oracle --model MODEL --firmware DAT --expected HELD_IMAGE --output NEW_DIRECTORY"

sliceRecord :: B.ByteString -> FileOffset -> ByteCount -> B.ByteString
sliceRecord bytes (FileOffset offset) (ByteCount size) = B.map complement (B.take size (B.drop offset bytes))

expectedDigest :: String
expectedDigest = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"

writeJSON :: FilePath -> Value -> IO ()
writeJSON path = BL.writeFile path . encode

runOracle :: Inputs -> IO ()
runOracle (Inputs (ModelExecutable model) (FirmwareInput firmware) (HeldImage expected) (OracleDirectory directory)) = do
  firmwareBytes <- H.readRegular (64 * 1024 * 1024) firmware
  firmwareDigest <- H.sha256 firmwareBytes
  H.ensure (firmwareDigest == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490") "firmware is not held X-M1 1.50 stock"
  held <- H.readRegular (16 * 1024 * 1024) expected
  heldDigest <- H.sha256 held
  H.ensure (heldDigest == expectedDigest && B.length held == 2631680) "held decoded image identity mismatch"
  parsed <- either H.failH pure (U.parseContainer firmwareBytes)
  H.ensure (all U.checkPassed (U.checks parsed ++ U.carriedChecks parsed)) "stock container checks failed"
  let parts = U.parsedParts parsed
      header = U.recHeader (U.partRecord4 parts)
      storedLength = toInteger (U.recTotalLen header)
  segment <- case filter ((== 4) . U.segId) (U.partSegments parts) of
    [entry] -> pure entry
    _ -> H.failH "image 4 segment is not unique"
  let recordOffset = toInteger U.stagingLength + toInteger (U.hMainOffset (U.parsedHeader parsed)) + toInteger (U.segFlash segment)
  H.ensure (recordOffset >= 0 && storedLength >= 20 && recordOffset + storedLength <= toInteger (B.length firmwareBytes)) "compressed record extent outside container"
  H.ensure (storedLength <= 16 * 1024 * 1024 && fromIntegral (U.recDecodedSize header) == B.length held) "compressed record capacity mismatch"
  let record = sliceRecord firmwareBytes (FileOffset (fromInteger recordOffset)) (ByteCount (fromInteger storedLength))
      recordPath = directory </> "record4.media"
      decodedPath = directory </> "decoded.bin"
  H.ensure (record == B.map complement (U.serializeRecord (U.partRecord4 parts))) "actual media slice differs from parsed record"
  recordDigest <- H.sha256 record
  executable <- H.readRegular (32 * 1024 * 1024) model
  executableDigest <- H.sha256 executable
  createDirectory directory
  B.writeFile recordPath record
  B.writeFile (directory </> "expected-held.bin") held
  let arguments = [recordPath, show (B.length held), decodedPath]
      inputEvidence = object
        [ "schema" .= (1 :: Int), "firmware_path" .= firmware, "firmware_sha256" .= firmwareDigest
        , "firmware_bytes" .= B.length firmwareBytes, "record_file_offset" .= recordOffset
        , "record_bytes" .= B.length record, "record_sha256" .= recordDigest
        , "expected_path" .= expected, "expected_sha256" .= heldDigest, "expected_bytes" .= B.length held
        , "model_path" .= model, "model_sha256" .= executableDigest, "model_arguments" .= arguments
        , "feed_unit_bytes" .= (131072 :: Int), "first_unit_header_bytes" .= (20 :: Int)
        , "device_or_guest_execution" .= False, "cold_boot_acceptance" .= False
        ]
  B.writeFile (directory </> "model-executable.bin") executable
  writeJSON (directory </> "inputs.json") inputEvidence
  process <- try (H.runBounded 30000000 (1024 * 1024) model arguments B.empty) :: IO (Either IOException (Bool, ExitCode, B.ByteString, B.ByteString))
  case process of
    Left failure -> do
      writeJSON (directory </> "result.json") (object ["passed" .= False, "spawn_error" .= show failure, "inputs" .= inputEvidence])
      H.failH "model process could not start; preserved input evidence"
    Right (limited, exitCode, output, errors) -> do
      B.writeFile (directory </> "model.stdout") output
      B.writeFile (directory </> "model.stderr") errors
      present <- doesFileExist decodedPath
      decoded <- if present then Just <$> H.readRegular (16 * 1024 * 1024) decodedPath else pure Nothing
      decodedDigest <- traverse H.sha256 decoded
      let parsedReply = parseModelReply output :: Either String ModelReply
          replyPasses = case parsedReply of
            Right reply -> replyStatus reply == Decoded && replyRecordBytes reply == ByteCount (B.length record)
              && replyDecodedBytes reply == Just (ByteCount (B.length held)) && case replyFailure reply of Nothing -> True; Just _ -> False
            Left _ -> False
          bytesMatch = decoded == Just held
          passed = not limited && exitCode == ExitSuccess && replyPasses && bytesMatch
      writeJSON (directory </> "result.json") (object
        [ "schema" .= (1 :: Int), "passed" .= passed, "inputs" .= inputEvidence
        , "process_limit_reached" .= limited, "process_exit" .= show exitCode
        , "model_reply_valid" .= either (const False) (const True) parsedReply
        , "model_reply_error" .= either Just (const Nothing) parsedReply
        , "actual_output_bytes" .= fmap B.length decoded, "actual_output_sha256" .= decodedDigest
        , "complete_bytes_match" .= bytesMatch, "model_status_and_lengths_match" .= replyPasses
        , "failure_output_is_initialized_buffer_not_claimed_decoded_length" .= True
        , "cold_boot_acceptance" .= False
        ])
      H.ensure passed "owned Method4 full-stream comparison failed; preserved all model outputs"
      putStrLn (directory </> "result.json")

main :: IO ()
main = getArgs >>= parseInputs >>= runOracle
