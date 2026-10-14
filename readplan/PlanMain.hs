-- Emits a read plan for the bounded adjustment-area channel from the private decoded images.
-- Host-only: it opens files and no device.
module Main (main) where

import Control.Exception (bracket)
import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Data.Word (Word32)
import Numeric (readHex, showHex)
import System.Environment (getArgs, getExecutablePath, getProgName)
import System.Exit (die)
import System.Posix.IO (OpenFileFlags(..), OpenMode(WriteOnly), closeFd, defaultFileFlags, openFd)
import System.Posix.Unistd (fileSynchronise)
import qualified System.Posix.IO.ByteString as Posix
import FujiAF.AdjustmentTables
import FujiAF.JSON (encodeCanonical)
import FujiAF.PrivatePath (requireNewPrivateFile, requirePrivateLocation)
import FujiAF.ReadPlan
import qualified FujiAF.Broker as P

imageLimit :: Int
imageLimit = 8 * 1024 * 1024

executableLimit :: Int
executableLimit = 256 * 1024 * 1024

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    (scope : name : output : specs) -> generate scope name output specs
    _ -> do
      program <- getProgName
      die ("usage: " ++ program ++ " SCOPE PLAN_ID NEW_PRIVATE_PLAN_JSON [IMAGE@LOAD_ADDRESS ...]\nscopes: "
            ++ unwords scopeNames ++ "\nrc0 scopes need the decoded images; adjustment-42000 takes none")

generate :: String -> String -> FilePath -> [String] -> IO ()
generate scopeText planText output specs = do
  scope <- orDie (parseScope scopeText)
  name <- orDie (planId planText)
  requireNewPrivateFile output
  parsed <- traverse (orDie . parseImageSpec) specs
  images <- traverse loadImage parsed
  generator <- sha256Of <$> (getExecutablePath >>= P.readBounded executableLimit)
  plan <- orDie (buildPlan scope name generator images)
  document <- orDie (planValue plan)
  let windows = planWindowList plan
      planBytes = encodeCanonical document
  index <- orDie (oracleIndex windows)
  writeNew output planBytes
  putStrLn ("scope " ++ scopeName scope ++ " plan " ++ planIdString name)
  forM_ (planTables plan) $ \resolution -> do
    let TableName table = tableName (resolvedTable resolution)
        CodeAddress at = tableAddress (resolvedTable resolution)
        offsets = [offset | ResolvedEntry _ (AdjOffset offset) <- resolvedEntries resolution]
    putStrLn ("table " ++ table ++ " " ++ showAddress at ++ " entries " ++ show (length offsets)
               ++ " resolved " ++ showOffset (minimum offsets) ++ ".." ++ showOffset (maximum offsets))
  unless (null (planTables plan))
    (putStrLn ("table windows " ++ show (length (tableWindowOffsets (planTables plan)))))
  putStrLn ("windows " ++ show (length windows) ++ " read bytes " ++ show (readBytes windows)
             ++ " transfers " ++ show (transferCount windows))
  putStrLn ("oracle window index " ++ show index ++ " offset " ++ showOffset oracleOffset
             ++ " byte offset 0x" ++ showHex oracleByteOffset "")
  forM_ (planImages plan) $ \image ->
    putStrLn ("image " ++ imagePath image ++ " sha256 " ++ sha256Text (imageDigest image))
  putStrLn ("generator sha256 " ++ sha256Text generator)
  putStrLn ("plan " ++ output ++ " sha256 " ++ sha256Text (sha256Of planBytes))

loadImage :: (FilePath, Word32) -> IO DecodedImage
loadImage (path, address) = do
  requirePrivateLocation path
  bytes <- P.readBounded imageLimit path
  pure (decodedImage path (CodeAddress address) bytes)

parseImageSpec :: String -> Either String (FilePath, Word32)
parseImageSpec spec = case break (== '@') spec of
  (path, '@' : address) | not (null path) -> (,) path <$> parseAddress address
  _ -> Left ("image argument " ++ show spec ++ " must be PATH@LOAD_ADDRESS")

parseAddress :: String -> Either String Word32
parseAddress ('0' : 'x' : digits@(_ : _)) = case readHex digits of
  [(value, "")] | value <= 0xffffffff -> Right (fromIntegral (value :: Integer))
  _ -> Left ("load address 0x" ++ digits ++ " is not a 32-bit hexadecimal address")
parseAddress other = Left ("load address " ++ show other ++ " must be hexadecimal with a 0x prefix")

orDie :: Either String a -> IO a
orDie = either die pure

writeNew :: FilePath -> B.ByteString -> IO ()
writeNew path bytes =
  bracket (openFd path WriteOnly defaultFileFlags {creat = Just 0o600, exclusive = True, nofollow = True, cloexec = True}) closeFd $ \fd -> do
    written <- Posix.fdWrite fd bytes
    unless (fromIntegral written == B.length bytes) (die ("short write to " ++ path))
    fileSynchronise fd
