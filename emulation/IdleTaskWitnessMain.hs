{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified Data.Aeson as J
import Data.Bits (complement)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as L
import Emulation.IdleTaskControls (refusalControls)
import Emulation.IdleTaskWitness
import Emulation.IdleTaskWire (Role(..))
import qualified Emulation.Image4Staging as Image4
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import System.Directory (canonicalizePath, doesPathExist)
import System.Environment (getArgs)
import System.Exit (ExitCode(..))
import System.FilePath ((</>))
import System.Process (proc)

data Mode = Qualify | RefusalControls deriving (Eq)
newtype InputFile = InputFile FilePath
newtype AnalysisRoot = AnalysisRoot FilePath
newtype ObservationDirectory = ObservationDirectory FilePath
newtype ScratchDirectory = ScratchDirectory FilePath
newtype OutputFile = OutputFile FilePath
data Common = Common InputFile InputFile AnalysisRoot InputFile InputFile InputFile InputFile ScratchDirectory

decodeImage4 :: InputFile -> ScratchDirectory -> B.ByteString -> IO B.ByteString
decodeImage4 (InputFile decoder) (ScratchDirectory scratch) dat = do
  _ <- Image4.validateScratchDirectory scratch
  parsed <- either H.failH pure (U.parseContainer dat)
  let record = U.partRecord4 (U.parsedParts parsed); size = U.recDecodedSize (U.recHeader record)
  H.ensure (size == 0x282800) "idle checker requires the canonical image4 decoded extent"
  executable <- canonicalizePath decoder
  H.withTemporaryAt scratch "idle-check-decode-" $ \root -> do
    let input = root </> "record4.bin"; output = root </> "image4.bin"
    B.writeFile input (B.map complement (U.serializeRecord record))
    (timedOut,status,_,errors) <- H.runBoundedProcess (H.ProcessTimeoutMicros 600000000) (H.OutputCapBytes (1024*1024))
      (proc executable [input,"0",output,show size]) B.empty
    H.ensure (not timedOut && status == ExitSuccess) ("idle checker image4 decode failed: " ++ C.unpack errors)
    H.readRegular 0x400000 output

sources :: InputFile -> InputFile -> Common -> IO SourceFiles
sources decoder (InputFile dat) (Common (InputFile canonical) (InputFile run) (AnalysisRoot analysis) (InputFile rom) (InputFile observer) (InputFile linked) (InputFile symbols) (ScratchDirectory scratch)) = do
  original <- H.readRegular 0x4000000 canonical
  runtime <- H.readRegular 0x4000000 dat
  decoded <- decodeImage4 decoder (ScratchDirectory scratch) runtime
  SourceFiles original runtime decoded <$> H.readRegular 0x100000 run <*> H.readRegular 0x10000 rom
    <*> H.readRegular 0x20000 (analysis </> "decode/loader-40000.bin") <*> H.readRegular 0x20000 (analysis </> "decode/loader-d0000.bin")
    <*> H.readRegular 0x400000 (analysis </> "lz/id4-0061c400.bin") <*> H.readRegular 0x10000000 observer
    <*> H.readRegular 16 linked <*> H.readRegular 0x10000 symbols <*> pure B.empty
observed :: ObservationDirectory -> IO ObservedFiles
observed (ObservationDirectory root) = ObservedFiles <$> H.readRegular 0x20000000 (root </> "idle-tasks.json")
  <*> H.readRegular 0x20000000 (root </> "memory-watch.json") <*> H.readRegular 0x10000000 (root </> "reset.fujimod")
role :: String -> IO Role
role "retained" = pure Retained
role "owned" = pure Owned
role _ = H.failH "invalid idle role"
mode :: String -> IO Mode
mode "qualify" = pure Qualify
mode "refusal-controls" = pure RefusalControls
mode _ = H.failH "invalid idle checker mode"
writeOutput :: OutputFile -> J.Value -> IO ()
writeOutput (OutputFile path) value = do
  exists <- doesPathExist path
  H.ensure (not exists) "idle checker output already exists"
  L.writeFile path (J.encode value)
main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [decoder,selected,program,canonical,dat,run,rom,analysis,observer,linked,symbols,contract,observations,scratch,output] -> do
      selectedMode <- mode selected
      selectedRole <- role program
      let common = Common (InputFile canonical) (InputFile run) (AnalysisRoot analysis) (InputFile rom) (InputFile observer) (InputFile linked) (InputFile symbols) (ScratchDirectory scratch)
      source <- sources (InputFile decoder) (InputFile dat) common
      binding <- H.readRegular 0x10000 contract
      receipt <- observed (ObservationDirectory observations)
      let bound = source {sourceContract=binding}
      value <- case selectedMode of
        Qualify -> do
          inputs <- either (H.failH . show) pure (admitInputs selectedRole bound)
          qualified <- either (H.failH . show) pure (admitReceipt inputs receipt)
          pure (qualificationValue qualified)
        RefusalControls -> either (H.failH . show) pure (refusalControls selectedRole bound receipt)
      writeOutput (OutputFile output) value
    [decoder,"compare",canonical,retainedDat,ownedDat,run,rom,analysis,observer,linked,symbols,contract,retainedDirectory,ownedDirectory,scratch,output] -> do
      let common = Common (InputFile canonical) (InputFile run) (AnalysisRoot analysis) (InputFile rom) (InputFile observer) (InputFile linked) (InputFile symbols) (ScratchDirectory scratch)
      binding <- H.readRegular 0x10000 contract
      firstSource <- sources (InputFile decoder) (InputFile retainedDat) common
      secondSource <- sources (InputFile decoder) (InputFile ownedDat) common
      first <- observed (ObservationDirectory retainedDirectory)
      second <- observed (ObservationDirectory ownedDirectory)
      retainedInputs <- either (H.failH . show) pure (admitInputs Retained firstSource {sourceContract=binding})
      ownedInputs <- either (H.failH . show) pure (admitInputs Owned secondSource {sourceContract=binding})
      retained <- either (H.failH . show) pure (admitReceipt retainedInputs first)
      owned <- either (H.failH . show) pure (admitReceipt ownedInputs second)
      value <- either (H.failH . show) pure (compareReceipts retained owned)
      writeOutput (OutputFile output) value
    _ -> H.failH "usage: idle-task-check qualify|refusal-controls ROLE SOURCE_DAT DAT RUN ROM ANALYSIS OBSERVER LINKED SYMBOLS CONTRACT OBS_DIR SCRATCH OUTPUT_JSON; or compare SOURCE_DAT RETAINED_DAT OWNED_DAT RUN ROM ANALYSIS OBSERVER LINKED SYMBOLS CONTRACT RETAINED_DIR OWNED_DIR SCRATCH OUTPUT_JSON"
