-- Build the Classic Chrome icon extension (reports/classic-chrome-icon.md)
-- from the reviewed Classic Chrome extension: its parts carried unchanged, the
-- icon's image-4 words appended to its image-4 patch set, an optional live-view
-- fragment appended to its image-5 patch set, a font patch set over the
-- FONT_ICON window, the id-0x83d record step, and a manifest that keeps the
-- 0x83e record step. `diff` decodes two built containers and refuses any byte
-- change outside the write set the build recorded.
module Main (main) where

import Control.Monad (forM, forM_, when)
import Data.Aeson (Value)
import Data.Bits (complement)
import qualified Data.ByteString as B
import Data.Char (isDigit, isHexDigit, isLower)
import Data.List (isPrefixOf, sortOn)
import Data.Word (Word32)
import qualified FujiAF.ClassicChromeIcon as I
import FujiAF.FdlFont
import qualified FujiAF.Host as H
import FujiAF.JSON (encodeCanonical)
import qualified FujiAF.LabelRecord as L
import qualified FujiAF.UpdateContainer as U
import Numeric (readHex, showHex)
import System.Directory (canonicalizePath, createDirectory, doesPathExist, getCurrentDirectory)
import System.Environment (getArgs, lookupEnv)
import System.FilePath (addTrailingPathSeparator, isAbsolute, takeDirectory, takeFileName, (</>))
import System.Process (callProcess)

newtype Sha256 = Sha256 String deriving (Eq, Show)

data Pinned = Pinned FilePath Sha256

-- One write of a patch set, expanded to 4-byte words at decode-file offsets.
data SetWord = SetWord
  { swOrder :: Int
  , swOffset :: Int
  , swNew :: B.ByteString
  }

-- A RAM address and the bytes the built container must hold there.
data Expected = Expected Word32 B.ByteString

usage :: String
usage =
  unlines
    [ "usage: build-classic-chrome-icon build [--output-sha256 HEX] [--liveview IMAGE5_PATCH_SET] CC_MANIFEST NEW_OUTPUT_DIRECTORY"
    , "       build-classic-chrome-icon diff CC_DAT ICON_DAT BUILD_DIRECTORY NEW_OUTPUT_DIRECTORY"
    ]

stockDat, stockId4 :: Pinned
stockDat = Pinned "private-data/firmware/x-m1-1.50/FWUP0002.DAT" (Sha256 "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490")
stockId4 = Pinned "private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin" (Sha256 "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")

-- The reviewed Classic Chrome candidate the icon extends.
ccOutput :: Sha256
ccOutput = Sha256 "16c830ad71b9755d22368ffaeacd6b8b4d78376c9568ac3f9717f45d48292c89"

stockId4Base :: Word32
stockId4Base = 0x0061c400

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    _ : "build" : rest -> parseBuild rest >>= \(digest, liveview, manifest, output) -> build digest liveview manifest output
    decoder : "diff" : [cc, icon, directory, output] -> diff decoder cc icon directory output
    _ -> H.failH usage
  where
    parseBuild = go (replicate 64 '0', Nothing)
    go (_, liveview) ("--output-sha256" : digest : rest) = do
      H.ensure (isSha256 digest) "--output-sha256: lowercase 64-digit SHA-256 required"
      go (digest, liveview) rest
    go (digest, _) ("--liveview" : path : rest) = go (digest, Just path) rest
    go (digest, liveview) [manifest, output] = pure (digest, liveview, manifest, output)
    go _ _ = H.failH usage

build :: String -> Maybe FilePath -> FilePath -> FilePath -> IO ()
build outputSha liveviewGiven manifestGiven outputGiven = do
  manifestPath <- resolve manifestGiven >>= canonicalizePath
  manifest <- H.readRegular (1024 * 1024) manifestPath >>= H.jsonRead
  H.ensure (Sha256 (H.str (H.get manifest "output_sha256")) == ccOutput) "the Classic Chrome manifest's output_sha256 is not the reviewed candidate 16c830ad"
  let Pinned _ (Sha256 stockSha) = stockDat
  H.ensure (H.str (H.get (H.get manifest "base") "sha256") == stockSha) "the Classic Chrome manifest's base is not the stock 1.50 DAT"
  (image4Step, image5Step, record83eStep, plainStep) <- case H.list (H.get manifest "steps") of
    [a, b, c, d]
      | map kind [a, b, c, d] == ["image-patch-set", "image-patch-set", "image2-record", "plain-patch-set"]
          && H.str (H.get c "id") == "0x83e" ->
          pure (a, b, c, d)
    _ -> H.failH "the Classic Chrome manifest is not image 4, image 5, the 0x83e record and the plain patch set"
  let directory = takeDirectory manifestPath
  image4Bytes <- pinnedPart directory image4Step
  image5Bytes <- pinnedPart directory image5Step
  plainBytes <- pinnedPart directory plainStep
  image4 <- H.jsonRead image4Bytes
  image5 <- H.jsonRead image5Bytes
  dat <- readPinned stockDat
  id4 <- readPinned stockId4
  parsed <- either (H.failH . ("stock DAT: " ++)) pure (U.parseContainer dat)
  let parts = U.parsedParts parsed

  -- Font window, the icon's writes and the loader view before and after.
  window <- either (H.failH . ("stock font window: " ++)) (pure . U.mediaImageBytes) (U.extractFont parts)
  windowSha <- H.sha256 window
  font <- either (H.failH . ("stock font: " ++)) pure (parseFdlFont window)
  fontWrites <- either (H.failH . ("icon font writes: " ++)) pure (I.iconFontWrites font)
  let patchedWindow = foldl (\acc w -> let FontOffset at = I.fwOffset w in B.take at acc <> I.fwNew w <> B.drop (at + B.length (I.fwNew w)) acc) window fontWrites
  patchedFont <- either (H.failH . ("patched font: " ++)) pure (parseFdlFont patchedWindow)
  let codes = map CodePoint [0 .. fromIntegral (fdlGlyphCount font) - 1]
  before <- either (H.failH . ("stock glyph lookup: " ++)) pure (mapM (lookupGlyph font) codes)
  after <- either (H.failH . ("patched glyph lookup: " ++)) pure (mapM (lookupGlyph patchedFont) codes)
  let changedCodes = [c | (c, b, a) <- zip3 codes before after, b /= a]
      I.GlyphProgram program = I.iconProgram
  H.ensure (changedCodes == map CodePoint [0xf072, 0xf073]) ("the font writes change the loader result of " ++ show changedCodes ++ ", not only U+F072 and U+F073")
  H.ensure
    (lookupGlyph patchedFont I.iconCode == Right (Just (Glyph I.iconRecordOffset I.iconGlyphFlags (Just (0, 0)) Nothing program)))
    "U+F073 does not resolve to the icon record and program"
  H.ensure (perMilleAt patchedFont I.iconCode == Right I.iconPerMille) "U+F073 per-mille is not 800"
  putStrLn ("font: " ++ show (length codes) ++ " codes looked up; only U+F072 and U+F073 change; U+F073 resolves to the record at " ++ hex32 (I.fontRamAddress I.iconRecordOffset))

  -- Image 4: pinned entries unchanged, the icon words appended.
  (image4Base, pinned4) <- expandSet image4
  let next4 = 1 + maximum (0 : map swOrder pinned4)
  icon4 <- forM (zip [next4 ..] I.iconImage4Writes) $ \(order, w) -> do
    let offset = fromIntegral (I.i4Address w) - fromIntegral image4Base
        covering = [p | p <- pinned4, swOffset p < offset + 4, offset < swOffset p + 4]
    current <- case covering of
      [] -> do
        H.ensure (I.i4Address w >= stockId4Base) (hex32 (I.i4Address w) ++ " lies in the image-4 prefix outside every pinned write")
        pure (B.take 4 (B.drop (fromIntegral (I.i4Address w - stockId4Base)) id4))
      [p] | swOffset p == offset -> pure (swNew p)
      _ -> H.failH (hex32 (I.i4Address w) ++ " partly overlaps a pinned image-4 write")
    H.ensure (current == U.putWord32 (I.i4Old w)) (hex32 (I.i4Address w) ++ ": the Classic Chrome word is " ++ hexBytes current ++ ", not " ++ hex32 (I.i4Old w))
    pure (wordJson order (I.i4Address w) offset (I.i4Old w) (I.i4New w) ("Icon: " ++ I.i4Role w))
  let combined4 =
        H.merge
          image4
          [ ("id", H.s "CLASSIC-CHROME-ICON-01-IMAGE4")
          , ("words", H.arr (H.list (H.get image4 "words") ++ icon4))
          , ("order_rule", H.s (H.str (H.get image4 "order_rule") ++ " Then the icon words: three label-icon words inside the CC labels arrays, the Q labels pointer, the Q values pointer."))
          , ("totals", H.obj [("pinned_orders", H.n (toInteger (next4 - 1))), ("icon_words", H.n (toInteger (length icon4)))])
          ]

  -- Image 5: pinned entries unchanged, the live-view fragment's words appended.
  (image5Base, pinned5) <- expandSet image5
  liveview <- forM liveviewGiven $ \given -> do
    path <- resolve given >>= canonicalizePath
    fragment <- H.readRegular (1024 * 1024) path >>= H.jsonRead
    H.ensure (H.get (H.get fragment "image") "segment_id" == H.get (H.get image5 "image") "segment_id") "the live-view fragment is not an image-5 patch set"
    (fragmentBase, _) <- expandSet fragment
    H.ensure (fragmentBase == image5Base) "the live-view fragment's decode base differs from the image-5 patch set's"
    H.ensure (null (H.list (H.get fragment "blobs"))) "the live-view fragment carries blobs; only words are merged"
    pure (H.list (H.get fragment "words"))
  let next5 = 1 + maximum (0 : map swOrder pinned5)
      fragmentWords = sortOn (H.num . flip H.get "order") (concat liveview)
  merged5 <- forM (zip [next5 ..] fragmentWords) $ \(order, w) -> do
    address <- hexField (H.get w "address")
    new <- hexField (H.get w "new")
    let offset = fromIntegral address - fromIntegral image5Base
    H.ensure (null [p | p <- pinned5, swOffset p < offset + 4, offset < swOffset p + 4]) (hex32 address ++ ": the live-view word overlaps a pinned image-5 write")
    pure (H.set "order" (H.n (toInteger order)) w, expectedJson address (U.putWord32 new))
  let extra5 = map fst merged5
  let combined5 =
        if null extra5
          then image5
          else
            H.merge
              image5
              [ ("id", H.s "CLASSIC-CHROME-ICON-01-IMAGE5")
              , ("words", H.arr (H.list (H.get image5 "words") ++ extra5))
              , ("totals", H.obj [("pinned_orders", H.n (toInteger (next5 - 1))), ("liveview_words", H.n (toInteger (length extra5)))])
              ]

  -- The font patch set over the extract-font window.
  let fontSet = fontPatchSet (H.get image4 "target") windowSha fontWrites

  -- The id-0x83d record's current bytes.
  let (_, extentSize, image2Load) = U.image2Extent parts
      image2Media = B.map complement (U.partImage2 parts)
  recordAt <- either H.failH pure (L.specialRecordOffset L.SpecialLabel83d image2Load extentSize)
  current83d <- either H.failH pure (L.specialRecordBytes (B.take 66 (B.drop recordAt image2Media)))

  output <- newOutputDirectory outputGiven
  let write name value = do
        let bytes = encodeCanonical value
        B.writeFile (output </> name) bytes
        digest <- H.sha256 bytes
        putStrLn ("wrote " ++ (output </> name) ++ " sha256 " ++ digest)
        pure digest
  digest4 <- write "cc-icon-image4.json" combined4
  digest5 <-
    if null extra5
      then B.writeFile (output </> "cc-image5.json") image5Bytes >> H.sha256 image5Bytes
      else write "cc-icon-image5.json" combined5
  B.writeFile (output </> "cc-plain.json") plainBytes
  digestPlain <- H.sha256 plainBytes
  digestFont <- write "cc-icon-font.json" fontSet
  let step path digest = H.obj [("kind", H.s "image-patch-set"), ("path", H.s path), ("sha256", H.s digest)]
      record83d =
        H.obj
          [ ("kind", H.s "image2-record")
          , ("id", H.s "0x83d")
          , ("text_utf16", H.s (L.labelNameUnitHex I.label83dName))
          , ("layout", H.arr (map (H.n . toInteger) (L.labelLayoutList I.label83dLayout)))
          , ("current_hex", H.s (L.specialRecordHex current83d))
          ]
      combinedManifest =
        H.obj
          [ ("schema_version", H.n 1)
          , ("id", H.s "CC-ICON")
          , ("title", H.s "Classic Chrome on RC0 with its film-simulation icon")
          , ("base", H.get manifest "base")
          , ( "steps"
            , H.arr
                [ step "cc-icon-image4.json" digest4
                , step (if null extra5 then "cc-image5.json" else "cc-icon-image5.json") digest5
                , record83d
                , record83eStep
                , H.obj [("kind", H.s "font-patch-set"), ("path", H.s "cc-icon-font.json"), ("sha256", H.s digestFont)]
                , H.obj [("kind", H.s "plain-patch-set"), ("path", H.s "cc-plain.json"), ("sha256", H.s digestPlain)]
                ]
            )
          , ("output_sha256", H.s outputSha)
          ]
  _ <- write "classic-chrome-icon.json" combinedManifest
  _ <-
    write
      "write-set.json"
      ( H.obj
          [ ("font", H.arr [expectedJson (I.fontRamAddress (I.fwOffset w)) (I.fwNew w) | w <- fontWrites])
          , ("image2", H.arr [expectedJson (L.specialRecordAddress L.SpecialLabel83d) I.label83dRecord])
          , ("image4", H.arr [expectedJson (I.i4Address w) (U.putWord32 (I.i4New w)) | w <- I.iconImage4Writes])
          , ("image5", H.arr (map snd merged5))
          ]
      )
  putStrLn
    ( "image 4: "
        ++ show (length pinned4)
        ++ " pinned words carried, "
        ++ show (length icon4)
        ++ " icon words; image 5: "
        ++ show (length extra5)
        ++ " live-view words"
        ++ (if liveviewGiven == Nothing then " (fragment pending)" else "")
    )
  where
    kind v = H.str (H.get v "kind")

fontPatchSet :: Value -> String -> [I.FontWrite] -> Value
fontPatchSet target windowSha writes =
  H.obj
    [ ("schema_version", H.n 1)
    , ("id", H.s "CLASSIC-CHROME-ICON-01-FONT")
    , ("tier", H.s "RC2")
    , ("recorded_at", H.s "2026-10-22")
    , ("target", target)
    , ( "image"
      , H.obj
          [ ("segment_id", H.n 3)
          , ("decode_base", H.s (hex32 I.fontLoadAddress))
          , ("decode_file", H.s "rebuild-update extract-font window of the step's input DAT (FONT_ICON.FDL and its loaded tail, media form)")
          , ("decode_sha256", H.s windowSha)
          , ("file_offset_rule", H.s "file_offset = address - 0x003bc000 = font file offset")
          , ("dump_correspondence", H.s "not established for this build")
          ]
      )
    , ("free_space", H.s "the loaded tail [0x7cfd4, 0x80000) past FONT_ICON.FDL's 0x7cfd4 bytes; every placed byte is zero in the stock window")
    , ("array_source", H.s "FujiAF.ClassicChromeIcon")
    , ("order_rule", H.s "U+F073 directory word, U+F073 per-mille word, then the glyph record, the Q values and the Q labels blobs")
    , ("blobs", H.arr blobs)
    , ("words", H.arr wordsJson)
    , ("totals", H.obj [("word_writes", H.n (toInteger (length wordWrites))), ("blob_words", H.n (toInteger blobWords))])
    , ("unchanged_surfaces", H.arr (map H.s ["every glyph's loader result except U+F072 and U+F073", "every per-mille value except U+F073"]))
    , ("container_effect", H.obj [("dat_extents", H.s "unchanged"), ("segment_words", H.s "unchanged")])
    , ("emulation_case_parameters", H.s "none")
    ]
  where
    wordWrites = [w | w <- writes, B.length (I.fwNew w) == 4]
    blobWrites = [w | w <- writes, B.length (I.fwNew w) /= 4]
    blobWords = sum [B.length (I.fwNew w) `div` 4 | w <- blobWrites]
    offsetOf w = let FontOffset at = I.fwOffset w in at
    wordsJson =
      [ wordJson order (I.fontRamAddress (I.fwOffset w)) (offsetOf w) (le (I.fwOld w)) (le (I.fwNew w)) (I.fwRole w)
      | (order, w) <- zip [1 ..] wordWrites
      ]
    blobs =
      [ H.obj
          [ ("id", H.s ("blob-" ++ show k))
          , ("role", H.s (I.fwRole w))
          , ("address", H.s (hex32 (I.fontRamAddress (I.fwOffset w))))
          , ("file_offset", H.s (hex32 (fromIntegral (offsetOf w))))
          , ("length", H.n (toInteger (B.length (I.fwNew w))))
          , ("order_range", H.arr [H.n (toInteger first), H.n (toInteger (first + B.length (I.fwNew w) `div` 4 - 1))])
          , ("current_hex", H.s (hexBytes (I.fwOld w)))
          , ("new_hex", H.s (hexBytes (I.fwNew w)))
          ]
      | (k, w, first) <- zip3 [1 :: Int ..] blobWrites (scanl (+) (length wordWrites + 1) [B.length (I.fwNew w) `div` 4 | w <- blobWrites])
      ]
    le bytes = U.word32At bytes 0

wordJson :: Int -> Word32 -> Int -> Word32 -> Word32 -> String -> Value
wordJson order address offset current new role =
  H.obj
    [ ("order", H.n (toInteger order))
    , ("address", H.s (hex32 address))
    , ("file_offset", H.s (hex32 (fromIntegral offset)))
    , ("current", H.s (hex32 current))
    , ("new", H.s (hex32 new))
    , ("role", H.s role)
    ]

-- The output base (file offset 0) and every write as 4-byte words.
expandSet :: Value -> IO (Word32, [SetWord])
expandSet set = do
  let image = H.get set "image"
  decodeBase <- hexField (H.get image "decode_base")
  base <- if "output_base" `elem` H.keys image then hexField (H.get image "output_base") else pure decodeBase
  wordList <- forM (H.list (H.get set "words")) $ \w -> do
    offset <- hexField (H.get w "file_offset")
    new <- hexField (H.get w "new")
    pure [SetWord (fromInteger (H.num (H.get w "order"))) (fromIntegral offset) (U.putWord32 new)]
  blobList <- forM (H.list (H.get set "blobs")) $ \b -> do
    offset <- hexField (H.get b "file_offset")
    new <- unhex (H.str (H.get b "new_hex"))
    first <- case H.list (H.get b "order_range") of
      [a, _] -> pure (fromInteger (H.num a))
      _ -> H.failH "blob order_range must be two orders"
    pure
      [ SetWord (first + k) (fromIntegral offset + 4 * k) (B.take 4 (B.drop (4 * k) new))
      | k <- [0 .. B.length new `div` 4 - 1]
      ]
  pure (base, concat (wordList ++ blobList))

expectedJson :: Word32 -> B.ByteString -> Value
expectedJson address bytes = H.obj [("address", H.s (hex32 address)), ("new_hex", H.s (hexBytes bytes))]

parseExpected :: Value -> IO Expected
parseExpected v = Expected <$> hexField (H.get v "address") <*> unhex (H.str (H.get v "new_hex"))

-- ---------------------------------------------------------------------------
-- diff: every changed byte between the Classic Chrome candidate and the icon
-- candidate lies in the recorded write set or in a container-derived field.
-- ---------------------------------------------------------------------------

diff :: FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
diff decoderGiven ccGiven iconGiven directoryGiven outputGiven = do
  decoder <- canonicalizePath decoderGiven
  ccBytes <- resolve ccGiven >>= canonicalizePath >>= H.readRegular (64 * 1024 * 1024)
  iconBytes <- resolve iconGiven >>= canonicalizePath >>= H.readRegular (64 * 1024 * 1024)
  ccSha <- H.sha256 ccBytes
  H.ensure (Sha256 ccSha == ccOutput) "CC_DAT is not the reviewed candidate 16c830ad"
  directory <- resolve directoryGiven >>= canonicalizePath
  writeSet <- H.readRegular (1024 * 1024) (directory </> "write-set.json") >>= H.jsonRead
  let group name = mapM parseExpected (H.list (H.get writeSet name))
  fontSet <- group "font"
  image2Set <- group "image2"
  image4Set <- group "image4"
  image5Set <- group "image5"
  cc <- either (H.failH . ("CC_DAT: " ++)) pure (U.parseContainer ccBytes)
  icon <- either (H.failH . ("ICON_DAT: " ++)) pure (U.parseContainer iconBytes)
  H.ensure (B.length ccBytes == B.length iconBytes) "the containers differ in length"
  output <- newOutputDirectory outputGiven

  -- Container parts: only the image records, the image-2 extent and the
  -- post-tail span (the font window) may change.
  let allowedParts = ["image 4 header", "image 4 stream", "image 5 header", "image 5 stream", "image 2 extent", "main post tail"]
      partDiffs = [d | d <- U.diffParts cc icon, U.diffChangedBytes d /= 0]
  forM_ partDiffs $ \d ->
    H.ensure (U.diffPart d `elem` allowedParts) ("container part " ++ U.diffPart d ++ " changed")
  let mainStart = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader cc))
      outsideMain = [i | i <- [0 .. mainStart - 1], B.index ccBytes i /= B.index iconBytes i]
  H.ensure (all (\i -> i >= U.mainChecksumOffset && i < U.mainChecksumOffset + 4) outsideMain) "a header byte other than the main checksum word changed"

  -- Image-2 extent.
  let (_, _, image2Load) = U.image2Extent (U.parsedParts cc)
      media parsed = B.map complement (U.partImage2 (U.parsedParts parsed))
  image2Changed <- changedWithin "image 2" image2Load (media cc) (media icon) image2Set

  -- Font window.
  ccFont <- either H.failH (pure . U.mediaImageBytes) (U.extractFont (U.parsedParts cc))
  iconFont <- either H.failH (pure . U.mediaImageBytes) (U.extractFont (U.parsedParts icon))
  H.ensure (U.partPostTail (U.parsedParts cc) /= U.partPostTail (U.parsedParts icon) || null fontSet) "the font window did not change"
  fontChanged <- changedWithin "font window" I.fontLoadAddress ccFont iconFont fontSet
  let postTailChanged = sum [U.diffChangedBytes d | d <- partDiffs, U.diffPart d == "main post tail"]
  H.ensure (postTailChanged == fontChanged) "the post-tail span changed outside the font window"

  -- Decoded images 4 and 5.
  let decoded label record segment = do
        let stem = output </> map (\c -> if c == ' ' then '-' else c) label
            recordPath = stem ++ "-record.bin"
            decodedPath = stem ++ "-decoded.bin"
        B.writeFile recordPath (B.map complement (U.serializeRecord record))
        callProcess decoder [recordPath, "0", decodedPath, show (U.recDecodedSize (U.recHeader record))]
        bytes <- H.readRegular (64 * 1024 * 1024) decodedPath
        putStrLn (label ++ ": decoded " ++ show (B.length bytes) ++ " bytes at " ++ hex32 (U.segLoad segment))
        pure (U.segLoad segment, bytes)
      entry k parsed = case [e | e <- U.partSegments (U.parsedParts parsed), U.segId e == k] of
        [e] -> pure e
        _ -> H.failH ("segment " ++ show k ++ " is not unique")
  (load4, cc4) <- entry (4 :: Word32) cc >>= decoded "CC image 4" (U.partRecord4 (U.parsedParts cc))
  (iconLoad4, icon4) <- entry 4 icon >>= decoded "icon image 4" (U.partRecord4 (U.parsedParts icon))
  H.ensure (load4 == iconLoad4) "the image-4 load moved"
  image4Changed <- changedWithin "image 4" load4 cc4 icon4 image4Set
  (load5, cc5) <- entry 5 cc >>= decoded "CC image 5" (U.partRecord5 (U.parsedParts cc))
  (_, icon5) <- entry 5 icon >>= decoded "icon image 5" (U.partRecord5 (U.parsedParts icon))
  iconSha <- H.sha256 iconBytes
  image5Changed <- changedWithin "image 5" load5 cc5 icon5 image5Set

  let summary =
        H.obj
          [ ("cc_dat_sha256", H.s ccSha)
          , ("icon_dat_sha256", H.s iconSha)
          , ("header_bytes_changed", H.arr [H.s (hex32 (fromIntegral i)) | i <- outsideMain])
          , ("container_parts_changed", H.obj [(U.diffPart d, H.n (toInteger (U.diffChangedBytes d))) | d <- partDiffs])
          , ( "write_set_groups"
            , H.obj
                [ ("font_window_bytes", H.n (toInteger fontChanged))
                , ("image2_bytes", H.n (toInteger image2Changed))
                , ("image4_decoded_bytes", H.n (toInteger image4Changed))
                , ("image5_decoded_bytes", H.n (toInteger image5Changed))
                ]
            )
          , ("verdict", H.s "every changed byte lies in the write set, the main checksum word or the re-encoded image-4/5 records")
          ]
  let bytes = encodeCanonical summary
  B.writeFile (output </> "changed-bytes.json") bytes
  B.putStr bytes
  putStrLn ""

-- Counts the bytes that differ between two images at `base`; refuses a change
-- outside the expected writes, and an expected write the new image lacks.
changedWithin :: String -> Word32 -> B.ByteString -> B.ByteString -> [Expected] -> IO Int
changedWithin label base old new expected = do
  H.ensure (B.length old == B.length new) (label ++ ": lengths differ")
  let offsets = [(fromIntegral (a - base), bytes) | Expected a bytes <- expected]
      allowed i = or [at <= i && i < at + B.length bytes | (at, bytes) <- offsets]
      changed = [i | (i, a, b) <- zip3 [0 :: Int ..] (B.unpack old) (B.unpack new), a /= b]
  forM_ offsets $ \(at, bytes) ->
    H.ensure (B.take (B.length bytes) (B.drop at new) == bytes) (label ++ ": the write at " ++ hex32 (base + fromIntegral at) ++ " is not carried")
  case filter (not . allowed) changed of
    [] -> pure ()
    (i : _) -> H.failH (label ++ ": byte " ++ hex32 (base + fromIntegral i) ++ " changed outside the write set")
  putStrLn (label ++ ": " ++ show (length changed) ++ " bytes changed, all inside " ++ show (length offsets) ++ " writes")
  pure (length changed)

pinnedPart :: FilePath -> Value -> IO B.ByteString
pinnedPart directory step = do
  let relative = H.str (H.get step "path")
  H.ensure (not (null relative) && not (isAbsolute relative)) "part path must be relative"
  bytes <- canonicalizePath (directory </> relative) >>= H.readRegular (16 * 1024 * 1024)
  digest <- H.sha256 bytes
  H.ensure (digest == H.str (H.get step "sha256")) (relative ++ " differs from its pinned SHA-256")
  pure bytes

readPinned :: Pinned -> IO B.ByteString
readPinned (Pinned path (Sha256 expected)) = do
  real <- resolve path >>= canonicalizePath
  bytes <- H.readRegular (64 * 1024 * 1024) real
  digest <- H.sha256 bytes
  H.ensure (digest == expected) (path ++ " differs from its pinned SHA-256 " ++ expected)
  pure bytes

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

hexField :: Value -> IO Word32
hexField value = case H.str value of
  '0' : 'x' : digits | not (null digits), [(n, "")] <- readHex digits, n <= (0xffffffff :: Integer) -> pure (fromInteger n)
  other -> H.failH ("hex word expected, got " ++ show other)

unhex :: String -> IO B.ByteString
unhex text = B.pack <$> go text
  where
    go (a : b : rest) | [(n, "")] <- readHex [a, b] = (fromInteger n :) <$> go rest
    go [] = pure []
    go _ = H.failH "malformed hex bytes"

isSha256 :: String -> Bool
isSha256 s = length s == 64 && all (\c -> isDigit c || (isHexDigit c && isLower c)) s

hex32 :: Word32 -> String
hex32 w = let digits = showHex w "" in "0x" ++ replicate (8 - length digits) '0' ++ digits

hexBytes :: B.ByteString -> String
hexBytes = concatMap (\b -> let d = showHex b "" in replicate (2 - length d) '0' ++ d) . B.unpack
