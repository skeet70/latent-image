{-# LANGUAGE OverloadedStrings #-}

-- | Whole-section placement over the declared carrier runs.
module Replacement.RtosPlatform.Place
  ( Section (..)
  , Placement (..)
  , PlaceError (..)
  , parseSections
  , solveSections
  , admitPlacement
  , placementValue
  , renderScript
  ) where

import Control.Monad (forM_, unless, when)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString.Char8 as C
import Data.List (isPrefixOf, sortOn)
import Data.Word (Word32)
import Numeric (readHex, showHex)
import Replacement.RtosOwnFirst.Regions (Extent (..), GuestAddress (..), extentBytes, extentStart)

data Section = Section
  { sectionName :: String
  , sectionBytes :: Int
  , sectionAlign :: Int
  }
  deriving (Eq, Show)

data Placement = Placement
  { placementSection :: Section
  , placementRun :: Int
  , placementOffset :: Int
  }
  deriving (Eq, Show)

data PlaceError
  = NoEligibleRun String Int Int
  | SearchExhausted [String]
  | SectionMisaligned String Int
  | SectionOutOfRun String Int Int Int
  | SectionsOverlap String String
  | PlacementIncomplete
  | S2PoolOutOfRange String Integer
  | S2PoolMissing
  deriving (Eq, Show)

parseSections :: C.ByteString -> Either PlaceError [Section]
parseSections bytes = Right [section | Just section <- map (parseRow . C.unpack) (C.lines bytes)]
  where
    parseRow line =
      let rest = dropWhile (not . isPrefixOf ".") (words line)
      in case rest of
           (name : kind : _addr : _off : size : _es : flg : _lk : _inf : align : _)
             | kind `elem` ["PROGBITS", "NOBITS"]
             , 'A' `elem` flg
             , not (any (`isPrefixOf` name) skipped)
             , Just n <- readHexText size
             , n > 0
             , Just a <- decInt align -> Just (Section name (fromIntegral n) a)
           _ -> Nothing
    skipped = [".ARM", ".rel", ".debug", ".comment", ".note", ".llvm", ".symtab", ".strtab", ".shstrtab", ".veneer"]

readHexText :: String -> Maybe Word32
readHexText text0 =
  let text = case text0 of
        '0' : 'x' : rest -> rest
        '0' : 'X' : rest -> rest
        _ -> text0
  in case text of
       digits@(_ : _) | all (`elem` ("0123456789abcdefABCDEF" :: String)) digits ->
         case readHex digits of
           [(value, "")] | value <= toInteger (maxBound :: Word32) -> Just (fromInteger value)
           _ -> Nothing
       _ -> Nothing

decInt :: String -> Maybe Int
decInt text = case reads text of
  [(n, "")] -> Just n
  _ -> Nothing

-- | Place whole sections deterministically. The S2 code and pool are first
-- fitted into one address window so every PC-relative pool load stays within
-- the 12-bit range; the remaining sections then fill the runs best-fit.
solveSections :: Int -> [Extent] -> [Section] -> Either PlaceError [Placement]
solveSections budget runExtents sections = do
  let caps = map extentBytes runExtents
      ordered = sortOn (negate . sectionBytes) sections
      (s2, rest) = partitionS2 ordered
  when (null s2) (Left (SearchExhausted ["no S2 sections"]))
  tryWindows caps windows s2 rest
  where
    windows = addressWindows runExtents
    tryWindows _ [] _ _ = Left (SearchExhausted ["no S2 window fits"])
    tryWindows caps (window : others) s2 rest =
      case preplace caps window s2 of
        Nothing -> tryWindows caps others s2 rest
        Just (cursors, preplaced) ->
          case greedy budget caps rest cursors [] of
            Nothing -> tryWindows caps others s2 rest
            Just (_, placed) -> do
              let placements = preplaced ++ placed
              admitPlacement runExtents sections placements
              pure placements

partitionS2 :: [Section] -> ([Section], [Section])
partitionS2 xs =
  let isS2 s = any (`isPrefixOf` sectionName s) [".text.s2_", ".rodata.s2_pool"]
  in ([x | x <- xs, isS2 x], [x | x <- xs, not (isS2 x)])

-- | Maximal windows of runs whose start addresses span at most 4095 bytes.
addressWindows :: [Extent] -> [[Int]]
addressWindows runExtents = go (zip [0 ..] (sortOn extentStart runExtents))
  where
    go [] = []
    go ((i, Extent (GuestAddress start) _) : rest) =
      let (inside, outside) = span (\(_, Extent (GuestAddress s) _) -> s - start <= 4095) rest
      in (i : map fst inside) : go outside

preplace :: [Int] -> [Int] -> [Section] -> Maybe ([Int], [Placement])
preplace caps window sections = go (sortOn (negate . sectionBytes) sections) (replicate (length caps) 0) []
  where
    go [] cursors placed = Just (cursors, reverse placed)
    go (s : rest) cursors placed =
      case [(k, alignTo (cursors !! k) (sectionAlign s)) | k <- window, alignTo (cursors !! k) (sectionAlign s) + sectionBytes s <= caps !! k] of
        [] -> Nothing
        fits ->
          case sortOn (\(j, o) -> (caps !! j - (o + sectionBytes s), j)) fits of
            [] -> Nothing
            ((k, off) : _) -> go rest (update k (off + sectionBytes s) cursors) (Placement s k off : placed)

greedy :: Int -> [Int] -> [Section] -> [Int] -> [Placement] -> Maybe ([Int], [Placement])
greedy budget caps sections cursors0 placed0 = go 0 sections cursors0 placed0
  where
    go _ [] cursors placed = Just (cursors, reverse placed)
    go nodes (s : rest) cursors placed
      | nodes > budget = Nothing
      | otherwise =
          case fits of
            [] -> Nothing
            _ -> tryFits (sortOn (\(k, off) -> (caps !! k - (off + sectionBytes s), k)) fits)
      where
        fits =
          [ (k, alignTo (cursors !! k) (sectionAlign s))
          | k <- [0 .. length caps - 1]
          , alignTo (cursors !! k) (sectionAlign s) + sectionBytes s <= caps !! k
          ]
        tryFits [] = Nothing
        tryFits ((k, off) : more) =
          case go (nodes + 1) rest (update k (off + sectionBytes s) cursors) (Placement s k off : placed) of
            Nothing -> tryFits more
            Just result -> Just result

update :: Int -> Int -> [Int] -> [Int]
update k value xs = [if i == k then value else x | (i, x) <- zip [0 ..] xs]

alignTo :: Int -> Int -> Int
alignTo value alignment = if alignment <= 1 then value else ((value + alignment - 1) `div` alignment) * alignment

admitPlacement :: [Extent] -> [Section] -> [Placement] -> Either PlaceError ()
admitPlacement runExtents sections placements = do
  let placedNames = sortOn id [sectionName (placementSection p) | p <- placements]
      expectedNames = sortOn id (map sectionName sections)
  unless (placedNames == expectedNames) (Left PlacementIncomplete)
  forM_ placements $ \p -> do
    let section = placementSection p
        run = runExtents !! placementRun p
        offset = placementOffset p
    when (offset `mod` sectionAlign section /= 0) (Left (SectionMisaligned (sectionName section) offset))
    unless (offset >= 0 && offset + sectionBytes section <= extentBytes run) (Left (SectionOutOfRun (sectionName section) offset (sectionBytes section) (extentBytes run)))
  forM_ (zip [0 ..] placements) $ \(i, left) ->
    forM_ (drop (i + 1) placements) $ \right ->
      when (placementRun left == placementRun right) $
        let a = placementOffset left
            b = a + sectionBytes (placementSection left)
            c = placementOffset right
            d = c + sectionBytes (placementSection right)
        in when (a < d && c < b) (Left (SectionsOverlap (sectionName (placementSection left)) (sectionName (placementSection right))))
  admitS2 runExtents placements

admitS2 :: [Extent] -> [Placement] -> Either PlaceError ()
admitS2 runExtents placements = do
  pool <- case [p | p <- placements, sectionName (placementSection p) == ".rodata.s2_pool"] of
    [p] -> Right p
    _ -> Left S2PoolMissing
  let poolAddress = addressOf runExtents pool
  forM_ [p | p <- placements, ".text.s2_" `isPrefixOf` sectionName (placementSection p)] $ \p -> do
    let base = toInteger (addressOf runExtents p)
        high = base + toInteger (sectionBytes (placementSection p))
        poolInteger = toInteger poolAddress
        distance = max (abs (poolInteger - base)) (abs (poolInteger - high))
    when (distance > 4095) (Left (S2PoolOutOfRange (sectionName (placementSection p)) distance))

addressOf :: [Extent] -> Placement -> Word32
addressOf runExtents p =
  let Extent (GuestAddress start) _ = runExtents !! placementRun p
  in start + fromIntegral (placementOffset p)

placementValue :: [Extent] -> [Placement] -> Value
placementValue runExtents placements =
  object
    [ "placements" .=
        [ object
            [ "section" .= sectionName (placementSection p)
            , "run" .= placementRun p
            , "offset" .= placementOffset p
            , "address" .= addressText (GuestAddress (addressOf runExtents p))
            , "bytes" .= sectionBytes (placementSection p)
            ]
        | p <- placements
        ]
    ]

addressText :: GuestAddress -> String
addressText (GuestAddress value) = "0x" ++ hexText value

hexText :: Word32 -> String
hexText value = let digits = showHex value "" in replicate (8 - length digits) '0' ++ digits

renderScript :: [Extent] -> [Placement] -> String
renderScript runExtents placements = unlines $
  [ "SECTIONS {" ]
    ++ [ "  " ++ outputName p ++ " " ++ addressText (GuestAddress (addressOf runExtents p)) ++ " : { KEEP(*(" ++ sectionName (placementSection p) ++ ")) }"
       | p <- sortedPlacements
       ]
    ++ [ "  /DISCARD/ : { *(.text) *(.rodata) *(.ARM.exidx) *(.ARM.exidx.*) *(.ARM.extab) *(.ARM.extab.*) *(.comment) *(.note) *(.note.*) *(.debug*) *(.llvm_addrsig) *(.ARM.attributes) *(.llvmbc) *(.llvmcmd) }"
       , "}"
       ]
  where
    sortedPlacements = sortOn (\p -> (placementRun p, placementOffset p)) placements
    outputName p = ".placed." ++ map (\c -> if c `elem` (".-" :: String) then '_' else c) (sectionName (placementSection p))
