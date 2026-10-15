module Main (main) where

import Control.Exception (onException)
import Control.Monad (foldM, unless, when)
import Data.Aeson (Value)
import Data.Bits (complement)
import qualified Data.ByteString as B
import Data.Char (isHexDigit)
import Data.List (isPrefixOf)
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Numeric (readHex, showHex)
import RecoveryFirmware
import System.Directory (canonicalizePath, createDirectory, doesPathExist, removeDirectoryRecursive, renameFile)
import System.Environment (getArgs)
import System.FilePath (addTrailingPathSeparator, isAbsolute, takeDirectory, takeFileName, (</>))
import System.IO (hFlush, stdout)
import System.Process (callProcess)

newtype Sha256 = Sha256 String deriving (Eq)
newtype LoadAddress = LoadAddress Integer
newtype FlashOffset = FlashOffset Integer
newtype SpanLength = SpanLength Integer

data ImageRecord = Image4 | Image5

-- Steps run in container order; each consumes the previous step's DAT.
data Step
  = PatchImage ImageRecord (Maybe LoadAddress) FilePath
  | WriteLabel String
  | ReplacePlain FlashOffset SpanLength FilePath

data Extension = Extension
  { extensionId :: String
  , extensionTitle :: String
  , extensionVersion :: String
  , extensionBase :: Sha256
  , extensionSteps :: [Step]
  , extensionOutput :: Sha256
  }

data Tools = Tools
  { toolDecoder :: FilePath
  , toolApplier :: FilePath
  , toolEncoder :: FilePath
  , toolRebuild :: FilePath
  }

usage :: String
usage = "usage: build-extension EXTENSION_JSON STOCK_DAT NEW_OUTPUT_DIRECTORY"

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [decoder, applier, encoder, rebuild, manifest, stock, output] ->
      build (Tools decoder applier encoder rebuild) manifest stock output
    _ -> H.failH usage

build :: Tools -> FilePath -> FilePath -> FilePath -> IO ()
build tools givenManifest givenStock givenOutput = do
  manifestPath <- resolve givenManifest >>= canonicalizePath
  extension <- parseExtension manifestPath
  firmware <- readFirmware givenStock
  let FirmwareDigest stockDigest = firmwareDigest firmware
  H.ensure (Sha256 stockDigest == extensionBase extension) "Stock DAT SHA-256 differs from the extension's base firmware"
  H.ensure (versionText (firmwareVersion firmware) == extensionVersion extension) "Stock DAT version words differ from the extension's firmware_version"
  output <- newOutputDirectory givenOutput
  putStrLn ("Extension " ++ extensionId extension ++ ": " ++ extensionTitle extension)
  hFlush stdout
  let work = output </> "work"
      final = output </> "FWUP0002.DAT"
  (`onException` removeDirectoryRecursive output) $ do
    createDirectory work
    built <- foldM (runStep tools firmware work) (firmwarePath firmware) (zip [1 :: Int ..] (extensionSteps extension))
    callProcess (toolRebuild tools) ["verify", built]
    digest <- H.readRegular (64 * 1024 * 1024) built >>= H.sha256
    H.ensure (Sha256 digest == extensionOutput extension) ("Built firmware SHA-256 " ++ digest ++ " differs from the extension's output_sha256")
    renameFile built final
    removeDirectoryRecursive work
    putStrLn ("PASS: " ++ final ++ " SHA-256 " ++ digest)

runStep :: Tools -> Firmware -> FilePath -> FilePath -> (Int, Step) -> IO FilePath
runStep tools firmware work current (index, step) = do
  let next = work </> ("step-" ++ show index ++ ".DAT")
      file name = work </> ("step-" ++ show index ++ "-" ++ name)
      rebuild = callProcess (toolRebuild tools)
  case step of
    PatchImage record load patchSet -> do
      let (kind, command) = case record of
            Image4 -> (Application4, "replace-record4")
            Image5 -> (Application5, "replace-record5")
      entry <- case [e | Segment k e <- firmwareSegments firmware, k == kind] of
        [e] -> pure e
        _ -> H.failH "Stock DAT segment table lacks the patched image"
      let window = toInteger (U.segFlashLen entry)
      media <- slice (firmwareMain firmware) (FileOffset (fromIntegral (U.segFlash entry))) (ByteCount (fromInteger window))
      B.writeFile (file "stock-record.bin") (B.map complement media)
      callProcess (toolDecoder tools) [file "stock-record.bin", "0", file "stock-image.bin"]
      callProcess (toolApplier tools) [patchSet, file "stock-image.bin", file "patched-image.bin"]
      callProcess (toolEncoder tools) [hex window, file "patched-image.bin", file "patched-record.bin"]
      let loadArguments = case load of
            Just (LoadAddress address) -> ["--load-address", hex address]
            Nothing -> []
      rebuild ([command, "--decoder", toolDecoder tools] ++ loadArguments ++ [current, file "patched-record.bin", file "patched-image.bin", next])
    WriteLabel text -> rebuild ["write-label", current, text, next]
    ReplacePlain (FlashOffset offset) _ blob -> rebuild ["replace-plain", "--flash-offset", hex offset, current, blob, next]
  pure next

parseExtension :: FilePath -> IO Extension
parseExtension path = do
  value <- H.readRegular (1024 * 1024) path >>= H.jsonRead
  H.fields value "schema_version id title base steps output_sha256" "extension"
  _ <- H.integer (H.get value "schema_version") 1 1 "extension schema_version"
  let base = H.get value "base"
  H.fields base "model firmware_version sha256" "extension base"
  H.ensure (H.str (H.get base "model") == "X-M1") "extension base model must be X-M1"
  baseDigest <- digestField (H.get base "sha256") "extension base sha256"
  outputDigest <- digestField (H.get value "output_sha256") "extension output_sha256"
  steps <- mapM (parseStep (takeDirectory path) baseDigest) (H.list (H.get value "steps"))
  H.ensure (not (null steps)) "extension has no steps"
  H.ensure (containerOrder steps) "extension steps must run image 4, image 5, label, then plain spans in increasing non-overlapping flash order, each image and the label at most once"
  pure Extension
    { extensionId = H.str (H.get value "id")
    , extensionTitle = H.str (H.get value "title")
    , extensionVersion = H.str (H.get base "firmware_version")
    , extensionBase = baseDigest
    , extensionSteps = steps
    , extensionOutput = outputDigest
    }

parseStep :: FilePath -> Sha256 -> Value -> IO Step
parseStep directory base value = case H.str (H.get value "kind") of
  "image-patch-set" -> do
    H.fields value "kind path sha256" "image-patch-set step"
    (path, bytes) <- pinnedFile directory value
    patchSet <- H.jsonRead bytes
    let image = H.get patchSet "image"
    H.ensure (Sha256 (H.str (H.get (H.get patchSet "target") "firmware_sha256")) == base) "patch set targets a different base firmware"
    segment <- H.integer (H.get image "segment_id") 4 5 "patch set image segment_id (images 4 and 5 are supported)"
    decodeBase <- hexValue (H.get image "decode_base") "patch set decode_base"
    outputBase <-
      if "output_base" `elem` H.keys image
        then hexValue (H.get image "output_base") "patch set output_base"
        else pure decodeBase
    case segment of
      4 -> pure (PatchImage Image4 (if outputBase == decodeBase then Nothing else Just (LoadAddress outputBase)) path)
      _ -> do
        H.ensure (outputBase == decodeBase) "image-5 patch sets cannot move the image's load address"
        pure (PatchImage Image5 Nothing path)
  "image2-label" -> do
    H.fields value "kind text" "image2-label step"
    let text = H.str (H.get value "text")
    H.ensure (not (null text)) "image2-label text must be nonempty"
    pure (WriteLabel text)
  "plain-span" -> do
    H.fields value "kind flash_offset path sha256" "plain-span step"
    offset <- hexValue (H.get value "flash_offset") "plain-span flash_offset"
    (path, bytes) <- pinnedFile directory value
    H.ensure (not (B.null bytes)) "plain-span file is empty"
    pure (ReplacePlain (FlashOffset offset) (SpanLength (toInteger (B.length bytes))) path)
  other -> H.failH ("Unsupported extension step kind " ++ show other ++ "; this tool cannot build the extension")

containerOrder :: [Step] -> Bool
containerOrder steps = and (zipWith before steps (drop 1 steps))
  where
    rank (PatchImage Image4 _ _) = 1 :: Int
    rank (PatchImage Image5 _ _) = 2
    rank (WriteLabel _) = 3
    rank (ReplacePlain {}) = 4
    before (ReplacePlain (FlashOffset a) (SpanLength n) _) (ReplacePlain (FlashOffset b) _ _) = a + n <= b
    before x y = rank x < rank y

-- Part paths are relative to the extension manifest.
pinnedFile :: FilePath -> Value -> IO (FilePath, B.ByteString)
pinnedFile directory value = do
  let relative = H.str (H.get value "path")
  H.ensure (not (null relative) && not (isAbsolute relative)) "extension part path must be relative to the extension manifest"
  expected <- digestField (H.get value "sha256") "extension part sha256"
  let path = directory </> relative
  bytes <- H.readRegular (16 * 1024 * 1024) path
  digest <- H.sha256 bytes
  H.ensure (Sha256 digest == expected) ("extension part " ++ relative ++ " differs from its pinned SHA-256")
  pure (path, bytes)

newOutputDirectory :: FilePath -> IO FilePath
newOutputDirectory given = do
  output <- resolve given
  root <- resolve "private-data" >>= canonicalizePath
  parent <- canonicalizePath (takeDirectory output)
  H.ensure (addTrailingPathSeparator root `isPrefixOf` addTrailingPathSeparator parent) "Output must be a new directory beneath private-data"
  H.ensure (takeFileName output `notElem` ["", ".", ".."]) "Output must name a new directory"
  exists <- doesPathExist output
  when exists (H.failH "Output directory already exists")
  createDirectory output
  pure output

digestField :: Value -> String -> IO Sha256
digestField value label = do
  let text = H.str value
  unless (length text == 64 && all (\c -> isHexDigit c && c `notElem` ['A' .. 'F']) text) $
    H.failH (label ++ ": lowercase 64-digit SHA-256 required")
  pure (Sha256 text)

hexValue :: Value -> String -> IO Integer
hexValue value label = case H.str value of
  '0' : 'x' : digits | not (null digits), [(n, "")] <- readHex digits -> pure n
  _ -> H.failH (label ++ ": 0x-prefixed hexadecimal required")

hex :: Integer -> String
hex n = "0x" ++ showHex n ""

versionText :: FirmwareVersion -> String
versionText (FirmwareVersion major minor) = show major ++ "." ++ pad (showHex minor "")
  where
    pad digits = replicate (2 - length digits) '0' ++ digits
