module Main where

import BootCard
import Control.Exception (SomeException, displayException, try)
import Control.Monad (forM_, unless)
import Data.Bits (complement)
import qualified Data.ByteString as B
import Data.List (isPrefixOf)
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import FullImageGuardPin (GuardDigest (..), expectedGuardDigest)
import System.Directory (canonicalizePath, createDirectory, getCurrentDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>), addTrailingPathSeparator, isAbsolute, takeDirectory, takeFileName)
import System.IO (hPutStrLn, stderr)

newtype FullImage = FullImage B.ByteString
newtype RawGuard = RawGuard B.ByteString
newtype PinnedRestore = PinnedRestore B.ByteString

data Inputs = Inputs Config FullImage RawGuard PinnedRestore

data Command
  = Build FilePath FilePath FilePath FilePath
  | Verify FilePath FilePath FilePath FilePath

fullImageEnd :: Int
fullImageEnd = 0x1720000

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
  ensure (not (B.null guard) && B.length guard <= 0x1400) "full-image guard exceeds reserved RAM span"
  digest <- H.sha256 guard
  let GuardDigest expected = expectedGuardDigest
  ensure (digest == expected) "guard is not the current linked full-image guard"
  pure guard

mappedInputs :: FilePath -> IO [(Int, B.ByteString)]
mappedInputs given = do
  directory <- resolve given
  let load name address size digest = do
        bytes <- readPinned (directory </> name) digest size
        pure (address, bytes)
  sequence
    [ load "kernel0.bin" 0x00040000 0x0000dae0 "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"
    , load "kernel1.bin" 0x000d0000 0x0000daa0 "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"
    , load "resources2.bin" 0x00130000 0x0024bac8 "236d12a11c8b386f71192675c12398358840327f21e392e6eaa91ddfd6e020a5"
    , load "resources3.bin" 0x003bc000 0x0007cfd4 "28cc50c8d3acb34b7c5c6b023414b24e124cc103062dadae3cd10b30a3d620d8"
    , load "section4.bin" 0x0061c400 0x00282800 "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
    , load "section5.bin" 0x00bf1a00 0x004f1e60 "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"
    , load "plain_image.bin" 0x01100000 0x00360000 "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d"
    ]

buildFullImage :: B.ByteString -> B.ByteString -> FilePath -> IO FullImage
buildFullImage staged guard mappedDirectory = do
  ensure (U.word32At staged 0x20 == 0x1800) "stock reset vector changed"
  ensure (U.word32At staged 0x1d5c == 0xeb00013b) "stock image-4 load call changed"
  ensure (U.word32At staged 0x1f5c == 0xebffff98) "stock section-table load call changed"
  tablePath <- resolve mappedDirectory
  table <- readPinned (tablePath </> "section_table.bin") "aa9216fb44c6d46d7ef17a23dc2ddca04d0402e7a23e93a44e798aa58284a25c" 0x400
  ensure (map (U.word32At table) [0,4,8,12] == [0x4061c040,0x4061c1a4,0,0x4061c184]) "section-table header changed"
  mapped <- mappedInputs mappedDirectory
  let boot = replaceAt 0x6000 guard
           $ replaceAt 0x1f5c (U.putWord32 0xe320f000)
           $ replaceAt 0x1d5c (U.putWord32 0xe3a00000)
           $ replaceAt 0x20 (U.putWord32 0x6000) staged
      preloadedTable = replaceAt 8 (U.putWord32 1) table
      placements = (0,boot) : (0x0061c000,preloadedTable) : mapped
      place accumulator (address, bytes) = replaceAt address bytes accumulator
      image = foldl place (B.replicate fullImageEnd 0) placements
  ensure (B.length image == fullImageEnd && B.length image `mod` 512 == 0) "full image length changed"
  ensure (B.take 0x40000 image == boot) "full image boot prefix changed"
  ensure (U.word32At image 0x0061c008 == 1) "preloaded flag absent"
  ensure (B.all (== 0) (B.take (0x00bf1a00-0x0089ec00) (B.drop 0x0089ec00 image))) "image-4 BSS is not zero"
  ensure (B.all (== 0) (B.take (0x0171be40-0x01460000) (B.drop 0x01460000 image))) "image-5 BSS is not zero"
  pure (FullImage image)

inputs :: FilePath -> FilePath -> FilePath -> IO Inputs
inputs datPath guardPath mappedDirectory = do
  (config, staged, restore) <- stockInputs datPath
  guard <- readGuard guardPath
  FullImage full <- buildFullImage staged guard mappedDirectory
  pure (Inputs config (FullImage full) (RawGuard (B.take 0x40000 full)) restore)

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
verifyInputs (Inputs config (FullImage full) (RawGuard raw) (PinnedRestore restore)) image =
  checked (verifyRecoveryCard config raw full restore image)

writeArtifact :: FilePath -> (FilePath, B.ByteString) -> IO ()
writeArtifact directory (name, bytes) = do
  let path = directory </> name
  B.writeFile path bytes
  digest <- H.sha256 bytes
  putStrLn (path ++ ": " ++ show (B.length bytes) ++ " bytes, sha256 " ++ digest)

run :: Command -> IO ()
run (Build datPath guardPath mappedDirectory outPath) = do
  prepared@(Inputs config (FullImage full) (RawGuard raw) (PinnedRestore restore)) <- inputs datPath guardPath mappedDirectory
  image <- checked (recoveryCardFat32 config raw full restore)
  verifyInputs prepared image
  output <- privateOutput outPath
  createDirectory output
  forM_ [("ffsys.bin",configBytes config),("ffboot.bin",full),("full-recovery-sd.img",image)] (writeArtifact output)
  saved <- H.readRegular (80*1024*1024) (output </> "full-recovery-sd.img")
  ensure (saved == image) "saved full-recovery image differs from emitted bytes"
  putStrLn "Full RAM image, mode-1 guard, preloaded table and FAT/raw separation host-verified."
  putStrLn "Physical entry, long ROM transfer, cache behavior and application startup remain unproved; this is not live-cleared."
run (Verify datPath guardPath mappedDirectory imagePath) = do
  prepared@(Inputs config (FullImage full) (RawGuard raw) (PinnedRestore restore)) <- inputs datPath guardPath mappedDirectory
  path <- resolve imagePath
  image <- H.readRegular (80*1024*1024) path
  expected <- checked (recoveryCardFat32 config raw full restore)
  ensure (image == expected) "full-recovery image differs from canonical emitted bytes"
  verifyInputs prepared image
  digest <- H.sha256 image
  putStrLn ("full recovery card layout verified; sha256 " ++ digest)

parseCommand :: [String] -> IO Command
parseCommand ["build",datPath,guardPath,mappedDirectory,output] = pure (Build datPath guardPath mappedDirectory output)
parseCommand ["verify",datPath,guardPath,mappedDirectory,image] = pure (Verify datPath guardPath mappedDirectory image)
parseCommand _ = H.failH "usage: build-full-recovery-card build PINNED_DAT LINKED_GUARD_BIN MAPPED_DIRECTORY NEW_PRIVATE_DIRECTORY | verify PINNED_DAT LINKED_GUARD_BIN MAPPED_DIRECTORY IMAGE"

main :: IO ()
main = do
  result <- try (getArgs >>= parseCommand >>= run) :: IO (Either SomeException ())
  case result of
    Left failure -> hPutStrLn stderr (displayException failure) >> exitFailure
    Right () -> pure ()
