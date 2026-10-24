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

data CardConfiguration = StockConfiguration | StageMarkerConfiguration StageMarker

-- ReleaseAtApply keeps the configuration's +0x10 bit 0 (clear in the stock file).
data PowerHold = ReleaseAtApply | HoldThroughLoad

configurationLabel :: CardConfiguration -> String
configurationLabel StockConfiguration = "stock file configuration"
configurationLabel (StageMarkerConfiguration AfterApply) = "stock file configuration plus the post-apply LED marker pairs and teardown bit"
configurationLabel (StageMarkerConfiguration BeforeMemoryWait) = "stock file configuration plus the LED marker in optional slots +0xb8/+0xc8/+0xd0 and teardown bit"

holdLabel :: PowerHold -> String
holdLabel ReleaseAtApply = ""
holdLabel HoldThroughLoad = ", +0x10 bit 0 set so 0x80000330 keeps 0xfff88084 = 1"

stageMarker :: String -> Maybe StageMarker
stageMarker "after-apply" = Just AfterApply
stageMarker "before-memory-wait" = Just BeforeMemoryWait
stageMarker _ = Nothing

inputs :: CardConfiguration -> PowerHold -> FilePath -> FilePath -> IO (Config, B.ByteString)
inputs kind hold datArg payloadArg = do
  datPath <- resolve datArg
  payloadPath <- resolve payloadArg
  dat <- H.readRegular (64*1024*1024) datPath
  digest <- H.sha256 dat
  H.ensure (digest == pinnedHash) "container is not pinned official X-M1 1.50"
  parsed <- checked (U.parseContainer dat)
  H.ensure (all U.checkPassed (U.checks parsed ++ U.carriedChecks parsed ++ U.stockChecks parsed)) "pinned container contract failed"
  stock <- checked (stockConfig (B.map complement (B.take 0x1048 (B.drop 0x214 dat))))
  marked <- case kind of
    StockConfiguration -> pure stock
    StageMarkerConfiguration marker -> checked (stageMarkerConfig marker stock)
  config <- case hold of
    ReleaseAtApply -> pure marked
    HoldThroughLoad -> checked (powerHoldConfig marked)
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

describeRaw :: CardConfiguration -> PowerHold -> Config -> B.ByteString -> IO ()
describeRaw kind hold config image = do
  loaded <- checked (readRawRom image)
  let payloadSize = show (B.length (mode2PayloadBytes loaded))
  putStrLn ("mode 1: " ++ configurationLabel kind ++ holdLabel hold ++ " and " ++ payloadSize ++ "-byte payload")
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

build :: CardConfiguration -> PowerHold -> FilePath -> FilePath -> FilePath -> IO ()
build kind hold datPath payloadPath outArg = do
  (config,payload) <- inputs kind hold datPath payloadPath
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
  describeRaw kind hold config saved
  putStrLn "static-inference: both modeled ROM loading paths verified; physical card, initialization and execution unproved"

verify :: CardConfiguration -> PowerHold -> FilePath -> FilePath -> FilePath -> IO ()
verify kind hold datPath payloadPath imagePath = do
  (config,payload) <- inputs kind hold datPath payloadPath
  resolvedImage <- resolve imagePath
  image <- H.readRegular (18*1024*1024) resolvedImage
  checked (verifyCard config payload image)
  digest <- H.sha256 image
  putStrLn ("ROM-parser verification passed; image sha256 " ++ digest)
  describeRaw kind hold config image

run :: [String] -> IO ()
run ("--power-hold":rest) = command HoldThroughLoad rest
run args = command ReleaseAtApply args

command :: PowerHold -> [String] -> IO ()
command hold ["build",datPath,payloadPath,outArg] = build StockConfiguration hold datPath payloadPath outArg
command hold ["build-stage-marker",marker,datPath,payloadPath,outArg]
  | Just position <- stageMarker marker = build (StageMarkerConfiguration position) hold datPath payloadPath outArg
command hold ["verify",datPath,payloadPath,imagePath] = verify StockConfiguration hold datPath payloadPath imagePath
command hold ["verify-stage-marker",marker,datPath,payloadPath,imagePath]
  | Just position <- stageMarker marker = verify (StageMarkerConfiguration position) hold datPath payloadPath imagePath
command _ _ = H.failH "usage: build-boot-card [--power-hold] build PINNED_DAT PAYLOAD_BIN PRIVATE_DATA_SUBDIRECTORY | [--power-hold] build-stage-marker after-apply|before-memory-wait PINNED_DAT PAYLOAD_BIN PRIVATE_DATA_SUBDIRECTORY | [--power-hold] verify PINNED_DAT PAYLOAD_BIN IMAGE | [--power-hold] verify-stage-marker after-apply|before-memory-wait PINNED_DAT PAYLOAD_BIN IMAGE"

main :: IO ()
main = do
  result <- try (getArgs >>= run) :: IO (Either SomeException ())
  case result of
    Left e -> hPutStrLn stderr (displayException e) >> exitFailure
    Right () -> pure ()
