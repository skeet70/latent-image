module Main (main) where

import Control.Exception (SomeException, bracket, displayException, try)
import Control.Monad (unless)
import Data.Bits (complement)
import qualified Data.ByteString as B
import Data.List (isPrefixOf)
import qualified FujiAF.Host as H
import qualified RamImage as R
import System.Directory (canonicalizePath, createDirectory, getCurrentDirectory, removeDirectoryRecursive)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>), addTrailingPathSeparator, isAbsolute, takeDirectory, takeFileName)
import System.IO (hPutStrLn, stderr)
import System.Process (callProcess)

data Tools = Tools FilePath FilePath FilePath FilePath FilePath FilePath FilePath FilePath R.NativeBootstrapFile

data RecoveryProfile = RamBoot | Restore | RecorderV2V5

resolve :: FilePath -> IO FilePath
resolve path = do
  root <- lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= maybe getCurrentDirectory pure
  pure (if isAbsolute path then path else root </> path)

build :: RecoveryProfile -> Tools -> FilePath -> FilePath -> IO ()
build RamBoot (Tools decoder _ _ _ _ _ _ _ bootstrap) firmware output =
  R.buildRamImage R.BootOnly (R.DecoderExecutable decoder) bootstrap firmware output
build Restore (Tools decoder _ _ _ _ _ _ _ bootstrap) firmware output =
  R.buildRamImage R.NativeUpdater (R.DecoderExecutable decoder) bootstrap firmware output
build RecorderV2V5 (Tools decoder applier encoder rebuild repair stage entry manifest _) givenStock givenOutput = do
  stockPath <- resolve givenStock >>= canonicalizePath
  stock <- H.readRegular (64*1024*1024) stockPath
  digest <- H.sha256 stock
  H.ensure (digest == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490") "Only the pinned official X-M1 1.50 DAT is supported"
  patch <- H.readRegular (1024*1024) manifest
  patchDigest <- H.sha256 patch
  H.ensure (patchDigest == "586bbb6fc6a9865e4419627e1251296fd5a5050e4072bc03811f7592c88a94e1") "RC0 patch manifest changed"
  output <- resolve givenOutput
  root <- resolve "private-data" >>= canonicalizePath
  parent <- canonicalizePath (takeDirectory output)
  H.ensure (addTrailingPathSeparator root `isPrefixOf` addTrailingPathSeparator parent) "Output must be a new directory beneath private-data"
  H.ensure (takeFileName output `notElem` ["", ".", ".."]) "Output must name a new directory"
  createDirectory output
  let work = output </> "work"
      record = work </> "stock-record4.bin"
      decoded = work </> "stock-id4.bin"
      patched = work </> "rc0-id4.bin"
      encoded = work </> "rc0-record4.bin"
      rc0 = work </> "rc0.DAT"
      card = output </> "card"
  bracket (createDirectory work) (const (removeDirectoryRecursive work)) $ \_ -> do
    B.writeFile record (B.map complement (B.take 0x146567 (B.drop (0x25c+0x160000) stock)))
    callProcess decoder [record, "0", decoded, "0x282800"]
    callProcess applier [manifest, decoded, patched]
    callProcess encoder ["0x160000", patched, encoded]
    callProcess rebuild ["replace-record4", "--decoder", decoder, "--load-address", "0x0061c340", stockPath, encoded, patched, rc0]
    callProcess repair ["build-physical-from-rc0", stockPath, rc0, stage, entry, card]
    image <- H.readRegular (18*1024*1024) (card </> "physical-block14-recovery-sd.img")
    imageHash <- H.sha256 image
    unless (B.length image == 17826304 && imageHash == "15d0cc28fc76d7d78ecf59a455284761333cd0e345929d3f88393fe991f5f343") $
      H.failH "Generated image differs from the successful v5 image; do not deploy"
  putStrLn ("PASS: exact v5 image generated at " ++ (card </> "physical-block14-recovery-sd.img"))
  putStrLn "Host generation only. This image is the recorder-rev-2 block-14 rollback to RC0, not a general stock-firmware restore."

main :: IO ()
main = do
  result <- try $ do
    args <- getArgs
    case args of
      decoder:applier:encoder:rebuild:repair:stage:entry:manifest:ramBootstrap:rest ->
        case rest of
          ["--help"] -> putStrLn $ unlines
            [ "Usage: bazel run //recovery:build-image -- restore KNOWN_GOOD_XM1_DAT NEW_PRIVATE_OUTPUT_DIRECTORY"
            , "       bazel run //recovery:build-image -- ram-boot KNOWN_GOOD_XM1_DAT NEW_PRIVATE_OUTPUT_DIRECTORY"
            , "       bazel run //recovery:build-image -- recorder-v2-v5 OFFICIAL_XM1_150_DAT NEW_PRIVATE_OUTPUT_DIRECTORY"
            , "restore derives a FAT32 RAM card requesting the ordinary updater, with unchanged DAT and visible confirmation."
            , "ram-boot is a diagnostic FAT16 RAM card without an updater request."
            , "Both are candidates, not accepted physical recovery. See recovery/README.md for persistence steps and limits."
            , "recorder-v2-v5 reproduces the historical exact recorder revision-2 repair. No card or camera I/O."
            ]
          [profileName, firmware, output] -> do
            profile <- case profileName of
              "ram-boot" -> pure RamBoot
              "restore" -> pure Restore
              "recorder-v2-v5" -> pure RecorderV2V5
              _ -> H.failH "Supported image modes: restore, ram-boot, recorder-v2-v5"
            tools <- Tools <$> canonicalizePath decoder <*> canonicalizePath applier
              <*> canonicalizePath encoder <*> canonicalizePath rebuild
              <*> canonicalizePath repair <*> canonicalizePath stage
              <*> canonicalizePath entry <*> canonicalizePath manifest
              <*> (R.NativeBootstrapFile <$> canonicalizePath ramBootstrap)
            build profile tools firmware output
          _ -> H.failH "Usage: build-image restore|ram-boot KNOWN_GOOD_XM1_DAT NEW_PRIVATE_OUTPUT_DIRECTORY | recorder-v2-v5 OFFICIAL_XM1_150_DAT NEW_PRIVATE_OUTPUT_DIRECTORY"
      _ -> H.failH "Run through //recovery:build-image to supply the Bazel-built tools"
  case result :: Either SomeException () of
    Left failure -> hPutStrLn stderr (displayException failure) >> exitFailure
    Right () -> pure ()
