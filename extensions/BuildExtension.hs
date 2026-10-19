module Main (main) where

import Control.Exception (onException)
import Control.Monad (foldM, when)
import Data.Bits (complement)
import qualified Data.ByteString as B
import Data.List (isPrefixOf)
import ExtensionManifest
import qualified FujiAF.Host as H
import qualified FujiAF.LabelRecord as L
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)
import RecoveryFirmware
import System.Directory (canonicalizePath, createDirectory, doesPathExist, removeDirectoryRecursive, renameFile)
import System.Environment (getArgs)
import System.FilePath (addTrailingPathSeparator, takeDirectory, takeFileName, (</>))
import System.IO (hFlush, stdout)
import System.Process (callProcess)

data Tools = Tools
  { toolDecoder :: FilePath
  , toolApplier :: FilePath
  , toolEncoder :: FilePath
  , toolRebuild :: FilePath
  }

-- Unpinned skips the output_sha256 comparison and reports the digest instead.
data Pinning = Pinned | Unpinned

usage :: String
usage = "usage: build-extension [--unpinned] EXTENSION_JSON STOCK_DAT NEW_OUTPUT_DIRECTORY"

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [decoder, applier, encoder, rebuild, "--unpinned", manifest, stock, output] ->
      build (Tools decoder applier encoder rebuild) Unpinned manifest stock output
    [decoder, applier, encoder, rebuild, manifest, stock, output] ->
      build (Tools decoder applier encoder rebuild) Pinned manifest stock output
    _ -> H.failH usage

build :: Tools -> Pinning -> FilePath -> FilePath -> FilePath -> IO ()
build tools pinning givenManifest givenStock givenOutput = do
  manifestPath <- resolve givenManifest >>= canonicalizePath
  extension <- parseExtension manifestPath
  firmware <- readFirmware givenStock
  let FirmwareDigest stockDigest = firmwareDigest firmware
  H.ensure (Sha256 stockDigest == extensionBase extension) "Stock DAT SHA-256 differs from the extension's base firmware"
  H.ensure (versionText (firmwareVersion firmware) == extensionVersion extension) "Stock DAT version words differ from the extension's firmware_version"
  output <- newOutputDirectory givenOutput
  putStrLn ("Extension " ++ extensionId extension ++ ": " ++ extensionTitle extension)
  hFlush stdout
  let work = output </> "work"
      final = output </> "FWUP0002.DAT"
  (`onException` removeDirectoryRecursive output) $ do
    createDirectory work
    built <- foldM (runStep tools firmware work) (firmwarePath firmware) (zip [1 :: Int ..] (extensionSteps extension))
    callProcess (toolRebuild tools) ["verify", built]
    digest <- H.readRegular (64 * 1024 * 1024) built >>= H.sha256
    case pinning of
      Pinned -> H.ensure (Sha256 digest == extensionOutput extension) ("Built firmware SHA-256 " ++ digest ++ " differs from the extension's output_sha256")
      Unpinned -> pure ()
    renameFile built final
    removeDirectoryRecursive work
    putStrLn $ case pinning of
      Pinned -> "PASS: " ++ final ++ " SHA-256 " ++ digest
      Unpinned -> "UNPINNED: " ++ final ++ " SHA-256 " ++ digest

runStep :: Tools -> Firmware -> FilePath -> FilePath -> (Int, Step) -> IO FilePath
runStep tools firmware work current (index, step) = do
  let next = work </> ("step-" ++ show index ++ ".DAT")
      file name = work </> ("step-" ++ show index ++ "-" ++ name)
      rebuild = callProcess (toolRebuild tools)
  case step of
    PatchImage record load patchSet -> do
      let (kind, command) = case record of
            Image4 -> (Application4, "replace-record4")
            Image5 -> (Application5, "replace-record5")
      entry <- case [e | Segment k e <- firmwareSegments firmware, k == kind] of
        [e] -> pure e
        _ -> H.failH "Stock DAT segment table lacks the patched image"
      let window = toInteger (U.segFlashLen entry)
      media <- slice (firmwareMain firmware) (FileOffset (fromIntegral (U.segFlash entry))) (ByteCount (fromInteger window))
      B.writeFile (file "stock-record.bin") (B.map complement media)
      callProcess (toolDecoder tools) [file "stock-record.bin", "0", file "stock-image.bin"]
      callProcess (toolApplier tools) [patchSet, file "stock-image.bin", file "patched-image.bin"]
      callProcess (toolEncoder tools) [hex window, file "patched-image.bin", file "patched-record.bin"]
      let loadArguments = case load of
            Just (LoadAddress address) -> ["--load-address", hex address]
            Nothing -> []
      rebuild ([command, "--decoder", toolDecoder tools] ++ loadArguments ++ [current, file "patched-record.bin", file "patched-image.bin", next])
    WriteLabel text -> rebuild ["write-label", current, text, next]
    WriteRecord ident name layout bytes ->
      rebuild
        [ "write-record"
        , "--id"
        , hex (L.specialLabelIdValue ident)
        , "--layout"
        , commaSeparated (map show (L.labelLayoutList layout))
        , "--current-hex"
        , L.specialRecordHex bytes
        , current
        , L.labelNameText name
        , next
        ]
    ReplacePlain (FlashOffset offset) _ blob -> rebuild ["replace-plain", "--flash-offset", hex offset, current, blob, next]
    PatchPlain decodeDigest patchSet -> do
      rebuild ["extract-plain", current, file "plain-window.bin"]
      digest <- H.readRegular (16 * 1024 * 1024) (file "plain-window.bin") >>= H.sha256
      H.ensure
        (Sha256 digest == decodeDigest)
        ("The plain window of " ++ current ++ " has SHA-256 " ++ digest ++ ", not the plain patch set's decode_sha256")
      callProcess (toolApplier tools) [patchSet, file "plain-window.bin", file "patched-plain.bin"]
      rebuild ["replace-plain", current, file "patched-plain.bin", next]
  pure next
  where
    commaSeparated = foldr1 (\a b -> a ++ "," ++ b)

newOutputDirectory :: FilePath -> IO FilePath
newOutputDirectory given = do
  output <- resolve given
  root <- resolve "private-data" >>= canonicalizePath
  parent <- canonicalizePath (takeDirectory output)
  H.ensure (addTrailingPathSeparator root `isPrefixOf` addTrailingPathSeparator parent) "Output must be a new directory beneath private-data"
  H.ensure (takeFileName output `notElem` ["", ".", ".."]) "Output must name a new directory"
  exists <- doesPathExist output
  when exists (H.failH "Output directory already exists")
  createDirectory output
  pure output

hex :: Integer -> String
hex n = "0x" ++ showHex n ""

versionText :: FirmwareVersion -> String
versionText (FirmwareVersion major minor) = show major ++ "." ++ pad (showHex minor "")
  where
    pad digits = replicate (2 - length digits) '0' ++ digits
