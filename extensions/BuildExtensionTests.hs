{-# LANGUAGE OverloadedStrings #-}

-- Behavioural tests for the extension manifest parser: the steps an accepted
-- manifest yields and the refusals for the image2-record, font-patch-set and
-- plain-patch-set step kinds and their container order. Manifests and parts are synthetic and
-- written to a temporary directory; the base digest is a placeholder. Each
-- refusal is one change to a manifest the same run accepts.
module Main where

import Control.Exception (IOException, SomeException, displayException, try)
import Data.Aeson (Value, encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import ExtensionManifest
import qualified FujiAF.Host as H
import qualified FujiAF.LabelRecord as L
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.IO (hPutStrLn, stderr)

baseDigest :: String
baseDigest = replicate 64 'a'

windowDigest :: String
windowDigest = replicate 64 'b'

zeroHex :: String
zeroHex = replicate 132 '0'

data Fixture = Fixture
  { fixtureRoot :: FilePath
  , fixtureCounter :: IORef Int
  , fixtureChecked :: IORef Int
  }

-- A patch set as the parser reads it: the target and the image block.
patchSet :: String -> [(String, Value)] -> Value
patchSet target image =
  H.obj
    [ ("schema_version", H.n 3)
    , ("target", H.obj [("model", H.s "X-M1"), ("firmware_version", H.s "1.50"), ("firmware_sha256", H.s target)])
    , ("image", H.obj image)
    , ("blobs", H.arr [])
    , ("words", H.arr [])
    ]

plainImage :: [(String, Value)]
plainImage = [("segment_id", H.n 6), ("decode_base", H.s "0x01100000"), ("decode_sha256", H.s windowDigest)]

-- Writes a part next to the manifests and returns the step naming it.
part :: Fixture -> String -> FilePath -> B.ByteString -> IO Value
part fixture kind name bytes = do
  B.writeFile (fixtureRoot fixture </> name) bytes
  digest <- H.sha256 bytes
  pure (H.obj [("kind", H.s kind), ("path", H.s name), ("sha256", H.s digest)])

jsonPart :: Fixture -> String -> FilePath -> Value -> IO Value
jsonPart fixture kind name value = part fixture kind name (BL.toStrict (encode value))

record :: String -> String -> [Integer] -> String -> Value
record ident text layout current =
  H.obj
    [ ("kind", H.s "image2-record")
    , ("id", H.s ident)
    , ("text", H.s text)
    , ("layout", H.arr (map H.n layout))
    , ("current_hex", H.s current)
    ]

ccRecord :: Value
ccRecord = record "0x83e" "CLASSIC CHROME" [40, 0, 30, 36, 48] zeroHex

label :: Value
label = H.obj [("kind", H.s "image2-label"), ("text", H.s "RECIPE 01")]

manifest :: [Value] -> Value
manifest steps =
  H.obj
    [ ("schema_version", H.n 1)
    , ("id", H.s "TEST")
    , ("title", H.s "synthetic")
    , ("base", H.obj [("model", H.s "X-M1"), ("firmware_version", H.s "1.50"), ("sha256", H.s baseDigest)])
    , ("steps", H.arr steps)
    , ("output_sha256", H.s (replicate 64 'c'))
    ]

parseSteps :: Fixture -> [Value] -> IO (Either IOException Extension)
parseSteps fixture steps = do
  modifyIORef' (fixtureCounter fixture) (+ 1)
  index <- readIORef (fixtureCounter fixture)
  let path = fixtureRoot fixture </> ("manifest-" ++ show index ++ ".json")
  BL.writeFile path (encode (manifest steps))
  try (parseExtension path)

accepts :: Fixture -> String -> [Value] -> IO [Step]
accepts fixture name steps =
  parseSteps fixture steps >>= \outcome -> case outcome of
    Left err -> H.failH (name ++ ": refused: " ++ displayException err)
    Right extension -> do
      modifyIORef' (fixtureChecked fixture) (+ 1)
      pure (extensionSteps extension)

refuses :: Fixture -> String -> [Value] -> IO ()
refuses fixture name steps =
  parseSteps fixture steps >>= \outcome -> case outcome of
    Left _ -> modifyIORef' (fixtureChecked fixture) (+ 1)
    Right _ -> H.failH (name ++ ": accepted")

expectSteps :: Fixture -> String -> [Step] -> [Step] -> IO ()
expectSteps fixture name expected actual = do
  H.ensure (expected == actual) (name ++ ": " ++ show actual ++ " against " ++ show expected)
  modifyIORef' (fixtureChecked fixture) (+ 1)

orFail :: String -> Either String a -> IO a
orFail name = either (\message -> H.failH (name ++ ": " ++ message)) pure

-- Replaces one field of a step object.
with :: String -> Value -> Value -> Value
with = H.set

without :: String -> Value -> Value
without key value = H.obj [(k, H.get value k) | k <- H.keys value, k /= key]

main :: IO ()
main = do
  outcome <- try run
  case outcome :: Either SomeException () of
    Left err -> hPutStrLn stderr (displayException err) >> exitFailure
    Right () -> pure ()

run :: IO ()
run = H.withTemporary "build-extension-tests" $ \root -> do
  fixture <- Fixture root <$> newIORef 0 <*> newIORef 0
  image4 <- jsonPart fixture "image-patch-set" "image4.json" (patchSet baseDigest [("segment_id", H.n 4), ("decode_base", H.s "0x0061c400"), ("output_base", H.s "0x0061c200")])
  image5 <- jsonPart fixture "image-patch-set" "image5.json" (patchSet baseDigest [("segment_id", H.n 5), ("decode_base", H.s "0x00bf1a00")])
  plain <- jsonPart fixture "plain-patch-set" "plain.json" (patchSet baseDigest plainImage)
  spanA <- part fixture "plain-span" "span-a.bin" (B.replicate 8 1)
  spanB <- part fixture "plain-span" "span-b.bin" (B.replicate 8 2)
  let spanAt offset step = with "flash_offset" (H.s offset) step
      plainVariant name image = jsonPart fixture "plain-patch-set" name (patchSet baseDigest image)

  -- The full order: image 4, image 5, the label, records by increasing id,
  -- one plain patch set.
  ident83c <- orFail "0x83c" (L.specialLabelId 0x83c)
  ident83e <- orFail "0x83e" (L.specialLabelId 0x83e)
  ccName <- orFail "name" (L.labelName "CLASSIC CHROME")
  blankName <- orFail "name" (L.labelName "BLANK")
  ccLayout <- orFail "layout" (L.labelLayout [40, 0, 30, 36, 48])
  blankLayout <- orFail "layout" (L.labelLayout [0, 0, 0, 0, 65535])
  zeros <- orFail "zeros" (L.specialRecordBytes (B.replicate 66 0))
  patterned <- orFail "pattern" (L.specialRecordBytes (B.pack [0 .. 65]))
  let patternHex = L.specialRecordHex patterned
      blankRecord = record "0x83c" "BLANK" [0, 0, 0, 0, 65535] patternHex
  full <- accepts fixture "full order" [image4, image5, label, blankRecord, ccRecord, plain]
  expectSteps
    fixture
    "full order steps"
    [ PatchImage Image4 (Just (LoadAddress 0x0061c200)) (root </> "image4.json")
    , PatchImage Image5 Nothing (root </> "image5.json")
    , WriteLabel "RECIPE 01"
    , WriteRecord ident83c blankName blankLayout patterned
    , WriteRecord ident83e ccName ccLayout zeros
    , PatchPlain (Sha256 windowDigest) (root </> "plain.json")
    ]
    full
  _ <- accepts fixture "record alone" [ccRecord]
  _ <- accepts fixture "label then 0x83e" [label, ccRecord]
  _ <- accepts fixture "record then plain spans" [ccRecord, spanAt "0x10" spanA, spanAt "0x18" spanB]
  _ <- accepts fixture "plain patch set alone" [plain]
  _ <- accepts fixture "27-character name" [record "0x83e" (replicate 27 '~') [0, 0, 0, 0, 0] zeroHex]

  -- Order and multiplicity.
  refuses fixture "duplicate record id" [ccRecord, ccRecord]
  refuses fixture "records by decreasing id" [ccRecord, blankRecord]
  refuses fixture "record before the label" [ccRecord, label]
  refuses fixture "label with record 0x83d" [label, record "0x83d" "X" [0, 0, 0, 0, 0] zeroHex]
  refuses fixture "record before image 5" [ccRecord, image5]
  refuses fixture "record after the plain patch set" [plain, ccRecord]
  refuses fixture "two plain patch sets" [plain, plain]
  refuses fixture "plain patch set then plain span" [plain, spanAt "0x10" spanA]
  refuses fixture "plain span then plain patch set" [spanAt "0x10" spanA, plain]
  refuses fixture "plain patch set before image 4" [plain, image4]

  -- image2-record fields.
  refuses fixture "id 0x83b" [record "0x83b" "X" [0, 0, 0, 0, 0] zeroHex]
  refuses fixture "id 0x83f" [record "0x83f" "X" [0, 0, 0, 0, 0] zeroHex]
  refuses fixture "decimal id" [record "2110" "X" [0, 0, 0, 0, 0] zeroHex]
  refuses fixture "four halfwords" [record "0x83e" "X" [0, 0, 0, 0] zeroHex]
  refuses fixture "six halfwords" [record "0x83e" "X" [0, 0, 0, 0, 0, 0] zeroHex]
  refuses fixture "halfword 65536" [record "0x83e" "X" [0, 0, 0, 0, 65536] zeroHex]
  refuses fixture "negative halfword" [record "0x83e" "X" [0, 0, -1, 0, 0] zeroHex]
  refuses fixture "string halfword" [with "layout" (H.arr [H.n 0, H.n 0, H.n 0, H.n 0, H.s "1"]) ccRecord]
  refuses fixture "130 hex digits" [record "0x83e" "X" [0, 0, 0, 0, 0] (replicate 130 '0')]
  refuses fixture "uppercase hex" [record "0x83e" "X" [0, 0, 0, 0, 0] (replicate 131 '0' ++ "A")]
  refuses fixture "empty name" [record "0x83e" "" [0, 0, 0, 0, 0] zeroHex]
  refuses fixture "28-character name" [record "0x83e" (replicate 28 'A') [0, 0, 0, 0, 0] zeroHex]
  refuses fixture "non-ascii name" [record "0x83e" "CAF\xc9" [0, 0, 0, 0, 0] zeroHex]
  refuses fixture "record without current_hex" [without "current_hex" ccRecord]
  refuses fixture "record with a path" [with "path" (H.s "plain.json") ccRecord]

  -- plain-patch-set fields and the patch set's image block.
  refuses fixture "plain step with a flash_offset" [with "flash_offset" (H.s "0x0") plain]
  refuses fixture "plain part digest" [with "sha256" (H.s (replicate 64 'd')) plain]
  segment5 <- plainVariant "plain-segment5.json" [("segment_id", H.n 5), ("decode_base", H.s "0x01100000"), ("decode_sha256", H.s windowDigest)]
  refuses fixture "plain segment 5" [segment5]
  shifted <- plainVariant "plain-shifted.json" [("segment_id", H.n 6), ("decode_base", H.s "0x011c0000"), ("decode_sha256", H.s windowDigest)]
  refuses fixture "plain decode_base 0x011c0000" [shifted]
  moved <- plainVariant "plain-moved.json" (plainImage ++ [("output_base", H.s "0x010fff00")])
  refuses fixture "plain output_base" [moved]
  same <- plainVariant "plain-same.json" (plainImage ++ [("output_base", H.s "0x01100000")])
  _ <- accepts fixture "plain output_base equal to decode_base" [same]
  shortDigest <- plainVariant "plain-short.json" [("segment_id", H.n 6), ("decode_base", H.s "0x01100000"), ("decode_sha256", H.s (replicate 63 'b'))]
  refuses fixture "plain decode_sha256 63 digits" [shortDigest]
  otherBase <- jsonPart fixture "plain-patch-set" "plain-other.json" (patchSet (replicate 64 'e') plainImage)
  refuses fixture "plain patch set for another firmware" [otherBase]
  plainAsImage <- jsonPart fixture "image-patch-set" "plain-as-image.json" (patchSet baseDigest plainImage)
  refuses fixture "segment 6 as an image-patch-set" [plainAsImage]

  -- font-patch-set: after the records, before the plain patch set, at most
  -- once; segment 3 at decode_base 0x003bc000.
  let fontImage = [("segment_id", H.n 3), ("decode_base", H.s "0x003bc000"), ("decode_sha256", H.s windowDigest)]
      fontVariant name image = jsonPart fixture "font-patch-set" name (patchSet baseDigest image)
      iconRecord = without "text" (with "text_utf16" (H.s "f073") (record "0x83d" "" [540, 24, 30, 36, 48] zeroHex))
  font <- fontVariant "font.json" fontImage
  ident83d <- orFail "0x83d" (L.specialLabelId 0x83d)
  iconName <- orFail "icon name" (L.labelNameUnits [0xf073])
  iconLayout <- orFail "icon layout" (L.labelLayout [540, 24, 30, 36, 48])
  withFont <- accepts fixture "records, font, plain" [image4, image5, iconRecord, ccRecord, font, plain]
  expectSteps
    fixture
    "records, font, plain steps"
    [ PatchImage Image4 (Just (LoadAddress 0x0061c200)) (root </> "image4.json")
    , PatchImage Image5 Nothing (root </> "image5.json")
    , WriteRecord ident83d iconName iconLayout zeros
    , WriteRecord ident83e ccName ccLayout zeros
    , PatchFont (Sha256 windowDigest) (root </> "font.json")
    , PatchPlain (Sha256 windowDigest) (root </> "plain.json")
    ]
    withFont
  refuses fixture "two font patch sets" [font, font]
  refuses fixture "font before a record" [font, ccRecord]
  refuses fixture "font after the plain patch set" [plain, font]
  fontSegment6 <- fontVariant "font-segment6.json" [("segment_id", H.n 6), ("decode_base", H.s "0x003bc000"), ("decode_sha256", H.s windowDigest)]
  refuses fixture "font segment 6" [fontSegment6]
  fontShifted <- fontVariant "font-shifted.json" [("segment_id", H.n 3), ("decode_base", H.s "0x003bd000"), ("decode_sha256", H.s windowDigest)]
  refuses fixture "font decode_base 0x003bd000" [fontShifted]
  fontMoved <- fontVariant "font-moved.json" (fontImage ++ [("output_base", H.s "0x003bbf00")])
  refuses fixture "font output_base" [fontMoved]
  refuses fixture "record with text and text_utf16" [with "text" (H.s "X") iconRecord]
  refuses fixture "uppercase unit hex" [with "text_utf16" (H.s "F073") iconRecord]
  refuses fixture "three hex digits" [with "text_utf16" (H.s "f07") iconRecord]
  refuses fixture "zero unit" [with "text_utf16" (H.s "0000") iconRecord]

  total <- readIORef (fixtureChecked fixture)
  putStrLn (show total ++ " build-extension manifest checks passed")
