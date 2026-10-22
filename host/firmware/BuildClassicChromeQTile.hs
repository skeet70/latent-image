-- Build the Classic Chrome Q-tile fragment (reports/classic-chrome-icon.md
-- section 14) over the combined icon candidate: its steps carried, the Q
-- tile's words and blob appended to its font patch set as later writes, and an
-- image-2 patch set with the Q glyph 0x37c record pointer inserted before the
-- font step. `diff` refuses any byte change between the icon candidate and the
-- Q-tile candidate outside the recorded write set and the main checksum word.
module Main (main) where

import Control.Monad (forM_, when)
import Data.Aeson (Value)
import Data.Bits (complement)
import qualified Data.ByteString as B
import Data.Char (isDigit, isHexDigit, isLower)
import Data.List (isPrefixOf)
import Data.Word (Word32)
import qualified FujiAF.ClassicChromeIcon as I
import qualified FujiAF.ClassicChromeQTile as Q
import FujiAF.FdlFont (FontOffset (..))
import qualified FujiAF.FontWindow as FW
import qualified FujiAF.Host as H
import FujiAF.JSON (encodeCanonical)
import qualified FujiAF.UpdateContainer as U
import Numeric (readHex, showHex)
import System.Directory (canonicalizePath, createDirectory, doesPathExist, getCurrentDirectory)
import System.Environment (getArgs, lookupEnv)
import System.FilePath (addTrailingPathSeparator, isAbsolute, takeDirectory, takeFileName, (</>))

-- The combined icon candidate the fragment extends.
iconOutput :: String
iconOutput = "78f7526002608540a8a1c6957306f29760371cd5146d2455587a0900bacaa7ba"

image2Load :: Word32
image2Load = 0x00130000

usage :: String
usage =
  unlines
    [ "usage: build-classic-chrome-q-tile build [--output-sha256 HEX] ICON_MANIFEST ICON_DAT NEW_OUTPUT_DIRECTORY"
    , "       build-classic-chrome-q-tile diff ICON_DAT Q_TILE_DAT BUILD_DIRECTORY NEW_OUTPUT_DIRECTORY"
    ]

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    "build" : "--output-sha256" : digest : [manifest, dat, output] -> do
      H.ensure (isSha256 digest) "--output-sha256: lowercase 64-digit SHA-256 required"
      build digest manifest dat output
    ["build", manifest, dat, output] -> build (replicate 64 '0') manifest dat output
    ["diff", icon, qTile, directory, output] -> diff icon qTile directory output
    _ -> H.failH usage

build :: String -> FilePath -> FilePath -> FilePath -> IO ()
build outputSha manifestGiven datGiven outputGiven = do
  manifestPath <- resolve manifestGiven >>= canonicalizePath
  manifest <- H.readRegular (1024 * 1024) manifestPath >>= H.jsonRead
  H.ensure (H.str (H.get manifest "output_sha256") == iconOutput) "the icon manifest's output_sha256 is not the combined icon candidate 78f75260"
  let steps = H.list (H.get manifest "steps")
      kinds = map (\v -> H.str (H.get v "kind")) steps
  H.ensure (kinds == ["image-patch-set", "image-patch-set", "image2-record", "image2-record", "font-patch-set", "plain-patch-set"]) "the icon manifest is not image 4, image 5, two records, the font patch set and the plain patch set"
  let directory = takeDirectory manifestPath
      fontStep = steps !! 4
  fontSet <- pinnedPart directory fontStep >>= H.jsonRead
  datBytes <- resolve datGiven >>= canonicalizePath >>= H.readRegular (64 * 1024 * 1024)
  datSha <- H.sha256 datBytes
  H.ensure (datSha == iconOutput) "ICON_DAT is not the combined icon candidate 78f75260"
  parsed <- either (H.failH . ("ICON_DAT: " ++)) pure (U.parseContainer datBytes)
  let parts = U.parsedParts parsed

  -- Font: the icon's writes, then the Q label words over its Q labels blob
  -- and the tile in the tail, both checked against the icon DAT's window.
  window <- either (H.failH . ("icon font window: " ++)) (pure . U.mediaImageBytes) (FW.extractFont parts)
  writes <- either (H.failH . ("Q tile font writes: " ++)) pure (Q.qTileFontWrites window)
  let existingWords = H.list (H.get fontSet "words")
      existingBlobs = H.list (H.get fontSet "blobs")
      lastOrder = maximum (0 : [H.num (H.get w "order") | w <- existingWords] ++ [H.num (H.list (H.get b "order_range") !! 1) | b <- existingBlobs])
      wordWrites = [w | w <- writes, B.length (I.fwNew w) == 4]
      blobWrites = [w | w <- writes, B.length (I.fwNew w) /= 4]
      offsetOf w = let FontOffset at = I.fwOffset w in at
      newWords =
        [ wordJson (fromInteger lastOrder + k) (I.fontRamAddress (I.fwOffset w)) (offsetOf w) (U.word32At (I.fwOld w) 0) (U.word32At (I.fwNew w) 0) ("Q tile: " ++ I.fwRole w)
        | (k, w) <- zip [1 ..] wordWrites
        ]
      firstBlobOrder = fromInteger lastOrder + length wordWrites + 1
      newBlobs =
        [ H.obj
            [ ("id", H.s ("blob-" ++ show (length existingBlobs + k)))
            , ("role", H.s ("Q tile: " ++ I.fwRole w))
            , ("address", H.s (hex32 (I.fontRamAddress (I.fwOffset w))))
            , ("file_offset", H.s (hex32 (fromIntegral (offsetOf w))))
            , ("length", H.n (toInteger (B.length (I.fwNew w))))
            , ("order_range", H.arr [H.n (toInteger first), H.n (toInteger (first + B.length (I.fwNew w) `div` 4 - 1))])
            , ("current_hex", H.s (hexBytes (I.fwOld w)))
            , ("new_hex", H.s (hexBytes (I.fwNew w)))
            ]
        | (k, w, first) <- zip3 [1 :: Int ..] blobWrites (scanl (+) firstBlobOrder [B.length (I.fwNew w) `div` 4 | w <- blobWrites])
        ]
      combinedFont =
        H.merge
          fontSet
          [ ("id", H.s "CLASSIC-CHROME-Q-TILE-01-FONT")
          , ("words", H.arr (existingWords ++ newWords))
          , ("blobs", H.arr (existingBlobs ++ newBlobs))
          , ("order_rule", H.s (H.str (H.get fontSet "order_rule") ++ " Then the Q tile: the Q label words of rows 5, 6 and 7, each over the Q labels blob, then the GVFF tile blob."))
          , ("totals", H.obj [("icon_orders", H.n lastOrder), ("q_tile_word_writes", H.n (toInteger (length newWords))), ("q_tile_blob_words", H.n (toInteger (sum [B.length (I.fwNew w) `div` 4 | w <- blobWrites])))])
          , ("unchanged_surfaces", H.arr (map H.s ["every glyph's loader result except U+F072 and U+F073", "every per-mille value except U+F073", "Q rows 0-4 and the Q values array"]))
          ]

  -- Image 2: the Q glyph 0x37c record, size and type checked, pointer moved.
  let image2 = B.map complement (U.partImage2 parts)
      (_, _, load) = U.image2Extent parts
      wordAt a = U.word32At image2 (fromIntegral (a - image2Load))
      record = Q.qGlyphRecordAddress Q.ccQGlyph
  H.ensure (load == image2Load) "the image-2 extent does not load at 0x00130000"
  H.ensure (wordAt record == Q.qTileRecordSize && wordAt (record + 4) == 1) "the Q glyph 0x37c record is not an 80 x 48 type-1 tile"
  forM_ Q.qTileImage2Writes $ \w -> H.ensure (wordAt (Q.i2Address w) == Q.i2Old w) (hex32 (Q.i2Address w) ++ ": the image-2 word is " ++ hex32 (wordAt (Q.i2Address w)) ++ ", not " ++ hex32 (Q.i2Old w))
  image2Sha <- H.sha256 image2
  let image2Set =
        H.obj
          [ ("schema_version", H.n 1)
          , ("id", H.s "CLASSIC-CHROME-Q-TILE-01-IMAGE2")
          , ("tier", H.s "RC2")
          , ("recorded_at", H.s "2026-10-22")
          , ("target", H.get fontSet "target")
          , ( "image"
            , H.obj
                [ ("segment_id", H.n 2)
                , ("decode_base", H.s (hex32 image2Load))
                , ("decode_file", H.s "rebuild-update extract-image2 extent of the step's input DAT (media form), after the 0x83d and 0x83e records")
                , ("decode_sha256", H.s image2Sha)
                , ("file_offset_rule", H.s "file_offset = address - 0x00130000")
                , ("dump_correspondence", H.s "not established for this build")
                ]
            )
          , ("free_space", H.s "none: one record word rewritten in place")
          , ("array_source", H.s "FujiAF.ClassicChromeQTile")
          , ("order_rule", H.s "the Q glyph 0x37c record pointer")
          , ("blobs", H.arr [])
          , ("words", H.arr [wordJson k (Q.i2Address w) (fromIntegral (Q.i2Address w - image2Load)) (Q.i2Old w) (Q.i2New w) (Q.i2Role w) | (k, w) <- zip [1 ..] Q.qTileImage2Writes])
          , ("totals", H.obj [("word_writes", H.n (toInteger (length Q.qTileImage2Writes)))])
          , ("unchanged_surfaces", H.arr (map H.s ["every Q glyph record except 0x37c's pointer word", "the special label records"]))
          , ("container_effect", H.obj [("dat_extents", H.s "unchanged"), ("segment_words", H.s "unchanged")])
          , ("emulation_case_parameters", H.s "none")
          ]

  output <- newOutputDirectory outputGiven
  let write name value = do
        let bytes = encodeCanonical value
        B.writeFile (output </> name) bytes
        digest <- H.sha256 bytes
        putStrLn ("wrote " ++ (output </> name) ++ " sha256 " ++ digest)
        pure digest
      copyPart step = do
        bytes <- pinnedPart directory step
        B.writeFile (output </> H.str (H.get step "path")) bytes
  mapM_ copyPart [steps !! 0, steps !! 1, steps !! 5]
  digestFont <- write "cc-q-tile-font.json" combinedFont
  digestImage2 <- write "cc-q-tile-image2.json" image2Set
  let partStep kind path digest = H.obj [("kind", H.s kind), ("path", H.s path), ("sha256", H.s digest)]
      combinedManifest =
        H.obj
          [ ("schema_version", H.n 1)
          , ("id", H.s "CC-ICON-Q-TILE")
          , ("title", H.s "Classic Chrome on RC0 with its film-simulation icon and full-size Q tiles")
          , ("base", H.get manifest "base")
          , ("steps", H.arr (take 4 steps ++ [partStep "image2-patch-set" "cc-q-tile-image2.json" digestImage2, partStep "font-patch-set" "cc-q-tile-font.json" digestFont, steps !! 5]))
          , ("output_sha256", H.s outputSha)
          ]
  _ <- write "classic-chrome-q-tile.json" combinedManifest
  _ <-
    write
      "write-set.json"
      ( H.obj
          [ ("font", H.arr [expectedJson (I.fontRamAddress (I.fwOffset w)) (I.fwNew w) | w <- writes])
          , ("image2", H.arr [expectedJson (Q.i2Address w) (U.putWord32 (Q.i2New w)) | w <- Q.qTileImage2Writes])
          ]
      )
  putStrLn ("Q tile: " ++ show (B.length Q.qTileGvff) ++ " GVFF bytes at " ++ hex32 Q.qTileAddress ++ "; " ++ show (length newWords) ++ " Q label words; " ++ show (length Q.qTileImage2Writes) ++ " image-2 word")

-- ---------------------------------------------------------------------------
-- diff: every changed byte between the icon candidate and the Q-tile
-- candidate lies in the recorded write set or the main checksum word.
-- ---------------------------------------------------------------------------

diff :: FilePath -> FilePath -> FilePath -> FilePath -> IO ()
diff iconGiven qGiven directoryGiven outputGiven = do
  iconBytes <- resolve iconGiven >>= canonicalizePath >>= H.readRegular (64 * 1024 * 1024)
  qBytes <- resolve qGiven >>= canonicalizePath >>= H.readRegular (64 * 1024 * 1024)
  iconSha <- H.sha256 iconBytes
  qSha <- H.sha256 qBytes
  H.ensure (iconSha == iconOutput) "ICON_DAT is not the combined icon candidate 78f75260"
  directory <- resolve directoryGiven >>= canonicalizePath
  writeSet <- H.readRegular (1024 * 1024) (directory </> "write-set.json") >>= H.jsonRead
  let group name = mapM parseExpected (H.list (H.get writeSet name))
  fontSet <- group "font"
  image2Set <- group "image2"
  icon <- either (H.failH . ("ICON_DAT: " ++)) pure (U.parseContainer iconBytes)
  qTile <- either (H.failH . ("Q_TILE_DAT: " ++)) pure (U.parseContainer qBytes)
  H.ensure (B.length iconBytes == B.length qBytes) "the containers differ in length"
  output <- newOutputDirectory outputGiven
  let partDiffs = [d | d <- U.diffParts icon qTile, U.diffChangedBytes d /= 0]
  forM_ partDiffs $ \d -> H.ensure (U.diffPart d `elem` ["image 2 extent", "main post tail"]) ("container part " ++ U.diffPart d ++ " changed")
  let mainStart = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader icon))
      outsideMain = [i | i <- [0 .. mainStart - 1], B.index iconBytes i /= B.index qBytes i]
  H.ensure (all (\i -> i >= U.mainChecksumOffset && i < U.mainChecksumOffset + 4) outsideMain) "a header byte other than the main checksum word changed"
  let media parsed = B.map complement (U.partImage2 (U.parsedParts parsed))
  image2Changed <- changedWithin "image 2" image2Load (media icon) (media qTile) image2Set
  iconFont <- either H.failH (pure . U.mediaImageBytes) (FW.extractFont (U.parsedParts icon))
  qFont <- either H.failH (pure . U.mediaImageBytes) (FW.extractFont (U.parsedParts qTile))
  fontChanged <- changedWithin "font window" I.fontLoadAddress iconFont qFont fontSet
  let postTailChanged = sum [U.diffChangedBytes d | d <- partDiffs, U.diffPart d == "main post tail"]
  H.ensure (postTailChanged == fontChanged) "the post-tail span changed outside the font window"
  let summary =
        H.obj
          [ ("icon_dat_sha256", H.s iconSha)
          , ("q_tile_dat_sha256", H.s qSha)
          , ("header_bytes_changed", H.arr [H.s (hex32 (fromIntegral i)) | i <- outsideMain])
          , ("container_parts_changed", H.obj [(U.diffPart d, H.n (toInteger (U.diffChangedBytes d))) | d <- partDiffs])
          , ("write_set_groups", H.obj [("font_window_bytes", H.n (toInteger fontChanged)), ("image2_bytes", H.n (toInteger image2Changed))])
          , ("verdict", H.s "every changed byte lies in the write set or the main checksum word; images 4, 5 and plain are unchanged")
          ]
      bytes = encodeCanonical summary
  B.writeFile (output </> "changed-bytes.json") bytes
  B.putStr bytes
  putStrLn ""

data Expected = Expected Word32 B.ByteString

expectedJson :: Word32 -> B.ByteString -> Value
expectedJson address bytes = H.obj [("address", H.s (hex32 address)), ("new_hex", H.s (hexBytes bytes))]

parseExpected :: Value -> IO Expected
parseExpected v = Expected <$> hexField (H.get v "address") <*> unhex (H.str (H.get v "new_hex"))

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

pinnedPart :: FilePath -> Value -> IO B.ByteString
pinnedPart directory step = do
  let relative = H.str (H.get step "path")
  H.ensure (not (null relative) && not (isAbsolute relative)) "part path must be relative"
  bytes <- canonicalizePath (directory </> relative) >>= H.readRegular (16 * 1024 * 1024)
  digest <- H.sha256 bytes
  H.ensure (digest == H.str (H.get step "sha256")) (relative ++ " differs from its pinned SHA-256")
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
    go (a : b : rest) | isHexDigit a && isHexDigit b = (fromIntegral (hexDigit a * 16 + hexDigit b) :) <$> go rest
    go [] = pure []
    go _ = H.failH "malformed hex bytes"

hexDigit :: Char -> Int
hexDigit c
  | isDigit c = fromEnum c - fromEnum '0'
  | isLower c = fromEnum c - fromEnum 'a' + 10
  | otherwise = fromEnum c - fromEnum 'A' + 10

isSha256 :: String -> Bool
isSha256 s = length s == 64 && all (\c -> isDigit c || (isHexDigit c && isLower c)) s

hex32 :: Word32 -> String
hex32 w = let digits = showHex w "" in "0x" ++ replicate (8 - length digits) '0' ++ digits

hexBytes :: B.ByteString -> String
hexBytes = concatMap (\b -> let d = showHex b "" in replicate (2 - length d) '0' ++ d) . B.unpack
