{-# LANGUAGE OverloadedStrings #-}
module Emulation.TaskWitnessCLI (TaskBody(..), Diagnostic(..), Operations(..), run) where

import qualified Data.Aeson as J
import Data.Bits (complement)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as L
import Emulation.TaskMemoryWire
import qualified Emulation.Image4Staging as Image4
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import System.Directory (canonicalizePath, doesPathExist)
import System.Environment (getArgs)
import System.Exit (ExitCode(..))
import System.FilePath ((</>))
import System.Process (proc)

data TaskBody = IdleBody | RootBody
newtype Diagnostic = Diagnostic String
data Operations = Operations
  { qualifyEvidence :: Role -> SourceFiles -> ObservedFiles -> Either Diagnostic J.Value
  , runControls :: Role -> SourceFiles -> ObservedFiles -> Either Diagnostic J.Value
  , compareEvidence :: SourceFiles -> ObservedFiles -> SourceFiles -> ObservedFiles -> Either Diagnostic J.Value }
data Mode = Qualify | RefusalControls
newtype InputFile = InputFile FilePath
newtype AnalysisRoot = AnalysisRoot FilePath
newtype ObservationDirectory = ObservationDirectory FilePath
newtype ScratchDirectory = ScratchDirectory FilePath
newtype OutputFile = OutputFile FilePath
data Common = Common InputFile InputFile AnalysisRoot InputFile InputFile InputFile InputFile ScratchDirectory

name :: TaskBody -> String
name IdleBody = "idle"
name RootBody = "root"
receiptFile :: TaskBody -> FilePath
receiptFile IdleBody = "idle-tasks.json"
receiptFile RootBody = "root-task.json"
decodeImage4 :: TaskBody -> InputFile -> ScratchDirectory -> B.ByteString -> IO B.ByteString
decodeImage4 body (InputFile decoder) (ScratchDirectory scratch) dat = do
  _ <- Image4.validateScratchDirectory scratch
  parsed <- either H.failH pure (U.parseContainer dat)
  let record = U.partRecord4 (U.parsedParts parsed); size = U.recDecodedSize (U.recHeader record)
  H.ensure (size == 0x282800) (name body ++ " checker requires the canonical image4 decoded extent")
  executable <- canonicalizePath decoder
  H.withTemporaryAt scratch (name body ++ "-check-decode-") $ \root -> do
    let input = root </> "record4.bin"; output = root </> "image4.bin"
    B.writeFile input (B.map complement (U.serializeRecord record))
    (timedOut,status,_,errors) <- H.runBoundedProcess (H.ProcessTimeoutMicros 600000000) (H.OutputCapBytes (1024*1024))
      (proc executable [input,"0",output,show size]) B.empty
    H.ensure (not timedOut && status == ExitSuccess) (name body ++ " checker image4 decode failed: " ++ C.unpack errors)
    H.readRegular 0x400000 output
staticSources :: TaskBody -> Common -> IO SourceFiles
staticSources body (Common (InputFile canonical) (InputFile sourceRun') (AnalysisRoot analysis) (InputFile rom) (InputFile observer) (InputFile linked) (InputFile symbols) _) = do
  original <- H.readRegular 0x4000000 canonical
  let limit = case body of IdleBody -> 16; RootBody -> 340
  SourceFiles original B.empty B.empty <$> H.readRegular 0x100000 sourceRun' <*> H.readRegular 0x10000 rom
    <*> H.readRegular 0x20000 (analysis </> "decode/loader-40000.bin") <*> H.readRegular 0x20000 (analysis </> "decode/loader-d0000.bin")
    <*> H.readRegular 0x400000 (analysis </> "lz/id4-0061c400.bin") <*> H.readRegular 0x10000000 observer
    <*> H.readRegular limit linked <*> H.readRegular 0x10000 symbols <*> pure B.empty
runtimeSources :: TaskBody -> InputFile -> InputFile -> ScratchDirectory -> SourceFiles -> IO SourceFiles
runtimeSources body decoder (InputFile dat) scratch base = do
  runtime <- H.readRegular 0x4000000 dat
  decoded <- decodeImage4 body decoder scratch runtime
  pure base {runDat=runtime,runDecoded=decoded}
observed :: TaskBody -> ObservationDirectory -> IO ObservedFiles
observed body (ObservationDirectory root) = ObservedFiles <$> H.readRegular 0x20000000 (root </> receiptFile body)
  <*> H.readRegular 0x20000000 (root </> "memory-watch.json") <*> H.readRegular 0x10000000 (root </> "reset.fujimod")
role :: TaskBody -> String -> IO Role
role _ "retained" = pure Retained
role _ "owned" = pure Owned
role body _ = H.failH ("invalid " ++ name body ++ " role")
mode :: TaskBody -> String -> IO Mode
mode _ "qualify" = pure Qualify
mode _ "refusal-controls" = pure RefusalControls
mode body _ = H.failH ("invalid " ++ name body ++ " checker mode")
writeOutput :: TaskBody -> OutputFile -> J.Value -> IO ()
writeOutput body (OutputFile path) value = do
  exists <- doesPathExist path
  H.ensure (not exists) (name body ++ " checker output already exists")
  L.writeFile path (J.encode value)
result :: Either Diagnostic a -> IO a
result = either (\(Diagnostic message) -> H.failH message) pure
run :: TaskBody -> Operations -> IO ()
run body operations = do
  arguments <- getArgs
  case arguments of
    [decoder,selected,program,canonical,dat,sourceRun',rom,analysis,observer,linked,symbols,contract,observations,scratch,output] -> do
      selectedMode <- mode body selected
      selectedRole <- role body program
      let common = Common (InputFile canonical) (InputFile sourceRun') (AnalysisRoot analysis) (InputFile rom) (InputFile observer) (InputFile linked) (InputFile symbols) (ScratchDirectory scratch)
      base <- staticSources body common
      source <- runtimeSources body (InputFile decoder) (InputFile dat) (ScratchDirectory scratch) base
      binding <- H.readRegular 0x10000 contract
      receipt <- observed body (ObservationDirectory observations)
      let bound = source {sourceContract=binding}
      value <- result (case selectedMode of Qualify -> qualifyEvidence operations selectedRole bound receipt; RefusalControls -> runControls operations selectedRole bound receipt)
      writeOutput body (OutputFile output) value
    [decoder,"compare",canonical,retainedDat,ownedDat,sourceRun',rom,analysis,observer,linked,symbols,contract,retainedDirectory,ownedDirectory,scratch,output] -> do
      let common = Common (InputFile canonical) (InputFile sourceRun') (AnalysisRoot analysis) (InputFile rom) (InputFile observer) (InputFile linked) (InputFile symbols) (ScratchDirectory scratch)
      base <- staticSources body common
      binding <- H.readRegular 0x10000 contract
      let bound = base {sourceContract=binding}
      firstSource <- runtimeSources body (InputFile decoder) (InputFile retainedDat) (ScratchDirectory scratch) bound
      secondSource <- runtimeSources body (InputFile decoder) (InputFile ownedDat) (ScratchDirectory scratch) bound
      first <- observed body (ObservationDirectory retainedDirectory)
      second <- observed body (ObservationDirectory ownedDirectory)
      value <- result (compareEvidence operations firstSource first secondSource second)
      writeOutput body (OutputFile output) value
    _ -> H.failH ("usage: " ++ name body ++ "-task-check qualify|refusal-controls ROLE SOURCE_DAT DAT RUN ROM ANALYSIS OBSERVER LINKED SYMBOLS CONTRACT OBS_DIR SCRATCH OUTPUT_JSON; or compare SOURCE_DAT RETAINED_DAT OWNED_DAT RUN ROM ANALYSIS OBSERVER LINKED SYMBOLS CONTRACT RETAINED_DIR OWNED_DIR SCRATCH OUTPUT_JSON")
