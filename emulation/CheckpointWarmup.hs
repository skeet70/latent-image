-- | Saves the shared portable checkpoints for the current tree: the RAM-push
-- card, E8 hold-Q and cardless attach checkpoints and the USB-toggle route
-- checkpoints. Each launcher restores a checkpoint whose key already exists
-- and cold-boots and saves the others, holding its own emulator slot.
module Main (main) where

import Control.Exception (IOException, catch, displayException)
import Control.Monad (forM, forM_, unless)
import Emulation.ViewerRuntime (Workspace (..), workspace)
import Data.Time.Clock.POSIX (getPOSIXTime)
import System.Directory (createDirectoryIfMissing, doesFileExist)
import System.Environment (getArgs, getExecutablePath)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>))
import System.IO (IOMode (WriteMode), hPutStrLn, stderr, withFile)
import System.Process (CreateProcess (..), StdStream (..), createProcess, proc, waitForProcess)

data Launcher = Xm1Viewer | UsbToggleEmu

launcherName :: Launcher -> String
launcherName Xm1Viewer = "xm1-viewer"
launcherName UsbToggleEmu = "usb-toggle-emu"

-- | One checkpoint-saving run: a name for its log, the launcher and its arguments.
data Warm = Warm String Launcher [String]

prefixDat, rc0ToggleDat, storedF7One, storedF7Zero, cardImage :: FilePath
prefixDat = "private-data/analysis/x-m1-1.50/update/prefix/FWUP0002-prefix.DAT"
rc0ToggleDat = "private-data/analysis/usb-toggle/rc0-pinned-dat/FWUP0002.DAT"
storedF7One = "private-data/analysis/x-m1-1.50/stored-f7/emulation/inputs/f7-1.nand"
storedF7Zero = "private-data/analysis/usb-toggle/emu/inputs/f7-0.nand"
cardImage = "private-data/analysis/classic-chrome-patch/ram-push/inputs/card.img"

-- | The inputs of the RAM-push e2e routes (recipe bank, Classic Chrome and its
-- icon share the card attach checkpoint) and of USB-toggle E8, then the
-- usb-toggle-emu route boots.
warms :: FilePath -> [Warm]
warms directory =
  [ attach "card" ["--firmware", prefixDat, "--installed-nand", storedF7One, "--sd-card-image", cardImage]
  , attach "e8-hold-q" ["--attach-hold-q", "--firmware", rc0ToggleDat, "--installed-nand", storedF7Zero, "--sd-card-image", cardImage]
  , attach "cardless" ["--cardless", "--firmware", prefixDat, "--installed-nand", storedF7One]
  , Warm "usb-toggle-route" UsbToggleEmu ["--warm-checkpoints", "--receipt-output", directory </> "usb-toggle-route"]
  ]
  where
    attach name inputs = Warm name Xm1Viewer
      (["--ram-push-e2e", "--warm-checkpoint", "--output", directory </> ("runtime-" ++ name)] ++ inputs)

runfile :: Launcher -> IO FilePath
runfile launcher = do
  executable <- getExecutablePath
  let path = (executable ++ ".runfiles") </> "_main" </> "emulation" </> launcherName launcher
  present <- doesFileExist path
  unless present (ioError (userError ("launcher missing from runfiles: " ++ path)))
  pure path

main :: IO ()
main = (getArgs >>= run) `catch` \problem -> do
  hPutStrLn stderr (displayException (problem :: IOException))
  exitFailure

run :: [String] -> IO ()
run arguments = do
  unless (null arguments) (ioError (userError
    "Usage: bazel run //emulation:checkpoint-warmup\n\nSaves the shared emulator checkpoints of this tree; takes no options."))
  micros <- (round . (* 1000000) <$> getPOSIXTime) :: IO Integer
  Workspace root <- workspace Nothing
  let directory = root </> "private-data" </> "checkpoint-warmup" </> show micros
  createDirectoryIfMissing True directory
  putStrLn ("checkpoint-warmup logs: " ++ directory)
  started <- forM (warms directory) $ \(Warm name launcher launcherArguments) -> do
    executable <- runfile launcher
    let logPath = directory </> (name ++ ".log")
    (_, _, _, process) <- withFile logPath WriteMode $ \logHandle ->
      createProcess (proc executable launcherArguments)
        { std_in = NoStream, std_out = UseHandle logHandle, std_err = UseHandle logHandle }
    pure (name, logPath, process)
  results <- forM started $ \(name, logPath, process) -> do
    code <- waitForProcess process
    putStrLn (name ++ ": " ++ either id (const "saved or present") (status code) ++ " (" ++ logPath ++ ")")
    pure code
  forM_ results $ \code -> unless (code == ExitSuccess) exitFailure
  where
    status ExitSuccess = Right ()
    status (ExitFailure code) = Left ("failed with exit " ++ show code)
