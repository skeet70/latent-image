module Main (main) where

import Control.Exception (SomeException, displayException, try)
import Control.Monad (unless)
import Data.Char (isSpace, toLower)
import System.Directory (canonicalizePath)
import System.Environment (getArgs, lookupEnv)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>), isAbsolute)
import System.IO (hFlush, hPutStrLn, stderr, stdout)
import System.Posix.Files (getFileStatus, isBlockDevice, isRegularFile, fileSize)
import System.Process (callProcess, readProcessWithExitCode)

newtype Card = Card FilePath
newtype Sha256 = Sha256 String
newtype ImageBytes = ImageBytes Integer
data RecoveryImage = RecoveryImage FilePath ImageBytes Sha256

data DeviceKind = WholeDisk | Partition

parseDigest :: String -> IO Sha256
parseDigest given = do
  let value = map toLower given
  ensure (length value == 64 && all (`elem` "0123456789abcdef") value) "Expected 64 hexadecimal SHA-256 characters"
  pure (Sha256 value)

ensure :: Bool -> String -> IO ()
ensure condition message = unless condition (ioError (userError message))

prompt :: String -> IO String
prompt message = putStr message >> hFlush stdout >> getLine

capture :: FilePath -> [String] -> IO String
capture executable args = do
  (code, result, errors) <- readProcessWithExitCode executable args ""
  ensure (code == ExitSuccess) (executable ++ " failed: " ++ errors)
  pure (unwords (words result))

kind :: FilePath -> IO DeviceKind
kind path = do
  value <- capture "lsblk" ["-dn", "-o", "TYPE", "--", path]
  case value of
    "disk" -> pure WholeDisk
    "part" -> pure Partition
    _ -> ioError (userError "Expected an SD disk or its partition, not a mapped device")

selectCard :: IO Card
selectCard = do
  callProcess "lsblk" ["-dp", "-o", "NAME,SIZE,MODEL,TRAN,RM,RO,TYPE"]
  given <- prompt "SD mount point or whole device in the separate host reader: "
  ensure (not (null given)) "No card selected"
  source <- if "/dev/" == take 5 given
    then pure given
    else capture "findmnt" ["-n", "-r", "-o", "SOURCE", "--mountpoint", given]
  path <- canonicalizePath source
  status <- getFileStatus path
  ensure (isBlockDevice status) "Not a block device"
  deviceKind <- kind path
  disk <- case deviceKind of
    WholeDisk -> pure path
    Partition -> do
      parent <- capture "lsblk" ["-dn", "-o", "PKNAME", "--", path]
      ensure (not (null parent) && not (any isSpace parent)) "Ambiguous parent disk"
      canonicalizePath ("/dev" </> parent)
  diskKind <- kind disk
  case diskKind of
    Partition -> ioError (userError "Not a whole disk")
    WholeDisk -> pure ()
  callProcess "lsblk" ["-p", "-o", "NAME,SIZE,MODEL,TYPE,MOUNTPOINTS", "--", disk]
  pure (Card disk)

unmounted :: Card -> IO ()
unmounted (Card disk) = do
  mounts <- capture "lsblk" ["-nr", "-o", "MOUNTPOINTS", "--", disk]
  ensure (null mounts) "Mounted or swap-active device. Unmount only the expendable SD card, then rerun; do not eject it yet."

checkCard :: ImageBytes -> Card -> IO ()
checkCard (ImageBytes imageBytes) card@(Card disk) = do
  status <- getFileStatus disk
  ensure (isBlockDevice status) "Not a block device"
  deviceKind <- kind disk
  case deviceKind of
    Partition -> ioError (userError "Not a whole disk")
    WholeDisk -> pure ()
  readonly <- capture "lsblk" ["-dn", "-o", "RO", "--", disk]
  ensure (readonly == "0") "Device is read-only"
  unmounted card
  sector <- capture "sudo" ["blockdev", "--getss", disk]
  ensure (sector == "512") "Reader does not expose 512-byte sectors"
  capacity <- capture "sudo" ["blockdev", "--getsize64", disk]
  case reads capacity :: [(Integer, String)] of
    [(bytes, "")] -> ensure (bytes >= imageBytes) "Card is smaller than the recovery image"
    _ -> ioError (userError "Invalid device capacity")

checkImage :: RecoveryImage -> IO ()
checkImage (RecoveryImage path (ImageBytes imageBytes) (Sha256 expected)) = do
  status <- getFileStatus path
  ensure (isRegularFile status && toInteger (fileSize status) == imageBytes) "Image type or size changed"
  digest <- capture "sha256sum" ["--zero", "--", path]
  ensure (take 64 digest == expected) "Image SHA-256 differs from the expected build/review digest"

resolveImage :: FilePath -> Sha256 -> IO RecoveryImage
resolveImage given digest = do
  ensure (not (null given)) "No image selected"
  root <- lookupEnv "BUILD_WORKSPACE_DIRECTORY"
  let located = case root of
        Just workspace | not (isAbsolute given) -> workspace </> given
        _ -> given
  path <- canonicalizePath located
  status <- getFileStatus path
  let bytes = toInteger (fileSize status)
  ensure (isRegularFile status && bytes > 0 && bytes `mod` 512 == 0) "Expected a nonempty, sector-aligned regular SD BOOT image"
  pure (RecoveryImage path (ImageBytes bytes) digest)

readback :: RecoveryImage -> Card -> IO ()
readback image@(RecoveryImage path imageSize@(ImageBytes imageBytes) _) card@(Card disk) = do
  checkImage image
  checkCard imageSize card
  callProcess "sudo" ["blockdev", "--flushbufs", disk]
  callProcess "sudo" ["cmp", "--bytes=" ++ show imageBytes, "--", path, disk]

prepare :: IO ()
prepare = do
  putStrLn "Camera must be disconnected. This destroys the selected spare SD card's partition/filesystem bytes. Preserve the successful recovery card and all backups."
  card@(Card disk) <- selectCard
  confirmation <- prompt ("ERASE the whole disk. Type " ++ disk ++ " to confirm: ")
  ensure (confirmation == disk) "Cancelled"
  given <- prompt "Recovery SD BOOT image location: "
  expected <- prompt "Expected SHA-256 from the image build/review: "
  digest <- parseDigest expected
  image@(RecoveryImage _ imageSize (Sha256 hash)) <- resolveImage given digest
  putStrLn ("Expected image SHA-256: " ++ hash)
  putStrLn "A matching hash verifies image identity, not compatibility or safety. Use only an image reviewed for your exact failure and firmware."
  checkImage image
  putStrLn "Unmount all partitions of this SD card using your desktop or umount. Do not format or eject it."
  _ <- prompt "Press Enter when unmounted: "
  checkCard imageSize card
  let RecoveryImage path _ _ = image
  callProcess "sudo" ["dd", "if=" ++ path, "of=" ++ disk, "bs=4M", "conv=notrunc,fsync", "status=progress"]
  readback image card
  putStrLn "PASS: image readback matches"
  _ <- prompt "Physically remove the SD card from the host reader and reinsert it. Do not format, repair or copy files onto it. Press Enter when done: "
  reinserted <- selectCard
  putStrLn "Unmount this card's partitions if the desktop mounted them. Do not eject it."
  _ <- prompt "Press Enter when unmounted for the read-only second check: "
  readback image reinserted
  putStrLn "PASS: card readback after physical reinsertion matches the expected recovery image"
  putStrLn "Remove and preserve the card. Both PASS results verify host preparation only; no camera operation is authorized or performed."

main :: IO ()
main = do
  result <- try $ do
    args <- getArgs
    case args of
      [] -> prepare
      ["--verify-image", path, "--sha256", expected] -> do
        digest <- parseDigest expected
        image <- resolveImage path digest
        checkImage image
        putStrLn "PASS: image matches the supplied SHA-256"
      ["--help"] -> putStrLn "Usage: prepare-card [--help | --verify-image IMAGE --sha256 HASH]\nInteractive, destructive spare-SD preparation with full readback and physical-reinsertion readback. Prompts for a mount point/device, erase confirmation, image and required expected SHA-256. A supplied hash does not establish camera compatibility. No camera I/O."
      _ -> ioError (userError "Usage: prepare-card [--help | --verify-image IMAGE --sha256 HASH]")
  case result :: Either SomeException () of
    Left failure -> hPutStrLn stderr (displayException failure) >> exitFailure
    Right () -> pure ()
