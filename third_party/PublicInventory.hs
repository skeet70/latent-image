{-# LANGUAGE OverloadedStrings #-}
-- Read exact Git objects only. No upstream import, build or camera API exists.
module PublicInventory (gitBytes, inventory, inventoryUsing, verifySelected) where

import qualified FujiAF.Host as H
import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value(..))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Char (isDigit)
import qualified Data.Text as T
import qualified Data.Text.Encoding as T
import System.Exit (ExitCode(..))
import System.Timeout (timeout)

withinInventoryBudget :: IO a -> IO a
withinInventoryBudget action = timeout 60000000 action >>= maybe (H.failH "inventory wall budget exceeded") pure

gitBytes :: FilePath -> [String] -> IO B.ByteString
gitBytes checkout arguments = do
  (stopped, status, output, errors) <- H.runBounded 10000000 (64 * 1024 * 1024) "git" (["-C", checkout] ++ arguments) B.empty
  H.ensure (not stopped && status == ExitSuccess) ("bounded Git read failed: " ++ C.unpack (B.take 4096 errors))
  pure output

utf8 :: B.ByteString -> IO String
utf8 bytes = either (H.failH . show) (pure . T.unpack) (T.decodeUtf8' bytes)

gitText :: FilePath -> [String] -> IO String
gitText checkout arguments = C.unpack . C.takeWhile (/= '\n') <$> gitBytes checkout arguments

inventory :: FilePath -> String -> String -> IO Value
inventory = inventoryUsing gitBytes

inventoryUsing :: (FilePath -> [String] -> IO B.ByteString) -> FilePath -> String -> String -> IO Value
inventoryUsing readGit checkout revision origin = withinInventoryBudget $ do
  H.ensure (length revision == 40 && all (\ch -> isDigit ch || ch `elem` ['a'..'f']) revision) "revision must be the exact lowercase 40-character commit ID"
  let readText arguments = C.unpack . C.takeWhile (/= '\n') <$> readGit checkout arguments
  observed <- readText ["rev-parse", revision ++ "^{commit}"]
  H.ensure (observed == revision) "revision must be the exact 40-character commit ID"
  tree <- readText ["rev-parse", revision ++ "^{tree}"]
  listing <- readGit checkout ["ls-tree", "-rz", "--full-tree", revision]
  H.ensure (B.length listing <= 4 * 1024 * 1024) "Git tree metadata cap exceeded"
  let entries = filter (not . B.null) (B.split 0 listing)
  H.ensure (length entries <= 100000) "Git tree entry cap exceeded"
  files <- forM entries $ \entry -> do
    let (metadata, suffix) = B.break (== 9) entry
    H.ensure (not (B.null suffix)) "malformed Git tree entry"
    case C.words metadata of
      [mode, kind, objectID] -> do
        H.ensure (kind == "blob") "submodule/non-blob requires a separate input inventory"
        path <- utf8 (B.drop 1 suffix)
        bytes <- readGit checkout ["cat-file", "blob", C.unpack objectID]
        H.ensure (B.length bytes <= 64 * 1024 * 1024) "Git blob cap exceeded"
        digest <- H.sha256 bytes
        pure $ H.obj [("path", H.s path), ("mode", H.s (C.unpack mode)),
          ("git_blob", H.s (C.unpack objectID)), ("size_bytes", H.n (fromIntegral (B.length bytes))), ("sha256", H.s digest)]
      _ -> H.failH "malformed Git blob metadata"
  pure $ H.obj [("schema_version", H.n 1), ("origin", H.s origin), ("revision", H.s revision),
    ("git_tree", H.s tree), ("coverage", H.s "All tracked blobs at this exact commit; hashes are not a claim of full audit."),
    ("privacy_class", H.s "Public upstream metadata only; no source/capture bytes copied."),
    ("hash_method", H.s "SHA-256 of exact Git blob bytes, excluding .git and generated/untracked files."), ("files", H.arr files)]

verifySelected :: [(String, FilePath)] -> Value -> IO Int
verifySelected locations document = withinInventoryBudget $ do
  H.fields document "schema_version coverage privacy_class hash_method sources" "selected inventory"
  H.ensure (H.get document "schema_version" == H.n 1) "unsupported inventory schema"
  let cappedArray cap label value = case value of
        Array values | length values <= cap -> pure (H.list value)
        _ -> H.failH (label ++ ": capped array required")
      requiredString label value = case value of
        String text | not (T.null text) -> pure (T.unpack text)
        _ -> H.failH (label ++ ": nonempty string required")
  sources <- cappedArray 64 "selected sources" (H.get document "sources")
  H.ensure (not (null sources)) "selected inventory requires sources"
  counts <- forM sources $ \source -> do
    H.fields source "origin revision git_tree files" "selected source"
    origin <- requiredString "selected origin" (H.get source "origin")
    checkout <- maybe (H.failH "unbound inventory origin") pure (lookup origin locations)
    revision <- requiredString "selected revision" (H.get source "revision")
    H.ensure (length revision == 40 && all (\ch -> isDigit ch || ch `elem` ['a'..'f']) revision) "invalid selected source revision"
    tree <- gitText checkout ["rev-parse", revision ++ "^{tree}"]
    H.ensure (tree == H.str (H.get source "git_tree")) "Git tree mismatch"
    files <- cappedArray 100000 "selected blobs" (H.get source "files")
    H.ensure (not (null files)) "selected source requires blobs"
    forM_ files $ \entry -> do
      H.fields entry "path size_bytes sha256 git_blob" "selected blob"
      path <- requiredString "selected path" (H.get entry "path")
      size <- H.integer (H.get entry "size_bytes") 0 (64 * 1024 * 1024) "selected blob size"
      let objectName = revision ++ ":" ++ path
      bytes <- gitBytes checkout ["show", objectName]
      digest <- H.sha256 bytes
      H.ensure (fromIntegral (B.length bytes) == size && digest == H.str (H.get entry "sha256")) ("Git blob size/SHA256 mismatch: " ++ objectName)
      blob <- gitText checkout ["rev-parse", objectName]
      unless (blob == H.str (H.get entry "git_blob")) (H.failH "Git blob ID mismatch")
    pure (length files)
  pure (sum counts)
