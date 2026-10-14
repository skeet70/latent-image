module Main where

import BootCard
import Control.Exception (SomeException, displayException, try)
import Control.Monad (forM_, unless)
import Data.Bits (complement)
import qualified Data.ByteString as B
import Data.List (isPrefixOf)
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Seg4ImageGuardPin (GuardDigest (..), expectedGuardDigest)
import System.Directory (canonicalizePath, createDirectory, getCurrentDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>), addTrailingPathSeparator, isAbsolute, takeDirectory, takeFileName)
import System.IO (hPutStrLn, stderr)

newtype CompactImage = CompactImage B.ByteString
newtype RawGuard = RawGuard B.ByteString
newtype PinnedRestore = PinnedRestore B.ByteString

data Inputs = Inputs Config CompactImage RawGuard PinnedRestore

data Command
  = Build FilePath FilePath FilePath FilePath
  | Verify FilePath FilePath FilePath FilePath

compactImageEnd :: Int
compactImageEnd = 0x2c2c00

checked :: Either String a -> IO a
checked = either H.failH pure

workspaceRoot :: IO FilePath
workspaceRoot = lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= maybe getCurrentDirectory pure

resolve :: FilePath -> IO FilePath
resolve path = do
  root <- workspaceRoot
  pure (if isAbsolute path then path else root </> path)

ensure :: Bool -> String -> IO ()
ensure condition message = unless condition (H.failH message)

replaceAt :: Int -> B.ByteString -> B.ByteString -> B.ByteString
replaceAt offset value bytes = B.take offset bytes <> value <> B.drop (offset + B.length value) bytes

readPinned :: FilePath -> String -> Int -> IO B.ByteString
readPinned path digest size = do
  bytes <- H.readRegular size path
  ensure (B.length bytes == size) (takeFileName path ++ " size mismatch")
  actual <- H.sha256 bytes
  ensure (actual == digest) (takeFileName path ++ " digest mismatch")
  pure bytes

stockInputs :: FilePath -> IO (Config, B.ByteString, PinnedRestore)
stockInputs given = do
  path <- resolve given
  dat <- H.readRegular (64*1024*1024) path
  digest <- H.sha256 dat
  ensure (digest == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490") "requires pinned official X-M1 1.50 DAT"
  parsed <- checked (U.parseContainer dat)
  ensure (all U.checkPassed (U.checks parsed ++ U.carriedChecks parsed ++ U.stockChecks parsed)) "official DAT contract failed"
  config <- checked (stockConfig (B.map complement (B.take 0x1048 (B.drop 0x214 dat))))
  let staged = B.map complement (B.take 0x40000 (B.drop (0x214 + 0x40048) dat))
  ensure (B.length staged == 0x40000) "native boot staging span truncated"
  primaryHash <- H.sha256 (B.take 0x4300 staged)
  secondaryHash <- H.sha256 (B.take 0x4c80 (B.drop 0x10000 staged))
  ensure (primaryHash == "c2da900dcd3f193b05b5ae968dfcb87723571462eafd84c0fed8467f66f7dff3") "primary boot record does not match"
  ensure (secondaryHash == "2f1bf3f0e483b892d2890599679c1b515cfc1be8b8c27cbb06d4d913e428abb4") "companion boot record does not match"
  pure (config, staged, PinnedRestore dat)

readGuard :: FilePath -> IO B.ByteString
readGuard given = do
  path <- resolve given
  guard <- H.readRegular 0x1400 path
  ensure (not (B.null guard) && B.length guard <= 0x1400) "segment-4 guard exceeds reserved RAM span"
  digest <- H.sha256 guard
  let GuardDigest expected = expectedGuardDigest
  ensure (digest == expected) "guard is not the current linked segment-4 guard"
  pure guard

buildCompactImage :: B.ByteString -> B.ByteString -> FilePath -> IO CompactImage
buildCompactImage staged guard mappedDirectory = do
  ensure (U.word32At staged 0x20 == 0x1800) "stock reset vector changed"
  ensure (U.word32At staged 0x1d5c == 0xeb00013b) "stock image-4 load call changed"
  ensure (U.word32At staged 0x1f5c == 0xebffff98) "stock section-table load call changed"
  directory <- resolve mappedDirectory
  table <- readPinned (directory </> "section_table.bin") "aa9216fb44c6d46d7ef17a23dc2ddca04d0402e7a23e93a44e798aa58284a25c" 0x400
  ensure (map (U.word32At table) [0,4,8,12] == [0x4061c040,0x4061c1a4,0,0x4061c184]) "section-table header changed"
  section4 <- readPinned (directory </> "section4.bin") "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16" 0x282800
  let boot = replaceAt 0x6000 guard
           $ replaceAt 0x1f5c (U.putWord32 0xe320f000)
           $ replaceAt 0x1d5c (U.putWord32 0xe3a00000)
           $ replaceAt 0x20 (U.putWord32 0x6000) staged
      preloadedTable = replaceAt 8 (U.putWord32 1) table
      image = boot <> preloadedTable <> section4
  ensure (B.length image == compactImageEnd && B.length image `mod` 512 == 0) "compact image length changed"
  ensure (B.take 0x40000 image == boot) "compact image boot prefix changed"
  ensure (U.word32At image 0x40008 == 1) "preloaded table flag absent"
  ensure (B.take 0x282800 (B.drop 0x40400 image) == section4) "stock segment 4 placement changed"
  pure (CompactImage image)

inputs :: FilePath -> FilePath -> FilePath -> IO Inputs
inputs datPath guardPath mappedDirectory = do
  (config, staged, restore) <- stockInputs datPath
  guard <- readGuard guardPath
  CompactImage compact <- buildCompactImage staged guard mappedDirectory
  pure (Inputs config (CompactImage compact) (RawGuard (B.take 0x40000 compact)) restore)

privateOutput :: FilePath -> IO FilePath
privateOutput given = do
  output <- resolve given
  ensure (takeFileName output `notElem` ["", ".", ".."]) "output must name a new directory"
  root <- workspaceRoot
  boundary <- canonicalizePath (root </> "private-data")
  parent <- canonicalizePath (takeDirectory output)
  ensure (addTrailingPathSeparator boundary `isPrefixOf` addTrailingPathSeparator parent) "output must be beneath private-data"
  pure output

verifyInputs :: Inputs -> B.ByteString -> IO ()
verifyInputs (Inputs config (CompactImage compact) (RawGuard raw) (PinnedRestore restore)) image =
  checked (verifyRecoveryCard config raw compact restore image)

writeArtifact :: FilePath -> (FilePath, B.ByteString) -> IO ()
writeArtifact directory (name, bytes) = do
  let path = directory </> name
  B.writeFile path bytes
  digest <- H.sha256 bytes
  putStrLn (path ++ ": " ++ show (B.length bytes) ++ " bytes, sha256 " ++ digest)

run :: Command -> IO ()
run (Build datPath guardPath mappedDirectory outPath) = do
  prepared@(Inputs config (CompactImage compact) (RawGuard raw) (PinnedRestore restore)) <- inputs datPath guardPath mappedDirectory
  image <- checked (recoveryCardFat32 config raw compact restore)
  verifyInputs prepared image
  output <- privateOutput outPath
  createDirectory output
  forM_ [("ffsys.bin",configBytes config),("ffboot.bin",compact),("seg4-recovery-sd.img",image)] (writeArtifact output)
  saved <- H.readRegular (80*1024*1024) (output </> "seg4-recovery-sd.img")
  ensure (saved == image) "saved segment-4 recovery image differs from emitted bytes"
  putStrLn "Compact stock segment 4, preloaded table, BSS clear guard and FAT/raw separation host-verified."
  putStrLn "Physical entry, ROM transfer, cache behavior, companion startup and application startup remain unproved; this is not live-cleared."
run (Verify datPath guardPath mappedDirectory imagePath) = do
  prepared@(Inputs config (CompactImage compact) (RawGuard raw) (PinnedRestore restore)) <- inputs datPath guardPath mappedDirectory
  path <- resolve imagePath
  image <- H.readRegular (80*1024*1024) path
  expected <- checked (recoveryCardFat32 config raw compact restore)
  ensure (image == expected) "segment-4 recovery image differs from canonical emitted bytes"
  verifyInputs prepared image
  digest <- H.sha256 image
  putStrLn ("segment-4 recovery card layout verified; sha256 " ++ digest)

parseCommand :: [String] -> IO Command
parseCommand ["build",datPath,guardPath,mappedDirectory,output] = pure (Build datPath guardPath mappedDirectory output)
parseCommand ["verify",datPath,guardPath,mappedDirectory,image] = pure (Verify datPath guardPath mappedDirectory image)
parseCommand _ = H.failH "usage: build-seg4-recovery-card build PINNED_DAT LINKED_GUARD_BIN MAPPED_DIRECTORY NEW_PRIVATE_DIRECTORY | verify PINNED_DAT LINKED_GUARD_BIN MAPPED_DIRECTORY IMAGE"

main :: IO ()
main = do
  result <- try (getArgs >>= parseCommand >>= run) :: IO (Either SomeException ())
  case result of
    Left failure -> hPutStrLn stderr (displayException failure) >> exitFailure
    Right () -> pure ()
