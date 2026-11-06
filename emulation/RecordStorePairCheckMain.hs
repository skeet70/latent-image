{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (SomeException, displayException, try)
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import Emulation.RecordStorePairCheck
import Emulation.Sha256 (sha256File, sha256Hex)
import System.Directory (doesPathExist)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))

need :: Bool -> String -> IO ()
need condition message = unless condition (ioError (userError message))
field :: T.Text -> Value -> IO Value
field name (Object fields) = maybe (ioError (userError ("missing " ++ T.unpack name))) pure (KM.lookup (K.fromText name) fields)
field _ _ = ioError (userError "expected object")
text :: Value -> IO T.Text
text (String value) = pure value
text _ = ioError (userError "expected string")
load :: FilePath -> IO Observation
load directory = do
  receipt <- B.readFile (directory </> "receipt.json")
  value <- either (ioError . userError) pure (A.eitherDecodeStrict' receipt)
  inputs <- field "inputs" value
  case inputs of
    Object bindings -> forM_ (KM.toList bindings) $ \(name,binding) -> do
      path <- field "path" binding >>= text
      expected <- field "sha256" binding >>= text
      actual <- sha256Hex <$> sha256File (T.unpack path)
      need (T.pack actual == expected) ("input file digest differs: " ++ K.toString name)
    _ -> ioError (userError "receipt inputs are not an object")
  files <- forM [minBound..maxBound] $ \file -> (,) file <$> B.readFile (directory </> sidecarName file)
  pure (Observation receipt (M.fromList files))

run :: [String] -> IO ()
run [stockDirectory,ownedDirectory,contractPath,linkedPath,symbolPath,routesPath,comparisonPath,outputPath] = do
  exists <- doesPathExist outputPath
  need (not exists) "output must be new"
  result <- try $ do
    bindings <- Bindings <$> B.readFile contractPath <*> B.readFile linkedPath <*> B.readFile symbolPath <*> B.readFile routesPath
    comparison <- B.readFile comparisonPath >>= either (ioError . userError) pure . parseComparisonContract
    forM_ (comparisonEvidencePath comparison) $ \path -> do
      evidence <- B.readFile path
      either (ioError . userError) pure (validateComparisonEvidence comparison evidence)
    stock <- load stockDirectory
    owned <- load ownedDirectory
    case (qualify Stock bindings stock,qualify Owned bindings owned) of
      (Left failure,_) -> pure (False,object ["schema" .= (2::Int),"outcome" .= ("refused"::T.Text),"role" .= ("stock"::T.Text),"error" .= failure])
      (_,Left failure) -> pure (False,object ["schema" .= (2::Int),"outcome" .= ("refused"::T.Text),"role" .= ("owned"::T.Text),"error" .= failure])
      (Right retained,Right candidate) -> do
        let compared = comparePair comparison retained candidate
        pure (either (const False) (const True) compared,comparisonValue comparison retained candidate compared)
  let (matched,report) = case result of
        Right successful -> successful
        Left failure -> (False,object ["schema" .= (2::Int),"outcome" .= ("refused"::T.Text),"error" .= displayException (failure::SomeException)])
  BL.writeFile outputPath (A.encode report <> "\n")
  when (not matched) exitFailure
run _ = ioError (userError "usage: record-store-pair-check STOCK_DIR OWNED_DIR CONTRACT_JSON LINKED_BIN SYMBOLS ROUTES_JSON COMPARISON_JSON OUTPUT_JSON")

main :: IO ()
main = getArgs >>= run
