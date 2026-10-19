{-# LANGUAGE LambdaCase, OverloadedStrings #-}
-- Classic Chrome patch for X-M1 1.50 (reports/classic-chrome-patch.md): the
-- private parameter file, the 100-byte data block, the image-4 prefix layout,
-- A32 BL encoding and the fixed in-place write lists. No I/O.
module FujiAF.ClassicChrome
  ( Address (..)
  , Group (..)
  , Image (..)
  , WordWrite (..)
  , Parameters (..)
  , Chroma (..)
  , Region (..)
  , RegionContent (..)
  , HookSite (..)
  , stockFirmwareSha256
  , parametersSchema
  , parseParameters
  , dataBlockAddress
  , dataBlockSize
  , encodeDataBlock
  , prefixBase
  , prefixEnd
  , image4DecodeBase
  , image5DecodeBase
  , plainDecodeBase
  , prefixLayout
  , regionAddress
  , regionSize
  , encodeBl
  , blTarget
  , parseSymbols
  , hookSites
  , rc0MatrixCells
  , rc0ValuePointers
  , rc0LabelPointers
  , rc0LabelsDestination
  , rc0LabelsDestinationStock
  , rc0ValuesStock
  , rawNodeLabelsWord
  , rawNodeValuesWord
  , rawValuesStock
  , rawLabelsStock
  , conversionRowWords
  , availabilityRowWords
  , image5FixedWords
  , plainGateWords
  , plainRawWords
  , captureConverter
  , settingsConverter
  , image2RecordAddress
  , image2RecordText
  , image2RecordLayout
  , image2Record
  , labelQuadruple
  , valuePair
  , listTerminatorLabels
  , valuesTerminator
  , word32LE
  ) where

import Control.Monad (unless, when)
import Data.Aeson (Value (..))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftL, shiftR, testBit, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Char (isHexDigit, isLower, isDigit, ord)
import Data.Int (Int64)
import Data.List (sort)
import Data.Scientific (floatingOrInteger)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word16, Word32, Word8)
import FujiAF.JSON (decodeStrictIntegers)
import Numeric (readHex)

newtype Address = Address Word32 deriving (Eq, Ord, Show)

data Group = GroupRC0 | GroupCC deriving (Eq, Ord, Show)

data Image = Image4 | Image5 | ImagePlain | Image2 deriving (Eq, Ord, Show)

data WordWrite = WordWrite
  { writeAddress :: Address
  , writeCurrent :: Word32
  , writeNew :: Word32
  , writeGroup :: Group
  , writeRole :: String
  }
  deriving (Eq, Show)

-- The four raw fields of the 0xb077 record, named by storage offset.
data Chroma = Chroma
  { chromaAt0 :: Word16
  , chromaAt2 :: Word16
  , chromaAt4 :: Word16
  , chromaAt6 :: Word16
  }
  deriving (Eq, Show)

-- Colour matrix row-major.
data Parameters = Parameters
  { paramColourMatrix :: [Word16]
  , paramChroma :: Chroma
  , paramToneHead :: [Word8]
  , paramToneCurve :: [Word16]
  }
  deriving (Eq, Show)

stockFirmwareSha256 :: String
stockFirmwareSha256 = "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"

parametersSchema :: String
parametersSchema = "latent-image.classic-chrome-parameters.v1"

type Parse = Either String

parseParameters :: B.ByteString -> Parse Parameters
parseParameters bytes = do
  value <- either (Left . ("parameters: " ++)) Right (decodeStrictIntegers bytes)
  top <- exactObject "parameters" ["schema", "target", "approximation_of", "provenance", "colour_matrix", "chroma", "tone_record", "reuse"] value
  schema <- text "schema" (top "schema")
  unless (schema == parametersSchema) (Left ("schema: expected " ++ parametersSchema))
  target <- exactObject "target" ["model", "firmware_version", "firmware_sha256"] (top "target")
  model <- text "target.model" (target "model")
  version <- text "target.firmware_version" (target "firmware_version")
  digest <- text "target.firmware_sha256" (target "firmware_sha256")
  unless (model == "X-M1" && version == "1.50" && digest == stockFirmwareSha256) (Left "target: must be X-M1 firmware 1.50 with the pinned DAT SHA-256")
  approximation <- text "approximation_of" (top "approximation_of")
  when (null approximation) (Left "approximation_of: nonempty string required")
  provenance <- exactObject "provenance" ["method", "inputs", "e_method", "e_model"] (top "provenance")
  method <- text "provenance.method" (provenance "method")
  when (null method) (Left "provenance.method: nonempty string required")
  inputs <- array "provenance.inputs" (provenance "inputs")
  mapM_ provenanceInput inputs
  _ <- integer "provenance.e_method" 0 (2 ^ (31 :: Int)) (provenance "e_method")
  case provenance "e_model" of
    Null -> pure ()
    other -> () <$ integer "provenance.e_model" 0 (2 ^ (31 :: Int)) other
  rows <- array "colour_matrix" (top "colour_matrix")
  unless (length rows == 3) (Left "colour_matrix: three rows required")
  matrix <- concat <$> mapM (fieldRow "colour_matrix row" 3) rows
  chroma <-
    fieldRow "chroma" 4 (top "chroma") >>= \case
      [a, b, c, d] -> pure (Chroma a b c d)
      _ -> Left "chroma: four integers required"
  tone <- exactObject "tone_record" ["head", "curve"] (top "tone_record")
  headValues <- array "tone_record.head" (tone "head")
  unless (length headValues == 9) (Left "tone_record.head: nine integers required")
  toneHead <- mapM (fmap fromInteger . integer "tone_record.head" 0 255) headValues
  curveValues <- array "tone_record.curve" (tone "curve")
  unless (length curveValues == 28) (Left "tone_record.curve: 28 integers required")
  curve <- mapM (integer "tone_record.curve" 0 0x3fff) curveValues
  unless (and (zipWith (<=) curve (drop 1 curve))) (Left "tone_record.curve: must be non-decreasing")
  unless (last curve == 0x3fff) (Left "tone_record.curve: must end at 0x3fff")
  reuse <- exactObject "reuse" ["luma_band", "chroma_gain", "hs_tone", "tone_scalar"] (top "reuse")
  mapM_ (\k -> text ("reuse." ++ k) (reuse k) >>= \v -> unless (v == "PROVIA") (Left ("reuse." ++ k ++ ": must be PROVIA in format version 1"))) ["luma_band", "chroma_gain", "hs_tone", "tone_scalar"]
  pure Parameters {paramColourMatrix = matrix, paramChroma = chroma, paramToneHead = toneHead, paramToneCurve = map fromInteger curve}
  where
    provenanceInput v = do
      input <- exactObject "provenance.inputs[]" ["path", "sha256"] v
      path <- text "provenance.inputs[].path" (input "path")
      when (null path) (Left "provenance.inputs[].path: nonempty string required")
      sha <- text "provenance.inputs[].sha256" (input "sha256")
      unless (isSha256 sha) (Left "provenance.inputs[].sha256: lowercase 64-digit SHA-256 required")
    -- 13-bit two's complement fields: bits 13-15 clear.
    fieldRow label count v = do
      values <- array label v
      unless (length values == count) (Left (label ++ ": " ++ show count ++ " integers required"))
      mapM (fmap fromInteger . integer label 0 0x1fff) values

exactObject :: String -> [String] -> Value -> Parse (String -> Value)
exactObject label names (Object o) = do
  let present = map K.toString (KM.keys o)
  case [k | k <- present, k `notElem` names] of
    (k : _) -> Left (label ++ ": unknown field " ++ show k)
    [] -> pure ()
  case [k | k <- names, k `notElem` present] of
    (k : _) -> Left (label ++ ": missing field " ++ show k)
    [] -> pure ()
  pure (\k -> maybe Null id (KM.lookup (K.fromString k) o))
exactObject label _ _ = Left (label ++ ": object required")

text :: String -> Value -> Parse String
text _ (String t) = pure (T.unpack t)
text label _ = Left (label ++ ": string required")

array :: String -> Value -> Parse [Value]
array _ (Array xs) = pure (V.toList xs)
array label _ = Left (label ++ ": array required")

integer :: String -> Integer -> Integer -> Value -> Parse Integer
integer label low high (Number x) = case floatingOrInteger x :: Either Double Integer of
  Right v | low <= v && v <= high -> pure v
  Right _ -> Left (label ++ ": integer out of range " ++ show low ++ ".." ++ show high)
  Left _ -> Left (label ++ ": integer required")
integer label _ _ _ = Left (label ++ ": integer required")

isSha256 :: String -> Bool
isSha256 s = length s == 64 && all (\c -> isDigit c || (isHexDigit c && isLower c)) s

dataBlockAddress :: Address
dataBlockAddress = Address 0x0061c200

dataBlockSize :: Int
dataBlockSize = 100

encodeDataBlock :: Parameters -> B.ByteString
encodeDataBlock p =
  B.concat
    [ "CCD1"
    , halves (paramColourMatrix p)
    , halves [chromaAt0 c, chromaAt4 c, chromaAt6 c, chromaAt2 c]
    , B.replicate 2 0
    , B.pack (paramToneHead p)
    , B.singleton 0
    , halves (paramToneCurve p)
    , B.replicate 2 0
    ]
  where
    c = paramChroma p
    halves = B.concat . map (\h -> B.pack [fromIntegral h, fromIntegral (h `shiftR` 8)])

prefixBase :: Address
prefixBase = Address 0x0061c200

prefixEnd :: Address
prefixEnd = Address 0x0061c400

image4DecodeBase :: Address
image4DecodeBase = Address 0x0061c400

image5DecodeBase :: Address
image5DecodeBase = Address 0x00bf1a00

plainDecodeBase :: Address
plainDecodeBase = Address 0x01100000

data RegionContent = DataBlock | RawValues | ShootingLabels | ShootingValues | RawLabels
  deriving (Eq, Show)

data Region = Region RegionContent Address Int | ZeroFill Address Int deriving (Eq, Show)

regionAddress :: RegionContent -> Address
regionAddress content = fst (placement content)

regionSize :: RegionContent -> Int
regionSize content = snd (placement content)

placement :: RegionContent -> (Address, Int)
placement = \case
  DataBlock -> (Address 0x0061c200, 100)
  RawValues -> (Address 0x0061c264, 56)
  ShootingLabels -> (Address 0x0061c2b0, 144)
  ShootingValues -> (Address 0x0061c340, 72)
  RawLabels -> (Address 0x0061c388, 112)

-- Section 6.1 of the design, in address order.
prefixLayout :: [Region]
prefixLayout =
  [ region DataBlock
  , region RawValues
  , ZeroFill (Address 0x0061c29c) 20
  , region ShootingLabels
  , region ShootingValues
  , region RawLabels
  , ZeroFill (Address 0x0061c3f8) 8
  ]
  where
    region content = let (a, n) = placement content in Region content a n

-- A32 BL, condition AL.
encodeBl :: Address -> Address -> Either String Word32
encodeBl (Address site) (Address target)
  | site .&. 3 /= 0 || target .&. 3 /= 0 = Left "BL site and target must be word-aligned"
  | delta < -0x2000000 || delta > 0x1fffffc = Left "BL target out of range"
  | otherwise = Right (0xeb000000 .|. (fromIntegral (delta `shiftR` 2) .&. 0x00ffffff))
  where
    delta = toInteger target - (toInteger site + 8) :: Integer

-- Target of an A32 BL with condition AL, decoded by sign-extending imm24.
blTarget :: Address -> Word32 -> Maybe Address
blTarget (Address site) word
  | word .&. 0xff000000 /= 0xeb000000 = Nothing
  | otherwise =
      let imm = fromIntegral (word .&. 0x00ffffff) :: Int64
          signed = if testBit imm 23 then imm - (1 `shiftL` 24) else imm
       in Just (Address (fromIntegral (fromIntegral site + 8 + 4 * signed :: Int64)))

-- llvm-nm lines `ADDR TYPE NAME`.
parseSymbols :: String -> Either String [(String, Address)]
parseSymbols = mapM symbol . filter (not . null . words) . lines
  where
    symbol line = case words line of
      [addr, [_], name] | length addr == 8, [(v, "")] <- readHex addr -> Right (name, Address v)
      _ -> Left ("symbol table: malformed line " ++ show line)

data HookSite = HookSite
  { hookSite :: Address
  , hookCurrent :: Word32
  , hookStub :: String
  , hookStock :: String
  , hookRole :: String
  }

hookSites :: [HookSite]
hookSites =
  [ HookSite (Address 0x00d67284) 0xebffff85 "cc_colour" "stock_colour_read" "colour matrix data read in FUN_00d671b4"
  , HookSite (Address 0x00d60f60) 0xebffff57 "cc_chroma" "stock_chroma_read" "chroma data read in FUN_00d60ea8"
  , HookSite (Address 0x00d64ef4) 0xebffef72 "cc_chroma" "stock_chroma_read" "chroma data read in FUN_00d64e2c"
  , HookSite (Address 0x00d677ac) 0xebe4a45c "cc_tone" "stock_tone_read" "tone record read in FUN_00d675d8"
  , HookSite (Address 0x00d677bc) 0xebe4a458 "cc_tone" "stock_tone_read" "tone record read in FUN_00d675d8"
  ]

rc0MatrixCells :: [WordWrite]
rc0MatrixCells =
  [ WordWrite (Address a) 0 1 GroupRC0 ("matrix key " ++ show key ++ ", column " ++ show col)
  | (key, row) <- [(11 :: Int, 0x00795db0), (12, 0x00795e0c)]
  , col <- [1, 2, 4, 5, 6, 7, 21 :: Int]
  , let a = row + 4 * fromIntegral col
  ]

rc0LabelsDestinationStock :: Word32
rc0LabelsDestinationStock = 0x0061c380

rc0LabelsDestination :: Word32
rc0LabelsDestination = let Address a = regionAddress ShootingLabels in a

-- Shooting, Fn and type-9 nodes.
rc0ValuePointers :: [WordWrite]
rc0ValuePointers =
  [ WordWrite (Address a) 0x0078319c values GroupRC0 "values-array pointer"
  | a <- [0x007823c4, 0x00785d94, 0x0077d1a4]
  ]
  where
    Address values = regionAddress ShootingValues

rc0LabelPointers :: [WordWrite]
rc0LabelPointers =
  [ WordWrite (Address a) 0x0078d4f8 rc0LabelsDestination GroupRC0 "labels-array pointer (destination 0x0061c2b0)"
  | a <- [0x007823c8, 0x00785d98, 0x0077d1a8]
  ]

-- RC0's installed shooting values {0,3,1,7,0xc,5,6} and terminator.
rc0ValuesStock :: B.ByteString
rc0ValuesStock = B.concat (map valuePair [0, 3, 1, 7, 0xc, 5, 6] ++ [valuesTerminator])

rawNodeLabelsWord :: WordWrite
rawNodeLabelsWord = WordWrite (Address 0x0078bd80) 0x0078427c a GroupCC "RAW conversion labels pointer"
  where
    Address a = regionAddress RawLabels

rawNodeValuesWord :: WordWrite
rawNodeValuesWord = WordWrite (Address 0x0078bd7c) 0x0078ed30 a GroupCC "RAW conversion values pointer"
  where
    Address a = regionAddress RawValues

-- Stock RAW values at 0x0078ed30, 48 bytes.
rawValuesStock :: (Address, B.ByteString)
rawValuesStock = (Address 0x0078ed30, B.concat (map valuePair [0, 1, 2, 5, 9] ++ [valuesTerminator]))

-- Stock RAW labels at 0x0078427c: five quadruples (read from the image) and the terminator.
rawLabelsStock :: (Address, Int)
rawLabelsStock = (Address 0x0078427c, 80)

conversionRowWords :: [WordWrite]
conversionRowWords =
  [ WordWrite (Address 0x0077ac48) 2 0xd GroupCC "conversion row 2 col0"
  , WordWrite (Address 0x0077ac50) 2 0xd GroupCC "conversion row 2 col2"
  ]

availabilityRowWords :: [WordWrite]
availabilityRowWords =
  WordWrite (Address 0x0079608c) 2 0xd GroupCC "availability key-2 row becomes key 0xd"
    : [ WordWrite (Address (0x0079608c + 4 + 4 * fromIntegral col)) 0 1 GroupCC ("availability key 0xd, column " ++ show col)
      | col <- [1, 2, 4, 5, 6, 7, 21 :: Int]
      ]

-- Image-5 words other than the five hook branches.
image5FixedWords :: [WordWrite]
image5FixedWords =
  run 0x00d7851c "marker setter FUN_00d7851c" [(0xe52db004, 0xe351000d), (0xe28db000, 0x03a0300d), (0xe3a03000, 0x13a03000)]
    ++ run 0x00d7852c "marker setter FUN_00d7851c" [(0xe28bd000, 0xe12fff1e)]
    ++ run 0x00eab2e0 "FilmMode FUN_00eab258" [(0xea00000b, 0xea00000f)]
    ++ run 0x00eab2f0 "FilmMode FUN_00eab258 switch table" [(0x00eab324, 0x00eab334), (0x00eab32c, 0x00eab334)]
    ++ run 0x00eab324 "FilmMode FUN_00eab258" [(0xe30f0fff, 0xe353000c), (0xea000008, 0x03a00c06), (0xe30f0fff, 0x13a00000)]
    ++ run 0x00eaac88 "Saturation FUN_00eaac74" [(0xea00004e, 0xea00004b)]
    ++ run 0x00eaacb0 "Saturation FUN_00eaac74 switch table" [(0x00eaadb8, 0x00eaadb0), (0x00eaadc0, 0x00eaadb0)]
    ++ run
      0x00eaadb0
      "Saturation FUN_00eaac74"
      [ (0xe3000301, 0xe2430007)
      , (0xea00000a, 0xe2800c03)
      , (0xe3000302, 0xea000009)
      , (0xea000008, 0xe353000d)
      , (0xe3000303, 0x0affffc0)
      , (0xea000006, 0xe3a00000)
      ]
    ++ run 0x00d461ec "RAW commit FUN_00d46010 row 10 to colour 8" [(0xe5832008, 0xe356000a), (0xe2466005, 0x03a02008), (0xe3560004, 0xe5832008), (0x8a000004, 0xea000004)]
    ++ run
      0x00d5ada4
      "RAW highlight FUN_00d588b8 colour 8 to row 10"
      ( zip
          [ 0xe3530000, 0x050b3018, 0x0a000164, 0xe3530005, 0x03a03001, 0x050b3018, 0x0a000160, 0xe3530003, 0x03a03002
          , 0x050b3018, 0x0a00015c, 0xe3530006, 0x03a03004, 0x050b3018, 0x0a000158, 0xe3530007, 0x03a03003
          ]
          [ 0xe3530005, 0x03a03001, 0x0a00000b, 0xe3530003, 0x03a03002, 0x0a000008, 0xe3530006, 0x03a03004, 0x0a000005
          , 0xe3530007, 0x03a03003, 0x0a000002, 0xe3530008, 0x03a0300a, 0x13a03000, 0xe50b3018, 0xea000156
          ]
      )

-- 23 direct #0x83d gates to #0x83e, two id-1 #0x83c gates to #0x83d.
plainGateWords :: [WordWrite]
plainGateWords =
  [ WordWrite (Address a) w (w + 1) GroupCC "label gate admits 0x83e"
  | (a, w) <-
      sort
        ( [ (a, 0xe300383d)
          | a <-
              [ 0x011122c0, 0x01112448, 0x01112664, 0x011127e8, 0x01112a74, 0x0111373c, 0x01113994, 0x01164308, 0x0116bcb4
              , 0x01178564, 0x01178694, 0x01178cf4, 0x01178fb8, 0x01179a58, 0x01179abc, 0x01179b14, 0x01179efc, 0x0117a734
              , 0x0117deb8, 0x0117def4, 0x0117df7c
              ]
          ]
            ++ [(0x011640f0, 0xe300a83d), (0x0116bbbc, 0xe300883d), (0x01178508, 0xe300383c), (0x011787b4, 0xe300383c)]
        )
  ]

plainRawWords :: [WordWrite]
plainRawWords =
  run
    0x012548a0
    "RAW request FUN_012546a8 colour 8 to col2 0xd"
    ( zip
        [ 0xe5943008, 0xe2433003, 0xe3530004, 0x979ff103, 0xea000004, 0x012548f0, 0x012548c8, 0x012548dc
        , 0x01254904, 0x01254918, 0xe3033a7c, 0xe3403151, 0xe3a02000, 0xe58327a4, 0xea000012, 0xe3033a7c
        ]
        [ 0xe5943008, 0xe3a02000, 0xe3530003, 0x03a02001, 0xe3530005, 0x03a02003, 0xe3530006, 0x03a0200b
        , 0xe3530007, 0x03a0200c, 0xe3530008, 0x03a0200d, 0xe3033a7c, 0xe3403151, 0xe58327a4, 0xea000011
        ]
    )

run :: Word32 -> String -> [(Word32, Word32)] -> [WordWrite]
run start role pairs = [WordWrite (Address (start + 4 * k)) c n GroupCC role | (k, (c, n)) <- zip [0 ..] pairs]

-- Replaced converter functions: (entry, extent bytes).
captureConverter :: (Address, Int)
captureConverter = (Address 0x00738fe8, 100)

settingsConverter :: (Address, Int)
settingsConverter = (Address 0x00736a74, 104)

image2RecordAddress :: Address
image2RecordAddress = Address 0x0037b808

image2RecordText :: String
image2RecordText = "CLASSIC CHROME"

image2RecordLayout :: [Word16]
image2RecordLayout = [40, 0, 30, 36, 48]

-- UTF-16LE name, zero unit, zero fill to +0x38, five layout halfwords: 66 bytes.
image2Record :: B.ByteString
image2Record = name <> B.replicate (0x38 - B.length name) 0 <> B.concat (map half image2RecordLayout)
  where
    name = B.concat (map (half . fromIntegral . ord) image2RecordText)
    half :: Word16 -> B.ByteString
    half h = B.pack [fromIntegral h, fromIntegral (h `shiftR` 8)]

labelQuadruple :: Word32 -> B.ByteString
labelQuadruple nameId = B.concat (map word32LE [0, 0, 0, nameId])

valuePair :: Word32 -> B.ByteString
valuePair token = word32LE token <> word32LE 0

listTerminatorLabels :: B.ByteString
listTerminatorLabels = B.replicate 16 0xff

valuesTerminator :: B.ByteString
valuesTerminator = valuePair 0x7ffffffe

word32LE :: Word32 -> B.ByteString
word32LE w = B.pack [fromIntegral (w `shiftR` s) | s <- [0, 8, 16, 24]]
