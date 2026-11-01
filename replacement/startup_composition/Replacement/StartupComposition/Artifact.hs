{-# LANGUAGE OverloadedStrings #-}
module Replacement.StartupComposition.Artifact
  ( SourceFiles (..), NativeFiles (..), CompositionSource, JoinedArtifact, StagedImage4, StagedCandidate
  , JoinError (..), JoinedProof, InstallBoundary, StackOffset, CodeAddress
  , canonicalSource, deriveArtifact, joinedProof, dispatcherArtifact, installerArtifact
  , dispatcherLow, installerLow, joinedLow, installationBoundaries, boundaryInstruction, boundaryContinuation
  , boundaryStack, boundaryLiveWords, stackOffsetValue, codeAddressValue
  , stageImage4, stagedBytes, stageCandidate, candidateBytes, contractValue, candidateContractValue
  ) where

import Control.Monad (forM, unless)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import Data.List (nub)
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Image4
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.KernelInstall.Admission as K
import qualified Replacement.KernelInstall.Artifact as I
import qualified Replacement.StartupDispatch.Admission as F
import qualified Replacement.StartupDispatch.Artifact as D


data SourceFiles = SourceFiles
  { sourceDat :: B.ByteString, sourceImage4 :: B.ByteString, sourceLoaderA :: B.ByteString, sourceLoaderB :: B.ByteString }
data NativeFiles = NativeFiles {nativeBytes :: B.ByteString, nativeSymbols :: B.ByteString}
data CompositionSource = CompositionSource B.ByteString B.ByteString D.CanonicalSource
newtype StackOffset = StackOffset Int deriving (Eq, Ord, Show)
newtype CodeAddress = CodeAddress Word32 deriving (Eq, Ord, Show)
data InstallBoundary = InstallBoundary
  { boundaryInstruction :: CodeAddress, boundaryContinuation :: CodeAddress
  , boundaryStack :: StackOffset, boundaryLiveWords :: [StackOffset]
  } deriving (Eq, Show)
data JoinedProof = JoinedProof
  { dispatcherLow :: StackOffset, installerLow :: StackOffset, joinedLow :: StackOffset
  , installationBoundaries :: [InstallBoundary]
  }
data JoinedArtifact = JoinedArtifact D.LinkedArtifact I.LinkedArtifact JoinedProof
data StagedImage4 = StagedImage4 CompositionSource JoinedArtifact B.ByteString
data StagedCandidate = StagedCandidate StagedImage4 Image4.StagedCandidate

data JoinError
  = SourceRejected D.ArtifactError | HeaderRootsRejected
  | DispatcherRejected D.ArtifactError | InstallerRejected I.ArtifactError
  | InstallBoundaryMissing | InstallPhaseRejected | InstallAlignmentRejected | InstallContinuationRejected
  | CallerFrameOverlap | StackBoundRejected
  deriving (Eq, Show)

require :: Bool -> JoinError -> Either JoinError ()
require condition failure = unless condition (Left failure)
stackOffsetValue :: StackOffset -> Int
stackOffsetValue (StackOffset value) = value
codeAddressValue :: CodeAddress -> Word32
codeAddressValue (CodeAddress value) = value

canonicalSource :: SourceFiles -> Either JoinError CompositionSource
canonicalSource files = do
  source <- either (Left . SourceRejected) Right
    (D.canonicalSource (sourceDat files) (sourceImage4 files) (sourceLoaderA files) (sourceLoaderB files))
  require (U.word32At (sourceLoaderA files) 0x10 == 0x4004dae0
    && U.word32At (sourceLoaderB files) 0x10 == 0x400ddaa0
    && U.word32At (sourceImage4 files) (0x7caf3c-0x61c400) == 3) HeaderRootsRejected
  pure (CompositionSource (sourceDat files) (sourceImage4 files) source)

deriveArtifact :: NativeFiles -> NativeFiles -> Either JoinError JoinedArtifact
deriveArtifact dispatcher installation = do
  dispatch <- either (Left . DispatcherRejected) Right (D.deriveArtifact (nativeBytes dispatcher) (nativeSymbols dispatcher))
  install <- either (Left . InstallerRejected) Right (I.deriveArtifact (nativeBytes installation) (nativeSymbols installation))
  let dispatchFrame = D.frameProof dispatch
      installationFrame = I.installationProof install
      calls = [boundary | boundary <- F.callBoundaries dispatchFrame,F.callEffect boundary == F.Install]
      installBottom = K.scratchLow installationFrame
      dispatchBottom = F.scratchLow dispatchFrame
  require (not (null calls)) InstallBoundaryMissing
  require (installBottom < 0 && installBottom >= -4096 && installBottom `mod` 4 == 0
    && dispatchBottom <= -48 && dispatchBottom >= -4096 && dispatchBottom `mod` 4 == 0) StackBoundRejected
  boundaries <- forM calls $ \boundary -> do
    require (F.callPhase boundary == F.Setup) InstallPhaseRejected
    let at = F.stackOffsetValue (F.callStack boundary)
        live = map F.stackOffsetValue (F.callLiveWords boundary)
    require (at `mod` 8 == 0) InstallAlignmentRejected
    require (all (\offset -> offset >= at && offset < 0 && offset `mod` 4 == 0) live) CallerFrameOverlap
    continuation <- maybe (Left InstallContinuationRejected) Right (F.callContinuation boundary)
    let instruction = F.instructionAddressValue (F.callInstruction boundary)
        returned = F.instructionAddressValue continuation
    require (instruction >= 0x69cdf8 && instruction < 0x69cf84
      && returned >= 0x69cdf8 && returned < 0x69cf84 && returned `mod` 4 == 0) InstallContinuationRejected
    pure (InstallBoundary (CodeAddress instruction) (CodeAddress returned) (StackOffset at) (map StackOffset live))
  let bottom = minimum (dispatchBottom:[stackOffsetValue (boundaryStack boundary)+installBottom | boundary <- boundaries])
  require (bottom >= -8192) StackBoundRejected
  pure (JoinedArtifact dispatch install (JoinedProof (StackOffset dispatchBottom) (StackOffset installBottom)
    (StackOffset bottom) (nub boundaries)))

joinedProof :: JoinedArtifact -> JoinedProof
joinedProof (JoinedArtifact _ _ proof) = proof
dispatcherArtifact :: JoinedArtifact -> D.LinkedArtifact
dispatcherArtifact (JoinedArtifact dispatch _ _) = dispatch
installerArtifact :: JoinedArtifact -> I.LinkedArtifact
installerArtifact (JoinedArtifact _ install _) = install

stageImage4 :: CompositionSource -> JoinedArtifact -> StagedImage4
stageImage4 source@(CompositionSource _ original _) artifact@(JoinedArtifact dispatch install _) =
  let dispatchBytes = D.linkedBytes dispatch
      installBytes = I.linkedBytes install
      start = 0x69cdf8-0x61c400
      middle = 0x69cf84-0x61c400
      installation = 0x725a04-0x61c400
      end = 0x725b54-0x61c400
      padding size linked = B.concat (replicate ((size-B.length linked) `div` 4) (U.putWord32 0xe320f000))
      staged = B.concat
        [ B.take start original,dispatchBytes,padding 396 dispatchBytes
        , B.take (installation-middle) (B.drop middle original),installBytes,padding 336 installBytes
        , B.drop end original
        ]
  in StagedImage4 source artifact staged

stagedBytes :: StagedImage4 -> B.ByteString
stagedBytes (StagedImage4 _ _ bytes) = bytes
stageCandidate :: Image4.Tools -> Image4.ScratchDirectory -> StagedImage4 -> IO StagedCandidate
stageCandidate tools scratch staged@(StagedImage4 (CompositionSource dat _ _) _ image) = do
  candidate <- Image4.runImage4Staging tools scratch dat dat image
  pure (StagedCandidate staged candidate)
candidateBytes :: StagedCandidate -> B.ByteString
candidateBytes (StagedCandidate _ candidate) = Image4.candidateDat candidate

contractValue :: StagedImage4 -> Value
contractValue (StagedImage4 (CompositionSource _ _ source) (JoinedArtifact dispatch install proof) image) = object
  [ "schema" .= (1 :: Int),"qualification" .= ("conditional-startup-composition" :: String)
  , "source" .= D.sourceValue source
  , "startup_dispatch" .= D.artifactValue dispatch,"kernel_install" .= I.artifactValue install
  , "component_linked_bytes" .= object ["dispatcher" .= B.length (D.linkedBytes dispatch),"installer" .= B.length (I.linkedBytes install)]
  , "joined_image4_sha256" .= SHA.sha256Hex (SHA.sha256 image)
  , "patches" .= ([[0x69cdf8,0x69cf84],[0x725a04,0x725b54]] :: [[Word32]])
  , "owned_stack" .= object
      [ "dispatcher_low" .= stackOffsetValue (dispatcherLow proof)
      , "installer_low_from_entry" .= stackOffsetValue (installerLow proof)
      , "combined_low_from_dispatcher_entry" .= stackOffsetValue (joinedLow proof)
      , "install_calls" .= [object
          [ "instruction" .= codeAddressValue (boundaryInstruction boundary)
          , "continuation" .= codeAddressValue (boundaryContinuation boundary)
          , "stack_from_dispatcher_entry" .= stackOffsetValue (boundaryStack boundary)
          , "caller_live_words" .= map stackOffsetValue (boundaryLiveWords boundary)
          ] | boundary <- installationBoundaries proof]
      ]
  , "open_premises" .=
      ([ "the combined bound covers only the dispatcher and owned installer, not other retained callee frames"
       , "actual natural incoming stack, complete writable backing, aliases, TCM and live-frame/resource separation"
       , "retained mode getter, optional word fill, cache and task submission keep their own source/runtime obligations"
       , "later root consumers, callbacks, pools and object lifetimes require their own continuous observation"
       , "no general cold-boot application/runtime execution independence or production cutover"
       ] :: [String])
  ]

candidateContractValue :: StagedCandidate -> Value
candidateContractValue (StagedCandidate staged candidate) = object
  [ "schema" .= (1 :: Int),"qualification" .= ("host-admitted-startup-composition" :: String)
  , "composition" .= contractValue staged
  , "candidate_dat_sha256" .= Image4.candidateSha256 candidate
  , "candidate_image4_sha256" .= Image4.candidateImage4Sha256 candidate
  ]
