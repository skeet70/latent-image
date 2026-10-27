-- | Fail-closed host thermal guard for one private transient service.
--
-- The service MainPID re-discovers k10temp/Tctl on every poll and starts the
-- target as its direct child. Systemd stops the whole service when this process
-- exits or is killed.
module Emulation.ThermalGuard
  ( SysfsRoot (..)
  , ThermalSensor
  , MilliCelsius (..)
  , PollMicroseconds (..)
  , ThermalPolicy (..)
  , ThermalState (..)
  , productionThermalPolicy
  , thermalFailureExit
  , discoverThermalSensor
  , readThermalState
  , runGuarded
  , validateWorkerService
  , callerMemoryProperties
  ) where

import Control.Concurrent (forkIO, killThread, myThreadId, threadDelay)
import Control.Concurrent.MVar (newEmptyMVar, tryPutMVar, tryReadMVar)
import Control.Exception
  ( AsyncException (ThreadKilled)
  , IOException
  , bracket
  , evaluate
  , fromException
  , throwTo
  , try
  , tryJust
  )
import Control.Monad (forM, void)
import Data.Char (isDigit, isSpace)
import Data.List (dropWhileEnd, isPrefixOf, isSuffixOf)
import Data.Maybe (catMaybes)
import System.Directory (doesDirectoryExist, doesFileExist, listDirectory)
import System.Exit (ExitCode (..))
import System.FilePath ((</>), takeFileName)
import System.IO (IOMode (ReadMode), hGetContents, hPutStrLn, stderr, withFile)
import System.Posix.Process (getProcessID)
import System.Process
  ( CreateProcess (..)
  , StdStream (Inherit)
  , createProcess
  , proc
  , readProcessWithExitCode
  , waitForProcess
  )
import System.Timeout (timeout)
import Text.Read (readMaybe)

newtype SysfsRoot = SysfsRoot FilePath

newtype ThermalSensor = ThermalSensor FilePath
  deriving (Eq)

newtype MilliCelsius = MilliCelsius Int
  deriving (Eq, Show)

newtype PollMicroseconds = PollMicroseconds Int

data ThermalPolicy = ThermalPolicy
  { policyLimit :: MilliCelsius
  , policyPoll :: PollMicroseconds
  }

data ThermalState
  = Cool MilliCelsius
  | TooHot MilliCelsius
  | SensorUnavailable String
  deriving (Eq, Show)

data LimitValue = LimitBytes Integer | LimitUnlimited

productionThermalPolicy :: ThermalPolicy
productionThermalPolicy = ThermalPolicy
  { policyLimit = MilliCelsius 85000
  , policyPoll = PollMicroseconds 1_000_000
  }

thermalFailureExit :: ExitCode
thermalFailureExit = ExitFailure 75

discoverThermalSensor :: SysfsRoot -> IO (Either String ThermalSensor)
discoverThermalSensor (SysfsRoot root) = do
  let hwmon = root </> "class" </> "hwmon"
  present <- doesDirectoryExist hwmon
  if not present then pure (Left ("missing hwmon directory: " ++ hwmon)) else do
    entries <- try (listDirectory hwmon) :: IO (Either IOException [String])
    case entries of
      Left failure -> pure (Left ("cannot list hwmon: " ++ show failure))
      Right names -> do
        found <- fmap concat $ forM (filter isHwmon names) (discoverEntry . (hwmon </>))
        pure $ case found of
          [sensor] -> Right sensor
          [] -> Left "no k10temp Tctl sensor"
          _ -> Left ("ambiguous k10temp Tctl sensors: " ++ show (length found))
  where
    isHwmon name = "hwmon" `isPrefixOf` name && all isDigit (drop 5 name)
    discoverEntry directory = do
      name <- readText (directory </> "name")
      case name of
        Just value | trim value == "k10temp" -> do
          entries <- try (listDirectory directory) :: IO (Either IOException [String])
          case entries of
            Left _ -> pure []
            Right labels -> fmap catMaybes $ forM labels $ \labelFile -> do
              let number = tempNumber labelFile
              case number of
                Nothing -> pure Nothing
                Just index -> do
                  label <- readText (directory </> ("temp" ++ show index ++ "_label"))
                  case label of
                    Just text | trim text == "Tctl" -> do
                      let input = directory </> ("temp" ++ show index ++ "_input")
                      exists <- doesFileExist input
                      pure (if exists then Just (ThermalSensor input) else Nothing)
                    _ -> pure Nothing
        _ -> pure []

tempNumber :: FilePath -> Maybe Int
tempNumber name = do
  rest <- stripPrefix "temp" name
  digits <- stripSuffix "_label" rest
  if not (null digits) && all isDigit digits then readMaybe digits else Nothing

stripPrefix :: String -> String -> Maybe String
stripPrefix prefix text
  | prefix `isPrefixOf` text = Just (drop (length prefix) text)
  | otherwise = Nothing

stripSuffix :: String -> String -> Maybe String
stripSuffix suffix text
  | suffix `isSuffixOf` text = Just (take (length text - length suffix) text)
  | otherwise = Nothing

readText :: FilePath -> IO (Maybe String)
readText path = do
  result <- try (readStrict path) :: IO (Either IOException String)
  pure (either (const Nothing) Just result)

readStrict :: FilePath -> IO String
readStrict path = withFile path ReadMode $ \handle -> do
  text <- hGetContents handle
  _ <- evaluate (length text)
  pure text

trim :: String -> String
trim = dropWhileEnd isSpace . dropWhile isSpace

readThermalState :: SysfsRoot -> ThermalSensor -> ThermalPolicy -> IO ThermalState
readThermalState root selected policy = do
  current <- discoverThermalSensor root
  case current of
    Left reason -> pure (SensorUnavailable reason)
    Right found
      | found /= selected -> pure (SensorUnavailable "selected sensor identity changed")
      | otherwise -> readValue found
  where
    readValue (ThermalSensor path) = do
      result <- try (readStrict path) :: IO (Either IOException String)
      pure $ case result of
        Left failure -> SensorUnavailable (show failure)
        Right text -> case readMaybe (trim text) of
          Just value | value > 0 -> classify policy (MilliCelsius value)
          _ -> SensorUnavailable "sensor is not one positive integer"

classify :: ThermalPolicy -> MilliCelsius -> ThermalState
classify policy value@(MilliCelsius reading)
  | reading >= limit = TooHot value
  | otherwise = Cool value
  where
    MilliCelsius limit = policyLimit policy

-- | Runs as the service MainPID. The target is a direct child; systemd's
-- KillMode=control-group removes the child when this process exits or dies.
runGuarded :: SysfsRoot -> ThermalSensor -> ThermalPolicy -> FilePath -> [String] -> IO ExitCode
runGuarded root sensor policy command arguments = do
  context <- validateWorkerService
  case context of
    Left reason -> ioError (userError reason)
    Right () -> do
      initial <- readThermalState root sensor policy
      case initial of
        Cool _ -> start
        TooHot value -> failClosed ("sensor is " ++ show value)
        SensorUnavailable reason -> failClosed ("sensor unavailable: " ++ reason)
  where
    failClosed reason = do
      hPutStrLn stderr ("thermal-guard: " ++ reason)
      pure thermalFailureExit
    start = do
      failure <- newEmptyMVar
      thread <- myThreadId
      (_, _, _, process) <- createProcess (proc command arguments)
        { std_in = Inherit
        , std_out = Inherit
        , std_err = Inherit
        }
      bracket (forkIO (watch failure thread)) killThread $ \_ -> do
        code <- waitForProcess process
        observed <- tryReadMVar failure
        case observed of
          Nothing -> pure code
          Just reason -> do
            hPutStrLn stderr ("thermal-guard: " ++ reason)
            pure thermalFailureExit
    watch failure thread = handleWatcher
      where
        handleWatcher = do
          result <- tryJust notAsync loop
          case result of
            Right () -> pure ()
            Left problem -> do
              let reason = "watchdog failed: " ++ show problem
              void (tryPutMVar failure reason)
              throwTo thread (userError reason)
        loop = do
          let PollMicroseconds delay = policyPoll policy
          threadDelay delay
          state <- readThermalState root sensor policy
          case state of
            Cool _ -> loop
            TooHot value -> stop ("sensor is " ++ show value)
            SensorUnavailable reason -> stop ("sensor unavailable: " ++ reason)
        stop reason = do
          void (tryPutMVar failure reason)
          throwTo thread (userError reason)
        notAsync exception = case fromException exception of
          Just ThreadKilled -> Nothing
          _ -> Just exception

-- | The internal worker must be the MainPID of the transient service created
-- by the public launcher, and that unit must carry the guard's KillMode.
validateWorkerService :: IO (Either String ())
validateWorkerService = do
  path <- readOwnScope
  let unit = takeFileName path
  if not ("thermal-guard-" `isPrefixOf` unit && ".service" `isSuffixOf` unit)
    then pure (Left "worker is not in a thermal-guard service")
    else do
      pid <- getProcessID
      mainPid <- systemctl 10_000_000
        ["--user", "show", "-p", "MainPID", "--value", unit]
      killMode <- systemctl 10_000_000
        ["--user", "show", "-p", "KillMode", "--value", unit]
      pure $ case (mainPid, killMode) of
        (Right (ExitSuccess, value, _), Right (ExitSuccess, mode, _))
          | readMaybe (trim value) == Just pid && trim mode == "control-group" -> Right ()
          | otherwise -> Left "worker is not the owned guard service MainPID"
        _ -> Left "cannot validate guard service"

-- | The caller's memory and swap limits, parsed at the cgroup boundary.
callerMemoryProperties :: IO [String]
callerMemoryProperties = do
  path <- readOwnScope
  let directory = "/sys/fs/cgroup" ++ path
  memory <- readLimit (directory </> "memory.max")
  swap <- readLimit (directory </> "memory.swap.max")
  pure
    [ "--property=MemoryMax=" ++ renderLimit memory
    , "--property=MemorySwapMax=" ++ renderLimit swap
    ]

readOwnScope :: IO FilePath
readOwnScope = do
  text <- readStrict "/proc/self/cgroup"
  case [path | line <- lines text, Just path <- [stripPrefix "0::" line], not (null path)] of
    (path : _) -> pure path
    _ -> ioError (userError "process is not inside a systemd cgroup")

readLimit :: FilePath -> IO LimitValue
readLimit path = do
  value <- trim <$> readStrict path
  case value of
    "max" -> pure LimitUnlimited
    digits | not (null digits) && all isDigit digits ->
      pure (LimitBytes (read digits))
    _ -> ioError (userError ("malformed cgroup limit: " ++ path))

renderLimit :: LimitValue -> String
renderLimit LimitUnlimited = "infinity"
renderLimit (LimitBytes value) = show value

systemctl :: Int -> [String] -> IO (Either String (ExitCode, String, String))
systemctl microseconds arguments = do
  result <- try (timeout microseconds (readProcessWithExitCode "systemctl" arguments ""))
    :: IO (Either IOException (Maybe (ExitCode, String, String)))
  pure $ case result of
    Left failure -> Left (show failure)
    Right Nothing -> Left "systemctl timed out"
    Right (Just value) -> Right value
