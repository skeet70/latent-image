{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Emulation.Image4Staging as Image4
import qualified FujiAF.Host as H
import qualified Replacement.KernelInstall.Artifact as I
import qualified Replacement.StartupComposition.Artifact as C
import qualified Replacement.StartupDispatch.Artifact as D
import qualified Replacement.StartupConsumers.Source as S
import Replacement.StartupConsumers.Witness
import Replacement.StartupConsumers.WitnessControls (checkRefusals)
import System.Environment (getArgs)
import System.FilePath ((</>))

data PairMode = Compare | RefusalControls
newtype ObservationDirectory = ObservationDirectory FilePath
newtype OutputFile = OutputFile FilePath
data Mode = Qualify ObservationDirectory OutputFile | Triple PairMode ObservationDirectory ObservationDirectory ObservationDirectory OutputFile

observedFiles :: ObservationDirectory -> IO ObservedFiles
observedFiles (ObservationDirectory directory) = ObservedFiles
  <$> H.readRegular 0x40000000 (directory </> "consumers.json")
  <*> H.readRegular 0x10000000 (directory </> "reset.fujimod")

nativeFiles :: FilePath -> FilePath -> FilePath -> IO NativeFiles
nativeFiles image symbols contract = NativeFiles
  <$> H.readRegular 0x10000 image <*> H.readRegular 0x100000 symbols <*> H.readRegular 0x100000 contract

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    encoder:rebuild:decoder:modeText:datPath:imagePath:loaderAPath:loaderBPath:installerImagePath:installerSymbolsPath:installerContractPath:dispatcherImagePath:dispatcherSymbolsPath:dispatcherContractPath:installerDatPath:joinedDatPath:joinedContractPath:runPath:romPath:observerPath:scratchPath:paths -> do
      selected <- case (modeText,paths) of
        ("qualify",[directory,output]) -> pure (Qualify (ObservationDirectory directory) (OutputFile output))
        ("compare",[retained,installer,joined,output]) -> pure (Triple Compare (ObservationDirectory retained) (ObservationDirectory installer) (ObservationDirectory joined) (OutputFile output))
        ("refusal-controls",[retained,installer,joined,output]) -> pure (Triple RefusalControls (ObservationDirectory retained) (ObservationDirectory installer) (ObservationDirectory joined) (OutputFile output))
        _ -> H.failH usage
      sources <- S.SourceFiles <$> H.readRegular 0x4000000 datPath <*> H.readRegular 0x4000000 imagePath
        <*> H.readRegular 0x20000 loaderAPath <*> H.readRegular 0x20000 loaderBPath
      installer <- nativeFiles installerImagePath installerSymbolsPath installerContractPath
      dispatcher <- nativeFiles dispatcherImagePath dispatcherSymbolsPath dispatcherContractPath
      producer <- ProducerFiles <$> H.readRegular 0x10000000 observerPath <*> H.readRegular 0x100000 runPath
        <*> H.readRegular 0x1000000 romPath <*> H.readRegular 0x4000000 installerDatPath
        <*> H.readRegular 0x4000000 joinedDatPath <*> H.readRegular 0x100000 joinedContractPath
      installationSource <- admitted (I.canonicalSource (S.sourceDat sources) (S.sourceImage4 sources) (S.sourceLoaderA sources) (S.sourceLoaderB sources))
      dispatchSource <- admitted (D.canonicalSource (S.sourceDat sources) (S.sourceImage4 sources) (S.sourceLoaderA sources) (S.sourceLoaderB sources))
      compositionSource <- admitted (C.canonicalSource (C.SourceFiles (S.sourceDat sources) (S.sourceImage4 sources) (S.sourceLoaderA sources) (S.sourceLoaderB sources)))
      installation <- admitted (I.deriveArtifact (nativeImage installer) (nativeSymbols installer))
      dispatch <- admitted (D.deriveArtifact (nativeImage dispatcher) (nativeSymbols dispatcher))
      composition <- admitted (C.deriveArtifact (C.NativeFiles (nativeImage dispatcher) (nativeSymbols dispatcher)) (C.NativeFiles (nativeImage installer) (nativeSymbols installer)))
      scratch <- Image4.validateScratchDirectory scratchPath
      let tools = Image4.Tools encoder rebuild decoder
      expectedInstaller <- I.stageCandidate tools scratch (I.stageImage4 installationSource installation)
      expectedDispatcher <- D.stageCandidate tools scratch (D.stageImage4 dispatchSource dispatch)
      expectedJoined <- C.stageCandidate tools scratch (C.stageImage4 compositionSource composition)
      inputs <- admitted (admitInputs sources installer dispatcher producer expectedInstaller expectedDispatcher expectedJoined)
      (result,OutputFile outputPath) <- case selected of
        Qualify directory output -> do
          files <- observedFiles directory
          checked <- admitted (admitReceipt inputs files)
          pure (qualificationValue checked,output)
        Triple pairMode retainedDirectory installerDirectory joinedDirectory output -> do
          retained <- observedFiles retainedDirectory
          installed <- observedFiles installerDirectory
          joined <- observedFiles joinedDirectory
          checkedRetained <- admitted (admitReceipt inputs retained)
          checkedInstaller <- admitted (admitReceipt inputs installed)
          checkedJoined <- admitted (admitReceipt inputs joined)
          admitted (compareReceipts checkedRetained checkedInstaller checkedJoined)
          result <- case pairMode of
            Compare -> pure (J.object ["schema" J..= (1 :: Int),"comparison" J..= ("retained-installer-joined-agree" :: String)
              ,"roles" J..= map qualificationValue [checkedRetained,checkedInstaller,checkedJoined]])
            RefusalControls -> either H.failH pure (checkRefusals inputs retained installed joined)
          pure (result,output)
      B.writeFile outputPath (L.toStrict (J.encode result))
    _ -> H.failH usage
  where
    admitted :: Show error => Either error value -> IO value
    admitted = either (H.failH . show) pure

usage :: String
usage = "usage: startup-consumers-check ENCODER REBUILD DECODER qualify|compare|refusal-controls SOURCE_DAT IMAGE4 LOADER_A LOADER_B INSTALLER_BIN INSTALLER_SYMBOLS INSTALLER_CONTRACT DISPATCHER_BIN DISPATCHER_SYMBOLS DISPATCHER_CONTRACT INSTALLER_DAT JOINED_DAT JOINED_CONTRACT RUN_JSON BOOT_ROM OBSERVER SCRATCH_DIR [OBSERVATION_DIR|RETAINED_DIR INSTALLER_DIR JOINED_DIR] OUT_JSON"
