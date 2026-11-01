{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified Data.Aeson as J
import qualified Data.ByteString.Lazy as L
import qualified Emulation.Image4Staging as Image4
import qualified FujiAF.Host as H
import qualified Replacement.KernelInstall.Artifact as A
import Replacement.KernelInstall.Witness
import Replacement.KernelInstall.WitnessControls (checkRefusals)
import System.Environment (getArgs)
import System.FilePath ((</>))

data PairMode = Compare | RefusalControls
newtype ObservationDirectory = ObservationDirectory FilePath
newtype OutputFile = OutputFile FilePath
data RunMode = Qualify ObservationDirectory OutputFile | Pair PairMode ObservationDirectory ObservationDirectory OutputFile

observedFiles :: ObservationDirectory -> IO ObservedFiles
observedFiles (ObservationDirectory directory) = ObservedFiles
  <$> H.readRegular 0x4000000 (directory </> "installation.json")
  <*> H.readRegular 0x10000000 (directory </> "reset.fujimod")
  <*> H.readRegular 0x4000000 (directory </> "memory-watch.json")

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    encoder:rebuild:decoder:modeText:datPath:imagePath:loaderAPath:loaderBPath:linkedPath:symbolsPath:scratchPath:candidatePath:contractPath:observerPath:paths -> do
      selected <- case (modeText,paths) of
        ("qualify",[directory,outputPath]) -> pure (Qualify (ObservationDirectory directory) (OutputFile outputPath))
        ("compare",[retained,owned,outputPath]) -> pure (Pair Compare (ObservationDirectory retained) (ObservationDirectory owned) (OutputFile outputPath))
        ("refusal-controls",[retained,owned,outputPath]) -> pure (Pair RefusalControls (ObservationDirectory retained) (ObservationDirectory owned) (OutputFile outputPath))
        _ -> H.failH "invalid checker mode or observation paths"
      sources <- SourceFiles <$> H.readRegular 0x4000000 datPath <*> H.readRegular 0x4000000 imagePath
        <*> H.readRegular 0x20000 loaderAPath <*> H.readRegular 0x20000 loaderBPath
      native <- NativeFiles <$> H.readRegular 0x1000 linkedPath <*> H.readRegular 0x10000 symbolsPath
      producer <- ProducerFiles <$> H.readRegular 0x10000000 observerPath <*> H.readRegular 0x100000 contractPath <*> H.readRegular 0x4000000 candidatePath
      canonical <- either (H.failH . show) pure (A.canonicalSource (sourceDat sources) (sourceImage sources) (sourceA sources) (sourceB sources))
      artifact <- either (H.failH . show) pure (A.deriveArtifact (nativeImage native) (nativeSymbols native))
      scratch <- Image4.validateScratchDirectory scratchPath
      expected <- A.stageCandidate (Image4.Tools encoder rebuild decoder) scratch (A.stageImage4 canonical artifact)
      inputs <- either (H.failH . show) pure (admitInputs sources native expected producer)
      (result,OutputFile outputPath) <- case selected of
        Qualify directory outputFile -> do
          files <- observedFiles directory
          checked <- either (H.failH . show) pure (admitReceipt inputs files)
          pure (qualificationValue checked,outputFile)
        Pair pairMode retainedDirectory ownedDirectory outputFile -> do
          retained <- observedFiles retainedDirectory
          owned <- observedFiles ownedDirectory
          checkedRetained <- either (H.failH . show) pure (admitReceipt inputs retained)
          checkedOwned <- either (H.failH . show) pure (admitReceipt inputs owned)
          either (H.failH . show) pure (compareReceipts checkedRetained checkedOwned)
          result <- case pairMode of
            Compare -> pure (J.object ["schema" J..= (1 :: Int),"comparison" J..= ("retained-owned-agree" :: String)
              ,"retained" J..= qualificationValue checkedRetained,"owned" J..= qualificationValue checkedOwned])
            RefusalControls -> either H.failH pure (checkRefusals inputs retained owned)
          pure (result,outputFile)
      L.writeFile outputPath (J.encode result)
    _ -> H.failH "usage: kernel-install-check ENCODER REBUILD DECODER qualify|compare|refusal-controls SOURCE_DAT IMAGE4 LOADER_A LOADER_B LINKED_BIN SYMBOLS SCRATCH_DIR CANDIDATE_DAT CONTRACT OBSERVER [RETAINED_DIR OWNED_DIR|OBSERVATION_DIR] OUT_JSON"
