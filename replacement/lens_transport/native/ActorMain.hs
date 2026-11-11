{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Emulation.Judge
import Emulation.Sha256 (sha256, sha256Hex)
import Emulation.SuiteAudit (MissingEntry (..))
import Numeric (showHex)
import Replacement.LensTransport.ActorSuite (CaseResult (..), ControlResult (..), ReplyResult (..), handlerExtents, runSuite, sourceIdentity)
import System.Directory (createDirectoryIfMissing)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitWith)
import System.FilePath ((</>))

hex :: Integral a => a -> String
hex value = "0x" ++ showHex (toInteger value) ""

main :: IO ()
main = do
  args <- getArgs
  case args of
    [runner,output,image,elf,codecElf,codecDefect,attachDefect,stopDefect] -> do
      createDirectoryIfMissing True output
      imageHash <- sha256Hex . sha256 <$> B.readFile image
      ownedHash <- sha256Hex . sha256 <$> B.readFile elf
      (results, controls, replies) <- runSuite runner output image elf codecElf codecDefect attachDefect stopDefect
      let reportBytes = encode (object
            [ "suite" .= ("lens-actor" :: String)
            , "source" .= sourceIdentity, "image_path" .= image, "image_sha256" .= imageHash
            , "owned_path" .= elf, "owned_sha256" .= ownedHash, "runner" .= runner
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
                [ "name" .= coName control, "case" .= coCase control, "expected" .= coExpected control
                , "reported" .= coReported control, "differences" .= coDifferences control
                , "diagnostic_differences" .= coDiagnostics control
                ] | control <- controls]
            , "replies" .= [object
                [ "name" .= rrName reply, "stock_code" .= hex (rrStockCode reply)
                , "owned_code" .= hex (rrOwnedCode reply)
                , "stock_actor_handler" .= hex (rrStockActorHandler reply)
                , "stock_actor_instructions" .= rrStockActorInstructions reply
                , "stock_codec_instructions" .= rrStockCodecInstructions reply
                , "differences" .= rrDifferences reply
                , "diagnostic_differences" .= rrDiagnostics reply
                ] | reply <- replies]
            , "scope" .= ("One request per case through the task 99 dispatcher at 0x006c2b6c with the kernel services and the owned actor against the same staged actor record: opcode routing over the 17 opcodes and six states, the effect ring, the shared publication block at 0x0095e1d0, the actor state word and the MMIO window. Each case records the stock handler the dispatcher selects and the instructions the stock side executed inside that handler's authenticated extent; two deliberately wrong actor variants and one wrong wire variant must be reported different. The reply family runs the stock reply parser at 0x006be368 and the owned wire entry on the same reply bytes, stages each decoder's status into that side's 0x730b actor record, and reports the stock parser's executed instructions for each scenario that carries reply bytes plus the stock response handler's instructions for every scenario (the timeout stages no reply bytes). Raw batch inputs and runner reports remain under the supplied output root. No physical driver, IRQ, deadline or lifecycle claim." :: String)
            ])
          reportDigest = sha256Hex (sha256 (L.toStrict reportBytes))
          bound = [MissingEntry address size ("handler-" ++ hex address) | (address, size, _) <- handlerExtents]
      report <- either fail pure (parseReport (L.toStrict reportBytes))
      let verdict = judgeReport (JudgeInput "lens-actor" bound (Just (Firmware image imageHash)) reportDigest) report
          passed = verdictStatus verdict == VerdictEqual
      L.writeFile (output </> "actor-report.json") reportBytes
      resultDirectory <- lookupEnv "XM1_RUN_RESULT"
      case resultDirectory of
        Just directory -> createDirectoryIfMissing True directory >> L.writeFile (directory </> "actor-report.json") reportBytes
        Nothing -> pure ()
      print (length results, passed, length controls, length replies)
      putStrLn (verdictText verdict)
      exitWith (verdictExitCode (verdictStatus verdict))
    _ -> fail "usage: actor-suite RUNNER OUTPUT IMAGE4 OWNED_ACTOR_ELF OWNED_CODEC_ELF DEFECT_REPLY_GATE_ELF DEFECT_NO_ATTACH_ELF DEFECT_STOP_MMIO_ELF"
