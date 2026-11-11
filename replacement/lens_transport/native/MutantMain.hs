{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE GHC2021 #-}

-- | Run the lens actor suite against wrong variants of the owned actor ELF.
--
-- The variants come from "Emulation.Mutation": sampled from the actor crate's
-- own function extents with a recorded seed. Each is a whole ELF that the
-- suite loads in place of the owned binary, so every case runs unchanged.
-- The report is the suite's own shape: the base comparison's cases, and one
-- control row per variant with the variant's address and kind. The judge
-- lists an undetected variant as a survivor.
module Main (main) where

import qualified ArmElf as E
import Control.Monad (forM)
import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.Word (Word32)
import Emulation.Judge
import Emulation.Mutation
import Emulation.ScenarioGeneration (GeneratorVersion (..), ScenarioSeed (..))
import Emulation.Sha256 (sha256, sha256Hex)
import Emulation.SuiteAudit (MissingEntry (..))
import Numeric (showHex)
import Replacement.LensTransport.ActorSuite
  ( CaseResult (..)
  , handlerExtents
  , runMutants
  , sourceIdentity
  )
import System.Directory (createDirectoryIfMissing)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitWith)
import System.FilePath ((</>))

hex :: Integral a => a -> String
hex value = "0x" ++ showHex (toInteger value) ""

-- | The actor's own functions: the crate's mangled symbols and its four
-- exported entries.
crate :: String
crate = "xm1_lens_actor"

entries :: [String]
entries = ["lens_actor_request", "lens_actor_wake", "lens_actor_timer", "lens_actor_device_callback"]

main :: IO ()
main = do
  args <- getArgs
  case args of
    [runner, output, image, elf, seedText, countText] -> do
      let seed = read seedText
          count = read countText
      createDirectoryIfMissing True output
      imageHash <- sha256Hex . sha256 <$> B.readFile image
      elfBytes <- B.readFile elf
      parsed <- either fail pure (E.parseElf elfBytes)
      extents <- either fail pure (extentsFor parsed entries [crate])
      sites <- either fail pure (eligibleSites parsed extents)
      let located = [(extent, site) | site <- sites, Just extent <- [extentAt extents (siteAddress site)]]
      variants <- either (fail . renderMutationError) pure (planMutations (ScenarioSeed seed) count located)
      let variantDirectory = output </> "variants"
      createDirectoryIfMissing True variantDirectory
      variantFiles <- forM variants $ \variant -> do
        patched <- either fail pure (patchElf parsed variant)
        let path = variantDirectory </> ("variant-" ++ show (variantIndex variant) ++ ".elf")
        B.writeFile path patched
        pure (variant, path, sha256Hex (sha256 patched))
      let manifest = encode (object
            [ "schema" .= (1 :: Int)
            , "generator_version" .= versionText mutationVersion
            , "seed" .= seed
            , "count" .= count
            , "elf" .= elf
            , "elf_sha256" .= sha256Hex (sha256 elfBytes)
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
                ] | (variant, path, digest) <- variantFiles]
            ])
      L.writeFile (output </> "variants.json") manifest
      (results, outcomes) <- runMutants runner (output </> "cases") image elf [(variant, path) | (variant, path, _) <- variantFiles]
      let reportBytes = encode (object
            [ "suite" .= ("lens-actor" :: String)
            , "source" .= sourceIdentity, "image_path" .= image, "image_sha256" .= imageHash
            , "owned_path" .= elf, "owned_sha256" .= sha256Hex (sha256 elfBytes), "runner" .= runner
            , "cases" .= [object
                [ "id" .= crId result
                , "stock_handler" .= hex (evidenceHandler evidence)
                , "stock_extent" .= evidenceExtent evidence
                , "stock_handler_instructions" .= (evidenceTrace evidence + evidenceProvenance evidence)
                , "stock_trace_instructions" .= evidenceTrace evidence
                , "stock_provenance_instructions" .= evidenceProvenance evidence
                , "stock_instructions" .= evidenceInstructions evidence
                , "stock_status" .= evidenceStatus evidence
                , "differences" .= crDifferences result
                ] | result <- results, let evidence = crEvidence result]
            , "controls" .= [object
                [ "name" .= reportControlName control
                , "reported" .= reportControlDetected control
                , "expected" .= reportControlExpected control
                , "differences" .= reportControlDifferences control
                , "address" .= hex (variantAddress variant)
                , "kind" .= mutantKindLabel (variantKind variant)
                , "index" .= variantIndex variant
                , "original" .= hex (variantOriginal variant)
                , "mutated" .= hex (variantMutated variant)
                , "cases" .= outcomeCases outcome
                ] | outcome <- outcomes, let control = variantControl outcome, let variant = outcomeVariant outcome]
            , "mutants" .= object
                [ "generator_version" .= versionText mutationVersion
                , "seed" .= seed
                , "count" .= count
                , "extents" .= length extents
                , "sites" .= length sites
                , "detected" .= length [() | outcome <- outcomes, outcomeDetected outcome]
                , "survivors" .= [reportControlName (variantControl outcome) | outcome <- outcomes, not (outcomeDetected outcome)]
                ]
            , "scope" .= ("The actor suite's cases run against each wrong variant of the owned actor ELF in place of the owned binary: one word changed, either a conditional branch's condition inverted or a store replaced by a no-op under its own condition. The stock side runs once per case and is reused. A variant some case reports different is detected; a variant no case reports is a survivor, listed with its address and kind. The stock extents and the image4 identity are the suite's own." :: String)
            ])
          reportDigest = sha256Hex (sha256 (L.toStrict reportBytes))
          bound = [MissingEntry address size ("handler-" ++ hex address) | (address, size, _) <- handlerExtents]
      report <- either fail pure (parseReport (L.toStrict reportBytes))
      let verdict = judgeReport (JudgeInput "lens-actor" bound (Just (Firmware image imageHash)) reportDigest) report
      L.writeFile (output </> "mutant-report.json") reportBytes
      resultDirectory <- lookupEnv "XM1_RUN_RESULT"
      case resultDirectory of
        Just directory -> do
          createDirectoryIfMissing True directory
          L.writeFile (directory </> "mutant-report.json") reportBytes
          L.writeFile (directory </> "mutant-verdict.json") (encode (verdictJson verdict))
          L.writeFile (directory </> "variants.json") manifest
        Nothing -> pure ()
      print (length results, length variants, length [() | outcome <- outcomes, outcomeDetected outcome])
      putStrLn (verdictText verdict)
      exitWith (verdictExitCode (verdictStatus verdict))
    _ -> fail "usage: actor-mutants RUNNER OUTPUT IMAGE4 OWNED_ACTOR_ELF SEED COUNT"

extentAt :: [Extent] -> Word32 -> Maybe Extent
extentAt extents address = case
  [ extent
  | extent <- extents
  , extentStart extent <= address
  , address < extentStart extent + extentSize extent
  ] of
  (extent : _) -> Just extent
  [] -> Nothing

versionText :: GeneratorVersion -> String
versionText (GeneratorVersion text) = text
