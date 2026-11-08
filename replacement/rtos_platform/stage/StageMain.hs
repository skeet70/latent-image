{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Control.Monad (forM_)
import Data.Aeson (Value, object, (.=))
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified FujiAF.Host as H
import Replacement.RtosOwnFirst.Regions (Extent (..), GuestAddress (..), domains, domainLabel, extentBytes, extentEnd, extentStart, hexText)
import Replacement.RtosPlatform.Artifact
import Replacement.RtosPlatform.Union
import System.Directory (createDirectory, doesPathExist)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))

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
    ["stage", carriersPath, binA, symA, disA, binB, symB, disB, datPath, loaderAPath, loaderBPath, output] -> do
      union <- either (H.failH . show) pure . parseUnion =<< readInput carriersPath
      dat <- readInput datPath
      loaderA <- readInput loaderAPath
      loaderB <- readInput loaderBPath
      source <- either (H.failH . show) pure (canonicalSource dat loaderA loaderB)
      linked <- Linked <$> readInput binA <*> readInput symA <*> readInput disA <*> readInput binB <*> readInput symB <*> readInput disB
      admitted <- either (H.failH . show) pure (admit union source linked)
      staged <- either (H.failH . show) pure (stage admitted)
      exists <- doesPathExist output
      H.ensure (not exists) (output ++ " already exists")
      createDirectory output
      B.writeFile (output </> "candidate.DAT") (stagedDat staged)
      forM_ domains $ \domain -> B.writeFile (output </> ("loader-" ++ domainLabel domain ++ ".bin")) (stagedLoader staged domain)
      L.writeFile (output </> "contract.json") (J.encode (contractValue staged) <> L.singleton 10)
    _ -> H.failH "usage: stage-platform fit CARRIERS | stage CARRIERS BIN_A SYMBOLS_A DISASM_A BIN_B SYMBOLS_B DISASM_B SOURCE_DAT LOADER_A LOADER_B NEW_OUTPUT_DIRECTORY"
  where
    isLeft (Left _) = True
    isLeft _ = False
