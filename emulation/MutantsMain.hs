{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Generate wrong variants of an owned ELF: one sampled site, one word.
--
-- The extents come from the ELF's own function symbols: every @--function@
-- name and every function whose mangled name contains a @--crate@ name. Each
-- variant is written as a whole ELF so a suite can load it in place of the
-- owned binary, and @variants.json@ records the seed, generator version and
-- sites that regenerate the set.
module Main (main) where

import qualified ArmElf as E
import Control.Monad (forM)
import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.List (nub)
import Data.Word (Word32)
import Emulation.Mutation
import Emulation.ScenarioGeneration (GeneratorVersion (..), ScenarioSeed (..))
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (showHex)
import System.Directory (createDirectoryIfMissing)
import System.Environment (getArgs)
import System.FilePath ((</>))

hex :: Word32 -> String
hex value = "0x" ++ replicate (8 - length text) '0' ++ text
  where
    text = showHex value ""

main :: IO ()
main = do
  arguments <- getArgs
  elfPath <- required "--elf" arguments
  seed <- read <$> required "--seed" arguments
  count <- read <$> required "--count" arguments
  output <- required "--out" arguments
  let functions = values "--function" arguments
      crates = values "--crate" arguments
  bytes <- B.readFile elfPath
  elf <- either fail pure (E.parseElf bytes)
  extents <- either fail pure (extentsFor elf functions crates)
  sites <- either fail pure (eligibleSites elf extents)
  let located = [(extent, site) | site <- sites, Just extent <- [extentFor extents (siteAddress site)]]
  variants <- either (fail . renderMutationError) pure (planMutations (ScenarioSeed seed) count located)
  createDirectoryIfMissing True output
  written <- forM variants $ \variant -> do
    patched <- either fail pure (patchElf elf variant)
    let path = output </> ("variant-" ++ show (variantIndex variant) ++ ".elf")
    B.writeFile path patched
    pure (variant, path, sha256Hex (sha256 patched))
  L.writeFile (output </> "variants.json") (encode (object
    [ "schema" .= (1 :: Int)
    , "generator_version" .= versionText mutationVersion
    , "seed" .= seed
    , "count" .= count
    , "elf" .= elfPath
    , "elf_sha256" .= sha256Hex (sha256 bytes)
    , "extents" .= [object ["name" .= extentName extent, "address" .= hex (extentStart extent), "size" .= extentSize extent] | extent <- extents]
    , "sites" .= [object ["address" .= hex (siteAddress site), "word" .= hex (siteWord site), "kind" .= mutantKindLabel (siteKind site)] | site <- sites]
    , "variants" .= [object
        [ "seed" .= seed
        , "index" .= variantIndex variant
        , "extent" .= variantExtent variant
        , "address" .= hex (variantAddress variant)
        , "original" .= hex (variantOriginal variant)
        , "mutated" .= hex (variantMutated variant)
        , "kind" .= mutantKindLabel (variantKind variant)
        , "path" .= path
        , "sha256" .= digest
        ] | (variant, path, digest) <- written]
    ]))
  putStrLn ("mutants seed=" ++ show seed ++ " version=" ++ versionText mutationVersion
    ++ " extents=" ++ show (length extents) ++ " sites=" ++ show (length sites)
    ++ " variants=" ++ show (length written) ++ " out=" ++ output)

extentFor :: [Extent] -> Word32 -> Maybe Extent
extentFor extents address = case
  [ extent
  | extent <- extents
  , extentStart extent <= address
  , address < extentStart extent + extentSize extent
  ] of
  (extent : _) -> Just extent
  [] -> Nothing

versionText :: GeneratorVersion -> String
versionText (GeneratorVersion text) = text

required :: String -> [String] -> IO String
required flag arguments = case values flag arguments of
  [] -> fail (flag ++ " is required")
  found -> pure (last found)

values :: String -> [String] -> [String]
values flag arguments = nub [value | (key, value) <- zip arguments (drop 1 arguments), key == flag]
