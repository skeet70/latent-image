{-# LANGUAGE OverloadedStrings #-}
-- | Opens, closes and inspects run directories under @private-data/scratch/NAME/runs/@.
module Main (main) where

import Control.Monad (forM_, when)
import Data.Either (isLeft)
import qualified FujiAF.RunOutput as Run
import FujiAF.StorageBudget (StorageBytes, parseByteCount)
import System.Directory (doesDirectoryExist)
import System.Environment (getArgs, lookupEnv)
import System.Exit (ExitCode (..), exitWith)
import System.FilePath (takeFileName)
import System.IO (hPutStrLn, stderr, stdout)
import System.Process (CreateProcess (..), createProcess, getPid, proc, waitForProcess)
import Text.Read (readMaybe)

data ExecOptions = ExecOptions
  { optionLabel :: Maybe String
  , optionWorkBudget :: Maybe String
  , optionCommand :: [String]
  }

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    "exec" : rest -> runExec rest
    "keep" : paths -> runKeep paths
    _ -> usage

usage :: IO a
usage = do
  hPutStrLn stderr "usage: run-output exec [--label LABEL] [--work-budget BYTES] -- COMMAND ARG..."
  hPutStrLn stderr "       run-output keep PATH..."
  exitWith (ExitFailure 2)

usageError :: String -> IO a
usageError message = hPutStrLn stderr message >> usage

failWith :: Int -> String -> IO a
failWith code message = hPutStrLn stderr message >> exitWith (ExitFailure code)

parseExec :: [String] -> Either String ExecOptions
parseExec = go (ExecOptions Nothing Nothing [])
 where
  go _ [] = Left "exec requires -- COMMAND ARG..."
  go options ("--" : command)
    | null command = Left "exec requires a command after --"
    | otherwise = Right options {optionCommand = command}
  go options ("--label" : value : rest)
    | optionLabel options == Nothing = go options {optionLabel = Just value} rest
  go options ("--work-budget" : value : rest)
    | optionWorkBudget options == Nothing = go options {optionWorkBudget = Just value} rest
  go _ _ = Left "exec takes only --label LABEL, --work-budget BYTES and -- COMMAND ARG..."

runExec :: [String] -> IO ()
runExec arguments = do
  options <- either usageError pure (parseExec arguments)
  (program, commandArguments) <- case optionCommand options of
    program : rest -> pure (program, rest)
    [] -> usage
  existing <- lookupEnv "XM1_RUN_DIR"
  inRun <- case existing of
    Just directory -> doesDirectoryExist directory
    Nothing -> pure False
  if inRun
    then runInherited program commandArguments >>= exitWith
    else do
      label <- either usageError pure (maybe (Right (Run.labelFromText (takeFileName program))) Run.parseLabel (optionLabel options))
      budget <- workBudget options
      poll <- pollInterval
      resolved <- Run.resolveTask
      task <- case resolved of
        Nothing -> failWith 1 "run-output: cannot resolve the workspace task"
        Just value -> pure value
      opened <- Run.openRun
        Run.OpenRequest
          { Run.openTask = task
          , Run.openLabel = label
          , Run.openOpener = Run.Exec
          , Run.openCommand = program : commandArguments
          }
      case opened of
        Run.RunRefused refusal -> hPutStrLn stderr (Run.describeRefusal refusal) >> exitWith (ExitFailure 75)
        Run.RunOpened handle -> executeInRun handle program commandArguments budget poll

workBudget :: ExecOptions -> IO StorageBytes
workBudget options = case optionWorkBudget options of
  Just text -> either (\_ -> byteCountError "--work-budget") pure (parseByteCount text)
  Nothing -> do
    configured <- lookupEnv "XM1_RUN_WORK_MAX"
    case configured of
      Nothing -> pure Run.defaultWorkBudget
      Just text -> either (\_ -> byteCountError "XM1_RUN_WORK_MAX") pure (parseByteCount text)

byteCountError :: String -> IO a
byteCountError name =
  usageError (name ++ " must be a positive byte count with optional K, M, G, T or KiB/MiB/GiB/TiB suffix")

pollInterval :: IO Run.Microseconds
pollInterval = do
  configured <- lookupEnv "XM1_RUN_POLL_MICROS"
  case configured of
    Nothing -> pure Run.defaultPollMicroseconds
    Just text -> case (readMaybe text :: Maybe Int) of
      Just value | value > 0 -> pure (Run.Microseconds value)
      _ -> usageError "XM1_RUN_POLL_MICROS must be a positive integer"

runInherited :: FilePath -> [String] -> IO ExitCode
runInherited program arguments = do
  (_, _, _, process) <- createProcess (proc program arguments)
  waitForProcess process

executeInRun :: Run.RunHandle -> FilePath -> [String] -> StorageBytes -> Run.Microseconds -> IO ()
executeInRun handle program arguments budget poll = do
  let paths = Run.runPaths handle
  environment <- Run.runEnvironment paths
  (_, _, _, process) <- createProcess (proc program arguments) {env = Just environment, create_group = True}
  child <- getPid process
  pid <- case child of
    Nothing -> ioError (userError "run-output: cannot read the child process id")
    Just value -> pure value
  (status, overBudget) <- Run.awaitWithinWorkBudget (Run.runWork paths) budget poll pid process
  result <- Run.closeRun handle (Just status) (if overBudget then Just Run.WorkBudget else Nothing)
  case (overBudget, status, Run.closeOutcome result, Run.closeFailure result) of
    (True, _, _, _) -> exitWith (ExitFailure 75)
    (False, ExitSuccess, Run.OutcomeFailed, Just Run.ResultTooLarge) -> exitWith (ExitFailure 75)
    _ -> exitWith status

runKeep :: [FilePath] -> IO ()
runKeep paths = do
  when (null paths) usage
  resolved <- Run.resolveTask
  task <- case resolved of
    Nothing -> failWith 1 "run-output keep: cannot resolve the workspace task"
    Just value -> pure value
  work <- lookupEnv "XM1_RUN_WORK"
  directory <- case work of
    Nothing -> failWith 1 "run-output keep: XM1_RUN_WORK is not set"
    Just value -> pure value
  results <- mapM (Run.keepInput task directory) paths
  forM_ results $ \result -> case result of
    Left message -> hPutStrLn stderr ("run-output keep: " ++ message)
    Right destination -> hPutStrLn stdout destination
  when (any isLeft results) (exitWith (ExitFailure 1))
