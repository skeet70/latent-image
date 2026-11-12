{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (foldM)
import qualified Data.Aeson as J
import qualified Data.Aeson.Types as JT
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Data.Map.Strict as M
import Emulation.SchedulerPairPlan
import qualified FujiAF.FirstStage as F
import qualified FujiAF.Host as H
import Data.Word (Word32)
import Numeric (readHex)
import Replacement.RtosOwnFirst.Regions (Domain (..))
import System.Environment (getArgs)
import System.IO (IOMode (WriteMode), withBinaryFile)
import System.Directory (doesPathExist)

-- | The aggregate's carrier manifest rows this plan needs.
data CarrierRow = CarrierRow Word32 String
data DemandRow = DemandRow { demandRowSymbol :: String, demandRowBytes :: Word32 }
data Options = Options { optionSymbolsA :: Maybe FilePath, optionSymbolsB :: Maybe FilePath, optionCarriers :: Maybe FilePath }

emptyOptions :: Options
emptyOptions = Options Nothing Nothing Nothing

-- | Flags may precede, follow or interleave the positional arguments.
parseOptions :: Options -> [String] -> Either String (Options, [String])
parseOptions options = go options []
 where
  go current positional [] = Right (current, reverse positional)
  go current positional ("--symbols-a":value:rest) = go current { optionSymbolsA=Just value } positional rest
  go current positional ("--symbols-b":value:rest) = go current { optionSymbolsB=Just value } positional rest
  go current positional ("--carriers":value:rest) = go current { optionCarriers=Just value } positional rest
  go _ _ (flag:_) | take 2 flag == "--" = Left ("unknown option " ++ flag)
  go current positional (value:rest) = go current (value:positional) rest

parseHexText :: String -> Maybe Word32
parseHexText text = case text of
  '0':'x':digits | [(word,"")] <- readHex digits, word <= toInteger (maxBound :: Word32) -> Just (fromInteger word)
  _ -> Nothing

parseCarriers :: B.ByteString -> Either String (M.Map Domain ([CarrierRow],[DemandRow]))
parseCarriers bytes = do
  value <- J.eitherDecodeStrict' bytes
  JT.parseEither (J.withObject "carriers" $ \o -> do
    domains <- o J..: "domains"
    rowsA <- domains J..: "a" >>= domainRows
    rowsB <- domains J..: "b" >>= domainRows
    pure (M.fromList [(DomainA,rowsA),(DomainB,rowsB)])) value
 where
  domainRows = J.withObject "carriers domain" $ \o -> do
    entries <- o J..: "entries"
    demand <- o J..: "demand"
    rows <- mapM (J.withObject "carrier entry" $ \entry -> do
      text <- entry J..: "address"
      symbol <- entry J..: "symbol"
      address <- maybe (fail ("carrier address " ++ text)) pure (parseHexText text)
      pure (CarrierRow address symbol)) entries
    demands <- mapM (J.withObject "carrier demand" $ \row -> DemandRow <$> row J..: "symbol" <*> row J..: "bytes") demand
    pure (rows,demands)

-- | One domain's symbol extents, admitted against the manifest's entry addresses and demand sizes.
buildDomain :: [F.Symbol] -> ([CarrierRow],[DemandRow]) -> Either String (M.Map String Body, M.Map Word32 String)
buildDomain symbols (rows,demands) = foldM admit (bodies, M.empty) rows
 where
  bodies = M.fromList [(F.symbolName symbol, Body (F.symbolAddress symbol) (F.symbolSize symbol))
                      | symbol <- symbols, F.symbolSize symbol > 0]
  sizes = M.fromList [(demandRowSymbol row, demandRowBytes row) | row <- demands]
  admit (known, entries) (CarrierRow address symbol) =
    case (M.lookup symbol known, M.lookup symbol sizes) of
      (Just body, Just bytes) | bytes /= bodyBytes body -> Left ("carrier symbol " ++ symbol ++ " differs from its demand size")
      _ -> Right (known, M.insert address symbol entries)

aggregateOf :: FilePath -> FilePath -> FilePath -> IO (Either String Aggregate)
aggregateOf carriersPath symbolsAPath symbolsBPath = do
  carriers <- B.readFile carriersPath
  symbolsA <- B.readFile symbolsAPath
  symbolsB <- B.readFile symbolsBPath
  pure $ do
    rows <- parseCarriers carriers
    parsedA <- F.parseSymbols symbolsA
    parsedB <- F.parseSymbols symbolsB
    (bodiesA, entriesA) <- buildDomain parsedA (rows M.! DomainA)
    (bodiesB, entriesB) <- buildDomain parsedB (rows M.! DomainB)
    pure (Aggregate (M.fromList [(DomainA,bodiesA),(DomainB,bodiesB)])
      (M.fromList ([((DomainA,address),symbol) | (address,symbol) <- M.toList entriesA]
                ++ [((DomainB,address),symbol) | (address,symbol) <- M.toList entriesB])))

main :: IO ()
main = do
  args <- getArgs
  (options, positional) <- either H.failH pure (parseOptions emptyOptions args)
  case positional of
    regionsPath:roleText:datPath:loaderAPath:loaderBPath:output:additionalRegions -> do
      exists <- doesPathExist output
      H.ensure (not exists) "output already exists"
      role <- either H.failH pure (parseRole roleText)
      aggregate <- case (optionSymbolsA options,optionSymbolsB options,optionCarriers options) of
        (Nothing,Nothing,Nothing) -> pure Nothing
        (Just symbolsA,Just symbolsB,Just carriersPath) -> do
          bound <- either H.failH pure =<< aggregateOf carriersPath symbolsA symbolsB
          pure (Just bound)
        _ -> H.failH "the aggregate needs --symbols-a, --symbols-b and --carriers together"
      regions <- mapM (\path -> B.readFile path >>= either H.failH pure . parseCatalog) (regionsPath:additionalRegions)
      dat <- B.readFile datPath
      a <- B.readFile loaderAPath
      b <- B.readFile loaderBPath
      plan <- either H.failH pure (derivePlanWithAggregate aggregate role dat regions (M.fromList [(DomainA,a),(DomainB,b)]))
      withBinaryFile output WriteMode (\handle -> L.hPut handle (J.encode (planValue plan) <> L.singleton 10))
    _ -> H.failH "usage: scheduler-pair-plan REGIONS ROLE DAT EXPECTED_LOADER_A EXPECTED_LOADER_B NEW_PLAN_JSON [ADDITIONAL_REGIONS ...] [--symbols-a PATH --symbols-b PATH --carriers PATH]"
