{-# LANGUAGE OverloadedStrings #-}

-- Behavioural tests for the Classic Chrome icon content and the FDL glyph
-- lookup. The encoder output and the arrays are compared against the pinned
-- digests of reports/classic-chrome-icon.md. The font cases run on a
-- synthetic font with the stock FONT_ICON geometry (0xf726 glyphs, directory
-- 0x51, commands 0x3dced, property 10 at 0x7c177 over 0xf000..0xf725, a
-- 0x80000-byte window): every code is looked up before and after the icon's
-- font writes.
module Main where

import Control.Exception (SomeException, displayException, try)
import Control.Monad (forM_, unless)
import Data.Bits (shiftL, (.|.))
import qualified Data.ByteString as B
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import Data.List (isInfixOf)
import Data.Word (Word16, Word32)
import qualified FujiAF.ClassicChromeIcon as I
import FujiAF.FdlFont
import qualified FujiAF.Host as H
import qualified FujiAF.LabelRecord as L
import FujiAF.UpdateContainer (putWord16, putWord32)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)

expectTrue :: IORef Int -> String -> Bool -> IO ()
expectTrue checked label ok = do
  H.ensure ok (label ++ ": failed")
  modifyIORef' checked (+ 1)

expectDigest :: IORef Int -> String -> Int -> String -> B.ByteString -> IO ()
expectDigest checked label size digest bytes = do
  actual <- H.sha256 bytes
  H.ensure (B.length bytes == size) (label ++ ": " ++ show (B.length bytes) ++ " bytes, not " ++ show size)
  H.ensure (actual == digest) (label ++ ": SHA-256 " ++ actual ++ ", not " ++ digest)
  modifyIORef' checked (+ 1)

contentCases :: IORef Int -> IO ()
contentCases checked = do
  let I.GlyphProgram program = I.iconProgram
  expectDigest checked "glyph program" 708 "f701df847dee3e07011df42f3eac28625ae53239331116d6f131f8d8c44935eb" program
  expectDigest checked "glyph record" 714 "b4bd628dfaf8567b8847ea1e9ca3add0a5098d4541e065b19f92115198805247" I.iconGlyphRecord
  expectDigest checked "Q values" 72 "01670314392c2618797a47aadebdad5fe6df4e55c564e6266d8df8c1d077b312" I.qValuesArray
  expectDigest checked "Q labels" 144 "56e79304725b4d70987cf95d6480725af8a0b9fa24559d92db88834bab277073" I.qLabelsArray
  expectTrue
    checked
    "0x83d record"
    ( I.label83dRecord
        == B.concat (map putWord16 [0xf073, 0]) <> B.replicate 52 0 <> B.concat (map putWord16 [540, 24, 30, 36, 48])
    )
  -- A one-row change to the art changes the program.
  let rows = I.iconArtRows I.iconArt
      art text = I.parseIconArt (unlines text)
      render = map (map (\ink -> if ink then '#' else '.'))
      altered = render (take 20 rows ++ [replicate 45 True] ++ drop 21 rows)
  case art ("# comment" : altered) of
    Right other -> expectTrue checked "changed art changes the program" (I.encodeGlyphProgram other /= I.iconProgram)
    Left message -> H.failH ("altered art refused: " ++ message)
  expectTrue checked "art round trip" (art (render rows) == Right I.iconArt)
  let below = drop 1 (render rows)
  expectTrue checked "short row refused" (either ("44 cells" `isInfixOf`) (const False) (art (replicate 44 '.' : below)))
  expectTrue checked "44 rows refused" (either ("44 rows" `isInfixOf`) (const False) (art below))
  expectTrue checked "foreign cell refused" (either ("'x'" `isInfixOf`) (const False) (art (('x' : replicate 44 '.') : below)))

-- The synthetic font: every fifth code present except U+F072 and U+F073;
-- codes divisible by 3 carry flags 1 (zero advance pair, u16 length) and the
-- rest flags 0. Programs are 6 bytes derived from the code.
glyphCount :: Int
glyphCount = 0xf726

presentCode :: Int -> Bool
presentCode c = c `mod` 5 == 0 && c `notElem` [0xf072, 0xf073]

recordFor :: Int -> B.ByteString
recordFor c
  | c `mod` 3 == 0 = putWord16 1 <> putWord16 2 <> putWord16 6 <> program
  | otherwise = program
  where
    program = B.pack [fromIntegral (c + k) | k <- [0 .. 5]]

syntheticFont :: B.ByteString
syntheticFont = B.take 0x80000 (header <> directory <> records <> padding <> properties <> B.replicate 0x80000 0)
  where
    commands = 0x3dced :: Int
    propertiesAt = 0x7c175 :: Int
    records = B.concat [recordFor c | c <- [0 .. glyphCount - 1], presentCode c]
    begins = scanl (+) 0 [if presentCode c then B.length (recordFor c) else 0 | c <- [0 .. glyphCount - 1]]
    flagsFor c = if c < glyphCount && presentCode c && c `mod` 3 == 0 then 1 else 0 :: Word32
    directory =
      B.concat [putWord32 (flagsFor c `shiftL` 24 .|. fromIntegral begin) | (c, begin) <- zip [0 ..] begins]
    header =
      B.concat
        [ "FDL Font"
        , B.replicate 0x14 0
        , putWord16 (fromIntegral glyphCount)
        , B.replicate 0x10 0
        , putWord32 0x51
        , putWord32 (fromIntegral commands)
        , putWord16 4
        , putWord16 0x100
        , putWord32 (fromIntegral propertiesAt)
        , B.replicate (0x51 - 0x3e) 0
        ]
    padding = B.replicate (propertiesAt - commands - B.length records) 0x5a
    perMille = [if c `elem` [0xf06a .. 0xf06e] then 800 else 1000 :: Word16 | c <- [0xf000 .. 0xf725 :: Int]]
    properties =
      putWord16 1 <> putWord16 10 <> putWord32 (14 + 2 * 0x726) <> putWord16 0xf000 <> putWord16 0xf725
        <> putWord16 0x010b <> putWord16 1000 <> B.concat (map putWord16 perMille)

applyWrites :: [I.FontWrite] -> B.ByteString -> B.ByteString
applyWrites writes bytes = foldl one bytes writes
  where
    one acc (I.FontWrite (FontOffset at) _ new _) = B.take at acc <> new <> B.drop (at + B.length new) acc

fontCases :: IORef Int -> IO ()
fontCases checked = do
  stock <- either H.failH pure (parseFdlFont syntheticFont)
  writes <- either H.failH pure (I.iconFontWrites stock)
  expectTrue
    checked
    "write offsets"
    (map I.fwOffset writes == [FontOffset 0x3c21d, FontOffset 0x7c26b, I.iconRecordOffset, I.qValuesOffset, I.qLabelsOffset])
  expectTrue checked "directory word" (map I.fwNew (take 1 writes) == [B.pack [0xf3, 0xf2, 0x03, 0x01]])
  expectTrue checked "old bytes are the window's" (and [I.fwOld w == B.take (B.length (I.fwNew w)) (B.drop at syntheticFont) | w <- writes, let FontOffset at = I.fwOffset w])
  patched <- either H.failH pure (parseFdlFont (applyWrites writes syntheticFont))
  let lookups font = mapM (lookupGlyph font . CodePoint . fromIntegral) [0 .. glyphCount - 1]
  before <- either H.failH pure (lookups stock)
  after <- either H.failH pure (lookups patched)
  let changed = [c | (c, b, a) <- zip3 [0 :: Int ..] before after, b /= a]
  expectTrue checked ("only U+F072 and U+F073 change: " ++ show changed) (changed == [0xf072, 0xf073])
  expectTrue checked "stock U+F073 absent" (before !! 0xf073 == Nothing)
  let I.GlyphProgram program = I.iconProgram
  expectTrue
    checked
    "patched U+F073"
    ( after !! 0xf073
        == Just (Glyph (FontOffset 0x7cfe0) (GlyphFlags 1) (Just (0, 0)) Nothing program)
    )
  expectTrue checked "per-mille U+F073" (perMilleAt patched I.iconCode == Right 800)
  forM_ [0xf072, 0xf074] $ \c ->
    expectTrue checked ("per-mille U+" ++ show c ++ " unchanged") (perMilleAt patched (CodePoint c) == perMilleAt stock (CodePoint c))
  -- The writes refuse a font that already has U+F073.
  expectTrue checked "present icon refused" (either ("already present" `isInfixOf`) (const False) (I.iconFontWrites patched))
  -- A record landing on nonzero bytes is refused.
  let dirty = B.take 0x7d000 syntheticFont <> B.singleton 1 <> B.drop 0x7d001 syntheticFont
  dirtyFont <- either H.failH pure (parseFdlFont dirty)
  expectTrue checked "nonzero tail refused" (either ("nonzero" `isInfixOf`) (const False) (I.iconFontWrites dirtyFont))
  unless (L.labelNameUnitList I.label83dName == [0xf073]) (H.failH "label83dName")

main :: IO ()
main = do
  outcome <- try $ do
    checked <- newIORef 0
    contentCases checked
    fontCases checked
    total <- readIORef checked
    putStrLn (show total ++ " classic-chrome-icon checks passed")
  case outcome :: Either SomeException () of
    Left err -> hPutStrLn stderr (displayException err) >> exitFailure
    Right () -> pure ()
