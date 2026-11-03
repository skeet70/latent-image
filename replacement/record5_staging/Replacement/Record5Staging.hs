module Replacement.Record5Staging
  ( Record5Window (..), Record5Refusal (..), validateRecord5Confinement
  , StagedRecord5, stagedRecord5Dat, stagedRecord5Image, stageRecord5, decodeRecord
  ) where

import Control.Monad (unless)
import Data.Bits (complement)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Emulation.Image4Staging as Tools
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import System.Directory (canonicalizePath)
import System.Environment (getEnvironment)
import System.Exit (ExitCode (..))
import System.FilePath ((</>))
import System.Process (CreateProcess (..), proc)

-- | Absolute file offsets [low, high) of the record-5 media window.
data Record5Window = Record5Window Int Int deriving (Eq, Show)

data Record5Refusal = Record5OutsideWindow deriving (Eq, Show)

data StagedRecord5 = StagedRecord5 B.ByteString B.ByteString

stagedRecord5Dat :: StagedRecord5 -> B.ByteString
stagedRecord5Dat (StagedRecord5 dat _) = dat

-- | The image the staged record carries, decoded back from the candidate.
stagedRecord5Image :: StagedRecord5 -> B.ByteString
stagedRecord5Image (StagedRecord5 _ image) = image

validateRecord5Confinement :: Record5Window -> B.ByteString -> B.ByteString -> Either Record5Refusal ()
validateRecord5Confinement (Record5Window low high) original candidate = do
  let checksumEnd = U.mainChecksumOffset + 4
      slice offset count = B.take count . B.drop offset
      require condition = unless condition (Left Record5OutsideWindow)
  require (B.length original == B.length candidate && low >= checksumEnd && low < high && high <= B.length original)
  require (B.take U.mainChecksumOffset original == B.take U.mainChecksumOffset candidate
    && slice checksumEnd (low-checksumEnd) original == slice checksumEnd (low-checksumEnd) candidate
    && B.drop high original == B.drop high candidate)

runTool :: FilePath -> FilePath -> [String] -> IO ()
runTool root executable arguments = do
  absolute <- canonicalizePath executable
  inherited <- getEnvironment
  let process = (proc absolute arguments) {cwd = Just root, env = Just (("TMPDIR",root) : filter ((/= "TMPDIR") . fst) inherited)}
  (stopped,status,_,errors) <- H.runBoundedProcess (H.ProcessTimeoutMicros 600000000) (H.OutputCapBytes (1024*1024)) process B.empty
  H.ensure (not stopped && status == ExitSuccess) ("record5 staging tool failed: " ++ C.unpack errors)

-- | Decodes a container record to its declared size through files under root.
decodeRecord :: Tools.Tools -> FilePath -> String -> U.CompressedRecord -> IO B.ByteString
decodeRecord tools root name record = do
  let streamPath = root </> (name ++ "-record.bin")
      imagePath = root </> (name ++ "-image.bin")
  B.writeFile streamPath (B.map complement (U.serializeRecord record))
  runTool root (Tools.decoderPath tools) [streamPath,"0",imagePath,show (U.recDecodedSize (U.recHeader record))]
  H.readRegular (16*1024*1024) imagePath

-- | Replaces record 5 of the base DAT, whose record decodes to the base image,
-- with the staged image. Only the record-5 window and the main checksum change.
stageRecord5 :: Tools.Tools -> Tools.ScratchDirectory -> B.ByteString -> B.ByteString -> B.ByteString -> IO StagedRecord5
stageRecord5 tools scratch base baseImage image = H.withTemporaryAt (Tools.scratchDirectoryPath scratch) "record5-stage-" $ \root -> do
  before <- either H.failH pure (U.parseContainer base)
  let beforeParts = U.parsedParts before
      sourceSize = fromIntegral (U.recDecodedSize (U.recHeader (U.partRecord5 beforeParts)))
      basePath = root </> "base.DAT"
      imagePath = root </> "image5.bin"
      recordPath = root </> "record5.bin"
      candidatePath = root </> "candidate.DAT"
  H.ensure (B.length image == sourceSize) "record5 staging changes the decoded image5 extent"
  readback <- decodeRecord tools root "base" (U.partRecord5 beforeParts)
  H.ensure (readback == baseImage) "record5 staging base record differs from its declared image5"
  B.writeFile basePath base
  B.writeFile imagePath image
  runTool root (Tools.encoderPath tools) ["0x2e0000",imagePath,recordPath]
  decoder <- canonicalizePath (Tools.decoderPath tools)
  runTool root (Tools.rebuildPath tools) ["replace-record5","--decoder",decoder,basePath,recordPath,imagePath,candidatePath]
  candidate <- H.readRegular (64*1024*1024) candidatePath
  final <- either H.failH pure (U.parseContainer candidate)
  let finalParts = U.parsedParts final
      (mediaLow,mediaHigh) = U.record5Window beforeParts
      mainStart = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader before))
  either (H.failH . show) pure (validateRecord5Confinement (Record5Window (mainStart+mediaLow) (mainStart+mediaHigh)) base candidate)
  H.ensure (U.hMainChecksum (U.parsedHeader final) == U.hMainChecksum (U.derivedHeader finalParts)) "record5 staging carries an invalid main checksum"
  carried <- decodeRecord tools root "carried" (U.partRecord5 finalParts)
  H.ensure (carried == image) "record5 staging carried image5 differs from the staged image"
  pure (StagedRecord5 candidate carried)
