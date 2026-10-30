-- Build a declared loader-A completion control DAT from a reviewed X-M1 1.50
-- container: `held` stops core 0 before the completion store at 0x40a08;
-- `non-one` makes that store write 0x400a0000 instead of 1.
module Main where

import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import FujiAF.FirstStage
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)

data Control = Held | NonOne

parseControl :: String -> Either String Control
parseControl "held" = Right Held
parseControl "non-one" = Right NonOne
parseControl other = Left ("unsupported completion control: " ++ other)

loaderA :: LoaderId
loaderA = LoaderId 0

controlPatch :: Control -> Either String LoaderWordPatch
-- 0x40a00 `mov r5, #1` becomes `b 0x40a00`.
controlPatch Held = loaderWordPatch loaderA 0x40a00 0xe3a05001 0xeafffffe
-- 0x40a08 `str r5, [r3, #0xc4]` becomes `str r3, [r3, #0xc4]`; r3 holds the
-- 0x40afc literal 0x400a0000.
controlPatch NonOne = loaderWordPatch loaderA 0x40a08 0xe58350c4 0xe58330c4

maxContainer :: Int
maxContainer = 64 * 1024 * 1024

checksPass :: String -> [U.Check] -> IO ()
checksPass title results = do
  forM_ [check | check <- results, not (U.checkPassed check)] $ \check ->
    hPutStrLn stderr ("FAIL " ++ title ++ " " ++ U.checkField check ++ ": stored " ++ U.checkStored check ++ ", expected " ++ U.checkExpected check)
  unless (all U.checkPassed results) exitFailure

main :: IO ()
main = do
  arguments <- getArgs
  (source, output, control) <- case arguments of
    [source, output, "--control", name] -> either H.failH (pure . (,,) source output) (parseControl name)
    _ -> H.failH "usage: build-completion-control SOURCE_DAT OUT_DAT --control held|non-one"
  bytes <- H.readRegular maxContainer source
  parsed <- either H.failH pure (U.parseContainer bytes)
  checksPass "source" (U.checks parsed ++ U.carriedChecks parsed)
  patch <- either H.failH pure (controlPatch control)
  updated <- either H.failH pure (applyLoaderWordPatches [patch] (U.parsedParts parsed))
  rebuilt <- either H.failH pure (U.rebuildContainer parsed updated)
  reparsed <- either H.failH pure (U.parseContainer rebuilt)
  checksPass "control" (U.checks reparsed ++ U.carriedChecks reparsed)
  either H.failH pure (verifyLoaderWordPatches [patch] parsed reparsed)
  H.ensure (B.length rebuilt == B.length bytes) "control DAT length differs"
  offset <- either H.failH pure (loaderWordPatchOffset updated patch)
  digest <- H.sha256 rebuilt
  B.writeFile output rebuilt
  putStrLn
    ( "completion control: loader word "
        ++ U.showHexWord (loaderPatchAddress patch)
        ++ " "
        ++ U.showHexWord (loaderPatchExpected patch)
        ++ " -> "
        ++ U.showHexWord (loaderPatchReplacement patch)
        ++ " at DAT offset "
        ++ show (U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader parsed)) + offset)
        ++ ", output "
        ++ output
        ++ ", sha256 "
        ++ digest
    )
