-- Compares captured adjustment-area windows against the firmware fallback adjustment image.
-- Host-only: it opens files and no device, and writes one new report file at a private path.
module Main (main) where

import Control.Exception (IOException, bracket, try)
import Control.Monad (unless)
import qualified Data.ByteString as B
import qualified Data.Text as T
import qualified Data.Text.Encoding as T
import Data.Word (Word32)
import Numeric (readHex)
import System.Environment (getArgs, getProgName)
import System.Exit (die)
import System.FilePath ((</>))
import System.Posix.Files (fileSize, getSymbolicLinkStatus, isRegularFile, isSymbolicLink)
import System.Posix.IO (OpenFileFlags(..), OpenMode(WriteOnly), closeFd, defaultFileFlags, openFd)
import System.Posix.Unistd (fileSynchronise)
import qualified System.Posix.IO.ByteString as Posix
import FujiAF.AdjustmentTables
import FujiAF.JSON (decodeStrictIntegers)
import FujiAF.PrivatePath (requireNewPrivateFile, requirePrivateDirectory, requirePrivateLocation)
import FujiAF.ReadPlan
import FujiAF.WindowCompare
import qualified FujiAF.Broker as P

imageLimit :: Int
imageLimit = 8 * 1024 * 1024

-- FujiAF.Read.planDocumentCap.
planLimit :: Int
planLimit = 1024 * 1024

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    (plan : captures : fallback : report : specs@(_ : _)) -> compare' plan captures fallback report specs
    _ -> do
      program <- getProgName
      die ("usage: " ++ program
            ++ " PLAN_JSON PRIVATE_CAPTURE_DIRECTORY PAYLOAD@FALLBACK_OFFSET NEW_PRIVATE_REPORT IMAGE@LOAD_ADDRESS ...")

compare' :: FilePath -> FilePath -> String -> FilePath -> [String] -> IO ()
compare' planPath captures fallbackSpec reportPath specs = do
  (fallbackPath, fallbackOffset) <- orDie (parseSpec fallbackSpec)
  parsed <- traverse (orDie . parseSpec) specs
  mapM_ requirePrivateLocation (planPath : fallbackPath : map fst parsed)
  requirePrivateDirectory captures
  requireNewPrivateFile reportPath
  planBytes <- P.readBounded planLimit planPath
  document <- orDie (decodeStrictIntegers planBytes)
  windows <- orDie (parsePlan document)
  payload <- P.readBounded imageLimit fallbackPath
  unless (B.length payload > fromIntegral fallbackOffset)
    (die ("fallback payload is shorter than its offset 0x" ++ drop 2 (showAddress fallbackOffset)))
  let fallback = fallbackImage (B.drop (fromIntegral fallbackOffset) payload)
  images <- traverse loadImage parsed
  set <- orDie (imageSet images)
  resolutions <- orDie (resolveTables set RowZeroOnly developTables)
  reports <- traverse (\window -> compareWindow fallback window <$> loadCapture captures window) windows
  let header =
        [ "plan " ++ planPath ++ " sha256 " ++ sha256Text (sha256Of planBytes)
        , "fallback " ++ fallbackPath ++ " at " ++ showAddress fallbackOffset
            ++ " sha256 " ++ sha256Text (sha256Of payload)
        ] ++ [ "image " ++ imagePath image ++ " sha256 " ++ sha256Text (imageDigest image) | image <- images ]
      text = unlines (header ++ reportLines resolutions reports)
  writeNew reportPath (T.encodeUtf8 (T.pack text))
  putStr text

loadCapture :: FilePath -> Window -> IO (Either String B.ByteString)
loadCapture directory window = do
  let path = directory </> captureName window
  status <- try (getSymbolicLinkStatus path)
  case status of
    Left (_ :: IOException) -> pure (Left ("no capture file " ++ captureName window))
    Right entry
      | isSymbolicLink entry -> pure (Left "capture file is a symlink")
      | not (isRegularFile entry) -> pure (Left "capture file is not a regular file")
      | fileSize entry /= fromIntegral windowLength ->
          pure (Left ("captured window is " ++ show (fileSize entry) ++ " bytes, expected " ++ show windowLength))
      | otherwise -> Right <$> P.readBounded (fromIntegral windowLength) path

loadImage :: (FilePath, Word32) -> IO DecodedImage
loadImage (path, address) = decodedImage path (CodeAddress address) <$> P.readBounded imageLimit path

parseSpec :: String -> Either String (FilePath, Word32)
parseSpec spec = case break (== '@') spec of
  (path, '@' : value) | not (null path) -> (,) path <$> parseHex value
  _ -> Left ("argument " ++ show spec ++ " must be PATH@HEXADECIMAL")

parseHex :: String -> Either String Word32
parseHex ('0' : 'x' : digits@(_ : _)) = case readHex digits of
  [(value, "")] | value <= 0xffffffff -> Right (fromIntegral (value :: Integer))
  _ -> Left ("0x" ++ digits ++ " is not a 32-bit hexadecimal value")
parseHex other = Left (show other ++ " must be hexadecimal with a 0x prefix")

orDie :: Either String a -> IO a
orDie = either die pure

writeNew :: FilePath -> B.ByteString -> IO ()
writeNew path bytes =
  bracket (openFd path WriteOnly defaultFileFlags {creat = Just 0o600, exclusive = True, nofollow = True, cloexec = True}) closeFd $ \fd -> do
    written <- Posix.fdWrite fd bytes
    unless (fromIntegral written == B.length bytes) (die ("short write to " ++ path))
    fileSynchronise fd
