{-# LANGUAGE LambdaCase #-}
-- Build the Classic Chrome extension parts (reports/classic-chrome-patch.md
-- section 8.1) from a parameter file, the linked stubs and the pinned stock
-- images. Every current word and blob is checked against the stock bytes, and
-- the image-4 write set against RC0's patch set, before anything is written.
module Main (main) where

import Control.Monad (forM, forM_, when)
import Data.Aeson (Value (Null))
import Data.Bits (complement)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Char (isDigit, isHexDigit, isLower)
import Data.List (isPrefixOf, sort, sortOn)
import Data.Time.Clock (getCurrentTime)
import Data.Time.Format (defaultTimeLocale, formatTime)
import Data.Word (Word32)
import FujiAF.ClassicChrome
import qualified FujiAF.Host as H
import FujiAF.JSON (encodeCanonical)
import qualified FujiAF.UpdateContainer as U
import Numeric (readHex, showHex)
import System.Directory (canonicalizePath, createDirectory, doesPathExist, getCurrentDirectory)
import System.Environment (getArgs, lookupEnv)
import System.FilePath (addTrailingPathSeparator, isAbsolute, takeDirectory, takeFileName, (</>))

newtype Sha256 = Sha256 String deriving (Eq)

data Pinned = Pinned FilePath Sha256

data Blob = Blob
  { blobId :: String
  , blobRole :: String
  , blobGroup :: Group
  , blobAddress :: Address
  , blobCurrent :: B.ByteString
  , blobNew :: B.ByteString
  }

data PatchSet = PatchSet
  { setImage :: Image
  , setBlobs :: [Blob]
  , setWords :: [WordWrite]
  }

usage :: String
usage = "usage: build-classic-chrome [--output-sha256 HEX] PARAMETERS_JSON NEW_OUTPUT_DIRECTORY"

stockId4, stockId5, stockDat, rc0PatchSet :: Pinned
stockId4 = Pinned "private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin" (Sha256 "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")
stockId5 = Pinned "private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin" (Sha256 "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a")
stockDat = Pinned "private-data/firmware/x-m1-1.50/FWUP0002.DAT" (Sha256 stockFirmwareSha256)
rc0PatchSet = Pinned "recovery/rc0-patch-set.json" (Sha256 "586bbb6fc6a9865e4419627e1251296fd5a5050e4072bc03811f7592c88a94e1")

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    capture : settings : symbols : rest -> do
      (pinnedOutput, parameters, output) <- case rest of
        ["--output-sha256", digest, p, o] -> do
          H.ensure (isSha256 digest) "--output-sha256: lowercase 64-digit SHA-256 required"
          pure (digest, p, o)
        [p, o] -> pure (replicate 64 '0', p, o)
        _ -> H.failH usage
      build capture settings symbols pinnedOutput parameters output
    _ -> H.failH usage

build :: FilePath -> FilePath -> FilePath -> String -> FilePath -> FilePath -> IO ()
build capturePath settingsPath symbolsPath outputSha parametersGiven outputGiven = do
  parametersPath <- resolve parametersGiven >>= canonicalizePath
  parameterBytes <- H.readRegular (1024 * 1024) parametersPath
  parameters <- either (H.failH . ("parameter file refused: " ++)) pure (parseParameters parameterBytes)
  parametersSha <- H.sha256 parameterBytes
  id4 <- readPinned stockId4
  id5 <- readPinned stockId5
  dat <- readPinned stockDat
  rc0 <- readPinned rc0PatchSet >>= H.jsonRead
  captureBlob <- canonicalizePath capturePath >>= H.readRegular 4096
  settingsBlob <- canonicalizePath settingsPath >>= H.readRegular 4096
  symbolText <- canonicalizePath symbolsPath >>= H.readRegular (64 * 1024)
  symbols <- either H.failH pure (parseSymbols (BC.unpack symbolText))
  parsed <- either (H.failH . ("stock DAT: " ++)) pure (U.parseContainer dat)
  plain <- either (H.failH . ("stock DAT plain window: " ++)) (pure . U.mediaImageBytes) (U.extractPlain (U.parsedParts parsed))
  plainSha <- H.sha256 plain

  let symbol name = maybe (H.failH ("symbol table lacks " ++ name)) pure (lookup name symbols)
      stock4 = imageView image4DecodeBase id4
      stock5 = imageView image5DecodeBase id5
      stockPlain = imageView plainDecodeBase plain
      dataBlock = encodeDataBlock parameters
  H.ensure (B.length dataBlock == dataBlockSize) "data block encoding is not 100 bytes"
  dataSym <- symbol "cc_data"
  H.ensure (dataSym == dataBlockAddress) "stub symbol cc_data differs from the data block address"

  -- Stub blobs replace the two converter functions from their entries.
  stubBlobs <- forM [("capture-converter", "cc_convert_capture", captureConverter, captureBlob, "canonical-to-capture converter FUN_00738fe8 rewritten, colour and chroma stubs"), ("settings-converter", "cc_convert_settings", settingsConverter, settingsBlob, "live-to-settings converter FUN_00736a74 rewritten, tone stub")] $
    \(name, entrySymbol, (entry, extent), bytes, role) -> do
      at <- symbol entrySymbol
      H.ensure (at == entry) (entrySymbol ++ " is not linked at the converter entry")
      H.ensure (not (B.null bytes) && B.length bytes `mod` 4 == 0 && B.length bytes <= extent) (name ++ " blob must be whole words inside the converter extent")
      current <- stock4 entry (B.length bytes)
      pure Blob {blobId = name, blobRole = role, blobGroup = GroupCC, blobAddress = entry, blobCurrent = current, blobNew = bytes}
  let insideStub (Address a) = or [s <= a && a < s + fromIntegral (B.length (blobNew b)) | b <- stubBlobs, let Address s = blobAddress b]

  -- RC0 rule: RC0's words with the labels destination moved.
  rc0Words <- mapM rc0Word (H.list (H.get rc0 "words"))
  let moved (a, c, n) = if n == rc0LabelsDestinationStock then (a, c, rc0LabelsDestination) else (a, c, n)
      ours = [(a, writeCurrent w, writeNew w) | w <- rc0MatrixCells ++ rc0LabelPointers ++ rc0ValuePointers, let Address a = writeAddress w]
  H.ensure (sort (map moved rc0Words) == sort ours) "RC0 writes differ from recovery/rc0-patch-set.json's words"
  rc0Values <- rc0Blob rc0 "values-array" (Address 0x0061c340)
  rc0Labels <- rc0Blob rc0 "labels-array" (Address rc0LabelsDestinationStock)
  H.ensure (rc0Values == rc0ValuesStock) "RC0 values array differs from the seven RC0 pairs and terminator"
  H.ensure (B.length rc0Labels == 128 && B.drop 112 rc0Labels == listTerminatorLabels) "RC0 labels array is not seven quadruples and the terminator"

  let (rawValuesAt, rawValuesExpected) = rawValuesStock
      (rawLabelsAt, rawLabelsCount) = rawLabelsStock
  rawValues <- stock4 rawValuesAt (B.length rawValuesExpected)
  H.ensure (rawValues == rawValuesExpected) "stock RAW values array differs from the five stock pairs and terminator"
  rawLabels <- stock4 rawLabelsAt (rawLabelsCount + 16)
  H.ensure (B.drop rawLabelsCount rawLabels == listTerminatorLabels) "stock RAW labels array lacks its terminator after five quadruples"

  let prefixNew content = case content of
        DataBlock -> dataBlock
        RawValues -> B.take 40 rawValues <> valuePair 10 <> valuesTerminator
        ShootingLabels -> B.take 112 rc0Labels <> labelQuadruple 0x83e <> listTerminatorLabels
        ShootingValues -> B.take 56 rc0Values <> valuePair 2 <> valuesTerminator
        RawLabels -> B.take rawLabelsCount rawLabels <> labelQuadruple 0x83e <> listTerminatorLabels
      prefixRole content = case content of
        DataBlock -> ("data-block", "CC data block (colour matrix, chroma, tone record)")
        RawValues -> ("raw-values", "RAW conversion values: stock five rows plus row 10")
        ShootingLabels -> ("shooting-labels", "shooting labels: RC0's seven quadruples plus CLASSIC CHROME 0x83e")
        ShootingValues -> ("shooting-values", "shooting values: RC0's seven rows plus row 2")
        RawLabels -> ("raw-labels", "RAW conversion labels: stock five quadruples plus CLASSIC CHROME 0x83e")
  prefixBlobs <- forM [c | Region c _ _ <- prefixLayout] $ \content -> do
    let bytes = prefixNew content
        (name, role) = prefixRole content
    H.ensure (B.length bytes == regionSize content) (name ++ " length differs from its prefix region")
    pure Blob {blobId = name, blobRole = role, blobGroup = GroupCC, blobAddress = regionAddress content, blobCurrent = B.replicate (B.length bytes) 0, blobNew = bytes}

  let nodePointers = concat [[l, v] | (l, v) <- zip rc0LabelPointers rc0ValuePointers] ++ [rawNodeLabelsWord, rawNodeValuesWord]
      image4 = PatchSet Image4 (prefixBlobs ++ stubBlobs) (rc0MatrixCells ++ conversionRowWords ++ availabilityRowWords ++ nodePointers)

  hooks <- forM hookSites $ \h -> do
    stub <- symbol (hookStub h)
    stockTarget <- symbol (hookStock h)
    H.ensure (insideStub stub) (hookStub h ++ " lies outside the stub blobs")
    H.ensure (blTarget (hookSite h) (hookCurrent h) == Just stockTarget) (hookStub h ++ ": current hook word does not branch to " ++ hookStock h)
    word <- either H.failH pure (encodeBl (hookSite h) stub)
    H.ensure (blTarget (hookSite h) word == Just stub) (hookStub h ++ ": encoded BL decodes to another target")
    pure (WordWrite (hookSite h) (hookCurrent h) word GroupCC ("bl " ++ hookStub h ++ " replaces the " ++ hookRole h))
  let image5 = PatchSet Image5 [] (sortOn writeAddress (image5FixedWords ++ hooks))
      plainSet = PatchSet ImagePlain [] (sortOn writeAddress (plainGateWords ++ plainRawWords))

  -- Current bytes against the pinned images.
  forM_ (setBlobs image4) $ \b -> case [() | Region c _ _ <- prefixLayout, regionAddress c == blobAddress b] of
    [] -> stock4 (blobAddress b) (B.length (blobCurrent b)) >>= \s -> H.ensure (s == blobCurrent b) (blobId b ++ ": current bytes differ from stock image 4")
    _ -> pure ()
  checkWords "image 4" stock4 (setWords image4)
  checkWords "image 5" stock5 (setWords image5)
  checkWords "plain image" stockPlain (setWords plainSet)
  let (_, image2Size, image2Load) = U.image2Extent (U.parsedParts parsed)
      image2Media = B.map complement (U.partImage2 (U.parsedParts parsed))
      Address recordAt = image2RecordAddress
      recordOffset = fromIntegral (recordAt - image2Load)
      recordCurrent = B.take (B.length image2Record) (B.drop recordOffset image2Media)
  H.ensure (recordAt >= image2Load && recordOffset + B.length image2Record <= image2Size) "image-2 record lies outside the image-2 extent"
  H.ensure (recordCurrent == B.replicate 66 0) "image-2 record 0x83e is not 66 zero bytes in the stock DAT"
  H.ensure (B.length image2Record == 66) "image-2 record encoding is not 66 bytes"

  output <- newOutputDirectory outputGiven
  recorded <- formatTime defaultTimeLocale "%Y-%m-%dT%H:%M:%SZ" <$> getCurrentTime
  dataSha <- H.sha256 dataBlock
  captureSha <- H.sha256 captureBlob
  settingsSha <- H.sha256 settingsBlob
  id4Sha <- H.sha256 id4
  id5Sha <- H.sha256 id5
  let provenance =
        H.obj
          [ ("parameters_file", H.s parametersPath)
          , ("parameters_sha256", H.s parametersSha)
          , ("data_block_sha256", H.s dataSha)
          , ("rc0_patch_set", H.s (pinnedPath rc0PatchSet))
          , ("rc0_patch_set_sha256", H.s (pinnedSha rc0PatchSet))
          , ("capture_blob_sha256", H.s captureSha)
          , ("settings_blob_sha256", H.s settingsSha)
          ]
      parts =
        [ ( "cc-image4.json"
          , manifest recorded image4 "CLASSIC-CHROME-01-IMAGE4" 4 image4DecodeBase (Just prefixBase) (pinnedPath stockId4) id4Sha provenance
          )
        , ("cc-image5.json", manifest recorded image5 "CLASSIC-CHROME-01-IMAGE5" 5 image5DecodeBase Nothing (pinnedPath stockId5) id5Sha Null)
        , ("cc-plain.json", manifest recorded plainSet "CLASSIC-CHROME-01-PLAIN" 6 plainDecodeBase Nothing (pinnedPath stockDat ++ " (rebuild-update extract-plain window)") plainSha Null)
        ]
  partShas <- forM parts $ \(name, value) -> do
    let bytes = encodeCanonical value <> BC.pack "\n"
    B.writeFile (output </> name) bytes
    (,) name <$> H.sha256 bytes
  let partSha name = maybe "" id (lookup name partShas)
      step kind name = H.obj [("kind", H.s kind), ("path", H.s name), ("sha256", H.s (partSha name))]
      extension =
        H.obj
          [ ("schema_version", H.n 1)
          , ("id", H.s "CC")
          , ("title", H.s "Classic Chrome on RC0")
          , ("base", H.obj [("model", H.s "X-M1"), ("firmware_version", H.s "1.50"), ("sha256", H.s stockFirmwareSha256)])
          , ( "steps"
            , H.arr
                [ step "image-patch-set" "cc-image4.json"
                , step "image-patch-set" "cc-image5.json"
                , H.obj
                    [ ("kind", H.s "image2-record")
                    , ("id", H.s "0x83e")
                    , ("text", H.s image2RecordText)
                    , ("layout", H.arr (map (H.n . toInteger) image2RecordLayout))
                    , ("current_hex", H.s (hexBytes recordCurrent))
                    ]
                , step "plain-patch-set" "cc-plain.json"
                ]
            )
          , ("output_sha256", H.s outputSha)
          ]
  B.writeFile (output </> "classic-chrome.json") (encodeCanonical extension <> BC.pack "\n")
  B.writeFile (output </> "data-block.bin") dataBlock

  let rows =
        concat
          [ [ (setImage set, address, current, new, role, group)
            | (address, current, new, role, group) <- map blobRow (setBlobs set) ++ map wordRow (setWords set)
            ]
          | set <- [image4, image5, plainSet]
          ]
          ++ [(Image2, image2RecordAddress, recordCurrent, image2Record, "label record 0x83e CLASSIC CHROME", GroupCC)]
      blobRow b = (blobAddress b, blobCurrent b, blobNew b, blobRole b, blobGroup b)
      wordRow w = (writeAddress w, word32LE (writeCurrent w), word32LE (writeNew w), writeRole w, writeGroup w)
      counts =
        [ (image, group, length matching, sum [differing c n | (_, _, c, n, _, _) <- matching])
        | image <- [Image4, Image5, ImagePlain, Image2]
        , group <- [GroupRC0, GroupCC]
        , let matching = [r | r@(i, _, _, _, _, g) <- rows, i == image, g == group]
        , not (null matching)
        ]
      rowJson (image, Address a, c, n, role, group) =
        H.obj [("image", H.s (imageName image)), ("address", H.s (hex32 a)), ("current", H.s (hexBytes c)), ("new", H.s (hexBytes n)), ("role", H.s (groupPrefix group ++ " " ++ role)), ("group", H.s (groupName group))]
      countJson (image, group, writes, changed) =
        H.obj [("image", H.s (imageName image)), ("group", H.s (groupName group)), ("writes", H.n (toInteger writes)), ("changed_bytes", H.n (toInteger changed))]
      summary =
        H.obj
          [ ("parameters_sha256", H.s parametersSha)
          , ("data_block_sha256", H.s dataSha)
          , ("stub_blobs", H.obj [("classic-chrome-capture.bin", H.s captureSha), ("classic-chrome-settings.bin", H.s settingsSha)])
          , ("counts", H.arr (map countJson counts))
          , ("writes", H.arr (map rowJson rows))
          ]
  B.writeFile (output </> "changed-words.json") (encodeCanonical summary <> BC.pack "\n")
  writeFile (output </> "changed-words.md") $
    unlines $
      [ "# Classic Chrome changed words"
      , ""
      , "Parameter file `" ++ parametersPath ++ "` SHA-256 `" ++ parametersSha ++ "`. Data block SHA-256 `" ++ dataSha ++ "`. Stub blobs: `classic-chrome-capture.bin` `" ++ captureSha ++ "`, `classic-chrome-settings.bin` `" ++ settingsSha ++ "`."
      , ""
      , "| Image | Group | Writes | Changed bytes |"
      , "| --- | --- | ---: | ---: |"
      ]
        ++ ["| " ++ imageName i ++ " | " ++ groupName g ++ " | " ++ show w ++ " | " ++ show c ++ " |" | (i, g, w, c) <- counts]
        ++ ["", "| Image | Address | Current | New | Group | Role |", "| --- | --- | --- | --- | --- | --- |"]
        ++ ["| " ++ imageName i ++ " | `" ++ hex32 a ++ "` | `" ++ hexBytes c ++ "` | `" ++ hexBytes n ++ "` | " ++ groupName g ++ " | " ++ groupPrefix g ++ " " ++ r ++ " |" | (i, Address a, c, n, r, g) <- rows]
  forM_ partShas $ \(name, digest) -> putStrLn (name ++ " sha256 " ++ digest)
  putStrLn ("data block sha256 " ++ dataSha ++ ", parameters sha256 " ++ parametersSha)
  forM_ counts $ \(i, g, w, c) -> putStrLn (imageName i ++ " " ++ groupName g ++ ": " ++ show w ++ " writes, " ++ show c ++ " changed bytes")
  putStrLn ("wrote " ++ output)

manifest :: String -> PatchSet -> String -> Integer -> Address -> Maybe Address -> String -> String -> Value -> Value
manifest recorded set identifier segment decodeBase outputBase decodeFile decodeSha arraySource =
  H.obj $
    [ ("schema_version", H.n 3)
    , ("id", H.s identifier)
    , ("tier", H.s "RC2")
    , ("recorded_at", H.s recorded)
    , ("target", H.obj [("model", H.s "X-M1"), ("firmware_version", H.s "1.50"), ("firmware_sha256", H.s stockFirmwareSha256)])
    , ( "image"
      , H.obj $
          [ ("segment_id", H.n segment)
          , ("decode_base", H.s (hexAddress decodeBase))
          , ("decode_file", H.s decodeFile)
          , ("decode_sha256", H.s decodeSha)
          , ("file_offset_rule", H.s ("output file_offset = address - " ++ hexAddress base ++ "; stock source offset = address - " ++ hexAddress decodeBase))
          , ("dump_correspondence", H.s "not established for this build")
          ]
            ++ [("output_base", H.s (hexAddress o)) | Just o <- [outputBase]]
      )
    , ("free_space", freeSpace)
    , ("array_source", arraySource)
    , ("order_rule", H.s orderRule)
    , ("blobs", H.arr blobsJson)
    , ("words", H.arr wordsJson)
    , ( "totals"
      , H.obj
          [ ("blob_words", H.n (toInteger blobWords))
          , ("word_writes", H.n (toInteger (length (setWords set))))
          , ("words", H.n (toInteger (blobWords + length (setWords set))))
          , ("bytes_written", H.n (toInteger (4 * (blobWords + length (setWords set)))))
          , ("prefix_bytes_added", H.n (toInteger prefixAdded))
          , ("changed_bytes", H.n (toInteger changed))
          ]
      )
    , ("unchanged_surfaces", H.arr (map H.s unchanged))
    , ("container_effect", containerEffect)
    , ("emulation_case_parameters", Null)
    ]
      ++ [("extension", H.obj [("kind", H.s "prefix"), ("length", H.n (toInteger prefixAdded)), ("current_hex", H.s (replicate (2 * prefixAdded) '0'))]) | prefixAdded > 0]
  where
    base = maybe decodeBase id outputBase
    Address baseValue = base
    Address decodeValue = decodeBase
    prefixAdded = fromIntegral (decodeValue - baseValue) :: Int
    blobWords = sum [B.length (blobNew b) `div` 4 | b <- setBlobs set]
    offset (Address a) = hex32 (a - baseValue)
    blobsJson =
      [ H.obj
          [ ("id", H.s (blobId b))
          , ("role", H.s (groupPrefix (blobGroup b) ++ " " ++ blobRole b))
          , ("address", H.s (hexAddress (blobAddress b)))
          , ("file_offset", H.s (offset (blobAddress b)))
          , ("length", H.n (toInteger (B.length (blobNew b))))
          , ("order_range", H.arr [H.n (toInteger first), H.n (toInteger (first + B.length (blobNew b) `div` 4 - 1))])
          , ("current_hex", H.s (hexBytes (blobCurrent b)))
          , ("new_hex", H.s (hexBytes (blobNew b)))
          ]
      | (b, first) <- zip (setBlobs set) (scanl (+) 1 [B.length (blobNew b) `div` 4 | b <- setBlobs set])
      ]
    wordsJson =
      [ H.obj
          [ ("order", H.n (toInteger order))
          , ("address", H.s (hexAddress (writeAddress w)))
          , ("file_offset", H.s (offset (writeAddress w)))
          , ("current", H.s (hex32 (writeCurrent w)))
          , ("new", H.s (hex32 (writeNew w)))
          , ("role", H.s (groupPrefix (writeGroup w) ++ " " ++ writeRole w))
          ]
      | (w, order) <- zip (setWords set) [blobWords + 1 ..]
      ]
    changed =
      sum [differing (blobCurrent b) (blobNew b) | b <- setBlobs set]
        + sum [differing (word32LE (writeCurrent w)) (word32LE (writeNew w)) | w <- setWords set]
    orderRule = case setImage set of
      Image4 -> "Prefix blobs in address order, the two converter blobs, RC0's 14 matrix cells, conversion row 2, the key-0xd availability row, then the node pointer words (labels before values per node: shooting, Fn, type-9, RAW conversion). The extension prefix is the zero template."
      _ -> "Address order; every write is in place."
    unchanged = case setImage set of
      Image4 -> ["Q menu", "FILM 1-3 brackets", "unlabelled nine-row list"]
      Image5 -> ["movie and live-view validator FUN_00eee958"]
      _ -> ["eight id-1 #0x83d layout, entry and locator gates", "record base pairs 0x0037b784"]
    freeSpace = case setImage set of
      Image4 ->
        H.obj
          [ ("chosen_region", H.arr [H.s (hexAddress prefixBase), H.s (hexAddress prefixEnd)])
          , ("layout", H.arr [regionJson r | r <- prefixLayout])
          , ("ownership_evidence", H.s "reports/classic-chrome-patch.md sections 6.1 and 6.2")
          ]
      _ -> Null
    regionJson (Region c a n) = H.obj [("content", H.s (show c)), ("address", H.s (hexAddress a)), ("length", H.n (toInteger n))]
    regionJson (ZeroFill a n) = H.obj [("content", H.s "zero"), ("address", H.s (hexAddress a)), ("length", H.n (toInteger n))]
    containerEffect = case setImage set of
      Image4 ->
        H.obj
          [ ("image4_load", H.s (hexAddress prefixBase))
          , ("decoded_size", H.s "0x00282a00")
          , ("segment_size", H.s "0x00282a28")
          , ("embedded_header_address", H.s (hexAddress decodeBase))
          , ("embedded_header", H.s "unchanged")
          , ("dat_extents", H.s "unchanged")
          ]
      _ -> H.obj [("load", H.s (hexAddress decodeBase)), ("decoded_size", H.s "unchanged"), ("dat_extents", H.s "unchanged")]

checkWords :: String -> (Address -> Int -> IO B.ByteString) -> [WordWrite] -> IO ()
checkWords label view ws = forM_ ws $ \w -> do
  bytes <- view (writeAddress w) 4
  let Address a = writeAddress w
  H.ensure (bytes == word32LE (writeCurrent w)) (label ++ " " ++ hex32 a ++ ": stock word " ++ hexBytes bytes ++ " differs from current " ++ hex32 (writeCurrent w))

imageView :: Address -> B.ByteString -> Address -> Int -> IO B.ByteString
imageView (Address base) image (Address a) count = do
  let at = fromIntegral a - fromIntegral base :: Integer
  H.ensure (a >= base && at + toInteger count <= toInteger (B.length image)) ("address " ++ hex32 a ++ " lies outside the image at " ++ hex32 base)
  pure (B.take count (B.drop (fromInteger at) image))

rc0Word :: Value -> IO (Word32, Word32, Word32)
rc0Word v = (,,) <$> hexField (H.get v "address") <*> hexField (H.get v "current") <*> hexField (H.get v "new")

rc0Blob :: Value -> String -> Address -> IO B.ByteString
rc0Blob rc0 name (Address expected) = case [b | b <- H.list (H.get rc0 "blobs"), H.str (H.get b "id") == name] of
  [b] -> do
    at <- hexField (H.get b "address")
    H.ensure (at == expected) ("RC0 " ++ name ++ " is not at " ++ hex32 expected)
    unhex (H.str (H.get b "new_hex"))
  _ -> H.failH ("RC0 patch set lacks blob " ++ name)

hexField :: Value -> IO Word32
hexField value = case H.str value of
  '0' : 'x' : digits | not (null digits), [(n, "")] <- readHex digits, n <= 0xffffffff -> pure (fromInteger n)
  other -> H.failH ("RC0 patch set: hex word expected, got " ++ show other)

unhex :: String -> IO B.ByteString
unhex text = B.pack <$> go text
  where
    go (a : b : rest) | [(n, "")] <- readHex [a, b] = (fromInteger n :) <$> go rest
    go [] = pure []
    go _ = H.failH "RC0 patch set: malformed hex blob"

readPinned :: Pinned -> IO B.ByteString
readPinned (Pinned path (Sha256 expected)) = do
  real <- resolve path >>= canonicalizePath
  bytes <- H.readRegular (64 * 1024 * 1024) real
  digest <- H.sha256 bytes
  H.ensure (digest == expected) (path ++ " differs from its pinned SHA-256 " ++ expected)
  pure bytes

pinnedPath :: Pinned -> FilePath
pinnedPath (Pinned p _) = p

pinnedSha :: Pinned -> String
pinnedSha (Pinned _ (Sha256 s)) = s

-- Relative paths name workspace files under `bazel run`.
resolve :: FilePath -> IO FilePath
resolve path = do
  root <- lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= maybe getCurrentDirectory pure
  pure (if isAbsolute path then path else root </> path)

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

differing :: B.ByteString -> B.ByteString -> Int
differing a b = length (filter id (B.zipWith (/=) a b))

isSha256 :: String -> Bool
isSha256 s = length s == 64 && all (\c -> isDigit c || (isHexDigit c && isLower c)) s

imageName :: Image -> String
imageName = \case
  Image4 -> "image4"
  Image5 -> "image5"
  ImagePlain -> "plain"
  Image2 -> "image2"

groupName :: Group -> String
groupName = \case
  GroupRC0 -> "RC0"
  GroupCC -> "CC"

groupPrefix :: Group -> String
groupPrefix g = groupName g ++ ":"

hexAddress :: Address -> String
hexAddress (Address a) = hex32 a

hex32 :: Word32 -> String
hex32 w = let digits = showHex w "" in "0x" ++ replicate (8 - length digits) '0' ++ digits

hexBytes :: B.ByteString -> String
hexBytes = concatMap (\b -> let d = showHex b "" in replicate (2 - length d) '0' ++ d) . B.unpack
