module Replacement.ApplicationPump.Staging
  ( ScratchDirectory, validateScratchDirectory, StagedCandidate, PackageError (..)
  , runImage5Staging, validatePackagedDelta, candidateDat, candidateSha256, candidateImage5Sha256
  ) where

import Control.Monad (unless)
import qualified Data.ByteString as B
import Data.List (tails)
import qualified Emulation.Image4Staging as Shared
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.Record5Staging as Record5
import System.Directory (canonicalizePath, doesDirectoryExist)
import System.FilePath (splitDirectories)

newtype ScratchDirectory = ScratchDirectory Shared.ScratchDirectory
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
  shared <- Shared.validateScratchDirectory configured
  exists <- doesDirectoryExist configured
  H.ensure exists "application pump scratch directory does not exist"
  resolved <- canonicalizePath configured
  H.ensure (any ((== ["private-data","scratch","app154"]) . take 3) (tails (splitDirectories resolved)))
    "application pump outputs must be under private-data/scratch/app154"
  pure (ScratchDirectory shared)

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
runImage5Staging tools (ScratchDirectory scratch) dat original image = do
  let digest = SHA.sha256Hex . SHA.sha256
  H.ensure (digest dat == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"
    && digest original == "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a")
    "image5 staging requires canonical X-M1 1.50 sources"
  H.ensure (B.length image == B.length original
    && B.take offset image == B.take offset original
    && B.drop (offset+32) image == B.drop (offset+32) original
    && B.take 32 (B.drop offset image) /= B.take 32 (B.drop offset original))
    "image5 staging changes bytes outside the owned pump or retains the stock body"
  staged <- Record5.stageRecord5 tools scratch dat original image
  either (H.failH . show) pure
    (admitPackaged dat image (Record5.stagedRecord5Dat staged) (Record5.stagedRecord5Image staged))
  where offset = 0xbfbd20 - 0xbf1a00
