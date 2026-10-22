-- The Classic Chrome film-simulation icon, U+F073 in FONT_ICON.FDL, and the
-- content the icon adds to the Classic Chrome candidate: the glyph program
-- encoded from host/firmware/classic-chrome-icon.txt, its font record, the Q
-- node arrays that list it, the id-0x83d label record that names it and the
-- image-4 words that point at them (reports/classic-chrome-icon.md).
module FujiAF.ClassicChromeIcon
  ( -- * Art
    IconArt
  , iconArtRows
  , parseIconArt
  , iconArt
    -- * Program
  , GlyphProgram (..)
  , encodeGlyphProgram
  , iconProgram
  , iconCode
  , iconGlyphRecord
  , iconGlyphFlags
    -- * Font placement
  , fontLoadAddress
  , fontRamAddress
  , iconRecordOffset
  , qValuesOffset
  , qLabelsOffset
  , qValuesArray
  , qLabelsArray
  , iconPerMille
  , FontWrite (..)
  , iconFontWrites
    -- * Image 2 and image 4
  , label83dName
  , label83dLayout
  , label83dRecord
  , Image4Write (..)
  , iconImage4Writes
  ) where

import Data.Bits (shiftL, (.|.))
import qualified Data.ByteString as B
import Data.Word (Word16, Word32, Word8)
import FujiAF.ClassicChromeIconArt (iconArtText)
import FujiAF.FdlFont
import qualified FujiAF.LabelRecord as L
import FujiAF.UpdateContainer (putWord16, putWord32)

iconSize :: Int
iconSize = 45

-- 45 rows of 45 cells, top row first; True is ink.
newtype IconArt = IconArt [[Bool]]
  deriving (Eq, Show)

iconArtRows :: IconArt -> [[Bool]]
iconArtRows (IconArt rows) = rows

-- Comment lines start with "# "; every other line is 45 cells of '#' (ink)
-- or '.'.
parseIconArt :: String -> Either String IconArt
parseIconArt text = do
  let art = filter (not . comment) (lines text)
  rows <- mapM row (zip [1 :: Int ..] art)
  if length rows /= iconSize
    then Left ("the art has " ++ show (length rows) ++ " rows, not " ++ show iconSize)
    else Right (IconArt rows)
  where
    comment line = take 2 line == "# "
    row (k, line)
      | length line /= iconSize = Left ("art row " ++ show k ++ " is " ++ show (length line) ++ " cells, not " ++ show iconSize)
      | otherwise = mapM cell line
      where
        cell '#' = Right True
        cell '.' = Right False
        cell c = Left ("art row " ++ show k ++ " has " ++ show c ++ ", not '#' or '.'")

iconArt :: IconArt
iconArt = either (error . ("classic-chrome-icon.txt: " ++)) id (parseIconArt iconArtText)

newtype GlyphProgram = GlyphProgram B.ByteString
  deriving (Eq, Show)

type Span = (Int, Int)

-- A run of rows from its start row, one span per row.
data Group = Group Int [Span]

rowSpans :: [Bool] -> [Span]
rowSpans cells = go (zip [0 ..] cells)
  where
    go [] = []
    go ((x, True) : rest) =
      let (run, after) = span snd rest
       in (x, x + length run) : go after
    go (_ : rest) = go rest

overlaps :: Span -> Span -> Bool
overlaps (a, b) (c, d) = a <= d && c <= b

-- A span joins the group of the one previous-row span it overlaps when that
-- span overlaps no other span of its row. The open list holds one group per
-- span of the latest row, in span order. Groups close in the row they stop
-- being extended, in open-list order; still-open groups follow.
groups :: [[Span]] -> [Group]
groups rows = finish (foldl step ([], [], []) (zip [0 ..] rows))
  where
    finish (open, closed, _) = closed ++ open
    step (open, closed, previous) (y, current) =
      let joins s = case filter (overlaps s) previous of
            [p] | filter (overlaps p) current == [s] -> Just p
            _ -> Nothing
          owner p = [g | g@(Group _ spans) <- open, last spans == p]
          next s = case joins s of
            Just p -> [Group start (spans ++ [s]) | Group start spans <- owner p]
            Nothing -> [Group y [s]]
          joined = [p | s <- current, Just p <- [joins s]]
          closing = [g | g@(Group _ spans) <- open, last spans `notElem` joined]
       in (concatMap next current, closed ++ closing, current)

-- min(255, round((p + 0.5) * 256 / 45)), ties to even.
coordinate :: Int -> Word8
coordinate p = fromIntegral (min 255 (round (fromIntegral (2 * p + 1) * 128 / fromIntegral iconSize :: Rational) :: Integer))

dedupe :: Eq a => [a] -> [a]
dedupe (a : rest@(b : _)) | a == b = dedupe rest
dedupe (a : rest) = a : dedupe rest
dedupe [] = []

-- Point instructions alternate destination registers 2 and 3; the close
-- instruction lines the last register to register 1.
chain :: Word8 -> Word8 -> [(Int, Int)] -> [Word8]
chain opcode close = go 0 2
  where
    go cur _ [] = [close, cur `shiftL` 4 .|. 1]
    go cur next ((y, x) : rest) =
      [opcode, cur `shiftL` 4 .|. next, coordinate y, coordinate x]
        ++ go next (if next == 2 then 3 else 2) rest

groupBody :: Group -> [Word8]
groupBody (Group s spans) =
  [0xb5, 0x00, coordinate s, coordinate (l s), 0xb5, 0x10, coordinate e, coordinate (r e)]
    ++ chain 0x92 0x03 leftPoints
    ++ chain 0x96 0x07 rightPoints
  where
    e = s + length spans - 1
    l y = fst (spans !! (y - s))
    r y = snd (spans !! (y - s))
    leftPoints =
      dropLeading (s, l s) . dedupe $
        concat
          [ if l y < l (y - 1)
              then [(y, l (y - 1)), (y, l y)]
              else if l y > l (y - 1) then [(y - 1, l (y - 1)), (y - 1, l y)] else []
          | y <- [s + 1 .. e]
          ]
          ++ [(e, l e)]
    rightPoints =
      dedupe . dropTrailing (e, r e) $
        (s, r s)
          : concat
            [ if r y > r (y - 1)
                then [(y, r (y - 1)), (y, r y)]
                else if r y < r (y - 1) then [(y - 1, r (y - 1)), (y - 1, r y)] else []
            | y <- [s + 1 .. e]
            ]
    dropLeading p (q : rest) | p == q = rest
    dropLeading _ ps = ps
    dropTrailing p ps = if not (null ps) && last ps == p then init ps else ps

-- `fa 00`, the u16 body length, then the groups joined by `70 00`.
encodeGlyphProgram :: IconArt -> GlyphProgram
encodeGlyphProgram (IconArt rows) =
  GlyphProgram (B.pack [0xfa, 0x00] <> putWord16 (fromIntegral (B.length body)) <> body)
  where
    body = B.intercalate (B.pack [0x70, 0x00]) (map (B.pack . groupBody) (groups (map rowSpans rows)))

iconProgram :: GlyphProgram
iconProgram = encodeGlyphProgram iconArt

iconCode :: CodePoint
iconCode = CodePoint 0xf073

-- Advance flag only: the record is the zero advance pair, the u16 program
-- length and the program.
iconGlyphFlags :: GlyphFlags
iconGlyphFlags = GlyphFlags 1

iconGlyphRecord :: B.ByteString
iconGlyphRecord = putWord16 0 <> putWord16 0 <> putWord16 (fromIntegral (B.length program)) <> program
  where
    GlyphProgram program = iconProgram

fontLoadAddress :: Word32
fontLoadAddress = 0x003bc000

fontRamAddress :: FontOffset -> Word32
fontRamAddress (FontOffset offset) = fontLoadAddress + fromIntegral offset

-- Inside the loaded tail [0x7cfd4, 0x80000) past the font file.
iconRecordOffset, qValuesOffset, qLabelsOffset :: FontOffset
iconRecordOffset = FontOffset 0x7cfe0
qValuesOffset = FontOffset 0x7d160
qLabelsOffset = FontOffset 0x7d1b0

-- Q node value pairs {row, 0}, ending with {0x7ffffffe, 0}.
qValuesArray :: B.ByteString
qValuesArray = B.concat [putWord32 v <> putWord32 0 | v <- [0, 3, 1, 7, 0xc, 5, 6, 2, 0x7ffffffe]]

-- Q node label quads {id, 0, 0, 0}, ending with sixteen 0xff bytes.
qLabelsArray :: B.ByteString
qLabelsArray =
  B.concat [putWord32 i <> B.replicate 12 0 | i <- [0xff00077b, 0xff00077c, 0xff000773, 0xff000777, 0xff00077a, 0x49a, 0x496, 0x83d]]
    <> B.replicate 16 0xff

-- The size the stock film icons U+F06A..F06E use.
iconPerMille :: Word16
iconPerMille = 800

-- One write into the font window: `old` is the window's current bytes.
data FontWrite = FontWrite
  { fwOffset :: FontOffset
  , fwOld :: B.ByteString
  , fwNew :: B.ByteString
  , fwRole :: String
  }
  deriving (Eq, Show)

-- The writes into a font window that lacks U+F073: its directory word, its
-- per-mille halfword (written as the 4 bytes starting there), the record and
-- the two Q arrays, each placed in zero bytes past the font's own data.
iconFontWrites :: FdlFont -> Either String [FontWrite]
iconFontWrites font = do
  existing <- lookupGlyph font iconCode
  require (existing == Nothing) "U+F073 is already present in the font"
  directoryAt <- directoryWordOffset font iconCode
  perMilleAt' <- perMilleOffset font iconCode
  let FontOffset commands = fdlCommandsOffset font
      FontOffset record = iconRecordOffset
      begin = record - commands
      GlyphFlags flags = iconGlyphFlags
      directoryNew = putWord32 (fromIntegral flags `shiftL` 24 .|. fromIntegral begin)
      perMilleNew = putWord16 iconPerMille <> slice (shiftOffset perMilleAt' 2) 2
      placed =
        [ (iconRecordOffset, iconGlyphRecord, "U+F073 glyph record")
        , (qValuesOffset, qValuesArray, "Classic Chrome Q node values")
        , (qLabelsOffset, qLabelsArray, "Classic Chrome Q node labels")
        ]
  require (begin > 0 && begin < 0x1000000) "the record offset is not a 24-bit begin past the commands"
  mapM_ (\(at, bytes, role) -> do
           inWindow at (B.length bytes)
           require (B.all (== 0) (slice at (B.length bytes))) (role ++ " lands on nonzero bytes"))
    placed
  inWindow perMilleAt' 4
  Right
    ( [ FontWrite directoryAt (slice directoryAt 4) directoryNew "U+F073 directory word: begin = record - commands, flags 1"
      , FontWrite perMilleAt' (slice perMilleAt' 4) perMilleNew "U+F073 per-mille size 800 (the next code's halfword unchanged)"
      ]
        ++ [FontWrite at (slice at (B.length bytes)) bytes role | (at, bytes, role) <- placed]
    )
  where
    bytes' = fdlBytes font
    slice (FontOffset at) size = B.take size (B.drop at bytes')
    shiftOffset (FontOffset at) k = FontOffset (at + k)
    inWindow (FontOffset at) size =
      require (at >= 0 && at + size <= B.length bytes') ("font write at " ++ show at ++ " runs past the window")
    require ok message = if ok then Right () else Left message

-- The icon code point alone.
label83dName :: L.LabelName
label83dName = either error id (L.labelNameUnits [0xf073])

-- The stock list-icon layout of labels 0x489 and 0x49a.
label83dLayout :: L.LabelLayout
label83dLayout = L.LabelLayout 540 24 30 36 48

-- 66 bytes, media form.
label83dRecord :: B.ByteString
label83dRecord = L.specialRecordBytesValue (L.specialRecordContent label83dName label83dLayout)

-- One image-4 word: RAM address, the word the Classic Chrome candidate holds
-- there and the new word.
data Image4Write = Image4Write
  { i4Address :: Word32
  , i4Old :: Word32
  , i4New :: Word32
  , i4Role :: String
  }
  deriving (Eq, Show)

iconImage4Writes :: [Image4Write]
iconImage4Writes =
  [ Image4Write 0x0061c320 0 0x83d "CC shooting quad field 0: preview icon label 0x83d"
  , Image4Write 0x0061c324 0 0x83d "CC shooting quad field 1: list icon label 0x83d"
  , Image4Write 0x0061c3dc 0 0x83d "CC RAW-conversion quad field 1: list icon label 0x83d"
  , Image4Write 0x0078b024 0x00789e2c (fontRamAddress qLabelsOffset) "Q node labels pointer"
  , Image4Write 0x0078b020 0x00782348 (fontRamAddress qValuesOffset) "Q node values pointer"
  ]
