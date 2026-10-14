{-# LANGUAGE ScopedTypeVariables #-}
-- af-session: host annotation of one external recording for the E01 stock
-- pilot. Reads video only; never opens a camera or USB device.
module Main (main) where

import Control.Exception (IOException, catch)
import FujiAF.Session (commandAnnotate, commandExtract, commandFrames, commandMeasure)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)

main :: IO ()
main = (dispatch `catch` \(e :: IOException) -> hPutStrLn stderr ("af-session: " ++ show e) >> exitFailure)

dispatch :: IO ()
dispatch = do
  args <- getArgs
  case args of
    ("frames":rest) -> commandFrames rest
    ("extract":rest) -> commandExtract rest
    ("annotate":rest) -> commandAnnotate rest
    ("measure":rest) -> commandMeasure rest
    ["--help"] -> putStr help
    _ -> hPutStrLn stderr help >> exitFailure

help :: String
help = unlines
  [ "af-session: external-video timing annotations for the E01 stock pilot"
  , ""
  , "Usage:"
  , "  af-session frames --video FILE [--ffprobe PATH] [--sha256sum PATH] [--out FILE]"
  , "      list presentation timestamps, spacing and the video digest"
  , "  af-session extract --video FILE --frames A-B,C --out DIR [--ffmpeg PATH]"
  , "      write the selected frames as PNGs named by frame index"
  , "  af-session annotate --video FILE --frames LISTING --session-id ID --trial-id ID"
  , "      --scene-id P01 --start near --start-before N --start-after N"
  , "      --confirmation-before N --confirmation-after N"
  , "      [--barrel-before N --barrel-after N] [--s1release-before N --s1release-after N]"
  , "      [--s2press-before N --s2press-after N] [--capture-before N --capture-after N] [--sync-uncertainty-ms MS]"
  , "      [--ambiguous-start|confirmation|barrel|s1release|s2press|capture]"
  , "      [--pair-id ID] [--roi TEXT] [--target-plane TEXT] [--sweep-id ID]"
  , "      [--hunting] [--focus-restart] [--wrong-plane-suspected]"
  , "      [--invalid-reason TEXT] [--notes TEXT] [--sha256sum PATH] [--out FILE]"
  , "      write one AF session annotation record"
  , "  af-session measure --image FILE --roi X:Y:W:H [--sweep-max VALUE] [--ffmpeg PATH] [--ffprobe PATH] [--out FILE]"
  , "      mean central-gradient acutance of a saved photograph's ROI"
  ]
