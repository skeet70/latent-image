{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Run the saclac engine comparison and write the report the shared judge reads.
module Main (main) where

import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Emulation.Judge
import Emulation.Sha256 (sha256, sha256Hex)
import Emulation.SuiteAudit (MissingEntry (..))
import Numeric (showHex)
import Replacement.Saclac.Suite
import System.Directory (createDirectoryIfMissing)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitWith)
import System.FilePath ((</>))

hex :: Integral a => a -> String
hex value = "0x" ++ showHex (toInteger value) ""

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [runner, output, image, elf, wrongElf] -> do
      createDirectoryIfMissing True output
      imageHash <- sha256Hex . sha256 <$> B.readFile image
      ownedHash <- sha256Hex . sha256 <$> B.readFile elf
      wrongHash <- sha256Hex . sha256 <$> B.readFile wrongElf
      (results, controls) <- runSuite runner output image elf wrongElf
      let reportBytes = encode (object
            [ "suite" .= ("saclac-engine" :: String)
            , "source" .= sourceIdentity
            , "image_path" .= image
            , "firmware_sha256" .= imageHash
            , "owned_path" .= elf
            , "owned_sha256" .= ownedHash
            , "wrong_sha256" .= wrongHash
            , "runner" .= runner
            , "cases" .= [object
                [ "id" .= caseResultName result
                , "entry" .= hex (caseResultEntry result)
                , "extent" .= caseResultExtent result
                , "stock_instructions_inside" .= (evidenceTrace evidence + evidenceProvenance evidence)
                , "stock_trace_instructions" .= evidenceTrace evidence
                , "stock_provenance_instructions" .= evidenceProvenance evidence
                , "stock_instructions" .= evidenceInstructions evidence
                , "stock_status" .= evidenceStatus evidence
                , "states" .=
                    [object ["role" .= role, "machines" .= values]
                    | (role, values) <- caseResultStates result]
                , "differences" .= caseResultDifferences result
                ] | result <- results, let evidence = caseResultEvidence result]
            , "controls" .= [object
                [ "name" .= controlName control
                , "reported" .= controlReported control
                , "expected" .= controlExpected control
                , "differences" .= controlDifferences control
                ] | control <- controls]
            , "scope" .= ("The dispatcher at 0x00efca6c and machine 0's router at 0x00efcb88 over stock's tables: seeded scenarios stage a machine's row and state, enter the dispatcher or the router, and compare the return word, the recorded trace and action calls, and the two halfword arrays. Every stock action word and the trace entry is a recording stub, so the action bodies are import rows. The owned image replaces exactly those two stock extents." :: String)
            ])
          reportDigest = sha256Hex (sha256 (L.toStrict reportBytes))
          bound = [MissingEntry address size name | (address, size, name) <- boundEntries]
      report <- either fail pure (parseReport (L.toStrict reportBytes))
      let verdict = judgeReport (JudgeInput "saclac-engine" bound (Just (Firmware image imageHash)) reportDigest) report
      L.writeFile (output </> "saclac-report.json") reportBytes
      L.writeFile (output </> "case-table.json") (encode caseTableValue)
      resultDirectory <- lookupEnv "XM1_RUN_RESULT"
      case resultDirectory of
        Just directory -> do
          createDirectoryIfMissing True directory
          L.writeFile (directory </> "saclac-report.json") reportBytes
          L.writeFile (directory </> "case-table.json") (encode caseTableValue)
          L.writeFile (directory </> "verdict.json") (encode (verdictJson verdict))
        Nothing -> pure ()
      print (length results, length controls, length [() | control <- controls, controlReported control])
      putStrLn (verdictText verdict)
      exitWith (verdictExitCode (verdictStatus verdict))
    _ -> fail "usage: saclac-suite RUNNER OUTPUT IMAGE5 OWNED_ELF WRONG_ELF"
