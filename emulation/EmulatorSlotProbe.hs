-- | Probe for @//emulation:emulator-slot-tests@: takes an emulator slot
-- through the library, reports reaching the in-slot run and holds the slot
-- until its stdin closes.
module Main (main) where

import Control.Exception (IOException, bracket, catch, displayException, evaluate, try)
import Data.List (isInfixOf, isPrefixOf)
import Data.Maybe (fromMaybe, listToMaybe)
import Emulation.EmulatorSlot (ExecutionClass (..), admissionGrant, grantPurpose, reexecInSlot, slotPolicyForClass)
import Foreign.Marshal.Alloc (free, mallocBytes)
import Foreign.Marshal.Utils (fillBytes)
import System.Environment (getEnvironment, getExecutablePath, lookupEnv)
import System.Exit (ExitCode (..), exitFailure, exitWith)
import System.IO (Handle, hClose, hFlush, hGetLine, hPutStrLn, stdout, stderr)
import System.Posix.Process (getProcessID)
import System.Posix.Types (ProcessID)
import System.Process (CreateProcess (..), StdStream (CreatePipe), createProcess, getPid, proc, readCreateProcessWithExitCode, readProcessWithExitCode, waitForProcess)
import System.Timeout (timeout)
import Text.Read (readMaybe)

main :: IO ()
main = run `catch` \failure -> do
  hPutStrLn stderr ("probe-failed " ++ displayException (failure :: IOException))
  exitFailure
  where
    run = do
      putStrLn "probe-starting"
      hFlush stdout
      selected <- lookupEnv "XM1_PROBE_CLASS"
      legacy <- lookupEnv "XM1_PROBE_LEGACY"
      policy <- slotPolicyForClass (if selected == Just "native" then NativeVerification else FullSystem)
      case legacy of
        Just "1" -> pure ()
        _ -> reexecInSlot policy []
      slot <- lookupEnv "XM1_EMULATOR_SLOT"
      groups <- readFile "/proc/self/cgroup"
      case [drop 3 line | line <- lines groups, take 3 line == "0::"] of
        [group] -> do
          putStrLn ("probe-unit " ++ reverse (takeWhile (/= '/') (reverse group)))
          limit <- try (readFile ("/sys/fs/cgroup" ++ group ++ "/memory.max") >>= \text -> evaluate (length text) >> pure text)
            :: IO (Either IOException String)
          case limit of
            Right value -> putStrLn ("probe-memory-max " ++ unwords (words value))
            Left _ -> pure ()
          swapLimit <- try (readFile ("/sys/fs/cgroup" ++ group ++ "/memory.swap.max") >>= \text -> evaluate (length text) >> pure text)
            :: IO (Either IOException String)
          case swapLimit of
            Right value -> putStrLn ("probe-memory-swap-max " ++ unwords (words value))
            Left _ -> pure ()
        _ -> pure ()
      pid <- getProcessID
      putStrLn ("probe-pid " ++ show pid)
      survivor <- lookupEnv "XM1_PROBE_SURVIVOR"
      case survivor of
        Just "1" -> do
          (_, _, _, child) <- createProcess (proc "sleep" ["600"])
          childPid <- getPid child
          putStrLn ("probe-survivor " ++ maybe "unknown" show childPid)
        _ -> pure ()
      nested <- lookupEnv "XM1_PROBE_NESTED"
      case nested of
        Just "1" -> do
          self <- getExecutablePath
          (code, _, _) <- readProcessWithExitCode self [] ""
          putStrLn ("probe-nested-exit " ++ show code)
        _ -> pure ()
      grant <- lookupEnv "XM1_PROBE_GRANT"
      case grant of
        Nothing -> pure ()
        Just mode -> runGrantProbe mode
      withProbeAllocation $ do
        putStrLn ("probe-in-slot " ++ fromMaybe "unassigned" slot)
        hFlush stdout
        _ <- getContents >>= evaluate . length
        status <- lookupEnv "XM1_PROBE_EXIT"
        exitWith (maybe ExitSuccess ExitFailure (status >>= readMaybe))

-- | Grants the one child this admitted process spawns the right to run in the
-- parent's slot, or spawns a child under a wrong or inherited grant.
runGrantProbe :: String -> IO ()
runGrantProbe mode = do
  pid <- getProcessID
  valid <- grantValue pid "probe-child"
  case mode of
    "valid" -> do
      (code, out) <- spawnGrantChild (Just valid) "none" []
      putStrLn ("probe-grant-exit " ++ show code ++ " " ++ show ("probe-in-slot" `isInfixOf` out))
      reportChild out
    "other" -> do
      other <- grantValue (pid + 1) "probe-child"
      (code, out) <- spawnGrantChild (Just other) "none" []
      putStrLn ("probe-grant-exit " ++ show code ++ " " ++ show ("probe-in-slot" `isInfixOf` out))
      putStrLn ("probe-grant-error " ++ show ("names another process" `isInfixOf` out))
    "wrong-slot" -> do
      other <- otherSlot
      (code, out) <- spawnGrantChild (Just valid) "none" [("XM1_EMULATOR_SLOT", other)]
      putStrLn ("probe-wrong-slot-exit " ++ show code ++ " " ++ show ("probe-in-slot" `isInfixOf` out))
      putStrLn ("probe-wrong-slot-error " ++ show ("does not name the slot the owner holds" `isInfixOf` out))
    "no-purpose" -> do
      (code, out) <- spawnGrantChild (Just (show pid ++ ":")) "none" []
      putStrLn ("probe-no-purpose-exit " ++ show code ++ " " ++ show ("probe-in-slot" `isInfixOf` out))
      putStrLn ("probe-no-purpose-error " ++ show ("needs a purpose" `isInfixOf` out))
    "moved" -> do
      self <- getExecutablePath
      environment <- getEnvironment
      let overrides = [("XM1_ADMISSION_INHERIT", valid), ("XM1_PROBE_GRANT", "none")]
      (code, out, err) <- readCreateProcessWithExitCode
        (proc "systemd-run" ["--user", "--scope", "--quiet", "--collect", "--", self])
        {env = Just (overrides ++ environment)} ""
      let observed = out ++ err
      putStrLn ("probe-moved-exit " ++ show code ++ " " ++ show ("probe-in-slot" `isInfixOf` observed))
      putStrLn ("probe-moved-error " ++ show ("runs outside the owner's cgroup" `isInfixOf` observed))
    "second" -> runSecondGrantedChild valid
    "regrant" -> do
      (code, out) <- spawnGrantChild (Just valid) "spawn-regrant" []
      putStrLn ("probe-grant-exit " ++ show code ++ " " ++ show ("probe-in-slot" `isInfixOf` out))
      putStrLn ("probe-regrant-line " ++ show ("probe-regrant-exit ExitFailure 1 False" `isInfixOf` out))
    "spawn-regrant" -> do
      mine <- grantValue pid "probe-grandchild"
      (code, out) <- spawnGrantChild (Just mine) "none" []
      putStrLn ("probe-regrant-exit " ++ show code ++ " " ++ show ("probe-in-slot" `isInfixOf` out))
    "grandchild" -> do
      (code, out) <- spawnGrantChild (Just valid) "spawn-grandchild" []
      putStrLn ("probe-grant-exit " ++ show code ++ " " ++ show ("probe-in-slot" `isInfixOf` out))
      putStrLn ("probe-grandchild-line " ++ show ("probe-grandchild-exit ExitFailure 1 True" `isInfixOf` out))
    "spawn-grandchild" -> do
      (code, out) <- spawnGrantChild Nothing "none" []
      putStrLn ("probe-grandchild-exit " ++ show code ++ " " ++ show ("is not from the parent process" `isInfixOf` out))
    "none" -> pure ()
    _ -> ioError (userError "unknown XM1_PROBE_GRANT mode")
 where
  reportChild out = do
    putStrLn ("probe-grant-slot " ++ fromMaybe "none" (outputValue "probe-in-slot " out))
    putStrLn ("probe-grant-unit " ++ fromMaybe "none" (outputValue "probe-unit " out))
    putStrLn ("probe-grant-memory-max " ++ fromMaybe "none" (outputValue "probe-memory-max " out))

-- | Holds one granted child open, then starts a second under the same grant.
runSecondGrantedChild :: String -> IO ()
runSecondGrantedChild grant = do
  self <- getExecutablePath
  environment <- getEnvironment
  let overrides = [("XM1_ADMISSION_INHERIT", grant), ("XM1_PROBE_GRANT", "none")]
  (Just input, Just out, _, first) <- createProcess (proc self [])
    { env = Just (overrides ++ environment), std_in = CreatePipe, std_out = CreatePipe }
  ready <- awaitSlot out
  (code, secondOut) <- spawnGrantChild (Just grant) "none" []
  hClose input
  _ <- timeout 15_000_000 (waitForProcess first)
  putStrLn ("probe-second-first " ++ show ready)
  putStrLn ("probe-second-exit " ++ show code ++ " " ++ show ("probe-in-slot" `isInfixOf` secondOut))
  putStrLn ("probe-second-error " ++ show ("another granted child" `isInfixOf` secondOut))

awaitSlot :: Handle -> IO Bool
awaitSlot out = fromMaybe False <$> timeout 15_000_000 go
  where
    go = do
      line <- try (hGetLine out) :: IO (Either IOException String)
      case line of
        Left _ -> pure False
        Right text
          | "probe-in-slot " `isPrefixOf` text -> pure True
          | otherwise -> go

outputValue :: String -> String -> Maybe String
outputValue prefix out =
  listToMaybe [drop (length prefix) line | line <- lines out, prefix `isPrefixOf` line]

grantValue :: ProcessID -> String -> IO String
grantValue pid purpose =
  either (ioError . userError) (pure . snd . admissionGrant pid) (grantPurpose purpose)

otherSlot :: IO String
otherSlot = do
  mine <- fromMaybe "" <$> lookupEnv "XM1_EMULATOR_SLOT"
  pure (if mine == "1" then "2" else "1")

spawnGrantChild :: Maybe String -> String -> [(String, String)] -> IO (ExitCode, String)
spawnGrantChild grant childMode extra = do
  self <- getExecutablePath
  environment <- getEnvironment
  let overrides = maybe [] (\value -> [("XM1_ADMISSION_INHERIT", value)]) grant
        ++ [("XM1_PROBE_GRANT", childMode)] ++ extra
  (code, out, err) <- readCreateProcessWithExitCode (proc self []) {env = Just (overrides ++ environment)} ""
  pure (code, out ++ err)

newtype AllocationBytes = AllocationBytes Int

withProbeAllocation :: IO a -> IO a
withProbeAllocation action = do
  requested <- lookupEnv "XM1_PROBE_ALLOCATE_BYTES"
  case requested of
    Nothing -> action
    Just text -> case AllocationBytes <$> readMaybe text of
      Just (AllocationBytes bytes) | bytes > 0 && toInteger bytes <= 8 * 1024 * 1024 * 1024 ->
        bracket (mallocBytes bytes) free $ \buffer -> do
          fillBytes buffer 165 bytes
          putStrLn ("probe-allocated-bytes " ++ show bytes)
          hFlush stdout
          action
      _ -> ioError (userError "XM1_PROBE_ALLOCATE_BYTES must be positive and at most 8589934592")
