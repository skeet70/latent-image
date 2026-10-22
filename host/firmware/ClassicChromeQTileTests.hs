-- The GVFF tile encoder over synthetic art and the Classic Chrome Q tile
-- content. No artefact is read.
module Main (main) where

import Control.Exception (SomeException, displayException, try)
import qualified Data.ByteString as B
import Data.Word (Word8)
import qualified FujiAF.ClassicChromeIcon as I
import qualified FujiAF.ClassicChromeQTile as Q
import FujiAF.FdlFont (FontOffset (..))
import FujiAF.Gvff
import qualified FujiAF.Host as H
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)

art :: [String] -> TileArt
art rows = either error id (parseTileArt (maximum (0 : map length rows)) (length rows) (unlines rows))

-- A square ring (one hole), two pixels touching only at a corner, and a
-- notched block.
samples :: [(String, TileArt)]
samples =
  [ ("ring", art ["#####", "#...#", "#...#", "#####"])
  , ("corner touch", art ["#...", ".#..", "..##", "..##"])
  , ("notched block", art ["######.", "##..###", "######.", "...##..", "...##.."])
  , ("empty", art ["...", "..."])
  ]

-- Ink exactly where the nonzero winding at the pixel centre is nonzero.
fillsArt :: [Contour] -> TileArt -> Bool
fillsArt contours a =
  and
    [ (windingAt contours (fromIntegral c + 0.5, -(fromIntegral r + 0.5)) /= 0) == ink
    | (r, row) <- zip [0 :: Int ..] (tileArtRows a)
    , (c, ink) <- zip [0 :: Int ..] row
    ]

checkSample :: (String, TileArt) -> IO ()
checkSample (name, a) = do
  let bytes = encodeTileGvff a
  g <- either (\e -> H.failH (name ++ ": decode: " ++ e)) pure (decodeGvff bytes)
  H.ensure (gvffLength g == B.length bytes) (name ++ ": the decoded paths do not end at the block's end")
  H.ensure (gvffBounds g == [0, 0, fromIntegral (tileArtWidth a), -(fromIntegral (tileArtHeight a))]) (name ++ ": bounds")
  H.ensure (gvffPaints g == [Paint 0x1b00 (keyPaintRgba k ++ [0]) | k <- [KeyRed, KeyBlack, KeyWhite]]) (name ++ ": key paints")
  case gvffPaths g of
    [background, inkPath] -> do
      H.ensure ((pathFlags background, pathFillPaint background) == (2, 2)) (name ++ ": background path")
      H.ensure ((pathFlags inkPath, pathStrokePaint inkPath, pathFillPaint inkPath, pathFillRule inkPath, pathDash inkPath) == (3, 0, 1, 0x1901, 0x29a)) (name ++ ": ink path fields")
      contours <- maybe (H.failH (name ++ ": ink path is not move/line/close")) pure (pathContours inkPath)
      H.ensure (contours == traceOutlines a) (name ++ ": the encoded outline is not the traced one")
      H.ensure (fillsArt contours a) (name ++ ": the outline does not fill exactly the ink")
    _ -> H.failH (name ++ ": not two paths")

main :: IO ()
main = do
  outcome <- try run
  case outcome :: Either SomeException () of
    Left err -> hPutStrLn stderr (displayException err) >> exitFailure
    Right () -> pure ()

run :: IO ()
run = do
  mapM_ checkSample samples
  -- A segment count that is a multiple of four still carries four padding
  -- bytes: a block (5 segments) and an L (7) make 12.
  let padded = art ["##.#", "##.#", "...#", "..##"]
  checkSample ("padding", padded)
  H.ensure (sum [length c + 1 | c <- traceOutlines padded] == 12) "the padding sample does not have 12 segments"
  H.ensure (traceOutlines (art ["#...", ".#.."]) == [[(0, -1), (1, -1), (1, 0), (0, 0)], [(1, -2), (2, -2), (2, -1), (1, -1)]]) "corner-touching pixels are not two counter-clockwise squares"
  H.ensure (traceOutlines (art ["###", "#.#", "###"]) == [[(0, -3), (3, -3), (3, 0), (0, 0)], [(1, -2), (1, -1), (2, -1), (2, -2)]]) "a ring is not a counter-clockwise outer and a clockwise hole"
  H.ensure (either (const True) (const False) (parseTileArt 3 2 "###\n#x#\n")) "art with a stray cell parses"

  -- The Classic Chrome tile.
  checkSample ("Classic Chrome Q tile", Q.qTileArt)
  H.ensure ((tileArtWidth Q.qTileArt, tileArtHeight Q.qTileArt) == (80, 48)) "the Q tile art is not 80 x 48"
  let FontOffset tailEnd = I.iconTailEnd
      FontOffset labelsAt = I.qLabelsOffset
  H.ensure (Q.qTileOffset == I.iconTailEnd && tailEnd `mod` 16 == 0 && tailEnd >= labelsAt + B.length I.qLabelsArray && Q.qTileAddress == 0x003bc000 + fromIntegral tailEnd) "the tile is not at the icon's tail end"
  H.ensure (map Q.qGlyphLabel [Q.ccQGlyph, Q.proNegStdTile, Q.proNegHiTile] == [0xff00037c, 0xff000779, 0xff000778]) "Q glyph labels"
  H.ensure ([B.take 4 (B.drop (16 * row) I.qLabelsArray) | row <- [5, 6, 7]] == map (B.pack . le . toInteger . Q.qGlyphLabel) [Q.proNegStdTile, Q.proNegHiTile, Q.ccQGlyph]) "the icon's Q labels rows 5-7 do not name the PRO Neg tiles and the Classic Chrome glyph"
  H.ensure (map Q.i2Address Q.qTileImage2Writes == [0x0022d28c] && map Q.i2New Q.qTileImage2Writes == [Q.qTileAddress]) "the image-2 write is not the 0x37c record pointer to the tile"

  -- The font writes over a window holding the icon's Q labels array.
  let FontOffset labels = I.qLabelsOffset
      window = B.replicate labels 0 <> I.qLabelsArray <> B.replicate (0x80000 - labels - B.length I.qLabelsArray) 0
  writes <- either H.failH pure (Q.qTileFontWrites window)
  H.ensure (map I.fwOffset writes == [FontOffset tailEnd] && map I.fwNew writes == [Q.qTileGvff]) "the font writes are not the tile at the tail end"
  H.ensure (either (const True) (const False) (Q.qTileFontWrites (B.replicate 0x80000 0))) "a window without the icon's Q labels is accepted"
  let dirty = B.take (tailEnd + 0x40) window <> B.singleton 1 <> B.drop (tailEnd + 0x41) window
  H.ensure (either (const True) (const False) (Q.qTileFontWrites dirty)) "a nonzero byte under the tile is accepted"
  putStrLn ("Classic Chrome Q tile: " ++ show (B.length Q.qTileGvff) ++ " GVFF bytes; all checks passed")
  where
    le :: Integer -> [Word8]
    le v = [fromIntegral ((v `div` (256 ^ k)) `mod` 256) | k <- [0 :: Int .. 3]]
