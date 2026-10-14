{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (forM, forM_)
import Data.Aeson (Value)
import Data.Char (isHexDigit, toLower)
import Data.Bits (complement)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (isPrefixOf)
import Data.Word (Word32)
import qualified FujiAF.Host as H
import qualified FujiAF.JSON as J
import qualified FujiAF.UpdateContainer as U
import System.Directory (canonicalizePath, createDirectory, createDirectoryIfMissing)
import System.Environment (getArgs)
import System.Exit (ExitCode (..))
import System.FilePath ((</>), splitDirectories)
import System.IO.Error (isDoesNotExistError, tryIOError)
import System.Posix.Files (FileStatus, getSymbolicLinkStatus, isDirectory, isSymbolicLink)

newtype Digest = Digest String deriving (Eq)
newtype FlashOffset = FlashOffset Word32
newtype RamAddress = RamAddress Word32
newtype ByteCount = ByteCount Word32

data SectionName = Boot | Kernel0 | Kernel1 | Resources2 | Resources3 | SectionTable | Section4 | Section5 | PlainImage

data Section = Section
  { sectionName :: SectionName
  , sectionAddress :: RamAddress
  , sectionFlash :: FlashOffset
  , sectionBytes :: B.ByteString
  , sectionExecutable :: Bool
  }

nameText :: SectionName -> String
nameText name = case name of
  Boot -> "boot"
  Kernel0 -> "kernel0"
  Kernel1 -> "kernel1"
  Resources2 -> "resources2"
  Resources3 -> "resources3"
  SectionTable -> "section_table"
  Section4 -> "section4"
  Section5 -> "section5"
  PlainImage -> "plain_image"

stockDigest :: Digest
stockDigest = Digest "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"


parseDigest :: String -> IO Digest
parseDigest value = do
  H.ensure (length value == 64 && all isHexDigit value) "expected SHA-256 must be 64 hexadecimal digits"
  pure (Digest (map toLower value))
digestValue :: Digest -> Value
digestValue (Digest value) = H.s value

slice :: B.ByteString -> FlashOffset -> ByteCount -> IO B.ByteString
slice media (FlashOffset offset) (ByteCount size) = do
  H.ensure (toInteger offset + toInteger size <= toInteger (B.length media)) "mapped span exceeds DAT media bytes"
  pure (B.take (fromIntegral size) (B.drop (fromIntegral offset) media))

inspectPath :: FilePath -> IO (Maybe FileStatus)
inspectPath path = do
  result <- tryIOError (getSymbolicLinkStatus path)
  case result of
    Right status -> pure (Just status)
    Left err | isDoesNotExistError err -> pure Nothing
             | otherwise -> ioError err

checkDirectories :: FilePath -> FilePath -> IO ()
checkDirectories root relative =
  forM_ (scanl (</>) root (splitDirectories relative)) $ \path -> do
    status <- inspectPath path
    case status of
      Nothing -> pure ()
      Just existing -> H.ensure (not (isSymbolicLink existing) && isDirectory existing) ("destination ancestor must be a nonsymlink directory: " ++ path)

checkAbsent :: FilePath -> IO ()
checkAbsent path = do
  status <- inspectPath path
  H.ensure (case status of Nothing -> True; Just _ -> False) ("output already exists: " ++ path)

sectionValue :: FilePath -> Section -> IO Value
sectionValue output section = do
  let file = nameText (sectionName section) ++ ".bin"
      RamAddress address = sectionAddress section
      FlashOffset offset = sectionFlash section
      bytes = sectionBytes section
  digest <- Digest <$> H.sha256 bytes
  B.writeFile (output </> file) bytes
  pure (H.obj
    [ ("name", H.s (nameText (sectionName section)))
    , ("file", H.s ("firmware/mapped-xm1/" ++ file))
    , ("address", H.n (toInteger address))
    , ("size", H.n (toInteger (B.length bytes)))
    , ("native_flash_offset", H.n (toInteger offset))
    , ("DAT_offset", H.n (toInteger offset + toInteger U.stagingLength))
    , ("sha256", digestValue digest)
    , ("executable", H.b (sectionExecutable section))
    ])

main :: IO ()
main = do
  args <- getArgs
  (expectedDigest, datPath, image4Path, image5Path, romPath, decoder, scratch) <- case args of
    [dat, image4, image5, rom, fflzd, root] ->
      pure (stockDigest, dat, image4, image5, rom, fflzd, root)
    ["--expected-sha256", expected, dat, image4, image5, rom, fflzd, root] -> do
      digest <- parseDigest expected
      pure (digest, dat, image4, image5, rom, fflzd, root)
    _ -> H.failH "usage: export-x70-map [--expected-sha256 SHA256] DAT DECODE_ID4 DECODE_ID5 ROM FFLZD SCRATCH_SOURCE"
  private <- canonicalizePath "private-data"
  root <- canonicalizePath scratch
  H.ensure (splitDirectories private `isPrefixOf` splitDirectories root && root /= private) "scratch must be below private-data"
  dat <- H.readRegular (64 * 1024 * 1024) datPath
  digest <- Digest <$> H.sha256 dat
  H.ensure (digest == expectedDigest) "DAT does not match the expected X-M1 1.50 input"
  parsed <- either H.failH pure (U.parseContainer dat)
  let results = U.checks parsed ++ U.carriedChecks parsed
        ++ if expectedDigest == stockDigest then U.stockChecks parsed else []
  forM_ results $ \check -> H.ensure (U.checkPassed check) ("container contract: " ++ U.checkField check)
  let parts = U.parsedParts parsed
      media = B.map complement (B.drop U.stagingLength dat)
      entries = U.partSegments parts
      output = root </> "firmware/mapped-xm1"
      romOutput = root </> "research/hardware/ff80-bootrom-20261003T190308Z-d56f5032"
      datOutput = root </> "firmware/original"
      findEntry ident = case filter ((== ident) . U.segId) entries of
        [entry] -> pure entry
        _ -> H.failH "required segment missing"
  forM_ ["firmware", "firmware/original", "research/hardware/ff80-bootrom-20261003T190308Z-d56f5032"] (checkDirectories root)
  forM_ [output, romOutput </> "bootrom.bin", datOutput </> "FWUP0002.DAT"] checkAbsent
  image4 <- H.readRegular (16 * 1024 * 1024) image4Path
  image5 <- H.readRegular (16 * 1024 * 1024) image5Path
  rom <- H.readRegular 16384 romPath
  romDigest <- Digest <$> H.sha256 rom
  H.ensure (B.length rom == 16384 && romDigest == Digest "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be") "ROM must be this body's captured block"
  createDirectory output
  let decode ident image record = do
        let source = output </> ("record" ++ show ident ++ ".bin")
            destination = output </> ("decoded" ++ show ident ++ ".bin")
            expectedSize = U.recDecodedSize (U.recHeader record)
        B.writeFile source (B.map complement (U.serializeRecord record))
        (timed, code, stdoutBytes, stderrBytes) <- H.runBounded 20000000 8192 decoder [source, "0", destination, show expectedSize] B.empty
        H.ensure (not timed && code == ExitSuccess) ("DAT decode failed: " ++ C.unpack (stdoutBytes <> stderrBytes))
        decoded <- H.readRegular (16 * 1024 * 1024) destination
        H.ensure (decoded == image) "supplied decoded artifact does not match its DAT record"
  decode (4 :: Int) image4 (U.partRecord4 parts)
  decode (5 :: Int) image5 (U.partRecord5 parts)
  boot <- slice media (FlashOffset 0x40000) (ByteCount 0x20000)
  table <- slice media (FlashOffset 0x140000) (ByteCount 0x400)
  rawSections <- forM [(0, Kernel0, True), (1, Kernel1, True), (2, Resources2, False), (3, Resources3, False)] $ \(ident, name, executable) -> do
    entry <- findEntry ident
    bytes <- slice media (FlashOffset (U.segFlash entry)) (ByteCount (U.segSize entry))
    pure (Section name (RamAddress (U.segLoad entry)) (FlashOffset (U.segFlash entry)) bytes executable)
  entry4 <- findEntry 4
  entry5 <- findEntry 5
  entry6 <- findEntry 6
  -- Entry 6's load/size duplicate entry 5; its consumer uses the extent descriptor.
  let plainAddress = U.word32At table 0x1a8
      plainSize = U.word32At table 0x1ac
  H.ensure (plainSize == U.segFlashLen entry6) "plain extent descriptor differs from segment flash length"
  plain <- slice media (FlashOffset (U.segFlash entry6)) (ByteCount plainSize)
  let sections = [Section Boot (RamAddress 0) (FlashOffset 0x40000) boot True]
        ++ rawSections
        ++ [ Section SectionTable (RamAddress 0x61c000) (FlashOffset 0x140000) table False
           , Section Section4 (RamAddress (U.segLoad entry4)) (FlashOffset (U.segFlash entry4)) image4 True
           , Section Section5 (RamAddress (U.segLoad entry5)) (FlashOffset (U.segFlash entry5)) image5 True
           , Section PlainImage (RamAddress plainAddress) (FlashOffset (U.segFlash entry6)) plain True
           ]
  rows <- mapM (sectionValue output) sections
  let bss entry = H.obj [("address", H.n (toInteger (U.segBss entry))), ("size", H.n (toInteger (U.segBssLen entry)))]
      manifest = H.obj
        [ ("source_sha256", digestValue digest)
        , ("native_flash_DAT_offset", H.n (toInteger U.stagingLength))
        , ("sections", H.arr rows)
        , ("unmapped_BSS", H.arr (map bss [entry4, entry5]))
        , ("notes", H.s (if expectedDigest == stockDigest
            then "X-M1 1.50 stock DAT; image4/5 independently decoded by the project C decoder and byte-compared with retained artifacts. Section4 size is decoded bytes, not its +0x28 segment size. Entry6 uses the plain extent descriptor. Entry7 has no resident load extent and is omitted. DAT has no physical spare or per-unit storage state."
            else "Caller-pinned X-M1 1.50 candidate DAT; image4/5 independently decoded by the project C decoder and byte-compared with retained artifacts. Section4 size is decoded bytes, not its +0x28 segment size. Entry6 uses the plain extent descriptor. Entry7 has no resident load extent and is omitted. DAT has no physical spare or per-unit storage state."))
        ]
  B.writeFile (output </> "manifest.json") (J.encodeCanonical manifest <> "\n")
  createDirectoryIfMissing True romOutput
  createDirectoryIfMissing True datOutput
  B.writeFile (romOutput </> "bootrom.bin") rom
  B.writeFile (datOutput </> "FWUP0002.DAT") dat
  H.jsonPrint manifest
