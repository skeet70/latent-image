{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified Data.Aeson as J
import qualified Data.ByteString.Lazy as L
import Emulation.StartupTaskControls (refusalControls)
import Emulation.StartupTaskWitness
import qualified FujiAF.Host as H
import System.Directory (doesPathExist)
import System.Environment (getArgs)
import System.FilePath ((</>))

data Mode = Qualify | RefusalControls
newtype InputFile = InputFile FilePath
newtype AnalysisRoot = AnalysisRoot FilePath
newtype ObservationDirectory = ObservationDirectory FilePath
newtype OutputFile = OutputFile FilePath

data Arguments = Arguments Mode InputFile InputFile InputFile AnalysisRoot InputFile ObservationDirectory OutputFile

arguments :: [String] -> IO Arguments
arguments [mode,dat,run,rom,analysis,observer,observations,output] = do
  selected <- case mode of "qualify" -> pure Qualify; "refusal-controls" -> pure RefusalControls; _ -> H.failH "invalid checker mode"
  pure (Arguments selected (InputFile dat) (InputFile run) (InputFile rom) (AnalysisRoot analysis) (InputFile observer) (ObservationDirectory observations) (OutputFile output))
arguments _ = H.failH "usage: startup-task-check qualify|refusal-controls DAT RUN ROM ANALYSIS_ROOT OBSERVER OBSERVATION_DIR OUTPUT_JSON"

main :: IO ()
main = do
  Arguments mode (InputFile dat) (InputFile run) (InputFile rom) (AnalysisRoot analysis) (InputFile observer) (ObservationDirectory observations) (OutputFile output) <- getArgs >>= arguments
  exists <- doesPathExist output
  if exists then H.failH "checker output already exists" else pure ()
  sources <- SourceFiles <$> H.readRegular 0x4000000 dat <*> H.readRegular 0x10000 run <*> H.readRegular 0x10000 rom
    <*> H.readRegular 0x20000 (analysis </> "decode/loader-40000.bin") <*> H.readRegular 0x20000 (analysis </> "decode/loader-d0000.bin")
    <*> H.readRegular 0x400000 (analysis </> "lz/id4-0061c400.bin") <*> H.readRegular 0x10000000 observer
  observed <- ObservedFiles <$> H.readRegular 0x8000000 (observations </> "startup-tasks.json")
    <*> H.readRegular 0x2000000 (observations </> "memory-watch.json") <*> H.readRegular 0x10000000 (observations </> "reset.fujimod")
  result <- case mode of
    Qualify -> do
      inputs <- either (H.failH . show) pure (admitInputs sources)
      checked <- either (H.failH . show) pure (admitReceipt inputs observed)
      pure (qualificationValue checked)
    RefusalControls -> either (H.failH . show) pure (refusalControls sources observed)
  L.writeFile output (J.encode result)
