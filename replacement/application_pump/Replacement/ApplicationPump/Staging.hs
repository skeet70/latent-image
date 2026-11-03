module Replacement.ApplicationPump.Staging
  ( ScratchDirectory, validateScratchDirectory, StagedCandidate, PackageError (..)
  , runImage5Staging, validatePackagedDelta, candidateDat, candidateSha256, candidateImage5Sha256
  ) where

import Control.Monad (unless)
import Data.Bits (complement)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (tails)
import qualified Emulation.Image4Staging as Shared
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import System.Directory (canonicalizePath, doesDirectoryExist)
import System.Environment (getEnvironment)
import System.Exit (ExitCode (..))
import System.FilePath ((</>), splitDirectories)
import System.Process (CreateProcess (..), proc)

newtype ScratchDirectory = ScratchDirectory FilePath
newtype CandidateDat = CandidateDat B.ByteString
newtype DecodedImage5 = DecodedImage5 B.ByteString
data StagedCandidate = StagedCandidate CandidateDat DecodedImage5
data PackageError = PackageMalformed | PackageExtentRejected | PackageDeltaRejected | PackageDecodeRejected deriving (Eq, Show)

candidateDat :: StagedCandidate -> B.ByteString
candidateDat (StagedCandidate (CandidateDat bytes) _) = bytes

candidateSha256 :: StagedCandidate -> String
candidateSha256 = SHA.sha256Hex . SHA.sha256 . candidateDat

candidateImage5Sha256 :: StagedCandidate -> String
candidateImage5Sha256 (StagedCandidate _ (DecodedImage5 image)) = SHA.sha256Hex (SHA.sha256 image)

validateScratchDirectory :: FilePath -> IO ScratchDirectory
validateScratchDirectory configured = do
  _ <- Shared.validateScratchDirectory configured
  exists <- doesDirectoryExist configured
  H.ensure exists "application pump scratch directory does not exist"
  resolved <- canonicalizePath configured
  H.ensure (any ((== ["private-data","scratch","app154"]) . take 3) (tails (splitDirectories resolved)))
    "application pump outputs must be under private-data/scratch/app154"
  pure (ScratchDirectory resolved)

runTool :: FilePath -> FilePath -> [String] -> IO ()
runTool root executable arguments = do
  absolute <- canonicalizePath executable
  inherited <- getEnvironment
  let process = (proc absolute arguments)
        { cwd = Just root
        , env = Just (("TMPDIR",root) : filter ((/= "TMPDIR") . fst) inherited)
        }
  (stopped,status,_,errors) <- H.runBoundedProcess (H.ProcessTimeoutMicros 600000000) (H.OutputCapBytes (1024*1024)) process B.empty
  H.ensure (not stopped && status == ExitSuccess) ("image5 staging tool failed: " ++ C.unpack errors)

require :: Bool -> PackageError -> Either PackageError ()
require condition refusal = unless condition (Left refusal)

admitPackaged :: B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Either PackageError StagedCandidate
admitPackaged original expected candidate carried = do
  validatePackagedDelta original expected candidate carried
  pure (StagedCandidate (CandidateDat candidate) (DecodedImage5 carried))

-- The caller supplies the decoded bytes; only runImage5Staging binds them to the carried stream.
validatePackagedDelta :: B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Either PackageError ()
validatePackagedDelta original expected candidate carried = do
  source <- either (const (Left PackageMalformed)) Right (U.parseContainer original)
  final <- either (const (Left PackageMalformed)) Right (U.parseContainer candidate)
  let sourceParts = U.parsedParts source
      finalParts = U.parsedParts final
      sourceRecord = U.partRecord5 sourceParts
      finalRecord = U.partRecord5 finalParts
      (windowStart,windowEnd) = U.record5Window sourceParts
      start = U.stagingLength + windowStart
      stop = start + max (B.length (U.serializeRecord sourceRecord)) (B.length (U.serializeRecord finalRecord))
      checksumEnd = U.mainChecksumOffset + 4
      unchanged a b = B.take (b-a) (B.drop a candidate) == B.take (b-a) (B.drop a original)
  require (B.length candidate == B.length original && stop <= U.stagingLength + windowEnd
    && unchanged 0 U.mainChecksumOffset && unchanged checksumEnd start
    && B.drop stop candidate == B.drop stop original && candidate /= original) PackageDeltaRejected
  validated <- either (const (Left PackageMalformed)) Right (U.rebuildContainer source finalParts)
  require (validated == candidate) PackageMalformed
  require (U.recDecodedSize (U.recHeader finalRecord) == fromIntegral (B.length expected)) PackageExtentRejected
  require (carried == expected) PackageDecodeRejected

runImage5Staging :: Shared.Tools -> ScratchDirectory -> B.ByteString -> B.ByteString -> B.ByteString -> IO StagedCandidate
runImage5Staging tools (ScratchDirectory scratch) dat original image = H.withTemporaryAt scratch "image5-stage-" $ \root -> do
  let digest = SHA.sha256Hex . SHA.sha256
  H.ensure (digest dat == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"
    && digest original == "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a")
    "image5 staging requires canonical X-M1 1.50 sources"
  H.ensure (B.length image == B.length original
    && B.take offset image == B.take offset original
    && B.drop (offset+32) image == B.drop (offset+32) original
    && B.take 32 (B.drop offset image) /= B.take 32 (B.drop offset original))
    "image5 staging changes bytes outside the owned pump or retains the stock body"
  let datPath = root </> "canonical.DAT"
      imagePath = root </> "image5.bin"
      recordPath = root </> "record5.bin"
      candidatePath = root </> "candidate.DAT"
      carriedPath = root </> "carried-record5.bin"
      decodedPath = root </> "carried-image5.bin"
  B.writeFile datPath dat
  B.writeFile imagePath image
  runTool root (Shared.encoderPath tools) ["0x2e0000",imagePath,recordPath]
  decoder <- canonicalizePath (Shared.decoderPath tools)
  runTool root (Shared.rebuildPath tools) ["replace-record5","--decoder",decoder,datPath,recordPath,imagePath,candidatePath]
  candidate <- H.readRegular (64*1024*1024) candidatePath
  parsed <- either H.failH pure (U.parseContainer candidate)
  let record = U.partRecord5 (U.parsedParts parsed)
  B.writeFile carriedPath (B.map complement (U.serializeRecord record))
  runTool root decoder [carriedPath,"0",decodedPath,show (B.length image)]
  carried <- H.readRegular (16*1024*1024) decodedPath
  either (H.failH . show) pure (admitPackaged dat image candidate carried)
  where offset = 0xbfbd20 - 0xbf1a00
