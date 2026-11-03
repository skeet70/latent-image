module Emulation.Image4Staging
  ( Tools (..), ScratchDirectory, validateScratchDirectory, scratchDirectoryPath, StagedCandidate, runImage4Staging
  , candidateDat, candidateSha256, candidateImage4Sha256
  ) where

import Data.Bits (complement)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (tails)
import Emulation.FirstStageWitness (sha256Hex)
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import System.Directory (canonicalizePath, doesDirectoryExist)
import System.Environment (getEnvironment)
import System.Exit (ExitCode (..))
import System.FilePath ((</>), splitDirectories)
import System.Process (CreateProcess (..), proc)

newtype CandidateDat = CandidateDat B.ByteString
newtype DecodedImage4 = DecodedImage4 B.ByteString
data StagedCandidate = StagedCandidate CandidateDat DecodedImage4
newtype ScratchDirectory = ScratchDirectory FilePath

data Tools = Tools
  { encoderPath :: FilePath
  , rebuildPath :: FilePath
  , decoderPath :: FilePath
  }

candidateDat :: StagedCandidate -> B.ByteString
candidateDat (StagedCandidate (CandidateDat bytes) _) = bytes

candidateSha256 :: StagedCandidate -> String
candidateSha256 = sha256Hex . candidateDat

candidateImage4Sha256 :: StagedCandidate -> String
candidateImage4Sha256 (StagedCandidate _ (DecodedImage4 bytes)) = sha256Hex bytes

runTool :: FilePath -> FilePath -> [String] -> IO ()
runTool root executable arguments = do
  absolute <- canonicalizePath executable
  inherited <- getEnvironment
  let process = (proc absolute arguments)
        { cwd = Just root
        , env = Just (("TMPDIR",root) : filter ((/= "TMPDIR") . fst) inherited)
        }
  (stopped,status,_,errors) <- H.runBoundedProcess (H.ProcessTimeoutMicros 600000000) (H.OutputCapBytes (1024*1024)) process B.empty
  H.ensure (not stopped && status == ExitSuccess) ("image4 staging tool failed: " ++ C.unpack errors)

validateScratchDirectory :: FilePath -> IO ScratchDirectory
validateScratchDirectory configured = do
  exists <- doesDirectoryExist configured
  H.ensure exists "image4 staging scratch directory does not exist"
  resolved <- canonicalizePath configured
  H.ensure (any ((== ["private-data","scratch"]) . take 2) (tails (splitDirectories resolved)))
    "image4 staging scratch directory is outside private-data/scratch"
  pure (ScratchDirectory resolved)

scratchDirectoryPath :: ScratchDirectory -> FilePath
scratchDirectoryPath (ScratchDirectory path) = path

-- The second DAT carries startup edits; its image4 record is still canonical.
runImage4Staging :: Tools -> ScratchDirectory -> B.ByteString -> B.ByteString -> B.ByteString -> IO StagedCandidate
runImage4Staging tools (ScratchDirectory scratch) canonical base decoded = H.withTemporaryAt scratch "image4-stage-" $ \root -> do
  H.ensure (sha256Hex canonical == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490")
    "image4 staging requires the canonical X-M1 1.50 DAT"
  source <- either H.failH pure (U.parseContainer canonical)
  staged <- either H.failH pure (U.parseContainer base)
  let sourceParts = U.parsedParts source
      baseParts = U.parsedParts staged
      originalRecord = U.partRecord4 sourceParts
      sourceSize = U.recDecodedSize (U.recHeader originalRecord)
      originalRecordPath = root </> "canonical-record4.bin"
      originalDecodedPath = root </> "canonical-image4.bin"
      basePath = root </> "startup.DAT"
      decodedPath = root </> "image4.bin"
      recordPath = root </> "record4.bin"
      finalPath = root </> "candidate.DAT"
      carriedPath = root </> "carried-record4.bin"
      carriedDecodedPath = root </> "carried-image4.bin"
      segment4 parts = filter ((== 4) . U.segId) (U.partSegments parts)
  H.ensure (U.serializeRecord (U.partRecord4 baseParts) == U.serializeRecord originalRecord
    && U.partGap45 baseParts == U.partGap45 sourceParts && segment4 baseParts == segment4 sourceParts)
    "image4 staging base has changed the canonical record window or placement"
  H.ensure (fromIntegral sourceSize == B.length decoded) "image4 staging changes the decoded extent"
  B.writeFile originalRecordPath (B.map complement (U.serializeRecord originalRecord))
  runTool root (decoderPath tools) [originalRecordPath,"0",originalDecodedPath,show sourceSize]
  originalDecoded <- H.readRegular (16*1024*1024) originalDecodedPath
  H.ensure (sha256Hex originalDecoded == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")
    "image4 staging canonical record decodes to a different source"
  B.writeFile basePath base
  B.writeFile decodedPath decoded
  runTool root (encoderPath tools) ["0x160000",decodedPath,recordPath]
  absoluteDecoder <- canonicalizePath (decoderPath tools)
  runTool root (rebuildPath tools) ["replace-record4","--decoder",absoluteDecoder,basePath,recordPath,decodedPath,finalPath]
  final <- H.readRegular (64*1024*1024) finalPath
  finalParsed <- either H.failH pure (U.parseContainer final)
  let finalParts = U.parsedParts finalParsed
      finalRecord = U.partRecord4 finalParts
      restored = finalParts {U.partRecord4 = U.partRecord4 baseParts,U.partGap45 = U.partGap45 baseParts}
  restoredDat <- either H.failH pure (U.rebuildContainer staged restored)
  H.ensure (restoredDat == base) "image4 staging changes bytes outside its record window and checksum"
  H.ensure (U.recDecodedSize (U.recHeader finalRecord) == sourceSize) "image4 staging changes decoded image4 size"
  B.writeFile carriedPath (B.map complement (U.serializeRecord finalRecord))
  runTool root (decoderPath tools) [carriedPath,"0",carriedDecodedPath,show sourceSize]
  carriedDecoded <- H.readRegular (16*1024*1024) carriedDecodedPath
  H.ensure (carriedDecoded == decoded) "image4 staging carried stream differs from the confined decoded artifact"
  pure (StagedCandidate (CandidateDat final) (DecodedImage4 carriedDecoded))
