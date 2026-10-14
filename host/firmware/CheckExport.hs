{-# LANGUAGE OverloadedStrings #-}
-- Host-only synthetic export guards plus optional exact private-byte comparison.
module Main where
import qualified FujiAF.Host as H
import Control.Exception (SomeException, bracket, displayException, try)
import Control.Monad (unless)
import Data.Bits (xor)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.IORef
import FujiAF.JSON (encodeCanonical)
import System.Directory
import System.Environment (getArgs)
import System.Exit (ExitCode(..), exitFailure)
import System.FilePath
import System.IO
import System.Posix.Files (createNamedPipe, createSymbolicLink, setFileMode)

privateTemporary :: FilePath -> (FilePath -> IO a) -> IO a
privateTemporary repository action = do
  let private = repository </> "private-data"
  createDirectoryIfMissing False private
  bracket (do (path, handle) <- openTempFile private "export-check-"; hClose handle; removeFile path; createDirectory path; setFileMode path 0o700; pure path) removeDirectoryRecursive action

checkExport :: FilePath -> [String] -> IO Int
checkExport repository optional = privateTemporary repository $ \temporary -> do
  let adapter = repository </> "host/firmware/export_payload.cjs"
      wrong = temporary </> "synthetic-wrong.dat"
      linked = temporary </> "linked.dat"
  B.writeFile wrong "synthetic unsupported DAT\n"
  createSymbolicLink wrong linked
  checked <- newIORef (0 :: Int)
  let reject checkout source destination message preload = do
        let prefix = maybe [] (\path -> ["--require", path]) preload
        (stopped, status, output, errors) <- H.runBounded 3000000 (1024*1024) "node" (prefix ++ [adapter, checkout, source, destination]) B.empty
        H.ensure (not stopped && status == ExitFailure 1) ("export rejection failed: " ++ C.unpack (output <> errors))
        H.ensure (C.pack message `B.isInfixOf` errors) ("export rejection message differs: " ++ C.unpack errors)
        exists <- doesPathExist destination
        H.ensure (not exists) "rejected export created output"
        modifyIORef' checked (+1)
      rejectPlain checkout source destination message = reject checkout source destination message Nothing
  rejectPlain temporary wrong (temporary </> "unsupported") "unsupported input"
  rejectPlain temporary linked (temporary </> "linked-output") "regular file"
  rejectPlain temporary wrong (repository </> "outside-ignored") "output parent must be inside"
  let fifo = temporary </> "no-writer.fifo"
  createNamedPipe fifo 0o600
  rejectPlain temporary fifo (temporary </> "fifo-output") "regular file"
  rejectPlain temporary temporary (temporary </> "directory-output") "regular file"
  rejectPlain temporary "/dev/null" (temporary </> "device-output") "regular file"
  let oversized = temporary </> "oversized.dat"
  withBinaryFile oversized WriteMode $ \handle -> hSetFileSize handle (64*1024*1024+1)
  rejectPlain temporary oversized (temporary </> "oversized-output") "regular file"
  let swapped = temporary </> "swap.dat"
      replacement = temporary </> "replacement.fifo"
      preload = temporary </> "replace-before-open.cjs"
  B.writeFile swapped "synthetic regular input\n"
  createNamedPipe replacement 0o600
  B.writeFile preload ("const fs = require('node:fs'); const original = fs.openSync.bind(fs);\n" <>
    "fs.openSync = function(filename, ...args) {\nif (filename === " <> encodeCanonical (H.s swapped) <>
    ") fs.renameSync(" <> encodeCanonical (H.s replacement) <> ", filename);\nreturn original(filename, ...args); };\n")
  reject temporary swapped (temporary </> "replacement-output") "regular file" (Just preload)
  case optional of
    [] -> pure ()
    [checkout, dat] -> do
      bytes <- H.readRegular (64*1024*1024) dat
      digest <- H.sha256 bytes
      H.ensure (B.length bytes == 38405148 && digest == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490") "exact public X-M1 1.50 DAT required"
      let changed = temporary </> "changed.dat"
      B.writeFile changed (B.init bytes <> B.singleton (B.last bytes `xor` 1))
      rejectPlain checkout changed (temporary </> "changed-output") "unsupported input"
      rejectPlain temporary dat (temporary </> "bad-source-output") "fatal:"
      let destination = temporary </> "success"
      (stopped, status, _, errors) <- H.runBounded 45000000 (1024*1024) "node" [adapter, checkout, dat, destination] B.empty
      H.ensure (not stopped && status == ExitSuccess) ("exact export failed: " ++ C.unpack errors)
      let expected = B.map (`xor` 255) (B.drop 532 bytes)
      payload <- H.readRegular (64*1024*1024) (destination </> "payload-complement.bin")
      H.ensure (payload == expected) "independent complement bytes differ"
      upstream <- H.readRegular (64*1024*1024) (destination </> "upstream-readable.bin")
      H.ensure (upstream == expected <> B.replicate 532 255) "independent upstream bytes/padding differ"
      original <- H.readRegular (1024*1024) (destination </> "inventory.json")
      (retryStopped, retryStatus, _, retryErrors) <- H.runBounded 45000000 (1024*1024) "node" [adapter, checkout, dat, destination] B.empty
      H.ensure (not retryStopped && retryStatus == ExitFailure 1 && "EEXIST" `B.isInfixOf` retryErrors) "existing-output retry was accepted"
      after <- H.readRegular (1024*1024) (destination </> "inventory.json")
      H.ensure (after == original) "rejected retry changed original inventory"
      modifyIORef' checked (+2)
    _ -> H.failH "pass both PATCHER_CHECKOUT EXACT_DAT, or neither"
  readIORef checked

main :: IO ()
main = do
  outcome <- try $ do
    arguments <- getArgs
    current <- getCurrentDirectory
    let (repository, optional) = case arguments of "--repo":root:rest -> (root,rest); _ -> (current,arguments)
    absolute <- canonicalizePath repository
    exists <- doesFileExist (absolute </> "spec/README.md")
    unless exists (H.failH "--repo must name this repository")
    count <- checkExport absolute optional
    putStrLn (show count ++ " host-only export boundary/independent-byte checks passed")
  case outcome :: Either SomeException () of
    Left err -> hPutStrLn stderr (displayException err) >> exitFailure
    Right () -> pure ()
