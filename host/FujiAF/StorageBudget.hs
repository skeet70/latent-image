{-# LANGUAGE DeriveDataTypeable #-}
module FujiAF.StorageBudget (StorageFailure, checkStorageBudget) where

import Control.Exception (Exception, IOException, catch, throwIO)
import Control.Monad (unless, when)
import Data.Char (isDigit)
import Data.Typeable (Typeable)
import System.Directory (canonicalizePath, makeAbsolute)
import System.Environment (lookupEnv)
import System.Exit (ExitCode (..))
import System.FilePath (takeDirectory)
import System.IO.Error (isDoesNotExistError)
import System.Posix.Files (getFileStatus, getSymbolicLinkStatus, isDirectory, isSymbolicLink)
import System.Process (readProcessWithExitCode)
import Text.Read (readMaybe)

newtype StorageBytes = StorageBytes Integer deriving (Eq, Ord, Show)
newtype StorageReserve = StorageReserve StorageBytes deriving Show

data StorageFailure
  = InvalidStorageReserve String
  | StorageProbeFailed FilePath String
  | StorageReserveUnavailable FilePath StorageBytes StorageReserve
  deriving (Typeable)

instance Show StorageFailure where
  show (InvalidStorageReserve input) =
    "XM1_STORAGE_MIN_FREE must be a positive byte count with optional K, M, G, T or KiB/MiB/GiB/TiB suffix: " ++ show input
  show (StorageProbeFailed path problem) = "Cannot establish available storage at " ++ path ++ ": " ++ problem
  show (StorageReserveUnavailable path (StorageBytes available) (StorageReserve (StorageBytes minimumBytes))) =
    "Storage admission refused at " ++ path ++ ": " ++ show available ++ " bytes available, " ++ show minimumBytes
      ++ " bytes required by XM1_STORAGE_MIN_FREE. Inventory idle compiler caches with //build-support:storage-maintenance; research evidence needs a reviewed compaction plan."

instance Exception StorageFailure

checkStorageBudget :: FilePath -> IO ()
checkStorageBudget output = do
  raw <- maybe "100G" id <$> lookupEnv "XM1_STORAGE_MIN_FREE"
  reserve@(StorageReserve minimumBytes) <- either throwIO pure (parseReserve raw)
  directory <- makeAbsolute output >>= existingDirectory
  (code, out, err) <- readProcessWithExitCode "df" ["-B1", "--output=avail", "--", directory] ""
  unless (code == ExitSuccess) (throwIO (StorageProbeFailed directory err))
  available <- case lines out of
    [_header, value] | Just count <- readMaybe value, count >= (0 :: Integer) -> pure (StorageBytes count)
    _ -> throwIO (StorageProbeFailed directory "df did not return one available-byte count")
  when (available < minimumBytes) (throwIO (StorageReserveUnavailable directory available reserve))

parseReserve :: String -> Either StorageFailure StorageReserve
parseReserve input = case span isDigit input of
  (digits@(_ : _), suffix) | Just multiplier <- lookup suffix units, Just count <- readMaybe digits, count > (0 :: Integer) ->
    Right (StorageReserve (StorageBytes (count * multiplier)))
  _ -> Left (InvalidStorageReserve input)
  where
    units = [("", 1), ("K", 1024), ("KiB", 1024), ("M", 1024 ^ (2 :: Int)), ("MiB", 1024 ^ (2 :: Int)),
      ("G", 1024 ^ (3 :: Int)), ("GiB", 1024 ^ (3 :: Int)), ("T", 1024 ^ (4 :: Int)), ("TiB", 1024 ^ (4 :: Int))]

existingDirectory :: FilePath -> IO FilePath
existingDirectory path = do
  status <- (Just <$> getSymbolicLinkStatus path) `catch` missing
  case status of
    Nothing -> ascend
    Just entry | isSymbolicLink entry -> do
      target <- canonicalizePath path
      resolved <- getFileStatus target
      if isDirectory resolved then pure target else existingDirectory (takeDirectory target)
    Just entry | isDirectory entry -> canonicalizePath path
    Just _ -> ascend
  where
    ascend = do
      let parent = takeDirectory path
      if parent == path
        then throwIO (StorageProbeFailed path "no existing ancestor directory")
        else existingDirectory parent
    missing :: IOException -> IO (Maybe a)
    missing problem = if isDoesNotExistError problem then pure Nothing else throwIO problem
