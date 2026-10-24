{-# LANGUAGE OverloadedStrings #-}
-- | The flash-boot route on jitl's FujiMod runtime through the X-M1 driver
-- (@emulation/fujimod-xm1@). 'validateFirmware' admits the DAT; the driver
-- answers the probe protocol, so 'judgeProbeOutput' judges it as for
-- @//emulation:flash-boot-check@.
module Main where

import Control.Exception (IOException, catch, displayException)
import Data.Aeson (Value (..), encode, object, toJSON, (.=))
import qualified Data.ByteString.Lazy as BL
import Data.List (sort)
import Emulation.EmulatorSlot (reexecInSlot, slotPolicyFromEnvironment)
import Emulation.FlashBootRoute
import Emulation.ViewerRuntime (DefaultRuntime (..), Firmware (..), Runtime (..), freshRuntime, requiredEnvironment, resolve, validateFirmware, workspace)
import GHC.Clock (getMonotonicTime)
import System.Directory (getFileSize, listDirectory)
import System.Environment (getArgs)
import System.Exit (ExitCode, exitFailure)
import System.FilePath ((</>), takeExtension)
import System.IO (hPutStrLn, stderr)
import System.Process (proc, readCreateProcessWithExitCode, readProcess)

usage :: String
usage = unlines
  [ "Usage: bazel run //emulation:fujimod-flash-boot-check -- --firmware DAT [--output DIR]"
  , ""
  , "Cold-boot the exact DAT bytes with the captured X-M1 ROM on FujiMod's EXR II runtime,"
  , "drive the route of //emulation:flash-boot-check and exit 0 only at SHOOTING MENU"
  , "(menu 1, setting 5, cursor 2) with no model stop. Needs FUJIMOD_XM1_DRIVER and"
  , "FUJIMOD_XM1_RUN from the devenv shell. Host-only: no camera or card-reader I/O."
  , ""
  , "  --firmware PATH  Candidate X-M1 1.50-compatible DAT"
  , "  --output PATH    Fresh directory beneath workspace/private-data/"
  , "                   (default: private-data/fujimod-flash-boot-check-PID-MICROS)"
  ]

data Command = Help | Check (Maybe Runtime) Firmware

parseCommand :: [String] -> IO Command
parseCommand arguments
  | "--help" `elem` arguments = pure Help
  | otherwise = go Nothing Nothing arguments
  where
    go output (Just firmware) [] = pure (Check output firmware)
    go _ Nothing [] = ioError (userError ("--firmware is required\n" ++ usage))
    go output _ ("--firmware" : value : rest) | not (null value) = go output (Just (Firmware value)) rest
    go _ firmware ("--output" : value : rest) | not (null value) = go (Just (Runtime value)) firmware rest
    go _ _ (argument : _) = ioError (userError ("Unknown option or missing value: " ++ argument ++ "\n" ++ usage))

sha256File :: FilePath -> IO Sha256
sha256File path = do
  output <- readProcess "sha256sum" ["--", path] ""
  maybe (ioError (userError ("sha256sum returned no digest for " ++ path))) pure (parseSha256 (take 64 output))

menuValue :: MenuState -> Value
menuValue (MenuState menu setting cursor) = object ["menu_id" .= menu, "setting_id" .= setting, "cursor" .= cursor]

milestoneValue :: Milestone -> Value
milestoneValue (Milestone name observed) = object
  [ "phase" .= name
  , "instructions" .= observedInstructions observed
  , "menu" .= fmap menuValue (observedMenu observed)
  , "frame_sha256" .= fmap (toJSON . sha256Text) (observedFrame observed)
  ]

failureValue :: CheckFailure -> Value
failureValue failure = case failure of
  BootStopped message _ -> record "boot-stopped" Nothing message
  ModelStopped phase message -> record "model-stopped" (Just phase) message
  InputRefused phase message -> record "input-refused" (Just phase) message
  DriveEnded phase message -> record "drive-ended" (Just phase) message
  MenuMissed reached -> object ["kind" .= ("menu-missed" :: String), "reached_menu" .= fmap menuValue reached]
  where
    record :: String -> Maybe Phase -> String -> Value
    record kind phase message = object ["kind" .= kind, "phase" .= fmap phaseName phase, "message" .= message]

exitText :: ExitCode -> String -> String
exitText code errors = unlines (reverse (take 3 (reverse (lines errors)))) ++ "exit: " ++ show code

check :: Maybe Runtime -> Firmware -> IO ()
check output (Firmware supplied) = do
  root <- workspace Nothing
  let dat = resolve root supplied
      rom = resolve root "private-data/analysis/x-m1-1.50/bootrom/bootrom-0x80000000-4000.bin"
  validateFirmware (Firmware dat)
  arguments <- getArgs
  policy <- slotPolicyFromEnvironment
  reexecInSlot policy arguments
  driver <- requiredEnvironment "FUJIMOD_XM1_DRIVER"
  run <- requiredEnvironment "FUJIMOD_XM1_RUN"
  Runtime directory <- freshRuntime root output (UniqueRuntime "fujimod-flash-boot-check")
  let commands = [probeCommand step | (_, steps) <- freshBootRoute, step <- steps] ++ ["quit"]
  started <- getMonotonicTime
  (code, transcript, errors) <- readCreateProcessWithExitCode
    (proc driver ["--run", run, "--update", dat, "--boot-rom", rom, "--output", directory </> "frames"])
    (unlines commands)
  finished <- getMonotonicTime
  writeFile (directory </> "drive.stdout") transcript
  writeFile (directory </> "drive.stderr") errors
  putStr transcript
  let DriveResult milestones final failure = judgeProbeOutput (exitText code errors) transcript
  datDigest <- sha256File dat
  datBytes <- getFileSize dat
  driverDigest <- sha256File driver
  runDigest <- sha256File run
  romDigest <- sha256File rom
  pngs <- map ((directory </> "frames") </>) . filter ((== ".png") . takeExtension) . sort <$> listDirectory (directory </> "frames")
  let receiptPath = directory </> "fujimod-flash-boot-receipt.json"
      receipt = object
        [ "schema" .= (1 :: Int)
        , "scope" .= ("emulated X-M1 1.50 cold boot of an exact DAT to SHOOTING MENU on FujiMod's EXR II runtime" :: String)
        , "result" .= (maybe "passed" (const "failed") failure :: String)
        , "failure" .= fmap failureValue failure
        , "firmware" .= object ["path" .= dat, "bytes" .= datBytes, "sha256" .= sha256Text datDigest]
        , "emulator_source" .= object
            [ "driver" .= object ["path" .= driver, "sha256" .= sha256Text driverDigest]
            , "run_document" .= object ["path" .= run, "sha256" .= sha256Text runDigest]
            , "rom_sha256" .= sha256Text romDigest
            ]
        , "drive_wall_seconds" .= (finished - started)
        , "route" .= [object ["phase" .= phaseName phase, "commands" .= map probeCommand steps] | (phase, steps) <- freshBootRoute]
        , "expected_menu" .= menuValue shootingMenu
        , "milestones" .= map milestoneValue milestones
        , "instructions" .= fmap observedInstructions final
        , "frame_sha256" .= (final >>= fmap (toJSON . sha256Text) . observedFrame)
        , "frame_png" .= pngs
        ]
  BL.writeFile receiptPath (encode receipt <> "\n")
  putStrLn ("fujimod-flash-boot-check receipt: " ++ receiptPath)
  case failure of
    Nothing -> putStrLn ("fujimod-flash-boot-check: passed: SHA-256 " ++ sha256Text datDigest ++ " reached SHOOTING MENU")
    Just stopped -> do
      hPutStrLn stderr ("fujimod-flash-boot-check: failed: SHA-256 " ++ sha256Text datDigest ++ ": " ++ show stopped)
      exitFailure

main :: IO ()
main = (getArgs >>= parseCommand >>= run) `catch` report
  where
    run Help = putStr usage
    run (Check output firmware) = check output firmware
    report :: IOException -> IO ()
    report errorValue = hPutStrLn stderr (displayException errorValue) >> exitFailure
