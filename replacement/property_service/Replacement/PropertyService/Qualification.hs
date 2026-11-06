{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}

-- | The bounded qualification driver of the owned property service: stage the current candidate from
-- the canonical sources through the existing stager, run the existing oracle on a fresh attempt with
-- the native batch cache, and publish one bounded verdict, a review packet and private logs.
module Replacement.PropertyService.Qualification
  ( Options (..)
  , defaultOptions
  , parseOptions
  , runQualification
  , StageFiles (..)
  , writeStageManifest
  , verifyStage
  , commitStage
  , Attempt (..)
  , JobView (..)
  , judgeAttempt
  , reviewAttempt
  , writeTerminal
  , identityHash
  ) where

import Control.Exception (SomeException, displayException, try)
import Control.Monad (forM, unless)
import Data.Aeson (Value (..), eitherDecodeStrict', object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.List (isPrefixOf, sort)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, isJust, isNothing)
import Data.String (fromString)
import Emulation.Sha256 (sha256, sha256Hex, sha256File)
import FujiAF.Host
  ( failH
  , get
  , keys
  , list
  , num
  , readRegular
  , runBounded
  , str
  )
import FujiAF.JSON (encodeCanonical)
import System.Directory
  ( canonicalizePath
  , createDirectoryIfMissing
  , doesDirectoryExist
  , doesFileExist
  , doesPathExist
  , getFileSize
  , makeAbsolute
  , renameDirectory
  , renameFile
  , setCurrentDirectory
  )
import System.Environment (lookupEnv)
import System.Exit (ExitCode (..))
import System.FilePath (takeDirectory, takeFileName, (</>))
import System.IO (hClose, openTempFile)

-- | The X-M1 1.50 reference image4 of the property oracle.
canonicalImage4 :: String
canonicalImage4 = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"

stageTimeoutMicros, oracleTimeoutMicros, stageOutputCap, oracleOutputCap :: Int
stageTimeoutMicros = 1800 * 1000 * 1000
oracleTimeoutMicros = 6 * 3600 * 1000 * 1000
stageOutputCap = 4 * 1024 * 1024
oracleOutputCap = 8 * 1024 * 1024

data Options = Options
  { optDat :: FilePath
  , optImage4 :: FilePath
  , optLoaderA :: FilePath
  , optLoaderB :: FilePath
  , optImage5 :: FilePath
  , optPlain :: FilePath
  , optImage2 :: FilePath
  , optLinked :: FilePath
  , optSymbols :: FilePath
  , optOut :: FilePath
  , optCache :: FilePath
  , optIds :: Maybe String
  , optRunner :: FilePath
  , optOracle :: FilePath
  , optStage :: FilePath
  , optStageEncoder :: FilePath
  , optStageRebuild :: FilePath
  , optStageDecoder :: FilePath
  , optDefects :: [(String, FilePath, FilePath)]
  }
  deriving (Eq, Show)

defaultOptions :: Options
defaultOptions =
  Options
    { optDat = ""
    , optImage4 = ""
    , optLoaderA = ""
    , optLoaderB = ""
    , optImage5 = ""
    , optPlain = ""
    , optImage2 = ""
    , optLinked = ""
    , optSymbols = ""
    , optOut = ""
    , optCache = ""
    , optIds = Nothing
    , optRunner = ""
    , optOracle = ""
    , optStage = ""
    , optStageEncoder = ""
    , optStageRebuild = ""
    , optStageDecoder = ""
    , optDefects = []
    }

-- | The value flags the command accepts, each replacing the earlier value. `--defect NAME BIN
-- SYMBOLS` may repeat; a repeated name replaces the earlier binary and symbol list.
parseOptions :: [String] -> Either String Options
parseOptions = go defaultOptions
  where
    go options ("--defect" : name : bin : symbols : rest) =
      go options {optDefects = optDefects options ++ [(name, bin, symbols)]} rest
    go _ ("--defect" : _) = Left "--defect needs NAME BIN SYMBOLS"
    go options ("--ids" : value : rest)
      | null value = Left "--ids needs a non-empty value"
      | otherwise = go options {optIds = Just value} rest
    go options (flag : rest) = case lookup flag valueFlags of
      Just set -> case rest of
        value : more -> go (set options value) more
        [] -> Left (flag ++ " needs a value")
      Nothing -> Left ("unknown argument: " ++ flag)
    go options [] = check options

    check options =
      let required =
            [ ("--dat", optDat options), ("--image4", optImage4 options), ("--loader-a", optLoaderA options)
            , ("--loader-b", optLoaderB options), ("--image5", optImage5 options), ("--plain", optPlain options)
            , ("--image2", optImage2 options), ("--linked", optLinked options), ("--symbols", optSymbols options)
            , ("--out", optOut options), ("--cache", optCache options), ("--runner", optRunner options)
            , ("--oracle", optOracle options), ("--stage", optStage options)
            , ("--stage-encoder", optStageEncoder options), ("--stage-rebuild", optStageRebuild options)
            , ("--stage-decoder", optStageDecoder options)
            ]
          missing = [flag | (flag, value) <- required, null value]
       in if null missing
            then Right options {optDefects = dedupeDefects (optDefects options)}
            else Left ("missing required flags: " ++ unwords missing)

    valueFlags =
      [ ("--dat", \options value -> options {optDat = value})
      , ("--image4", \options value -> options {optImage4 = value})
      , ("--loader-a", \options value -> options {optLoaderA = value})
      , ("--loader-b", \options value -> options {optLoaderB = value})
      , ("--image5", \options value -> options {optImage5 = value})
      , ("--plain", \options value -> options {optPlain = value})
      , ("--image2", \options value -> options {optImage2 = value})
      , ("--linked", \options value -> options {optLinked = value})
      , ("--symbols", \options value -> options {optSymbols = value})
      , ("--out", \options value -> options {optOut = value})
      , ("--cache", \options value -> options {optCache = value})
      , ("--runner", \options value -> options {optRunner = value})
      , ("--oracle", \options value -> options {optOracle = value})
      , ("--stage", \options value -> options {optStage = value})
      , ("--stage-encoder", \options value -> options {optStageEncoder = value})
      , ("--stage-rebuild", \options value -> options {optStageRebuild = value})
      , ("--stage-decoder", \options value -> options {optStageDecoder = value})
      ]

dedupeDefects :: [(String, FilePath, FilePath)] -> [(String, FilePath, FilePath)]
dedupeDefects defects =
  [(name, bin, symbols) | (name, (bin, symbols)) <- M.toList (M.fromList [(name, (bin, symbols)) | (name, bin, symbols) <- defects])]

identityHash :: Value -> String
identityHash = sha256Hex . sha256 . encodeCanonical

fileDigest :: FilePath -> IO String
fileDigest path = sha256Hex <$> sha256File path

-- Stage manifest ---------------------------------------------------------------

data StageFiles = StageFiles
  { stageCandidateDatPath :: FilePath
  , stageContractPath :: FilePath
  , stageImage4Path :: FilePath
  , stageCandidateDatSha256 :: String
  , stageContractSha256 :: String
  , stageImage4Sha256 :: String
  }
  deriving (Eq, Show)

stageOutputNames :: [(String, String)]
stageOutputNames = [("candidate_dat", "candidate.dat"), ("contract", "contract.json"), ("image4", "image4.bin")]

-- | Writes @manifest.json@ over the given role/file pairs, which must be direct children of
-- @directory@.
writeStageManifest :: FilePath -> Value -> [(String, FilePath)] -> IO ()
writeStageManifest directory identity files = do
  outputs <- forM files $ \(role, path) -> do
    digest <- fileDigest path
    size <- getFileSize path
    pure (object ["role" .= role, "file" .= takeFileName path, "sha256" .= digest, "size" .= size])
  let manifest =
        object
          [ "schema_version" .= (1 :: Int)
          , "stage_key" .= identityHash identity
          , "identity" .= identity
          , "outputs" .= outputs
          ]
  B.writeFile (directory </> "manifest.json") (encodeCanonical manifest)

-- | A completed stage is reusable only when its manifest names the current identity and every
-- output is present at the recorded size and digest.
verifyStage :: FilePath -> Value -> IO (Either String StageFiles)
verifyStage directory expected = do
  attempted <- try (readStageManifest directory >>= checkStage directory expected)
  pure $ case attempted of
    Left (exception :: SomeException) -> Left (directory ++ ": " ++ displayException exception)
    Right result -> result

readStageManifest :: FilePath -> IO Value
readStageManifest directory = do
  let path = directory </> "manifest.json"
  exists <- doesFileExist path
  unless exists (failH (path ++ ": no completed stage manifest"))
  bytes <- readRegular (1024 * 1024) path
  either (\problem -> failH (path ++ ": " ++ problem)) pure (eitherDecodeStrict' bytes :: Either String Value)

checkStage :: FilePath -> Value -> Value -> IO (Either String StageFiles)
checkStage directory expected manifest
  | num (get manifest "schema_version") /= 1 = pure (Left "stage manifest schema differs")
  | str (get manifest "stage_key") /= identityHash expected = pure (Left "stage key differs from the current identity")
  | get manifest "identity" /= expected = pure (Left "stage identity differs from the current identity")
  | otherwise = do
      checked <- forM stageOutputNames $ \(role, file) -> checkOutput directory manifest role file
      pure $ case checked of
        [Right (_, datPath, datSha), Right (_, contractPath, contractSha), Right (_, image4Path, image4Sha)] ->
          Right
            StageFiles
              { stageCandidateDatPath = datPath
              , stageContractPath = contractPath
              , stageImage4Path = image4Path
              , stageCandidateDatSha256 = datSha
              , stageContractSha256 = contractSha
              , stageImage4Sha256 = image4Sha
              }
        _ -> Left "stage manifest does not have the complete candidate, contract and image4 outputs"

checkOutput :: FilePath -> Value -> String -> String -> IO (Either String (String, FilePath, String))
checkOutput directory manifest role file = case matches of
  [record] -> do
    let path = directory </> file
    exists <- doesFileExist path
    if not exists
      then pure (Left (path ++ ": missing from the completed stage"))
      else do
        size <- getFileSize path
        digest <- fileDigest path
        if fromIntegral size /= num (get record "size")
          then pure (Left (path ++ ": size differs from the completed stage"))
          else
            if digest /= str (get record "sha256")
              then pure (Left (path ++ ": sha256 differs from the completed stage"))
              else pure (Right (role, path, digest))
  _ -> pure (Left (directory ++ ": manifest has no single " ++ role ++ " output"))
  where
    matches = [record | record <- list (get manifest "outputs"), str (get record "role") == role, str (get record "file") == file]

-- | Renames a complete temporary stage into place. Returns False when the destination already
-- exists; the caller verifies that destination instead of overwriting it.
commitStage :: FilePath -> FilePath -> IO (Either String Bool)
commitStage partial final = do
  exists <- doesPathExist final
  if exists
    then pure (Right False)
    else do
      attempted <- try (renameDirectory partial final)
      case attempted of
        Right () -> pure (Right True)
        Left (exception :: SomeException) -> do
          appeared <- doesPathExist final
          pure $ if appeared then Right False else Left (displayException exception)

freePath :: FilePath -> String -> IO FilePath
freePath base prefix = go (1 :: Int)
  where
    go index = do
      let candidate = base </> (prefix ++ show index)
      exists <- doesPathExist candidate
      if exists then go (index + 1) else pure candidate

quarantine :: FilePath -> IO (Either String FilePath)
quarantine path = do
  attempted <- try $ do
    let rejected = takeDirectory path </> "rejected"
    createDirectoryIfMissing True rejected
    target <- freePath rejected (takeFileName path ++ "-")
    renameDirectory path target
    pure target
  pure $ case attempted of
    Right target -> Right target
    Left (exception :: SomeException) -> Left (displayException exception)

-- Driver -----------------------------------------------------------------------

data JobView = JobView
  { jobOptions :: Options
  , jobKey :: String
  , jobStageKey :: String
  , jobIdentity :: Value
  , jobStageStatus :: String
  , jobStageFiles :: StageFiles
  , jobStageLogs :: [FilePath]
  , jobNotes :: [String]
  }

data Attempt = Attempt
  { attemptExit :: Int
  , attemptTimedOut :: Bool
  , attemptResults :: Maybe Value
  , attemptResultsError :: String
  , attemptDir :: FilePath
  , attemptStdoutLog :: FilePath
  , attemptStderrLog :: FilePath
  }

runQualification :: Options -> IO (Int, Value)
runQualification options = do
  lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= mapM_ setCurrentDirectory
  prepared <- try (prepareJob options) :: IO (Either SomeException (Either String JobView))
  case prepared of
    Left exception -> do
      let message = displayException exception
      writeError options message
      pure (2, errorVerdict options message)
    Right (Left message) -> do
      writeError options message
      pure (2, errorVerdict options message)
    Right (Right view) -> do
      attempted <- try (runAttempt view) :: IO (Either SomeException Attempt)
      case attempted of
        Left exception -> finalize view (failedAttempt (displayException exception))
        Right attempt -> finalize view attempt
  where
    finalize view attempt = do
      let (code, verdict) = judgeAttempt view attempt
      ignoreIO (writeTerminal (optOut (jobOptions view)) verdict (reviewAttempt view attempt))
      ignoreIO (appendAttempt view attempt)
      pure (code, verdict)

ignoreIO :: forall a. IO a -> IO ()
ignoreIO action = do
  _ <- try action :: IO (Either SomeException a)
  pure ()

writeError :: Options -> String -> IO ()
writeError options message = ignoreIO $ do
  out <- makeAbsolute (optOut options)
  createDirectoryIfMissing True out
  let resolved = options {optOut = out}
  writeTerminal out (errorVerdict resolved message) (errorReview resolved message)

prepareJob :: Options -> IO (Either String JobView)
prepareJob options = do
  resolved <- resolveInputs options
  digest <- fileDigest (optImage4 resolved)
  if digest /= canonicalImage4
    then pure (Left "--image4 is not the canonical X-M1 1.50 image4")
    else do
      out <- makeAbsolute (optOut resolved) >>= createAndCanonicalize
      cache <- makeAbsolute (optCache resolved) >>= createAndCanonicalize
      let withPaths = resolved {optOut = out, optCache = cache}
      stageIdentity <- stageIdentityValue withPaths
      let stageKey = identityHash stageIdentity
      jobIdentity <- jobIdentityValue withPaths stageIdentity stageKey
      let jobKey = identityHash jobIdentity
      staged <- prepareStage withPaths stageIdentity stageKey jobKey
      pure $ case staged of
        Left message -> Left message
        Right (files, status, logs, notes) ->
          Right (JobView withPaths jobKey stageKey jobIdentity status files logs notes)
  where
    createAndCanonicalize path = do
      createDirectoryIfMissing True path
      canonicalizePath path

resolveInputs :: Options -> IO Options
resolveInputs options = do
  dat <- resolve (optDat options) "--dat"
  image4 <- resolve (optImage4 options) "--image4"
  loaderA <- resolve (optLoaderA options) "--loader-a"
  loaderB <- resolve (optLoaderB options) "--loader-b"
  image5 <- resolve (optImage5 options) "--image5"
  plain <- resolve (optPlain options) "--plain"
  image2 <- resolve (optImage2 options) "--image2"
  linked <- resolve (optLinked options) "--linked"
  symbols <- resolve (optSymbols options) "--symbols"
  runner <- resolve (optRunner options) "--runner"
  oracle <- resolve (optOracle options) "--oracle"
  stage <- resolve (optStage options) "--stage"
  encoder <- resolve (optStageEncoder options) "--stage-encoder"
  rebuild <- resolve (optStageRebuild options) "--stage-rebuild"
  decoder <- resolve (optStageDecoder options) "--stage-decoder"
  defects <- forM (optDefects options) $ \(name, bin, symbolPath) -> do
    resolvedBin <- resolve bin ("--defect " ++ name ++ " binary")
    resolvedSymbols <- resolve symbolPath ("--defect " ++ name ++ " symbols")
    pure (name, resolvedBin, resolvedSymbols)
  pure
    options
      { optDat = dat
      , optImage4 = image4
      , optLoaderA = loaderA
      , optLoaderB = loaderB
      , optImage5 = image5
      , optPlain = plain
      , optImage2 = image2
      , optLinked = linked
      , optSymbols = symbols
      , optRunner = runner
      , optOracle = oracle
      , optStage = stage
      , optStageEncoder = encoder
      , optStageRebuild = rebuild
      , optStageDecoder = decoder
      , optDefects = defects
      }
  where
    resolve path label = do
      exists <- doesFileExist path
      unless exists (failH (label ++ ": file not found: " ++ path))
      canonicalizePath path

sourceRoles :: Options -> [(String, FilePath)]
sourceRoles options =
  [ ("dat", optDat options), ("image4", optImage4 options), ("loader_a", optLoaderA options)
  , ("loader_b", optLoaderB options), ("image5", optImage5 options), ("plain", optPlain options)
  , ("image2", optImage2 options)
  ]

digestsOf :: [(String, FilePath)] -> IO [(String, String)]
digestsOf roles = forM roles $ \(role, path) -> do
  digest <- fileDigest path
  pure (role, digest)

stageIdentityValue :: Options -> IO Value
stageIdentityValue options = do
  sources <- digestsOf (sourceRoles options)
  linked <- fileDigest (optLinked options)
  symbols <- fileDigest (optSymbols options)
  tools <-
    digestsOf
      [ ("stage", optStage options)
      , ("encoder", optStageEncoder options)
      , ("rebuild", optStageRebuild options)
      , ("decoder", optStageDecoder options)
      ]
  pure $
    object
      [ "schema_version" .= (1 :: Int)
      , "sources" .= object [fromString role .= digest | (role, digest) <- sources]
      , "candidate" .= object ["linked" .= linked, "symbols" .= symbols]
      , "tools" .= object [fromString role .= digest | (role, digest) <- tools]
      ]

jobIdentityValue :: Options -> Value -> String -> IO Value
jobIdentityValue options stageIdentity stageKey = do
  oracleDigest <- fileDigest (optOracle options)
  runnerDigest <- fileDigest (optRunner options)
  defects <- forM (optDefects options) $ \(name, bin, symbolPath) -> do
    binDigest <- fileDigest bin
    symbolsDigest <- fileDigest symbolPath
    pure (object ["name" .= name, "bin_sha256" .= binDigest, "symbols_sha256" .= symbolsDigest])
  pure $
    object
      [ "schema_version" .= (1 :: Int)
      , "stage_key" .= stageKey
      , "sources" .= get stageIdentity "sources"
      , "candidate" .= get stageIdentity "candidate"
      , "comparison" .= object
          [ "ids" .= fmap splitCommas (optIds options)
          , "full_native" .= isNothing (optIds options)
          , "defects" .= defects
          ]
      , "oracle" .= object ["path" .= optOracle options, "sha256" .= oracleDigest]
      , "runner" .= object ["path" .= optRunner options, "sha256" .= runnerDigest]
      ]

prepareStage :: Options -> Value -> String -> String -> IO (Either String (StageFiles, String, [FilePath], [String]))
prepareStage options identity stageKey jobKey = do
  exists <- doesPathExist final
  if exists
    then do
      isDirectory <- doesDirectoryExist final
      if not isDirectory
        then pure (Left (final ++ ": stage path is not a directory"))
        else do
          checked <- verifyStage final identity
          case checked of
            Right files -> pure (Right (files, "reused", [], []))
            Left reason -> do
              moved <- quarantine final
              case moved of
                Left problem -> pure (Left (reason ++ "; cannot quarantine the rejected stage: " ++ problem))
                Right rejected -> stageFresh (reason ++ "; rejected to " ++ rejected)
    else stageFresh ""
  where
    final = optOut options </> "stages" </> stageKey
    stageFresh note = do
      createDirectoryIfMissing True (optOut options </> "stages")
      temp <- freePath (optOut options </> "stages") (".partial-" ++ stageKey ++ "-")
      createDirectoryIfMissing True temp
      let logs = optOut options </> "logs"
          scratch = "private-data/scratch/PropertyService2/qualify" </> jobKey </> "stage"
          stdoutLog = logs </> ("stage-" ++ stageKey ++ ".stdout.log")
          stderrLog = logs </> ("stage-" ++ stageKey ++ ".stderr.log")
          arguments =
            [ optStageEncoder options
            , optStageRebuild options
            , optStageDecoder options
            , optDat options
            , optImage4 options
            , optLoaderA options
            , optLoaderB options
            , optImage5 options
            , optPlain options
            , optImage2 options
            , optLinked options
            , optSymbols options
            , scratch
            , temp </> "candidate.dat"
            , temp </> "contract.json"
            , temp </> "image4.bin"
            ]
      createDirectoryIfMissing True logs
      createDirectoryIfMissing True scratch
      (timedOut, exit, stdoutBytes, stderrBytes) <- runBounded stageTimeoutMicros stageOutputCap (optStage options) arguments B.empty
      B.writeFile stdoutLog stdoutBytes
      B.writeFile stderrLog stderrBytes
      if timedOut
        then pure (Left ("staging timed out; logs " ++ stdoutLog ++ " " ++ stderrLog))
        else
          if exit /= ExitSuccess
            then pure (Left ("staging failed: " ++ tailText stderrBytes ++ "; logs " ++ stdoutLog ++ " " ++ stderrLog))
            else do
              let outputs =
                    [ ("candidate_dat", temp </> "candidate.dat")
                    , ("contract", temp </> "contract.json")
                    , ("image4", temp </> "image4.bin")
                    ]
              writeStageManifest temp identity outputs
              committed <- commitStage temp final
              case committed of
                Left problem -> pure (Left ("cannot publish the completed stage: " ++ problem))
                Right _ -> do
                  checked <- verifyStage final identity
                  pure $ case checked of
                    Left problem -> Left ("published stage failed its integrity check: " ++ problem)
                    Right files -> Right (files, "staged", [stdoutLog, stderrLog], [note | not (null note)])

tailText :: B.ByteString -> String
tailText bytes =
  let text = BC.unpack bytes
      rows = lines text
   in unlines (drop (max 0 (length rows - 8)) rows)

oracleArguments :: JobView -> FilePath -> [String]
oracleArguments view directory =
  concat
    [ ["--runner", optRunner options]
    , ["--linked", optLinked options]
    , ["--symbols", optSymbols options]
    , ["--image4", optImage4 options]
    , ["--candidate-image4", stageImage4Path (jobStageFiles view)]
    , ["--contract", stageContractPath (jobStageFiles view)]
    , ["--dat", optDat options]
    , ["--loader-a", optLoaderA options]
    , ["--loader-b", optLoaderB options]
    , ["--image5", optImage5 options]
    , ["--plain", optPlain options]
    , ["--image2", optImage2 options]
    , ["--out", directory]
    , ["--cache", optCache options]
    ]
    ++ maybe [] (\ids -> ["--ids", ids]) (optIds options)
    ++ concat [["--defect", name, bin, symbols] | (name, bin, symbols) <- optDefects options]
  where
    options = jobOptions view

runAttempt :: JobView -> IO Attempt
runAttempt view = do
  let options = jobOptions view
      scratchBase = "private-data/scratch/PropertyService2/qualify" </> jobKey view </> "attempts"
      logs = optOut options </> "logs"
  createDirectoryIfMissing True scratchBase
  createDirectoryIfMissing True logs
  directory <- freePath scratchBase "attempt-"
  let attemptName = takeFileName directory
      stdoutLog = logs </> (attemptName ++ ".stdout.log")
      stderrLog = logs </> (attemptName ++ ".stderr.log")
  outcome <- try (runBounded oracleTimeoutMicros oracleOutputCap (optOracle options) (oracleArguments view directory) B.empty)
  case outcome of
    Left (exception :: SomeException) -> do
      B.writeFile stdoutLog B.empty
      B.writeFile stderrLog (BC.pack (displayException exception))
      pure (Attempt 1 False Nothing (displayException exception) directory stdoutLog stderrLog)
    Right (timedOut, exit, stdoutBytes, stderrBytes) -> do
      B.writeFile stdoutLog stdoutBytes
      B.writeFile stderrLog stderrBytes
      (results, resultsError) <- readAttemptResults (directory </> "results.json")
      pure (Attempt (exitCode exit) timedOut results resultsError directory stdoutLog stderrLog)

readAttemptResults :: FilePath -> IO (Maybe Value, String)
readAttemptResults path = do
  exists <- doesFileExist path
  if not exists
    then pure (Nothing, path ++ ": the oracle wrote no results.json")
    else do
      attempted <- try (readRegular (64 * 1024 * 1024) path)
      pure $ case attempted of
        Left (exception :: SomeException) -> (Nothing, path ++ ": " ++ displayException exception)
        Right bytes -> case eitherDecodeStrict' bytes :: Either String Value of
          Left problem -> (Nothing, path ++ ": " ++ problem)
          Right value -> (Just value, "")

exitCode :: ExitCode -> Int
exitCode ExitSuccess = 0
exitCode (ExitFailure code) = code

data FailureRow = FailureRow
  { rowPredicate :: String
  , rowCase :: String
  , rowKind :: String
  , rowDetail :: String
  }

judgeAttempt :: JobView -> Attempt -> (Int, Value)
judgeAttempt view attempt = case attemptResults attempt of
  Nothing -> (2, verdictValue view attempt "error" (failureRows attempt) Nothing)
  Just results ->
    let declared = boolField (get results "passed")
        rows = failureRows attempt
        passed = declared && attemptExit attempt == 0 && null rows
     in (if passed then 0 else 1, verdictValue view attempt (if passed then "passed" else "failed") rows (Just results))

failureRows :: Attempt -> [FailureRow]
failureRows attempt = case attemptResults attempt of
  Nothing ->
    [FailureRow (if attemptTimedOut attempt then "timeout" else "oracle") "" "" (fromMaybe "the oracle produced no results" (nonEmpty (attemptResultsError attempt)))]
  Just results ->
    let modelRows = differenceRows "model:" results
        passRows = differenceRows "pass2:" results
        controlRows = controlFailureRows results
        exitRows = [FailureRow "oracle_exit" "" "" ("oracle exited " ++ show (attemptExit attempt)) | attemptExit attempt /= 0]
        declared = boolField (get results "passed")
        declaredRow =
          [ FailureRow "results" "" "" "results.json reports failure with no failure row"
          | not declared
          , null (modelRows ++ passRows ++ controlRows)
          ]
     in modelRows ++ passRows ++ controlRows ++ exitRows ++ declaredRow

failedAttempt :: String -> Attempt
failedAttempt message = Attempt 1 False Nothing message "" "" ""

nonEmpty :: String -> Maybe String
nonEmpty text = if null text then Nothing else Just text

differenceRows :: String -> Value -> [FailureRow]
differenceRows prefix results =
  [ FailureRow (takeWhile (/= ':') key) (str (get sample "case")) key (str (get sample "detail"))
  | key <- sort (keys (get results "differences"))
  , prefix `isPrefixOf` key
  , sample <- list (get (get results "differences") key)
  ]

controlFailureRows :: Value -> [FailureRow]
controlFailureRows results =
  [ FailureRow ("control:" ++ name) caseId kind detail
  | control <- list (get results "controls")
  , not (boolField (get control "holds"))
  , let name = str (get control "name")
        samples = list (get control "first_differences")
        (caseId, kind, detail) = case samples of
          sample : _ -> (str (get sample "case"), str (get sample "kind"), str (get sample "detail"))
          [] -> ("", "", "control recorded no failing case")
  ]

boolField :: Value -> Bool
boolField (Bool value) = value
boolField _ = False

splitCommas :: String -> [String]
splitCommas text = [item | item <- split text, not (null item)]
  where
    split value = case break (== ',') value of
      (item, []) -> [item]
      (item, _ : rest) -> item : split rest

resultsPath :: Attempt -> FilePath
resultsPath attempt = attemptDir attempt </> "results.json"

logsOf :: JobView -> Attempt -> [FilePath]
logsOf view attempt = filter (not . null) (attemptStdoutLog attempt : attemptStderrLog attempt : jobStageLogs view)

verdictValue :: JobView -> Attempt -> String -> [FailureRow] -> Maybe Value -> Value
verdictValue view attempt status rows maybeResults =
  object
    [ "schema_version" .= (1 :: Int)
    , "verdict" .= status
    , "native_only" .= True
    , "scope" .= object ["subset" .= subset, "ids" .= ids, "full_native" .= not subset, "established" .= establishedText status subset]
    , "identity" .= object
        [ "job" .= jobKey view
        , "stage" .= jobStageKey view
        , "sources" .= get (jobIdentity view) "sources"
        , "candidate" .= get (jobIdentity view) "candidate"
        , "contract_sha256" .= stageContractSha256 files
        , "candidate_dat_sha256" .= stageCandidateDatSha256 files
        , "staged_image4_sha256" .= stageImage4Sha256 files
        , "runner" .= get (jobIdentity view) "runner"
        , "oracle" .= get (jobIdentity view) "oracle"
        ]
    , "stage" .= object ["status" .= jobStageStatus view, "path" .= stageDirectory, "notes" .= jobNotes view]
    , "execution" .= executionValue maybeResults attempt
    , "counts" .= object ["cases" .= num (get shape "cases_per_role"), "grid_cases" .= num (get shape "grid_cases"), "controls" .= length controls]
    , "failure" .= maybe Null (failureJson view attempt) (listToMaybeSafe rows)
    , "failures" .= [failureJson view attempt row | row <- take 20 rows]
    , "artifacts" .= object
        [ "results" .= resultsPath attempt
        , "verdict" .= optOut (jobOptions view) </> "verdict.json"
        , "review_packet" .= optOut (jobOptions view) </> "review-packet.json"
        , "logs" .= logsOf view attempt
        , "stage" .= stageDirectory
        ]
    , "limits" .= limitText subset
    ]
  where
    files = jobStageFiles view
    stageDirectory = takeDirectory (stageImage4Path files)
    results = fromMaybe Null maybeResults
    shape = get results "case_shape"
    controls = list (get results "controls")
    subset = case maybeResults of
      Just _ -> boolField (get shape "subset")
      Nothing -> isJust (optIds (jobOptions view))
    ids =
      if not subset
        then []
        else case maybeResults of
          Just _ -> words (str (get shape "ids"))
          Nothing -> maybe [] splitCommas (optIds (jobOptions view))

executionValue :: Maybe Value -> Attempt -> Value
executionValue maybeResults attempt =
  object
    [ "cache" .= get execution "cache"
    , "batches_executed" .= num (get execution "batches_executed")
    , "batches_reused" .= num (get execution "batches_reused")
    , "cases_executed" .= num (get execution "cases_executed")
    , "cases_reused" .= num (get execution "cases_reused")
    , "oracle_exit" .= attemptExit attempt
    , "attempt" .= attemptDir attempt
    ]
  where
    execution = get (fromMaybe Null maybeResults) "execution"

failureJson :: JobView -> Attempt -> FailureRow -> Value
failureJson _ attempt row =
  object
    [ "predicate" .= rowPredicate row
    , "case" .= rowCase row
    , "kind" .= rowKind row
    , "detail" .= rowDetail row
    , "results" .= resultsPath attempt
    , "log" .= attemptStderrLog attempt
    ]

listToMaybeSafe :: [a] -> Maybe a
listToMaybeSafe rows = case rows of
  row : _ -> Just row
  [] -> Nothing

establishedText :: String -> Bool -> String
establishedText "passed" True = "native-only subset comparison passed; every control ran in full; full native coverage, research, system and hardware acceptance are not established"
establishedText "passed" False = "native-only full property comparison passed; research, system and hardware acceptance are not established"
establishedText _ _ = "native-only comparison did not pass; no acceptance is established"

limitText :: Bool -> [String]
limitText subset =
  [ "Native-only: the current candidate and the canonical stock getter/setter run on the host native runner"
  , "Does not establish research, system, hardware, timing, persistence or full-replacement acceptance"
  ]
    ++ ["Subset: only the ids named by --ids were compared; every control still ran in full" | subset]

reviewAttempt :: JobView -> Attempt -> Value
reviewAttempt view attempt =
  let (_, verdict) = judgeAttempt view attempt
      status = str (get verdict "verdict")
      results = fromMaybe Null (attemptResults attempt)
   in object
        [ "schema_version" .= (1 :: Int)
        , "kind" .= ("property-service-review-packet" :: String)
        , "job" .= jobKey view
        , "verdict" .= status
        , "identity" .= jobIdentity view
        , "stage" .= object
            [ "key" .= jobStageKey view
            , "status" .= jobStageStatus view
            , "path" .= takeDirectory (stageImage4Path (jobStageFiles view))
            , "outputs" .= object
                [ "candidate_dat_sha256" .= stageCandidateDatSha256 (jobStageFiles view)
                , "contract_sha256" .= stageContractSha256 (jobStageFiles view)
                , "image4_sha256" .= stageImage4Sha256 (jobStageFiles view)
                ]
            , "notes" .= jobNotes view
            ]
        , "execution" .= executionValue (attemptResults attempt) attempt
        , "scope" .= object
            [ "subset" .= boolField (get (get results "case_shape") "subset")
            , "ids" .= words (str (get (get results "case_shape") "ids"))
            , "case_shape" .= get results "case_shape"
            ]
        , "controls" .= [controlRow control | control <- list (get results "controls")]
        , "failures" .= [failureJson view attempt row | row <- take 50 (failureRows attempt)]
        , "artifacts" .= object
            [ "results" .= resultsPath attempt
            , "verdict" .= optOut (jobOptions view) </> "verdict.json"
            , "review_packet" .= optOut (jobOptions view) </> "review-packet.json"
            , "logs" .= logsOf view attempt
            , "stage" .= takeDirectory (stageImage4Path (jobStageFiles view))
            , "attempt" .= attemptDir attempt
            ]
        , "acceptance" .= object
            [ "native_only" .= True
            , "research_accepted" .= False
            , "system_accepted" .= False
            , "hardware_accepted" .= False
            , "note" .= ("Native-only property-service comparison; no automatic research, system or hardware acceptance" :: String)
            ]
        , "limits" .= limitText (boolField (get (get results "case_shape") "subset"))
        ]

controlRow :: Value -> Value
controlRow control =
  object
    [ "name" .= str (get control "name")
    , "kind" .= str (get control "kind")
    , "note" .= str (get control "note")
    , "cases" .= num (get control "cases")
    , "failed_cases" .= num (get control "failed_cases")
    , "holds" .= boolField (get control "holds")
    ]

errorVerdict :: Options -> String -> Value
errorVerdict options message =
  object
    [ "schema_version" .= (1 :: Int)
    , "verdict" .= ("error" :: String)
    , "native_only" .= True
    , "scope" .= object ["subset" .= isJust (optIds options), "ids" .= maybe [] splitCommas (optIds options), "full_native" .= isNothing (optIds options), "established" .= establishedText "error" False]
    , "identity" .= Null
    , "stage" .= Null
    , "execution" .= Null
    , "counts" .= Null
    , "failure" .= object ["predicate" .= ("driver" :: String), "case" .= ("" :: String), "kind" .= ("" :: String), "detail" .= bounded message, "results" .= Null, "log" .= Null]
    , "failures" .= [object ["predicate" .= ("driver" :: String), "detail" .= bounded message]]
    , "artifacts" .= object
        [ "results" .= Null
        , "verdict" .= optOut options </> "verdict.json"
        , "review_packet" .= optOut options </> "review-packet.json"
        , "logs" .= ([] :: [String])
        , "stage" .= Null
        ]
    , "limits" .= limitText (isJust (optIds options))
    ]

errorReview :: Options -> String -> Value
errorReview options message =
  object
    [ "schema_version" .= (1 :: Int)
    , "kind" .= ("property-service-review-packet" :: String)
    , "verdict" .= ("error" :: String)
    , "identity" .= Null
    , "failure" .= object ["predicate" .= ("driver" :: String), "detail" .= bounded message]
    , "acceptance" .= object
        [ "native_only" .= True
        , "research_accepted" .= False
        , "system_accepted" .= False
        , "hardware_accepted" .= False
        , "note" .= ("No comparison ran; no acceptance is established" :: String)
        ]
    , "artifacts" .= object
        [ "verdict" .= optOut options </> "verdict.json"
        , "review_packet" .= optOut options </> "review-packet.json"
        ]
    , "limits" .= limitText (isJust (optIds options))
    ]

bounded :: String -> String
bounded message = take 2000 message

writeTerminal :: FilePath -> Value -> Value -> IO ()
writeTerminal out verdict review = do
  createDirectoryIfMissing True out
  atomicWrite (out </> "verdict.json") (encodeCanonical verdict)
  atomicWrite (out </> "review-packet.json") (encodeCanonical review)

atomicWrite :: FilePath -> B.ByteString -> IO ()
atomicWrite path bytes = do
  (temporary, handle) <- openTempFile (takeDirectory path) ".qualify-"
  hClose handle
  B.writeFile temporary bytes
  renameFile temporary path

appendAttempt :: JobView -> Attempt -> IO ()
appendAttempt view attempt = do
  let (_, verdict) = judgeAttempt view attempt
      line =
        encodeCanonical
          ( object
              [ "attempt" .= attemptDir attempt
              , "exit" .= attemptExit attempt
              , "verdict" .= str (get verdict "verdict")
              , "results" .= resultsPath attempt
              ]
          )
  B.appendFile (optOut (jobOptions view) </> "attempts.jsonl") (line <> "\n")
