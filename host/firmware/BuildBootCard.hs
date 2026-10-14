module Main where

import BootCard
import Control.Exception (SomeException, displayException, try)
import Control.Monad (forM_)
import Data.Bits (complement)
import qualified Data.ByteString as B
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Data.List (isPrefixOf)
import Numeric (showHex)
import System.Directory (canonicalizePath, createDirectory, getCurrentDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>), addTrailingPathSeparator, isAbsolute, takeDirectory, takeFileName)
import System.IO (hPutStrLn, stderr)

pinnedHash :: String
pinnedHash = "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"

checked :: Either String a -> IO a
checked = either H.failH pure

workspaceRoot :: IO FilePath
workspaceRoot = do
  workspace <- lookupEnv "BUILD_WORKSPACE_DIRECTORY"
  maybe (pure =<< getCurrentDirectory) pure workspace

-- Relative paths resolve against $BUILD_WORKSPACE_DIRECTORY, the convention
-- //emulation and //transport use, since bazel run has no fixed working directory.
resolve :: FilePath -> IO FilePath
resolve given = do
  root <- workspaceRoot
  pure (if isAbsolute given then given else root </> given)

inputs :: FilePath -> FilePath -> IO (Config, B.ByteString)
inputs datArg payloadArg = do
  datPath <- resolve datArg
  payloadPath <- resolve payloadArg
  dat <- H.readRegular (64*1024*1024) datPath
  digest <- H.sha256 dat
  H.ensure (digest == pinnedHash) "container is not pinned official X-M1 1.50"
  parsed <- checked (U.parseContainer dat)
  H.ensure (all U.checkPassed (U.checks parsed ++ U.carriedChecks parsed ++ U.stockChecks parsed)) "pinned container contract failed"
  config <- checked (stockConfig (B.map complement (B.take 0x1048 (B.drop 0x214 dat))))
  payload <- H.readRegular (bootPayloadByteCount maxBootPayloadBytes) payloadPath
  pure (config,payload)

privateOutput :: FilePath -> IO FilePath
privateOutput given = do
  H.ensure (takeFileName given `notElem` ["", ".", ".."]) "output must name a new directory"
  root <- workspaceRoot
  guard <- canonicalizePath (root </> "private-data")
  parent <- canonicalizePath (takeDirectory (root </> given))
  H.ensure (addTrailingPathSeparator guard `isPrefixOf` addTrailingPathSeparator parent) "output must be a new directory beneath the workspace private-data"
  pure (root </> given)

describeRaw :: Config -> B.ByteString -> IO ()
describeRaw config image = do
  loaded <- checked (readRawRom image)
  let payloadSize = show (B.length (mode2PayloadBytes loaded))
  putStrLn ("mode 1: stock file configuration and " ++ payloadSize ++ "-byte payload")
  putStrLn ("mode 2: raw configuration overlay and the same " ++ payloadSize ++ "-byte payload at LBA 2")
  forM_ (rawConfigDifferences config loaded) $ \difference -> do
    let ConfigWordOffset off = configWordOffset difference
        target | off == 0x18 = "loader sector count"
               | off >= 0xe0 && off <= 0x324 = "register " ++ hex (0xfff01000 + off - 0xe0)
               | otherwise = "unbound consumer"
    putStrLn ("raw config +" ++ hex off ++ " (" ++ target ++ "): "
      ++ hex (stockWord difference) ++ " -> " ++ hex (rawWord difference))
  putStrLn "not live-cleared: the MBR-derived register differences require target-specific review; darkness cannot identify payload execution"
  putStrLn "raw-image-only layout: retained stock MBR geometry is not a conventional partition table; formatting or copying the files loses the raw-mode overlay"
  where
    hex :: (Integral a, Show a) => a -> String
    hex value = "0x" ++ showHex value ""

run :: [String] -> IO ()
run ["build",datPath,payloadPath,outArg] = do
  (config,payload) <- inputs datPath payloadPath
  image <- checked (bootCard config payload)
  out <- privateOutput outArg
  createDirectory out
  forM_ [("ffsys.bin",configBytes config),("ffboot.bin",payload),("diagnostic-sd.img",image)] $ \(name,bytes) -> do
    let path = out </> name
    B.writeFile path bytes
    digest <- H.sha256 bytes
    putStrLn (path ++ ": " ++ show (B.length bytes) ++ " bytes, sha256 " ++ digest)
  saved <- H.readRegular (18*1024*1024) (out </> "diagnostic-sd.img")
  checked (verifyCard config payload saved)
  describeRaw config saved
  putStrLn "static-inference: both modeled ROM loading paths verified; physical card, initialization and execution unproved"
run ["verify",datPath,payloadPath,imagePath] = do
  (config,payload) <- inputs datPath payloadPath
  resolvedImage <- resolve imagePath
  image <- H.readRegular (18*1024*1024) resolvedImage
  checked (verifyCard config payload image)
  digest <- H.sha256 image
  putStrLn ("ROM-parser verification passed; image sha256 " ++ digest)
  describeRaw config image
run _ = H.failH "usage: build-boot-card build PINNED_DAT PAYLOAD_BIN PRIVATE_DATA_SUBDIRECTORY | verify PINNED_DAT PAYLOAD_BIN IMAGE"

main :: IO ()
main = do
  result <- try (getArgs >>= run) :: IO (Either SomeException ())
  case result of
    Left e -> hPutStrLn stderr (displayException e) >> exitFailure
    Right () -> pure ()
