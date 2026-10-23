{-# LANGUAGE OverloadedStrings #-}
-- Emits the walked payload field table the storage kernel, the host oracle and
-- the tests all read, from the authoritative map
-- reports/custom-settings.fields.json over pinned image 4 (conversion-table
-- pointers) and image 5 (the commit dispatcher's arm words).
--
-- Fail-closed: a row missing a key, rows that are not in payload order, rows
-- whose bit ranges overlap, a width over one byte without a matching byte
-- count, a conversion table that does not terminate inside the image, a
-- property outside the dispatcher's table, a walk without the properties the
-- bank's stack-index stores name (see 'requiredWalked'), or an input whose
-- SHA-256 is not the map's pinned value stops the run. In --check mode nothing
-- is written and a difference from the emitted files is an error.
--
--   emit-recipe-bank-fields MAP.json ID4.bin ID5.bin C_OUT.inc HS_OUT.hs
--   emit-recipe-bank-fields --check MAP.json ID4.bin ID5.bin C_OUT.inc HS_OUT.hs
module Main where

import Data.Aeson ((.:), (.:?))
import qualified Data.Aeson as A
import qualified Data.Aeson.Types as A
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.List (sortOn)
import qualified Data.Text as T
import Data.Maybe (isJust)
import Data.Word (Word16, Word32, Word8)
import Numeric (readHex, showHex)
import System.Environment (getArgs)

import FujiAF.Host (ensure, failH, sha256)

-- the pinned decode bases and the two structures the emitted columns come from
image4Base, image5Base, tablePointers, commitTable, commitSharedArm :: Integer
image4Base = 0x0061c400
image5Base = 0x00bf1a00
tablePointers = 0x0081b09c
commitTable = 0x00d4fcd4
commitSharedArm = 0x00d52664

payloadBytes :: Int
payloadBytes = 0x28

data Row = Row
  { rowOffset :: Int
  , rowShift :: Int
  , rowWidth :: Int
  , rowBytes :: Int
  , rowMask :: Word16
  , rowProperty :: Word16
  , rowLabel :: Maybe String
  , rowEdit :: Maybe Int
  , rowNote :: String
  }

data MapFile = MapFile
  { mapId4Sha :: String
  , mapId5Sha :: String
  , mapWalkCount :: Int
  , mapEditCount :: Int
  , mapWalk :: [Row]
  , mapExcluded :: [Row]
  }

main :: IO ()
main = do
  args <- getArgs
  let (check, rest) = case args of
        ("--check" : more) -> (True, more)
        _ -> (False, args)
  (mapPath, id4Path, id5Path, cOut, hsOut) <- case rest of
    [a, b, c, d, e] -> pure (a, b, c, d, e)
    _ -> failH "usage: emit-recipe-bank-fields [--check] MAP.json ID4.bin ID5.bin C_OUT.inc HS_OUT.hs"
  mapRaw <- B.readFile mapPath
  id4 <- B.readFile id4Path
  id5 <- B.readFile id5Path
  m <- either failH pure (A.eitherDecodeStrict' mapRaw >>= A.parseEither parseMap)
  mapDigest <- sha256 mapRaw
  id4Digest <- sha256 id4
  id5Digest <- sha256 id5
  ensure (id4Digest == mapId4Sha m) ("image 4: sha256 " ++ id4Digest ++ " is not the map's pinned " ++ mapId4Sha m)
  ensure (id5Digest == mapId5Sha m) ("image 5: sha256 " ++ id5Digest ++ " is not the map's pinned " ++ mapId5Sha m)
  ensure (length (mapWalk m) == mapWalkCount m) ("map: " ++ show (length (mapWalk m)) ++ " walk rows against the recorded " ++ show (mapWalkCount m))
  let edits = sortOn (maybe 0 id . rowEdit) (filter (isJust . rowEdit) (mapWalk m))
  ensure (length edits == mapEditCount m) ("map: " ++ show (length edits) ++ " edit rows against the recorded " ++ show (mapEditCount m))
  ensure (map rowEdit edits == map (Just . id) [0 .. mapEditCount m - 1]) "map: edit indices are not 0..count-1"
  validateRows (mapWalk m ++ mapExcluded m)
  validateWalkOrder (mapWalk m)
  validateRequired (mapWalk m)
  let tablesE = mapM (conversionTable id4 . rowProperty) (mapWalk m) :: Either String [[Word16]]
      armsE = mapM (commitArm id5 . rowProperty) (mapWalk m) :: Either String [Bool]
  _tables <- either failH pure tablesE
  arms <- either failH pure armsE
  let mask = unboundMask (mapWalk m ++ mapExcluded m)
      placed = zipWith (\r a -> (r, tablePointer id4 (rowProperty r), a)) (mapWalk m) arms
      cText = emitC mapDigest id4Digest mask (mapWalk m) edits
      hsText = emitHaskell mapDigest id4Digest id5Digest mask placed edits
  if check
    then do
      cOld <- B.readFile cOut
      hsOld <- B.readFile hsOut
      ensure (cOld == cText) (cOut ++ " differs from the emitted table")
      ensure (hsOld == hsText) (hsOut ++ " differs from the emitted table")
      putStrLn
        ( "recipe-bank fields: "
            ++ show (length (mapWalk m))
            ++ " walk rows and "
            ++ show (length edits)
            ++ " edit rows reproduce"
        )
    else do
      B.writeFile cOut cText
      B.writeFile hsOut hsText
      putStrLn
        ( "recipe-bank fields: emitted "
            ++ show (length (mapWalk m))
            ++ " walk rows and "
            ++ show (length edits)
            ++ " edit rows from "
            ++ mapDigest
        )

-- parsing ---------------------------------------------------------------

parseMap :: A.Value -> A.Parser MapFile
parseMap = A.withObject "map" $ \o -> do
  pinned <- o .: "pinned_input"
  id4Sha <- pinned .: "sha256"
  commit <- o .: "commit_dispatcher_input"
  id5Sha <- commit .: "sha256"
  pfm <- o .: "payload_field_map"
  walkCount <- pfm .: "walk_count"
  editCount <- pfm .: "edit_count"
  fields <- pfm .: "fields" :: A.Parser [A.Value]
  excluded <- pfm .: "bound_not_walked" :: A.Parser [A.Value]
  walk <- mapM parseRow fields
  notWalked <- mapM parseRow excluded
  pure (MapFile id4Sha id5Sha walkCount editCount walk notWalked)

parseRow :: A.Value -> A.Parser Row
parseRow = A.withObject "field row" $ \o -> do
  off <- hexText =<< o .: "offset"
  bits <- o .: "bits"
  prop <- hexText =<< o .: "property"
  bytes <- maybe 0 id <$> o .:? "bytes"
  label <- maybe A.Null id <$> o .:? "label"
  edit <- maybe (-1) id <$> o .:? "edit_index"
  note <- maybe "" id <$> o .:? "evidence"
  (shift, width) <- either fail pure (bitsRange bits)
  let byteCount = if bytes == 0 then (width + 7) `div` 8 else bytes
  pure
    Row
      { rowOffset = off
      , rowShift = shift
      , rowWidth = width
      , rowBytes = byteCount
      , rowMask = ((1 `shiftL` width) - 1) `shiftL` shift
      , rowProperty = fromIntegral prop
      , rowLabel = case label of
          A.String t -> Just (T.unpack t)
          _ -> Nothing
      , rowEdit = if edit < 0 then Nothing else Just edit
      , rowNote = note
      }

-- | A JSON hex string such as "0x1c6" or "0x05".
hexText :: String -> A.Parser Int
hexText t = case t of
  '0' : 'x' : rest -> case readHex rest of
    [(v, "")] -> pure v
    _ -> fail ("not a hexadecimal integer: " ++ t)
  _ -> fail ("hexadecimal string required: " ++ t)

-- | A bits field: "b" or "lo-hi", with lo <= hi, inside one 16-bit word.
bitsRange :: String -> Either String (Int, Int)
bitsRange s = case break (== '-') s of
  (one, "") -> case reads one of
    [(b, "")] | b >= 0 && b <= 15 -> Right (b, 1)
    _ -> Left ("bits field not admitted: " ++ s)
  (lo, '-' : hi) -> case (reads lo, reads hi) of
    ([(l, "")], [(h, "")]) | l >= 0 && h >= l && h <= 15 -> Right (l, h - l + 1)
    _ -> Left ("bits range not admitted: " ++ s)
  _ -> Left ("bits field not admitted: " ++ s)

-- | Every row stays inside the payload, a multi-byte row is shifted by zero,
-- and no two rows (walked or bound-not-walked) share a payload bit.
validateRows :: [Row] -> IO ()
validateRows rows = do
  mapM_ one rows
  case [(a, b) | (i, a) <- zip [0 :: Int ..] rows, (j, b) <- zip [0 :: Int ..] rows, i < j, sharesBit a b] of
    [] -> pure ()
    (a, b) : _ ->
      failH
        ( "field rows overlap: property "
            ++ showHex (rowProperty a) ""
            ++ " and "
            ++ showHex (rowProperty b) ""
        )
 where
  one r = do
    ensure (rowWidth r >= 1 && rowWidth r <= 16) ("field row width out of range: " ++ rowNote r)
    ensure (rowBytes r == (rowWidth r + 7) `div` 8) ("field row byte count disagrees with its width: " ++ rowNote r)
    ensure (rowOffset r >= 0 && rowOffset r + rowBytes r <= payloadBytes) ("field row leaves the payload: " ++ rowNote r)
    ensure (rowShift r + rowWidth r <= 8 * rowBytes r) ("field row bits leave their bytes: " ++ rowNote r)
    ensure (rowWidth r <= 8 || rowShift r == 0) ("a multi-byte row must not be shifted: " ++ rowNote r)
  sharesBit a b = or [byte == byte' && mask .&. mask' /= 0 | (byte, mask) <- rowBits a, (byte', mask') <- rowBits b]

rowBits :: Row -> [(Int, Int)]
rowBits r =
  [ (rowOffset r + bit `div` 8, 1 `shiftL` (bit `mod` 8))
    | bit <- [rowShift r .. rowShift r + rowWidth r - 1]
  ]

-- | The walked order is the apply order and is part of the map's contract:
-- rb_recall drives the stock commits in this order and some arms write other
-- walked fields (image quality's writes 0x1d, bracketing's writes 0x1b1), so
-- the rows ascend by (payload offset, bit shift) and the two full-width
-- halfword rows lead. The bound-not-walked rows carry no order and are checked
-- by 'validateRows' alone.
validateWalkOrder :: [Row] -> IO ()
validateWalkOrder rows = do
  let order = [(rowOffset r, rowShift r) | r <- rows]
  ensure (order == sortOn id order) "walk rows are not in payload order"
  ensure (all (\r -> rowBytes r == 1) (drop 2 rows)) "a walk row after the two halfwords is multi-byte"
  case take 2 rows of
    [a, b] -> do
      ensure
        (rowOffset a == 0x00 && rowBytes a == 2 && rowShift a == 0 && rowMask a == 0xffff)
        "the first walk row is not the full-width +0x00 halfword"
      ensure
        (rowOffset b == 0x02 && rowBytes b == 2 && rowShift b == 0 && rowMask b == 0xffff)
        "the second walk row is not the full-width +0x02 halfword"
    _ -> failH "the walk table does not start with its two halfword rows"

-- | Properties whose walk index the bank uses as a stack-record skip-word
-- index (rb_select's 0x1c6, rb_admit_base's 0xf9 and 0xfa); a miss sends that
-- store outside the frame, and menu/recipe_bank_allowlist.json declares those
-- stores on the strength of this check.
requiredWalked :: [Word16]
requiredWalked = [0x1c6, 0xf9, 0xfa]

validateRequired :: [Row] -> IO ()
validateRequired rows =
  mapM_
    (\p -> ensure (p `elem` map rowProperty rows) ("walk rows lack property 0x" ++ showHex p ""))
    requiredWalked

-- | Bytes carrying bits that no walked row and no bound-not-walked row covers:
-- the kernel requires them zero and zeroes them on a remembered payload.
unboundMask :: [Row] -> [Word8]
unboundMask rows = [fromIntegral (0xff - covered i) | i <- [0 .. payloadBytes - 1]]
 where
  bits = concatMap rowBits rows
  covered i = foldr (.|.) (0 :: Int) [mask | (byte, mask) <- bits, byte == i]

-- pinned structures -----------------------------------------------------

-- | The conversion-table rows of one property as {col0, col2} pairs: a null
-- pointer word means the field carries no table domain.
conversionTable :: B.ByteString -> Word16 -> Either String [Word16]
conversionTable image prop
  | pointer == 0 = Right []
  | otherwise = go 0
 where
  pointer = toInteger (tablePointer image prop) :: Integer
  go :: Int -> Either String [Word16]
  go k
    | k > 4096 = Left ("conversion table at " ++ showHex pointer "" ++ " does not terminate inside image 4")
    | otherwise = case u32le image (fromIntegral (pointer + 12 * fromIntegral k - image4Base)) of
        Nothing -> Left ("conversion table at " ++ showHex pointer "" ++ " leaves image 4")
        Just word0 ->
          let col0 = fromIntegral (word0 .&. 0xffff) :: Word16
              col2 = fromIntegral (word0 `shiftR` 16) :: Word16
           in if col0 == 0xffff
                then Right []
                else case u32le image (fromIntegral (pointer + 12 * fromIntegral k + 4 - image4Base)) of
                  Just m | m == 0x7ffffffe -> (col0 :) . (col2 :) <$> go (k + 1)
                  _ -> Left ("conversion table at " ++ showHex pointer "" ++ " carries an unexpected middle word")

-- | Whether the property has its own arm in the commit dispatcher: the word at
-- 0x00d4fcd4 + 4*(property-10) in image 5 is either an arm address or a B to
-- the shared arm at 0x00d52664, the same encoding the census reads.
commitArm :: B.ByteString -> Word16 -> Either String Bool
commitArm image prop
  | p < 10 || p > 646 = Left ("property outside the commit dispatcher table: " ++ showHex prop "")
  | otherwise = case u32le image (fromIntegral (commitTable + 4 * (p - 10) - image5Base)) of
      Nothing -> Left ("commit dispatcher table leaves image 5 at property " ++ showHex prop "")
      Just word
        | word `shiftR` 24 == 0xea ->
            let imm = toInteger (word .&. 0xffffff) :: Integer
                target = commitTable + 4 * (p - 10) + 8 + signExtend imm * 4
             in pure (target /= commitSharedArm)
        | otherwise -> pure (toInteger word /= commitSharedArm)
 where
  p = toInteger prop
  signExtend v = if v >= 0x800000 then v - 0x1000000 else v

u32le :: B.ByteString -> Int -> Maybe Word32
u32le bytes o
  | o < 0 || o + 4 > B.length bytes = Nothing
  | otherwise = Just (sum [fromIntegral (B.index bytes (o + i)) * (256 ^ i) | i <- [0 :: Int .. 3]])

-- | The property's conversion-table pointer word: 0 means the field carries no
-- table domain.
tablePointer :: B.ByteString -> Word16 -> Word32
tablePointer image prop =
  maybe 0 id (u32le image (fromIntegral (tablePointers + 4 * fromIntegral prop - image4Base)))

-- emission --------------------------------------------------------------

emitC :: String -> String -> [Word8] -> [Row] -> [Row] -> B.ByteString
emitC mapDigest id4Digest mask walk edits = BC.pack (unlines (header ++ walkTable ++ editTable ++ maskTable))
 where
  header =
    [ "/* Generated by //host/firmware:emit-recipe-bank-fields; do not edit."
    , " *"
    , " * Source: reports/custom-settings.fields.json sha256 " ++ mapDigest ++ ";"
    , " * image 4 (conversion-table pointers) sha256 " ++ id4Digest ++ "."
    , " * " ++ show (length walk) ++ " walked rows in apply order, then the nine recipe-edit rows."
    , " */"
    ]
  walkTable =
    [ "#define RB_FIELD_COUNT " ++ show (length walk) ++ "u"
    , "static const struct rb_field RB_FIELDS[RB_FIELD_COUNT] = {"
    ]
      ++ map rowC walk
      ++ ["};"]
  editTable =
    [ "#define RB_EDIT_FIELD_COUNT " ++ show (length edits) ++ "u"
    , "static const struct rb_field RB_EDIT_FIELDS[RB_EDIT_FIELD_COUNT] = {"
    ]
      ++ map rowC edits
      ++ ["};"]
  maskTable =
    [ "/* payload bits no walk row and no bound-not-walked row carries: the"
    , " * record holds them zero and a remembered payload zeroes them */"
    , "static const uint8_t RB_UNBOUND_MASK[RB_PAYLOAD] = {"
    ]
      ++ byteLines mask
      ++ ["};"]

rowC :: Row -> String
rowC r =
  "  {"
    ++ hex 4 (fromIntegral (rowProperty r))
    ++ ", "
    ++ hex 2 (rowOffset r)
    ++ ", "
    ++ show (rowShift r)
    ++ "u, "
    ++ show (rowBytes r)
    ++ "u, "
    ++ hex 4 (fromIntegral (rowMask r))
    ++ "u},"

emitHaskell :: String -> String -> String -> [Word8] -> [(Row, Word32, Bool)] -> [Row] -> B.ByteString
emitHaskell mapDigest id4Digest id5Digest mask placed edits = BC.pack (unlines (header ++ values ++ editValues ++ maskValues))
 where
  header =
    [ "{-# LANGUAGE OverloadedStrings #-}"
    , "-- Generated by //host/firmware:emit-recipe-bank-fields; do not edit."
    , "--"
    , "-- Source: reports/custom-settings.fields.json sha256 " ++ mapDigest ++ ","
    , "-- over image 4 sha256 " ++ id4Digest ++ " (conversion-table pointers) and"
    , "-- image 5 sha256 " ++ id5Digest ++ " (commit dispatcher arm words)."
    , "-- " ++ show (length placed) ++ " walked rows and " ++ show (length edits) ++ " recipe-edit rows."
    , "module FujiAF.RecipeBankFields"
    , "  ( PayloadField (..)"
    , "  , payloadFields"
    , "  , editFields"
    , "  , unboundMask"
    , "  , payloadFieldCount"
    , "  , editFieldCount"
    , "  ) where"
    , ""
    , "import Data.Word (Word16, Word32, Word8)"
    , ""
    , "-- | One walked payload field: the property, the field's place in the 0x28"
    , "-- payload, the pinned conversion table that carries row to stored code (0"
    , "-- for the null pointer word), and whether the property has its own arm in"
    , "-- the commit dispatcher 0x00d4fc74 (the other rows commit through the"
    , "-- shared arm at 0x00d52664)."
    , "data PayloadField = PayloadField"
    , "  { fieldProperty :: Word16"
    , "  , fieldOffset :: Word8"
    , "  , fieldShift :: Word8"
    , "  , fieldBytes :: Word8"
    , "  , fieldMask :: Word16"
    , "  , fieldLabel :: Maybe String"
    , "  , fieldTable :: Word32"
    , "  , fieldCommitArm :: Bool"
    , "  }"
    , "  deriving (Eq, Show)"
    , ""
    , "payloadFieldCount :: Int"
    , "payloadFieldCount = " ++ show (length placed)
    , ""
    , "editFieldCount :: Int"
    , "editFieldCount = " ++ show (length edits)
    , ""
    , "payloadFields :: [PayloadField]"
    , "payloadFields ="
    ]
  values = "  [" : commaRows [fieldH r table arm | (r, table, arm) <- placed] ++ ["  ]"]
  editValues =
    [ ""
    , "-- | The nine recipe-edit fields, in the design's order."
    , "editFields :: [PayloadField]"
    , "editFields ="
    ]
      ++ ("  [" : commaRows [fieldH r (tableFor r) (armFor r) | r <- edits] ++ ["  ]"])
  commaRows rows = case rows of
    [] -> []
    [one] -> ["  " ++ one]
    (one : rest) -> ("  " ++ one ++ ",") : commaRows rest
  tableFor r = maybe 0 id (lookup (rowProperty r) [(rowProperty r', t) | (r', t, _) <- placed])
  armFor r = maybe False id (lookup (rowProperty r) [(rowProperty r', a) | (r', _, a) <- placed])
  maskValues =
    [ ""
    , "-- | Bits no walk row and no bound-not-walked row carries, per payload byte."
    , "unboundMask :: [Word8]"
    , "unboundMask ="
    , "  ["
    ]
      ++ byteLines mask
      ++ ["  ]"]

fieldH :: Row -> Word32 -> Bool -> String
fieldH r table arm =
  "PayloadField "
    ++ hex 4 (fromIntegral (rowProperty r))
    ++ " "
    ++ hex 2 (rowOffset r)
    ++ " "
    ++ show (rowShift r)
    ++ " "
    ++ show (rowBytes r)
    ++ " "
    ++ hex 4 (fromIntegral (rowMask r))
    ++ " "
    ++ maybe "Nothing" (\l -> "(Just " ++ show l ++ ")") (rowLabel r)
    ++ " "
    ++ hex 8 (fromIntegral table)
    ++ " "
    ++ (if arm then "True" else "False")

byteLines :: [Word8] -> [String]
byteLines bs = go (chunksOf 8 bs)
 where
  go [] = []
  go [c] = ["  " ++ join (map (hex 2 . fromIntegral) c)]
  go (c : cs) = ("  " ++ join (map (hex 2 . fromIntegral) c) ++ ",") : go cs
  join = foldr (\a b -> if null b then a else a ++ ", " ++ b) ""
  chunksOf n xs = case splitAt n xs of
    (a, []) -> [a]
    (a, b) -> a : chunksOf n b

hex :: Int -> Int -> String
hex width value = "0x" ++ replicate (width - length s) '0' ++ s where s = showHex value ""
