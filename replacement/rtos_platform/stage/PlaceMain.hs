{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Control.Monad (forM, forM_, unless, when)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as L
import Data.List (isPrefixOf, partition)
import Data.Word (Word32)
import Replacement.RtosOwnFirst.Regions (Domain (..), GuestAddress (..), extentStart, hexText)
import Replacement.RtosPlatform.Place
import Replacement.RtosPlatform.Union
import System.Environment (getArgs)
import System.Exit (die)

main :: IO ()
main = do
  args <- getArgs
  case args of
    [carriersPath, domainText, readelfPath, output, manifest] -> do
      carriers <- B.readFile carriersPath
      union <- either (die . show) pure (parseUnion carriers)
      domain <- case domainText of
        "a" -> pure DomainA
        "b" -> pure DomainB
        _ -> die "domain must be a or b"
      let dm = domainMap union domain
          tagged = groupedRuns dm
          runExtents = map snd tagged
          reservedGroups = map fst (unionReserved union)
          declared = concatMap snd (unionReserved union)
          reservedSection s = any (`isPrefixOf` sectionName s) declared
          groupsFor name = [group | (group, names) <- unionReserved union, any (`isPrefixOf` name) names]
      forM_ [e | e <- mapEntries dm, entryPinned e] $ \e ->
        unless (entryGroup e `elem` reservedGroups)
          (die ("pinned entry " ++ entryName e ++ " belongs to a group without reserved prefixes"))
      case mapEntries dm of
        [] -> die "carrier union has no entries"
        (firstEntry : _) -> do
          text <- C.readFile readelfPath
          sections <- either (die . show) pure (parseSections text)
          forM_ sections $ \section ->
            when (length (groupsFor (sectionName section)) > 1)
              (die ("section " ++ sectionName section ++ " matches more than one reserved group"))
          let (reserved, free) = partition reservedSection sections
              groupRuns group = [i | (i, (g, _)) <- zip [0 ..] tagged, g == group]
          pinRuns <- forM [e | e <- mapEntries dm, entryPinned e] $ \e ->
            case [i | (i, extent) <- zip [0 ..] runExtents, extentStart extent == offsetBy 4 (entryAddress e)] of
              [i] -> pure (entryGroup e, entrySection e, i)
              _ -> die ("no carrier run starts four bytes after the pinned entry " ++ entryName e)
          reservedPlacements <- fmap concat $ forM (unionReserved union) $ \(group, names) -> do
            let bound = [s | s <- reserved, any (`isPrefixOf` sectionName s) names]
                pins = [(name, i) | (g, name, i) <- pinRuns, g == group]
            either (die . show) pure (placeReserved runExtents pins (groupRuns group) bound)
          let reservedRuns = concatMap groupRuns reservedGroups
              openRuns = [i | i <- [0 .. length runExtents - 1], i `notElem` reservedRuns]
          freePlacements <- either (die . show) pure (solveOpenSections 50000 runExtents openRuns free)
          let placements = reservedPlacements ++ freePlacements
          either (die . show) (const (pure ())) (admitPlacement runExtents sections placements)
          let entryLines =
                [ "  .veneer_" ++ slug (entryVeneer e) ++ " " ++ addressText (entryAddress e) ++ " : { KEEP(*(.veneer." ++ slug (entryVeneer e) ++ ")) }"
                | e <- mapEntries dm
                ]
              externalLines =
                [ externalName e ++ " = " ++ addressText (externalAddress e) ++ ";" | e <- mapExternals dm ]
              scriptLines = lines (renderScript runExtents placements)
              body = unlines $
                [ "ENTRY(" ++ entryVeneer firstEntry ++ ")" ]
                  ++ take 2 scriptLines
                  ++ entryLines
                  ++ drop 2 scriptLines
                  ++ externalLines
          writeFile output body
          L.writeFile manifest (J.encode (placementValue runExtents placements) <> L.singleton 10)
    _ -> die "usage: place-platform CARRIERS a|b READELF_TEXT OUT_LD OUT_MANIFEST"
  where
    offsetBy :: Word32 -> GuestAddress -> GuestAddress
    offsetBy n (GuestAddress value) = GuestAddress (value + n)
    slug value = drop (length ("xm1_platform_veneer_" :: String)) value
    addressText (GuestAddress value) = hexText value
