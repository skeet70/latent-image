-- GVFF vector resources as the X-M1 1.50 Q-id drawer reads them
-- (reports/classic-chrome-icon.md section 14): the record layout, an encoder
-- for pixel art in the stock Q-tile form and a decoder for checking either.
--
-- Layout (byte-read over all 2,246 stock blocks in image 2; field names from
-- the x70view lead, tools/gvff.py): a 56-byte header (`GVFF`, +0x08 u16 2, u16
-- 1, +0x0c 0x001902cf, +0x10 header size 56, +0x14 path count, +0x18 paint
-- count, +0x1c ramp count, +0x28 four float bounds), the ramps, 36-byte
-- paints (+0x04 kind, +0x08 five floats), then the paths: 13 words (+0x00
-- draw flags, 1 stroke 2 fill, +0x08 blend, +0x0c stroke paint, +0x10 fill
-- paint, +0x14 dash index, +0x18 fill rule, +0x1c join, +0x20 cap, +0x24
-- stroke width, +0x28 miter limit, +0x2c segment count, +0x30 coordinate
-- bytes), the segment bytes, 4 - count mod 4 padding bytes, the coordinates.
module FujiAF.Gvff
  ( -- * Pixel art
    TileArt
  , tileArtWidth
  , tileArtHeight
  , tileArtRows
  , parseTileArt
    -- * Outlines
  , Vertex
  , Contour
  , traceOutlines
  , windingAt
    -- * Encoding
  , KeyPaint (..)
  , keyPaintRgba
  , encodeTileGvff
    -- * Decoding
  , Gvff (..)
  , Paint (..)
  , Path (..)
  , decodeGvff
  , pathContours
  ) where

import Control.Monad (unless, when)
import Data.Bits (shiftL, shiftR, (.|.))
import qualified Data.ByteString as B
import Data.List (sortOn)
import qualified Data.Map.Strict as M
import Data.Word (Word32, Word8)
import GHC.Float (castFloatToWord32, castWord32ToFloat)

-- Rows top first; True is ink.
data TileArt = TileArt Int Int [[Bool]]
  deriving (Eq, Show)

tileArtWidth, tileArtHeight :: TileArt -> Int
tileArtWidth (TileArt w _ _) = w
tileArtHeight (TileArt _ h _) = h

tileArtRows :: TileArt -> [[Bool]]
tileArtRows (TileArt _ _ rows) = rows

-- Comment lines start with "# "; every other line is `width` cells of '#'
-- (ink) or '.', and there are `height` of them.
parseTileArt :: Int -> Int -> String -> Either String TileArt
parseTileArt width height text = do
  let art = filter (\line -> take 2 line /= "# ") (lines text)
  rows <- mapM row (zip [1 :: Int ..] art)
  unless (length rows == height) (Left ("the art has " ++ show (length rows) ++ " rows, not " ++ show height))
  Right (TileArt width height rows)
  where
    row (k, line)
      | length line /= width = Left ("art row " ++ show k ++ " is " ++ show (length line) ++ " cells, not " ++ show width)
      | otherwise = mapM cell line
      where
        cell '#' = Right True
        cell '.' = Right False
        cell c = Left ("art row " ++ show k ++ " has " ++ show c ++ ", not '#' or '.'")

-- A pixel corner in the GVFF frame: x right, y up, the art's top-left corner
-- at (0, 0), so row r spans y from -r to -(r + 1).
type Vertex = (Int, Int)

-- A closed outline, first vertex not repeated.
type Contour = [Vertex]

-- The boundary of the ink, one contour per boundary component, each with the
-- ink on its left: outer boundaries run counter-clockwise and holes
-- clockwise, so the nonzero rule fills exactly the ink. Where two ink pixels
-- touch only at a corner the walk turns left, keeping them on separate
-- contours. Collinear vertices are dropped; each contour starts at its
-- smallest vertex and contours are ordered by that vertex.
traceOutlines :: TileArt -> [Contour]
traceOutlines (TileArt width height rows) = sortOn (take 1) (map simplify (walkAll edges))
  where
    ink c r = c >= 0 && r >= 0 && c < width && r < height && (rows !! r) !! c
    edges =
      M.fromListWith
        (++)
        [ (from, [to])
        | r <- [0 .. height - 1]
        , c <- [0 .. width - 1]
        , ink c r
        , (from, to) <-
            [((c, -r - 1), (c + 1, -r - 1)) | not (ink c (r + 1))]
              ++ [((c + 1, -r - 1), (c + 1, -r)) | not (ink (c + 1) r)]
              ++ [((c + 1, -r), (c, -r)) | not (ink c (r - 1))]
              ++ [((c, -r), (c, -r - 1)) | not (ink (c - 1) r)]
        ]
    walkAll remaining = case M.lookupMin remaining of
      Nothing -> []
      Just (start, _) ->
        let (contour, rest) = walk start start Nothing [] remaining
         in contour : walkAll rest
    walk start at incoming acc remaining =
      let outs = M.findWithDefault [] at remaining
          preferred = maybe outs (\d -> [o | turn <- [left d, d, right d], o <- outs, direction at o == turn] ++ outs) incoming
       in case preferred of
            [] -> (reverse (at : acc), remaining)
            next : _ ->
              let remaining' = M.update (\os -> let os' = filter (/= next) os in if null os' then Nothing else Just os') at remaining
               in if next == start
                    then (reverse (at : acc), remaining')
                    else walk start next (Just (direction at next)) (at : acc) remaining'
    direction (x0, y0) (x1, y1) = (signum (x1 - x0), signum (y1 - y0))
    left (dx, dy) = (-dy, dx)
    right (dx, dy) = (dy, -dx)
    simplify contour =
      let n = length contour
          at k = contour !! (k `mod` n)
          keep k = direction (at (k - 1)) (at k) /= direction (at k) (at (k + 1))
          kept = [at k | k <- [0 .. n - 1], keep k]
          smallest = minimum kept
          (before, after) = break (== smallest) kept
       in after ++ before

-- The nonzero winding number of the contours at a point.
windingAt :: [Contour] -> (Double, Double) -> Int
windingAt contours (px, py) = sum [crossing a b | contour <- contours, (a, b) <- zip contour (drop 1 contour ++ take 1 contour)]
  where
    crossing (x0, y0) (x1, y1)
      | fy0 <= py && fy1 > py && side > 0 = 1
      | fy0 > py && fy1 <= py && side < 0 = -1
      | otherwise = 0
      where
        fx0 = fromIntegral x0
        fy0 = fromIntegral y0
        fx1 = fromIntegral x1
        fy1 = fromIntegral y1
        side = (fx1 - fx0) * (py - fy0) - (px - fx0) * (fy1 - fy0)

-- The three solid key colours of the stock Q tiles. The Q-id drawer's GVFF
-- loader (id4 0x006da9b0) compares each solid paint with its key table and
-- substitutes the drawing state's colour.
data KeyPaint = KeyRed | KeyBlack | KeyWhite
  deriving (Eq, Show, Enum, Bounded)

keyPaintRgba :: KeyPaint -> [Float]
keyPaintRgba KeyRed = [1, 0, 0, 1]
keyPaintRgba KeyBlack = [0, 0, 0, 1]
keyPaintRgba KeyWhite = [1, 1, 1, 1]

paintIndex :: KeyPaint -> Word32
paintIndex = fromIntegral . fromEnum

-- OpenVG constants the stock tiles carry.
paintColor, ruleNonZero, joinMiter, joinRound, capButt, capRound, noDash :: Word32
paintColor = 0x1b00
ruleNonZero = 0x1901
joinMiter = 0x1800
joinRound = 0x1801
capButt = 0x1700
capRound = 0x1701
noDash = 0x29a

segClose, segMove, segLine :: Word8
segClose = 0
segMove = 2
segLine = 4

-- A Q tile in the stock form: the key paints red, black, white; path 0 fills
-- the bounds with white; path 1 fills the ink black and strokes its outline
-- red, 0.75 wide, round joins and caps. Bounds (0, 0, width, -height).
encodeTileGvff :: TileArt -> B.ByteString
encodeTileGvff art = header <> B.concat (map paint [minBound .. maxBound]) <> background <> inkPath
  where
    w = fromIntegral (tileArtWidth art) :: Float
    h = fromIntegral (tileArtHeight art) :: Float
    header =
      B.pack (map (fromIntegral . fromEnum) "GVFF")
        <> le32 0
        <> le32 0x00010002
        <> le32 0x001902cf
        <> le32 56
        <> le32 2
        <> le32 3
        <> le32 0
        <> le32 0
        <> le32 0xfffffffe
        <> B.concat (map leF [0, 0, w, -h])
    paint key = le32 0 <> le32 paintColor <> B.concat (map leF (keyPaintRgba key ++ [0])) <> le32 0 <> le32 0
    background =
      path 2 KeyRed KeyWhite joinMiter capButt 1 10 [[(w, -h), (0, -h), (0, 0), (w, 0)]]
    inkPath =
      path 3 KeyRed KeyBlack joinRound capRound 0.75 4 [[(fromIntegral x, fromIntegral y) | (x, y) <- c] | c <- traceOutlines art]
    path flags stroke fill join cap width miter contours =
      let segments = B.pack (concat [segMove : replicate (length c - 1) segLine ++ [segClose] | c <- contours])
          coords = B.concat [leF x <> leF y | c <- contours, (x, y) <- c]
       in B.concat
            ( map
                le32
                [flags, 0, 1, paintIndex stroke, paintIndex fill, noDash, ruleNonZero, join, cap]
            )
            <> leF width
            <> leF miter
            <> le32 (fromIntegral (B.length segments))
            <> le32 (fromIntegral (B.length coords))
            <> segments
            <> B.replicate (4 - B.length segments `mod` 4) 0
            <> coords

data Paint = Paint {paintKind :: Word32, paintParameters :: [Float]}
  deriving (Eq, Show)

data Path = Path
  { pathFlags :: Word32
  , pathStrokePaint :: Word32
  , pathFillPaint :: Word32
  , pathDash :: Word32
  , pathFillRule :: Word32
  , pathStrokeWidth :: Float
  , pathSegments :: [Word8]
  , pathCoordinates :: [Float]
  }
  deriving (Eq, Show)

data Gvff = Gvff {gvffBounds :: [Float], gvffRamps :: Int, gvffPaints :: [Paint], gvffPaths :: [Path], gvffLength :: Int}
  deriving (Eq, Show)

-- Decodes the header, paints and paths of a block whose segments are close,
-- move, line and cubic (absolute), refusing anything else; `gvffLength` is the
-- number of bytes they span.
decodeGvff :: B.ByteString -> Either String Gvff
decodeGvff bytes = do
  unless (B.take 4 bytes == B.pack (map (fromIntegral . fromEnum) "GVFF")) (Left "no GVFF magic")
  unless (B.length bytes >= 56 && word 0x10 == 56) (Left "header size is not 56")
  let paths = fromIntegral (word 0x14) :: Int
      paints = fromIntegral (word 0x18)
      ramps = fromIntegral (word 0x1c)
      bounds = [float (0x28 + 4 * k) | k <- [0 .. 3]]
  (afterRamps, _) <- foldlM (\(at, ()) _ -> do
                               count <- wordAt at
                               pure (at + 4 + 20 * fromIntegral count, ())) (56, ()) [1 .. ramps]
  let paintAt k = Paint (word (afterRamps + 36 * k + 4)) [float (afterRamps + 36 * k + 8 + 4 * j) | j <- [0 .. 4]]
      pathsStart = afterRamps + 36 * paints
  when (pathsStart > B.length bytes) (Left "paints run past the block")
  (end, decoded) <- foldlM decodePath (pathsStart, []) [1 .. paths]
  Right (Gvff bounds ramps (map paintAt [0 .. paints - 1]) (reverse decoded) end)
  where
    word at = le32At bytes at
    float at = castWord32ToFloat (word at)
    wordAt at = if at + 4 <= B.length bytes then Right (word at) else Left "block truncated"
    decodePath (at, acc) _ = do
      _ <- wordAt (at + 48)
      let field k = word (at + 4 * k)
          count = fromIntegral (field 11)
          coordBytes = fromIntegral (field 12)
          segments = B.unpack (B.take count (B.drop (at + 52) bytes))
          padding = 4 - count `mod` 4
          coordsAt = at + 52 + count + padding
          next = coordsAt + coordBytes
      when (next > B.length bytes) (Left ("path at " ++ show at ++ " runs past the block"))
      let needed = sum [maybe 0 id (lookup s [(0, 0), (2, 2), (4, 2), (12, 6)]) | s <- segments]
      unless (all (`elem` [segClose, segMove, segLine, 12]) segments) (Left ("path at " ++ show at ++ " has an unsupported segment"))
      unless (coordBytes == 4 * needed) (Left ("path at " ++ show at ++ ": coordinate bytes do not match its segments"))
      let path =
            Path
              { pathFlags = field 0
              , pathStrokePaint = field 3
              , pathFillPaint = field 4
              , pathDash = field 5
              , pathFillRule = field 6
              , pathStrokeWidth = castWord32ToFloat (field 9)
              , pathSegments = segments
              , pathCoordinates = [castWord32ToFloat (word (coordsAt + 4 * k)) | k <- [0 .. needed - 1]]
              }
      Right (next, path : acc)
    foldlM f z xs = case xs of
      [] -> Right z
      (x : rest) -> f z x >>= \z' -> foldlM f z' rest

-- The contours of a path made of move, line and close segments, with integer
-- coordinates; Nothing for any other path.
pathContours :: Path -> Maybe [Contour]
pathContours path = go (pathSegments path) (pathCoordinates path) [] []
  where
    go [] [] current done = if null current then Just (reverse done) else Nothing
    go (s : rest) coords current done
      | s == segClose = go rest coords [] (reverse current : done)
      | s == segMove || s == segLine, (x : y : more) <- coords, Just v <- vertex x y =
          if s == segMove && not (null current) then Nothing else go rest more (v : current) done
      | otherwise = Nothing
    go _ _ _ _ = Nothing
    vertex x y =
      let ix = round x
          iy = round y
       in if fromIntegral ix == x && fromIntegral iy == y then Just (ix, iy) else Nothing

le32 :: Word32 -> B.ByteString
le32 v = B.pack [fromIntegral (v `shiftR` k) | k <- [0, 8, 16, 24]]

leF :: Float -> B.ByteString
leF = le32 . castFloatToWord32

le32At :: B.ByteString -> Int -> Word32
le32At bs o = foldr (\k acc -> acc .|. (fromIntegral (B.index bs (o + k)) `shiftL` (8 * k))) 0 [0 .. 3]
