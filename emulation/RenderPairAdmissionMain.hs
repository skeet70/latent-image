{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | @render-pair-admission@ verifies the input identity of a render-pair role and the recorded
-- identity of its completed receipt, and imports a review-packet-bound stock receipt.
module Main (main) where

import qualified Data.Aeson as J
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.Types as J
import qualified Data.ByteString.Lazy as BL
import Data.List (stripPrefix)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Data.Word (Word32)
import Emulation.RenderPairAdmission
import Numeric (readHex)
import System.Directory (doesDirectoryExist)
import System.Environment (getArgs)
import System.Exit (exitFailure, exitSuccess)
import System.FilePath ((</>))
import System.IO (hPutStrLn, stderr)
import Text.Read (readMaybe)

data Options = Options
  { optionName :: Maybe String
  , optionRole :: Maybe String
  , optionSourceCommit :: Maybe String
  , optionHeadCommit :: Maybe String
  , optionSdk :: Maybe FilePath
  , optionFujiModPaths :: Maybe FilePath
  , optionTrajectory :: Maybe FilePath
  , optionTrajectorySha :: Maybe String
  , optionFirmware :: Maybe FilePath
  , optionNand :: Maybe FilePath
  , optionBootRom :: Maybe FilePath
  , optionRunDocument :: Maybe FilePath
  , optionArchive :: Maybe FilePath
  , optionArchiveSha :: Maybe String
  , optionSourceIdentity :: Maybe FilePath
  , optionChecker :: Maybe FilePath
  , optionPrelinked :: Maybe FilePath
  , optionPrelinkedBase :: Maybe Word32
  , optionPrelinkedSize :: Maybe Word32
  , optionBindAdmission :: Maybe FilePath
  , optionReceipt :: Maybe FilePath
  , optionPacket :: Maybe FilePath
  , optionDestination :: Maybe FilePath
  }

emptyOptions :: Options
emptyOptions = Options Nothing Nothing Nothing Nothing Nothing Nothing Nothing Nothing Nothing Nothing
  Nothing Nothing Nothing Nothing Nothing Nothing Nothing Nothing Nothing Nothing Nothing Nothing Nothing

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    command : rest -> case command of
      "inputs" -> withExpected rest $ \expected -> verifyInputs expected >>= reportInputs expected
      "verify" -> withExpected rest $ \expected -> withReceipt rest $ \directory ->
        verifyReceipt expected directory >>= report "reuse" expected
      "bind" -> withExpected rest $ \expected -> withReceipt rest $ \directory ->
        bindReceipt expected directory GuestRun >>= report "bind" expected
      "import" -> withExpected rest $ \expected -> withReceipt rest $ \directory -> withPacket rest $ \packet ->
        withDestination rest $ \destination -> importReceipt expected packet directory destination >>= report "import" expected
      _ -> usage
    [] -> usage

reportInputs :: Expected -> Either String () -> IO ()
reportInputs expected result = case result of
  Left problem -> do
    hPutStrLn stderr ("render-pair-admission: " ++ problem)
    exitFailure
  Right () -> do
    putStrLn (jsonLine
      [ ("result", J.String "inputs")
      , ("name", J.String (expectedName expected))
      , ("role", J.String (roleText (expectedRole expected)))
      , ("trajectory_sha256", J.String (digestText (expectedTrajectorySha expected)))
      , ("archive_sha256", maybe J.Null (J.String . digestText) (expectedArchiveSha expected))
      ])
    exitSuccess

report :: String -> Expected -> Either String ReceiptIdentity -> IO ()
report operation expected result = case result of
  Left problem -> do
    hPutStrLn stderr ("render-pair-admission: " ++ problem)
    exitFailure
  Right identity -> do
    putStrLn (jsonLine
      [ ("result", J.String (T.pack operation))
      , ("name", J.String (expectedName expected))
      , ("role", J.String (roleText (expectedRole expected)))
      , ("trajectory_sha256", J.String (digestText (expectedTrajectorySha expected)))
      , ("archive_sha256", maybe J.Null (J.String . digestText) (expectedArchiveSha expected))
      , ("checkpoint", J.String (checkpointText (identityCheckpoint identity)))
      , ("kernel_elf_sha256", J.String (digestText (identityKernelElf identity)))
      , ("observation_count", J.toJSON (identityObservationCount identity))
      , ("phases", J.toJSON (identityPhases identity))
      ])
    exitSuccess

jsonLine :: [(String, J.Value)] -> String
jsonLine fields = T.unpack (TE.decodeUtf8 (BL.toStrict (J.encode (J.object [K.fromString key J..= value | (key, value) <- fields]))))

withExpected :: [String] -> (Expected -> IO ()) -> IO ()
withExpected arguments action = case parseOptions arguments of
  Left problem -> refuse problem
  Right options -> do
    expected <- expectedFrom options
    case expected of
      Left problem -> refuse problem
      Right value -> action value

expectedFrom :: Options -> IO (Either String Expected)
expectedFrom options = do
  pins <- traverse readPins (optionBindAdmission options)
  pure $ do
    name <- required "name" (optionName options)
    roleText' <- required "role" (optionRole options)
    role <- parseRoleText roleText'
    source <- required "source-commit" (optionSourceCommit options)
    headCommit <- required "head-commit" (optionHeadCommit options)
    sdk <- required "sdk" (optionSdk options)
    paths <- required "fujimod-paths" (optionFujiModPaths options)
    trajectory <- required "trajectory" (optionTrajectory options)
    trajectorySha <- digest "trajectory-sha256" (optionTrajectorySha options)
    firmware <- required "firmware" (optionFirmware options)
    nand <- required "nand" (optionNand options)
    bootRom <- required "boot-rom" (optionBootRom options)
    runDocument <- required "run-document" (optionRunDocument options)
    archiveSha <- traverse (digestValue "archive-sha256") (optionArchiveSha options)
    pure Expected
      { expectedName = T.pack name
      , expectedRole = role
      , expectedSourceCommit = CommitId (T.pack source)
      , expectedHeadCommit = CommitId (T.pack headCommit)
      , expectedSdk = sdk
      , expectedFujiModPaths = paths
      , expectedTrajectory = trajectory
      , expectedTrajectorySha = trajectorySha
      , expectedFirmware = firmware
      , expectedNand = nand
      , expectedBootRom = bootRom
      , expectedRunDocument = runDocument
      , expectedArchive = optionArchive options
      , expectedArchiveSha = archiveSha
      , expectedSourceIdentity = optionSourceIdentity options
      , expectedChecker = optionChecker options
      , expectedPrelinked = optionPrelinked options
      , expectedPrelinkedBase = optionPrelinkedBase options
      , expectedPrelinkedSize = optionPrelinkedSize options
      , expectedPins = pins
      }
 where
  required label = maybe (Left ("missing --" ++ label)) Right
  digest label = maybe (Left ("missing --" ++ label)) (either Left Right . parseDigestText)
  digestValue :: String -> String -> Either String Digest
  digestValue label value = either (Left . ((label ++ ": ") ++)) Right (parseDigestText value)
  parseDigestText :: String -> Either String Digest
  parseDigestText text = J.parseEither J.parseJSON (J.String (T.pack text))
  parseRoleText :: String -> Either String Role
  parseRoleText text = J.parseEither J.parseJSON (J.String (T.pack text))

readPins :: FilePath -> IO InputPins
readPins path = do
  directory <- doesDirectoryExist path
  let recordPath = if directory then path </> "admission.json" else path
  bytes <- BL.readFile recordPath
  case J.eitherDecode bytes of
    Left problem -> ioError (userError ("admission record: " ++ problem))
    Right value -> case J.parseEither (J.withObject "admission" (\fields -> fields J..: "inputs")) value of
      Left problem -> ioError (userError ("admission inputs: " ++ problem))
      Right pins -> pure pins

withReceipt :: [String] -> (FilePath -> IO ()) -> IO ()
withReceipt arguments action = case parseOptions arguments of
  Left problem -> refuse problem
  Right options -> maybe (refuse "missing --receipt") action (optionReceipt options)

withPacket :: [String] -> (FilePath -> IO ()) -> IO ()
withPacket arguments action = case parseOptions arguments of
  Left problem -> refuse problem
  Right options -> maybe (refuse "missing --packet") action (optionPacket options)

withDestination :: [String] -> (FilePath -> IO ()) -> IO ()
withDestination arguments action = case parseOptions arguments of
  Left problem -> refuse problem
  Right options -> maybe (refuse "missing --destination") action (optionDestination options)

refuse :: String -> IO a
refuse problem = do
  hPutStrLn stderr ("render-pair-admission: " ++ problem)
  exitFailure

usage :: IO a
usage = do
  hPutStrLn stderr "usage: render-pair-admission inputs|verify|bind|import EXPECTED [--receipt DIR] [--packet FILE] [--destination DIR]"
  hPutStrLn stderr "EXPECTED: --name --role --source-commit --head-commit --sdk --fujimod-paths --trajectory --trajectory-sha256"
  hPutStrLn stderr "          --firmware --nand --boot-rom --run-document"
  hPutStrLn stderr "          [--archive --archive-sha256] [--source-identity FILE] [--checker FILE] [--bind-admission DIR]"
  exitFailure

parseOptions :: [String] -> Either String Options
parseOptions = go emptyOptions
 where
  go options [] = Right options
  go options ("--name" : value : rest) = go options {optionName = Just value} rest
  go options ("--role" : value : rest) = go options {optionRole = Just value} rest
  go options ("--source-commit" : value : rest) = go options {optionSourceCommit = Just value} rest
  go options ("--head-commit" : value : rest) = go options {optionHeadCommit = Just value} rest
  go options ("--sdk" : value : rest) = go options {optionSdk = Just value} rest
  go options ("--fujimod-paths" : value : rest) = go options {optionFujiModPaths = Just value} rest
  go options ("--trajectory" : value : rest) = go options {optionTrajectory = Just value} rest
  go options ("--trajectory-sha256" : value : rest) = go options {optionTrajectorySha = Just value} rest
  go options ("--firmware" : value : rest) = go options {optionFirmware = Just value} rest
  go options ("--nand" : value : rest) = go options {optionNand = Just value} rest
  go options ("--boot-rom" : value : rest) = go options {optionBootRom = Just value} rest
  go options ("--run-document" : value : rest) = go options {optionRunDocument = Just value} rest
  go options ("--archive" : value : rest) = go options {optionArchive = Just value} rest
  go options ("--archive-sha256" : value : rest) = go options {optionArchiveSha = Just value} rest
  go options ("--source-identity" : value : rest) = go options {optionSourceIdentity = Just value} rest
  go options ("--checker" : value : rest) = go options {optionChecker = Just value} rest
  go options ("--prelinked" : value : rest) = go options {optionPrelinked = Just value} rest
  go options ("--prelinked-base" : value : rest) = case readHexWord value of
    Nothing -> Left ("--prelinked-base is not a hex word: " ++ value)
    Just base -> go options {optionPrelinkedBase = Just base} rest
  go options ("--prelinked-size" : value : rest) = case readSize value of
    Nothing -> Left ("--prelinked-size is not a byte count: " ++ value)
    Just size -> go options {optionPrelinkedSize = Just size} rest
  go options ("--bind-admission" : value : rest) = go options {optionBindAdmission = Just value} rest
  go options ("--receipt" : value : rest) = go options {optionReceipt = Just value} rest
  go options ("--packet" : value : rest) = go options {optionPacket = Just value} rest
  go options ("--destination" : value : rest) = go options {optionDestination = Just value} rest
  go _ (unexpected : _) = Left ("unexpected argument " ++ unexpected)

readHexWord :: String -> Maybe Word32
readHexWord text = case stripPrefix "0x" text of
  Just digits | [(value, "")] <- readHex digits -> Just value
  _ -> Nothing

readSize :: String -> Maybe Word32
readSize text = case stripPrefix "0x" text of
  Just digits | [(value, "")] <- readHex digits -> Just value
  Just _ -> Nothing
  Nothing -> readMaybe text
