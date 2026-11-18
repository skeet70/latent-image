{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main where

import qualified ArmElf as E
import Control.Monad (forM)
import Data.Aeson (encode, object, (.=))
import Data.Aeson.Types (Pair)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.Word (Word32, Word64)
import Emulation.Mutation
import Emulation.ScenarioGeneration (GeneratorVersion(..), ScenarioSeed(..))
import Emulation.Sha256 (sha256, sha256Hex)
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.FixedImage.Layout as Fixed
import System.Directory (createDirectory)
import System.Environment (getArgs)
import System.FilePath ((</>))
import Text.Read (readMaybe)

newtype FunctionName = FunctionName String
newtype VariantCount = VariantCount Int

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["site", elfPath, addressText, output] -> do
      address <- maybe (H.failH "invalid site address") pure (readMaybe addressText :: Maybe Word32)
      chosen elfPath address output
    [elfPath, functions, seedText, countText, output] -> do
      seed <- maybe (H.failH "invalid mutation seed") (pure . ScenarioSeed) (readMaybe seedText :: Maybe Word64)
      count <- case readMaybe countText of
        Just value | value > 0 && value <= mutationCountLimit -> pure (VariantCount value)
        _ -> H.failH "invalid mutation count"
      generate elfPath (map FunctionName (splitCommas functions)) seed count output
    _ -> H.failH "usage: time-mutants ELF FUNCTION[,FUNCTION...] SEED COUNT OUTPUT_DIRECTORY | time-mutants site ELF ADDRESS OUTPUT_DIRECTORY"

splitCommas :: String -> [String]
splitCommas text = case break (== ',') text of
  (name, ',' : rest) -> name : splitCommas rest
  (name, _) -> [name]

-- | Samples COUNT variants across the eligible sites of the named functions.
generate :: FilePath -> [FunctionName] -> ScenarioSeed -> VariantCount -> FilePath -> IO ()
generate elfPath functions seed@(ScenarioSeed seedWord) (VariantCount count) output = do
  (elf, bytes) <- readElf elfPath
  extents <- either H.failH pure (extentsFor elf [name | FunctionName name <- functions] [])
  sites <- either H.failH pure (eligibleSites elf extents)
  let located = [(extent, site) | extent <- extents, site <- sites, extentStart extent <= siteAddress site, siteAddress site < extentStart extent + extentSize extent]
  variants <- either (H.failH . renderMutationError) pure (planMutations seed count located)
  let GeneratorVersion version = mutationVersion
  emit elf bytes output variants
    ["seed" .= seedWord, "generator_version" .= version, "functions" .= [name | FunctionName name <- functions], "eligible_sites" .= length sites]

-- | Emits the one variant of an eligible site in the time-service functions, chosen by address.
chosen :: FilePath -> Word32 -> FilePath -> IO ()
chosen elfPath address output = do
  (elf, bytes) <- readElf elfPath
  extents <- either H.failH pure (extentsFor elf [] ["xm1_time"])
  sites <- either H.failH pure (eligibleSites elf extents)
  (extent, site) <- case [(extent, site) | extent <- extents, site <- sites, siteAddress site == address, extentStart extent <= address, address < extentStart extent + extentSize extent] of
    [found] -> pure found
    _ -> H.failH ("no eligible time-service site at " ++ show address)
  let variant = Variant
        { variantSeed = ScenarioSeed 0, variantIndex = 0, variantExtent = extentName extent
        , variantAddress = address, variantOriginal = siteWord site
        , variantMutated = mutatedWord (siteKind site) (siteWord site), variantKind = siteKind site }
      GeneratorVersion version = mutationVersion
  emit elf bytes output [variant] ["site" .= address, "generator_version" .= version, "function" .= extentName extent]

readElf :: FilePath -> IO (E.Elf, B.ByteString)
readElf path = do
  bytes <- H.readRegular (4 * 1024 * 1024) path
  elf <- either H.failH pure (E.parseElf bytes)
  pure (elf, bytes)

emit :: E.Elf -> B.ByteString -> FilePath -> [Variant] -> [Pair] -> IO ()
emit elf bytes output variants header = do
  let symbol name = case [E.symbolValue value | value <- E.elfSymbols elf, E.symbolName value == name, E.symbolSection value /= E.Undefined] of
        [value] -> pure value
        _ -> H.failH ("missing or duplicate symbol " ++ name)
  first <- symbol "__fixed_body_start"
  end <- symbol "__fixed_body_end"
  H.ensure (first == Fixed.imageBase + Fixed.headerBytes && end > first && end <= Fixed.imageLimit) "fixed body bounds differ"
  body <- B.pack <$> either H.failH pure (traverse (maybe (Left "fixed body has a file gap") Right . E.byteAt elf) [first..end-1])
  createDirectory output
  rows <- forM variants $ \variant -> do
    patched <- either H.failH pure (patchElf elf variant)
    let offset = fromIntegral (variantAddress variant - first)
        name = "variant-" ++ show (variantIndex variant)
        changed = B.take offset body <> U.putWord32 (variantMutated variant) <> B.drop (offset + 4) body
    H.ensure (variantAddress variant >= first && offset + 4 <= B.length body && U.word32At body offset == variantOriginal variant) "mutation leaves fixed body"
    B.writeFile (output </> name ++ ".elf") patched
    B.writeFile (output </> name ++ ".bin") changed
    pure (object ["name" .= name, "address" .= variantAddress variant, "function" .= variantExtent variant, "original" .= variantOriginal variant, "mutated" .= variantMutated variant, "kind" .= mutantKindLabel (variantKind variant), "body_sha256" .= sha256Hex (sha256 changed)])
  B.writeFile (output </> "variants.json") (L.toStrict (encode (object
    (["schema" .= (1 :: Int), "source_elf_sha256" .= sha256Hex (sha256 bytes), "source_body_sha256" .= sha256Hex (sha256 body), "variants" .= rows] ++ header))))
