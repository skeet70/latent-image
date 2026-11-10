{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Control.Monad (forM_)
import Data.Aeson (FromJSON (..), Value, object, withObject, (.:), (.=))
import qualified Data.Aeson as J
import Data.Aeson.Types (Parser)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.Word (Word32)
import qualified FujiAF.Host as H
import Numeric (readHex)
import Replacement.RtosOwnFirst.Regions (Extent (..), GuestAddress (..), domains, domainLabel, extentBytes, extentEnd, extentStart, hexText)
import Replacement.RtosPlatform.Artifact
import Replacement.RtosPlatform.Union
import System.Directory (createDirectory, doesPathExist)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))

newtype PlacementRow = PlacementRow (Word32, Int)
instance FromJSON PlacementRow where
  parseJSON = withObject "placement" $ \o -> do
    address <- o .: "address" :: Parser String
    bytes <- o .: "bytes"
    case address of
      '0' : 'x' : digits -> case readHex digits of
        [(value, "")] | value <= toInteger (maxBound :: Word32) -> pure (PlacementRow (fromInteger value, bytes))
        _ -> fail "bad placement address"
      _ -> fail "bad placement address"

newtype PlacementFile = PlacementFile [PlacementRow]
instance FromJSON PlacementFile where
  parseJSON = withObject "placement file" $ \o -> PlacementFile <$> o .: "placements"

placementSpans :: B.ByteString -> Either String [(Word32, Int)]
placementSpans bytes = case J.eitherDecodeStrict bytes of
  Left err -> Left err
  Right (PlacementFile rows) -> Right [span_ | PlacementRow span_ <- rows]

readInput :: FilePath -> IO B.ByteString
readInput path = H.readRegularWith (pure ()) False 0x10000000 path

fitValue :: Union -> Value
fitValue union = object
  [ "domains" .=
      [ object
          [ "domain" .= domainLabel domain
          , "runs" .= map extentValue (runs (domainMap union domain))
          , "total_run_bytes" .= totalRunBytes (domainMap union domain)
          , "total_demand_bytes" .= totalDemandBytes (domainMap union domain)
          , "result" .= resultValue (fit (domainMap union domain))
          ]
      | domain <- domains
      ]
  ]
  where
    extentValue extent = object ["start" .= addressText (extentStart extent), "end" .= addressText (extentEnd extent), "bytes" .= extentBytes extent]
    addressText (GuestAddress value) = hexText value
    resultValue (Right ()) = object ["status" .= ("fits" :: String)]
    resultValue (Left (CarrierDeficit domain group symbol bytes largest)) =
      object ["status" .= ("deficit" :: String), "domain" .= domainLabel domain, "group" .= groupLabel group, "symbol" .= symbol, "bytes" .= bytes, "largest_run" .= largest]
    resultValue (Left (TotalDeficit domain demand supply)) =
      object ["status" .= ("total-deficit" :: String), "domain" .= domainLabel domain, "demand" .= demand, "supply" .= supply]

main :: IO ()
main = do
  args <- getArgs
  case args of
    ["fit", carriersPath] -> do
      union <- either (H.failH . show) pure . parseUnion =<< readInput carriersPath
      let results = [fit (domainMap union domain) | domain <- domains]
      L.putStr (J.encode (fitValue union) <> L.singleton 10)
      if any isLeft results then exitFailure else pure ()
    ["stage", carriersPath, binA, symA, disA, placeA, binB, symB, disB, placeB, datPath, loaderAPath, loaderBPath, output] -> do
      union <- either (H.failH . show) pure . parseUnion =<< readInput carriersPath
      dat <- readInput datPath
      loaderA <- readInput loaderAPath
      loaderB <- readInput loaderBPath
      spansA <- either H.failH pure . placementSpans =<< readInput placeA
      spansB <- either H.failH pure . placementSpans =<< readInput placeB
      source <- either (H.failH . show) pure (canonicalSource dat loaderA loaderB)
      linked <- Linked <$> readInput binA <*> readInput symA <*> readInput disA <*> pure spansA <*> readInput binB <*> readInput symB <*> readInput disB <*> pure spansB
      admitted <- either (H.failH . show) pure (admit union source linked)
      staged <- either (H.failH . show) pure (stage admitted)
      exists <- doesPathExist output
      H.ensure (not exists) (output ++ " already exists")
      createDirectory output
      B.writeFile (output </> "candidate.DAT") (stagedDat staged)
      forM_ domains $ \domain -> B.writeFile (output </> ("loader-" ++ domainLabel domain ++ ".bin")) (stagedLoader staged domain)
      L.writeFile (output </> "contract.json") (J.encode (contractValue staged) <> L.singleton 10)
    _ -> H.failH "usage: stage-platform fit CARRIERS | stage CARRIERS BIN_A SYMBOLS_A DISASM_A PLACEMENT_A BIN_B SYMBOLS_B DISASM_B PLACEMENT_B SOURCE_DAT LOADER_A LOADER_B NEW_OUTPUT_DIRECTORY"
  where
    isLeft (Left _) = True
    isLeft _ = False
