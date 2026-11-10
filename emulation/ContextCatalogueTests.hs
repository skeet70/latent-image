{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value (..), (.:), withObject)
import Data.Aeson.Types (parseMaybe)
import qualified Data.ByteString as B
import Data.Either (isLeft)
import Emulation.ContextCatalogue
import Emulation.LoaderCode (loaderSpans)
import Emulation.WatchObservation (WatchAddress, watchAddress, watchAddressValue)
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  inputs <- case arguments of
    [loaderA, loaderB, symbolsA, symbolsB] -> OwnedUnionCatalogueInputs <$> B.readFile loaderA <*> B.readFile loaderB <*> B.readFile symbolsA <*> B.readFile symbolsB
    _ -> fail "usage: context-catalogue-tests LOADER_A LOADER_B SYMBOLS_A SYMBOLS_B"
  catalogue <- either fail pure (bindOwnedUnionContextCatalogue inputs)
  check "owned catalogue tag differs" (contextCatalogueTag catalogue == "owned_union")
  let claim = parseMaybe (withObject "catalogue provenance" $ \fields ->
        (,) <$> fields .: "runtime_context_observed" <*> fields .: "installed_lifetime_admitted") (contextCatalogueProvenance catalogue)
  check "static catalogue claimed runtime admission" (claim == Just (False, False))
  let loaderA = catalogueLoaderA inputs
      loaderB = catalogueLoaderB inputs
      sources = slices (watchAddress 0x40000) loaderA ++ slices (watchAddress 0xd0000) loaderB
  check "owned immutable source roundtrip refused" (not (isLeft (parseContextCatalogue (String "owned_union") sources)))
  check "changed primary loader accepted" (isLeft (bindOwnedUnionContextCatalogue (inputs { catalogueLoaderA = mutate 0 loaderA })))
  check "changed secondary writer accepted" (isLeft (bindOwnedUnionContextCatalogue (inputs { catalogueLoaderB = mutate (0xd3d04 - 0xd0000) loaderB })))
  check "different symbol table accepted" (isLeft (bindOwnedUnionContextCatalogue (inputs { catalogueSymbolsA = mutate 0 (catalogueSymbolsA inputs) })))
  check "secondary loader inferred by relocation accepted" (isLeft (bindOwnedUnionContextCatalogue (inputs { catalogueLoaderB = loaderA })))
  check "owned loaders accepted as stock" (isLeft (bindStockContextCatalogue loaderA loaderB))
  check "partial immutable sources accepted" (isLeft (parseContextCatalogue (String "owned_union") (drop 1 sources)))
  check "changed immutable instruction accepted" (isLeft (parseContextCatalogue (String "owned_union")
    (slices (watchAddress 0x40000) (mutate (0x43d48 - 0x40000) loaderA) ++ slices (watchAddress 0xd0000) loaderB)))
  check "raw catalogue override accepted" (isLeft (parseContextCatalogue (Object mempty) sources))
  putStrLn "actual owned-union context catalogue source controls passed"
  where
    check name condition = unless condition (fail name)
    mutate offset bytes = B.take offset bytes <> B.singleton (B.index bytes offset + 1) <> B.drop (offset + 1) bytes

slices :: WatchAddress -> B.ByteString -> [ContextSourceSlice]
slices start bytes =
  [ContextSourceSlice (watchAddress (fromIntegral low)) (B.take (fromIntegral (high - low)) (B.drop (fromIntegral (low - base)) bytes))
  | (low, high) <- loaderSpans base (fromIntegral (B.length bytes))]
  where base = fromIntegral (watchAddressValue start)
