{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as L
import Replacement.RtosOwnFirst.Regions (Domain (..), GuestAddress (..), hexText)
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
          runExtents = runs dm
      case mapEntries dm of
        [] -> die "carrier union has no entries"
        (firstEntry : _) -> do
          text <- C.readFile readelfPath
          sections <- either (die . show) pure (parseSections text)
          placements <- either (die . show) pure (solveSections 2000000 runExtents sections)
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
    slug value = drop (length ("xm1_platform_veneer_" :: String)) value
    addressText (GuestAddress value) = hexText value
