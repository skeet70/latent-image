module Main (main) where

import BootCard
import Control.Exception (SomeException, displayException, try)
import Control.Monad (forM_)
import Data.Bits (complement)
import qualified Data.ByteString as B
import Data.List (isPrefixOf)
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import DualNativeRollbackPin (RollbackDigest (..), expectedRollbackDigest)
import StagedRecoveryBootstrapPin (BootstrapDigest (..), expectedBootstrapDigest)
import System.Directory (canonicalizePath, createDirectory, getCurrentDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>), addTrailingPathSeparator, isAbsolute, takeDirectory, takeFileName)
import System.IO (hPutStrLn, stderr)

newtype NativeStage = NativeStage B.ByteString
newtype EntryBootstrap = EntryBootstrap B.ByteString

data Inputs = Inputs Config EntryBootstrap NativeStage

data Command = Build FilePath FilePath FilePath FilePath | Verify FilePath FilePath FilePath FilePath

checked :: Either String a -> IO a
checked = either H.failH pure

workspaceRoot :: IO FilePath
workspaceRoot = lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= maybe getCurrentDirectory pure

resolve :: FilePath -> IO FilePath
resolve path = do
  root <- workspaceRoot
  pure (if isAbsolute path then path else root </> path)

stockInputs :: FilePath -> IO (Config, B.ByteString)
stockInputs given = do
  path <- resolve given
  dat <- H.readRegular (64*1024*1024) path
  digest <- H.sha256 dat
  H.ensure (digest == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490") "requires pinned official X-M1 1.50 DAT"
  parsed <- checked (U.parseContainer dat)
  H.ensure (all U.checkPassed (U.checks parsed ++ U.carriedChecks parsed ++ U.stockChecks parsed)) "official DAT contract failed"
  config <- checked (stockConfig (B.map complement (B.take 0x1048 (B.drop 0x214 dat))))
  let staged = B.map complement (B.take 0x40000 (B.drop (0x214+0x40048) dat))
  H.ensure (B.length staged == 0x40000) "native boot staging span truncated"
  H.ensure (U.word32At staged 0x20 == 0x1800) "stock native reset vector changed"
  H.ensure (U.word32At staged 0x1fdc == 0xeb000018) "stock native close call changed"
  pure (config, staged)

readPinned :: Int -> FilePath -> String -> IO B.ByteString
readPinned limit given wanted = do
  path <- resolve given
  bytes <- H.readRegular limit path
  digest <- H.sha256 bytes
  H.ensure (digest == wanted) "linked input is not the current build output"
  pure bytes

assembleStage :: B.ByteString -> B.ByteString -> NativeStage
assembleStage staged stub = NativeStage (B.concat
  [ B.take 0x20 staged
  , U.putWord32 0x6000
  , B.take (0x1fdc-0x24) (B.drop 0x24 staged)
  , U.putWord32 0xeb001037
  , B.take (0x6000-0x1fe0) (B.drop 0x1fe0 staged)
  , stub
  , B.drop (0x6000+B.length stub) staged
  ])

inputs :: FilePath -> FilePath -> FilePath -> IO Inputs
inputs datPath stubPath entryPath = do
  (config, staged) <- stockInputs datPath
  let RollbackDigest rollbackDigest = expectedRollbackDigest
      BootstrapDigest bootstrapDigest = expectedBootstrapDigest
  stub <- readPinned 0x180 stubPath rollbackDigest
  H.ensure (B.length stub > 0xc0 && B.length stub <= 0x180 && B.length stub `mod` 4 == 0) "dual rollback exceeds reserved staging space"
  entry <- readPinned 512 entryPath bootstrapDigest
  H.ensure (B.length entry == 512) "entry bootstrap must occupy one sector"
  pure (Inputs config (EntryBootstrap entry) (assembleStage staged stub))

canonicalImage :: Inputs -> IO B.ByteString
canonicalImage (Inputs config (EntryBootstrap entry) (NativeStage stage)) = checked (stagedRecoveryCard config entry stage)

verifyInputs :: Inputs -> B.ByteString -> IO ()
verifyInputs (Inputs config (EntryBootstrap entry) (NativeStage stage)) image = do
  checked (verifyStagedRecoveryCard config entry stage image)
  H.ensure (B.length image == 17826304) "staged recovery image size changed"
  H.ensure (U.word32At stage 0x20 == 0x6000) "dual guard vector missing"
  H.ensure (U.word32At stage 0x1fdc == 0xeb001037) "dual rollback call missing"
  raw <- checked (readRawRom image)
  H.ensure (rawConfigDifferences config raw ==
    [ ConfigDifference (ConfigWordOffset 0x18) 2 1
    , ConfigDifference (ConfigWordOffset 0x1c0) 0 0x40000
    , ConfigDifference (ConfigWordOffset 0x1c4) 0 0x08000000
    , ConfigDifference (ConfigWordOffset 0x1fc) 0x01000101 0xaa550101
    ]) "staged raw configuration changed"

privateOutput :: FilePath -> IO FilePath
privateOutput given = do
  output <- resolve given
  H.ensure (takeFileName output `notElem` ["", ".", ".."] ) "output must name a new directory"
  root <- workspaceRoot
  guard <- canonicalizePath (root </> "private-data")
  parent <- canonicalizePath (takeDirectory output)
  H.ensure (addTrailingPathSeparator guard `isPrefixOf` addTrailingPathSeparator parent) "output must be beneath private-data"
  pure output

writeArtifact :: FilePath -> (FilePath, B.ByteString) -> IO ()
writeArtifact directory (name, bytes) = do
  let path = directory </> name
  B.writeFile path bytes
  saved <- H.readRegular (18*1024*1024) path
  H.ensure (saved == bytes) "saved artifact differs from emitted bytes"
  digest <- H.sha256 saved
  putStrLn (path ++ ": " ++ show (B.length saved) ++ " bytes, sha256 " ++ digest)

run :: Command -> IO ()
run (Build datPath stubPath entryPath outPath) = do
  prepared@(Inputs config (EntryBootstrap entry) (NativeStage stage)) <- inputs datPath stubPath entryPath
  image <- canonicalImage prepared
  verifyInputs prepared image
  output <- privateOutput outPath
  createDirectory output
  forM_ [("ffsys.bin", configBytes config), ("ffboot.bin", entry), ("ffrecov.bin", stage), ("staged-recovery-sd.img", image)] (writeArtifact output)
  putStrLn "Stage 1 is one sector: steady LED means admitted entry and second-stage loading."
  putStrLn "Continuous blinking means entry-frame or second-stage-load refusal. The existing finite route marker follows a complete load."
run (Verify datPath stubPath entryPath imagePath) = do
  prepared <- inputs datPath stubPath entryPath
  expected <- canonicalImage prepared
  path <- resolve imagePath
  image <- H.readRegular (18*1024*1024) path
  H.ensure (image == expected) "staged recovery image differs from canonical bytes"
  verifyInputs prepared image
  digest <- H.sha256 image
  putStrLn ("staged recovery card layout verified; sha256 " ++ digest)

parseCommand :: [String] -> IO Command
parseCommand ["build", datPath, stubPath, entryPath, output] = pure (Build datPath stubPath entryPath output)
parseCommand ["verify", datPath, stubPath, entryPath, image] = pure (Verify datPath stubPath entryPath image)
parseCommand _ = H.failH "usage: build-staged-recovery-card build PINNED_DAT LINKED_ROLLBACK_BIN LINKED_ENTRY_BIN NEW_PRIVATE_DIRECTORY | verify PINNED_DAT LINKED_ROLLBACK_BIN LINKED_ENTRY_BIN IMAGE"

main :: IO ()
main = do
  result <- try (getArgs >>= parseCommand >>= run) :: IO (Either SomeException ())
  case result of
    Left failure -> hPutStrLn stderr (displayException failure) >> exitFailure
    Right () -> pure ()
