module Main where

import BootCard
import Control.Exception (SomeException, displayException, try)
import Control.Monad (forM_)
import Data.Bits (complement)
import qualified Data.ByteString as B
import Data.List (isPrefixOf)
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import RecoveryStageReport (decodeStageReports, renderStageEvidence)
import System.Directory (canonicalizePath, createDirectory, getCurrentDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>), addTrailingPathSeparator, isAbsolute, takeDirectory, takeFileName)
import System.IO (hPutStrLn, stderr)
import NativeRollbackPin (RollbackDigest (..), expectedRollbackDigest)

newtype BootOffset = BootOffset Int
newtype StockBootSpan = StockBootSpan B.ByteString
newtype RollbackStub = RollbackStub B.ByteString
newtype NativeBootstrap = NativeBootstrap B.ByteString
newtype PinnedRestore = PinnedRestore B.ByteString

data RecoveryInputs = RecoveryInputs
  { recoveryConfig :: Config
  , recoveryBootstrap :: NativeBootstrap
  , recoveryRestore :: PinnedRestore
  }

data StockInputs = StockInputs Config StockBootSpan

data Command = Build FilePath FilePath FilePath | Verify FilePath FilePath FilePath | Inspect FilePath

checked :: Either String a -> IO a
checked = either H.failH pure

workspaceRoot :: IO FilePath
workspaceRoot = lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= maybe getCurrentDirectory pure

resolve :: FilePath -> IO FilePath
resolve path = do
  root <- workspaceRoot
  pure (if isAbsolute path then path else root </> path)

stockInputs :: FilePath -> IO (StockInputs, PinnedRestore)
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
  primaryHash <- H.sha256 (B.take 0x4300 staged)
  secondaryHash <- H.sha256 (B.take 0x4c80 (B.drop 0x10000 staged))
  H.ensure (primaryHash == "c2da900dcd3f193b05b5ae968dfcb87723571462eafd84c0fed8467f66f7dff3") "primary boot record does not match"
  H.ensure (secondaryHash == "2f1bf3f0e483b892d2890599679c1b515cfc1be8b8c27cbb06d4d913e428abb4") "companion boot record does not match"
  forM_ [(BootOffset 0x20,0x1800),(BootOffset 0x1fdc,0xeb000018),(BootOffset 0x1fe0,0xe3003000),(BootOffset 0x1ac4,0x4300),(BootOffset 0x1ac8,0x5280)] $ \(BootOffset offset,wanted) ->
    H.ensure (U.word32At staged offset == wanted) "native vector, call or BSS contract changed"
  pure (StockInputs config (StockBootSpan staged), PinnedRestore dat)

readStub :: FilePath -> IO RollbackStub
readStub given = do
  path <- resolve given
  bytes <- H.readRegular 0x1400 path
  H.ensure (B.length bytes > 0x1000 && B.length bytes <= 0x1400 && B.length bytes `mod` 4 == 0) "rollback image must include the entry hooks, reporter and two report buffers below 0x7400"
  digest <- H.sha256 bytes
  let RollbackDigest expected = expectedRollbackDigest
  H.ensure (digest == expected) "stub is not the current build's linked native rollback"
  pure (RollbackStub bytes)

assembleBootstrap :: StockBootSpan -> RollbackStub -> NativeBootstrap
assembleBootstrap (StockBootSpan staged) (RollbackStub stub) = NativeBootstrap (B.concat
  [ B.take 0x20 staged
  , U.putWord32 0x6000
  , B.take (0x1fdc-0x24) (B.drop 0x24 staged)
  , U.putWord32 0xeb001019
  , U.putWord32 0xea00103a
  , B.take (0x6000-0x1fe4) (B.drop 0x1fe4 staged)
  , stub
  , B.drop (0x6000+B.length stub) staged
  ])

inputs :: FilePath -> FilePath -> IO RecoveryInputs
inputs datPath stubPath = do
  (StockInputs config staged, restore) <- stockInputs datPath
  stub <- readStub stubPath
  pure (RecoveryInputs config (assembleBootstrap staged stub) restore)

privateOutput :: FilePath -> IO FilePath
privateOutput given = do
  output <- resolve given
  H.ensure (takeFileName output `notElem` ["", ".", ".."]) "output must name a new directory"
  root <- workspaceRoot
  guard <- canonicalizePath (root </> "private-data")
  parent <- canonicalizePath (takeDirectory output)
  H.ensure (addTrailingPathSeparator guard `isPrefixOf` addTrailingPathSeparator parent) "output must be beneath private-data"
  pure output

verifyRecovery :: RecoveryInputs -> B.ByteString -> IO ()
verifyRecovery (RecoveryInputs config (NativeBootstrap bootstrap) (PinnedRestore restore)) image = do
  checked (verifyRecoveryCard config bootstrap bootstrap restore image)
  raw <- checked (readRawRom image)
  H.ensure (rawConfigDifferences config raw ==
    [ ConfigDifference (ConfigWordOffset 0x18) 2 512
    , ConfigDifference (ConfigWordOffset 0x1c0) 0 0x00040000
    , ConfigDifference (ConfigWordOffset 0x1c4) 0 0x08000000
    , ConfigDifference (ConfigWordOffset 0x1fc) 0x01000101 0xaa550101
    ]) "raw configuration has additional controller changes"

writeArtifact :: FilePath -> (FilePath,B.ByteString) -> IO ()
writeArtifact directory (name,bytes) = do
  let path = directory </> name
  B.writeFile path bytes
  digest <- H.sha256 bytes
  putStrLn (path ++ ": " ++ show (B.length bytes) ++ " bytes, sha256 " ++ digest)

run :: Command -> IO ()
run (Build datPath stubPath outPath) = do
  prepared@(RecoveryInputs config (NativeBootstrap bootstrap) (PinnedRestore restore)) <- inputs datPath stubPath
  image <- checked (recoveryCard config bootstrap bootstrap restore)
  verifyRecovery prepared image
  output <- privateOutput outPath
  createDirectory output
  forM_ [("ffsys.bin",configBytes config),("ffboot.bin",bootstrap),("native-recovery-sd.img",image)] (writeArtifact output)
  saved <- H.readRegular (80*1024*1024) (output </> "native-recovery-sd.img")
  H.ensure (saved == image) "saved recovery image differs from emitted bytes"
  verifyRecovery prepared saved
  putStrLn "Native records, the mode-1 guard, post-load RAM rollback and stage-journal halt were host-verified."
  putStrLn ("The FAT volume carries the pinned restore DAT; report sectors begin at LBA " ++ show recoveryReportBaseLba ++ ".")
  putStrLn "Physical entry, native initialization and SD transmit behavior remain unproved; this is not live-cleared."
  putStrLn "Raw mode stages only the guarded bootstrap, and the entry guard refuses non-mode-1 execution."
  putStrLn "The bootstrap halts after native close without releasing the peer. No persistent-media writer consumes the carried restore DAT."
run (Verify datPath stubPath imagePath) = do
  prepared <- inputs datPath stubPath
  path <- resolve imagePath
  image <- H.readRegular (80*1024*1024) path
  let RecoveryInputs config (NativeBootstrap bootstrap) (PinnedRestore restore) = prepared
  expected <- checked (recoveryCard config bootstrap bootstrap restore)
  H.ensure (image == expected) "recovery image differs from canonical emitted bytes"
  verifyRecovery prepared image
  digest <- H.sha256 image
  putStrLn ("native recovery card layout verified; sha256 " ++ digest)
run (Inspect imagePath) = do
  path <- resolve imagePath
  image <- H.readRegular (80*1024*1024) path
  sectors <- checked (recoveryReportSectors image)
  mapM_ (putStrLn . renderStageEvidence) (decodeStageReports sectors)

parseCommand :: [String] -> IO Command
parseCommand ["build",datPath,stubPath,output] = pure (Build datPath stubPath output)
parseCommand ["verify",datPath,stubPath,image] = pure (Verify datPath stubPath image)
parseCommand ["inspect",image] = pure (Inspect image)
parseCommand _ = H.failH "usage: build-recovery-card build PINNED_DAT LINKED_STUB_BIN NEW_PRIVATE_DIRECTORY | verify PINNED_DAT LINKED_STUB_BIN IMAGE | inspect IMAGE"

main :: IO ()
main = do
  result <- try (getArgs >>= parseCommand >>= run) :: IO (Either SomeException ())
  case result of
    Left failure -> hPutStrLn stderr (displayException failure) >> exitFailure
    Right () -> pure ()
