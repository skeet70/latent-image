{-# LANGUAGE OverloadedStrings #-}
module Replacement.TaskStorage.Artifact
  ( CanonicalSource, LinkedArtifact, StagedImage4, StagedCandidate
  , canonicalSource, deriveArtifact, stageImage4, stagedBytes, contractValue
  , stageCandidate, candidateBytes, candidateContractValue
  ) where

import Control.Monad (forM_, unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits ((.&.), (.|.))
import qualified Data.ByteString as B
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import qualified Emulation.Image4Staging as Image4
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U

newtype GuestAddress = GuestAddress Word32
newtype RequestedBytes = RequestedBytes Word32
newtype TaskIndex = TaskIndex Word32
newtype SourceIdentity = SourceIdentity SHA.Sha256

data CanonicalSource = CanonicalSource B.ByteString B.ByteString SourceIdentity SourceIdentity SourceIdentity SourceIdentity
data LinkedArtifact = LinkedArtifact B.ByteString SourceIdentity SourceIdentity GuestAddress GuestAddress
data StagedImage4 = StagedImage4 CanonicalSource LinkedArtifact B.ByteString
data StagedCandidate = StagedCandidate StagedImage4 Image4.StagedCandidate

require :: Bool -> String -> Either String ()
require condition message = unless condition (Left message)

imageBase, transitionStart, transitionEnd :: Word32
imageBase = 0x61c400
transitionStart = 0x725cc4
transitionEnd = 0x725da8

identity :: B.ByteString -> SourceIdentity
identity = SourceIdentity . SHA.sha256

identityText :: SourceIdentity -> String
identityText (SourceIdentity digest) = SHA.sha256Hex digest

canonicalSource :: B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Either String CanonicalSource
canonicalSource dat image loaderA loaderB = do
  let source = identity dat
      imageDigest = identity image
      primary = identity loaderA
      secondary = identity loaderB
  require (identityText source == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490") "task storage requires the canonical original DAT"
  require (identityText imageDigest == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") "task storage image4 identity differs"
  require (identityText primary == "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882") "task storage loader A identity differs"
  require (identityText secondary == "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec") "task storage loader B identity differs"
  forM_ imageWords $ \(address, expected) ->
    require (U.word32At image (fromIntegral (address - imageBase)) == expected) "task storage source instruction differs"
  forM_ syncWords $ \(address, expected) ->
    require (U.word32At loaderA (fromIntegral (address - 0x40000)) == expected) "task storage retained synchronization differs"
  pure (CanonicalSource dat image source imageDigest primary secondary)

imageWords :: [(Word32, Word32)]
imageWords =
  [ (0x69cef8,0xe3a03067), (0x69cf14,0xe3a05b01), (0x69cf34,0xeb0223a7), (0x7caf34,2)
  , (0x725c6c,0xe92d4df0), (0x725c90,0xe7923003), (0x725ca0,0xe5933164)
  , (0x725cc0,0xebe472c3), (0x725cd4,0xe5932874), (0x725cf8,0x13e04020)
  , (0x725d14,0x13e04011), (0x725d1c,0xe5882000), (0x725d30,0xe583287c)
  , (0x725d4c,0xe0622003), (0x725d50,0xe0822007), (0x725d54,0xe3520a4b)
  , (0x725d60,0xe3c33101), (0x725d64,0xe5883000), (0x725d70,0xe3c72003)
  , (0x725d74,0xe593100c), (0x725d7c,0xe583200c), (0x725d80,0xe5982000)
  , (0x725d94,0xe5842874), (0x725d98,0xe5847878), (0x725da0,0xe584387c)
  , (0x725dac,0xebe47292), (0x725dbc,0xe5933168), (0x725dd0,0x409c280c), (0x725dd4,0x40a10200)
  , (0x725df0,0xe1d020b0), (0x725dfc,0xebfff5f5), (0x725e04,0xe5941000)
  , (0x725e1c,0xe5933e38), (0x725e34,0xe5933e34), (0x725e50,0xe5813e38)
  , (0x725e54,0xe5940000), (0x725e68,0xe5931e38), (0x725e74,0xebffff7c)
  , (0x725e84,0xebfff609), (0x725ea0,0xe583545c), (0x725efc,0xe5913080)
  , (0x725f54,0xe5932e38), (0x725f5c,0xe5930e34), (0x725f68,0xebfff096)
  , (0x725f78,0xebe471ad), (0x725f9c,0xe5913000), (0x72601c,0xe591300c)
  , (0x7260fc,0xe583487c), (0x72623c,0xebffff95)
  , (0x89ea98,0x40a10200)
  ]

syncWords :: [(Word32, Word32)]
syncWords = zip [0x427d4,0x427d8 .. 0x42824]
  [ 0xe1902f9f,0xe3520000,0x13e00000,0x112fff1e,0x01802f91,0xe3520000
  , 0x0e072fba,0x03a00000,0x012fff1e,0xeafffff5,0xe3a01000,0xe1902f9f
  , 0xe3520000,0x03e00000,0x012fff1e,0x11802f91,0xe3520000,0x0e072fba
  , 0x03a00000,0x012fff1e,0xeafffff4
  ]

branch :: Word32 -> Word32 -> Word32 -> Either String Word32
branch opcode site target = do
  let displacement = toInteger target - toInteger site - 8
  require (target .&. 3 == 0 && displacement >= -0x2000000 && displacement < 0x2000000) "task storage branch target is outside A32 range"
  pure (opcode .|. (fromInteger (displacement `div` 4) .&. 0xffffff))

deriveArtifact :: B.ByteString -> B.ByteString -> Either String LinkedArtifact
deriveArtifact image symbolBytes = do
  symbols <- F.parseSymbols symbolBytes
  let at name = F.symbolAddress <$> F.lookupSymbol symbols name
  start <- at "__task_storage_start"
  end <- at "__task_storage_end"
  entry <- F.lookupSymbol symbols "xm1_task_storage_entry"
  rust <- F.lookupSymbol symbols "xm1_task_storage_transition"
  resume <- at "xm1_task_storage_resume"
  require (start == transitionStart && end <= transitionEnd && end > start && end .&. 3 == 0
    && toInteger (B.length image) == toInteger end - toInteger start) "task storage linked extent leaves its owned window"
  require (F.symbolAddress entry == start && F.symbolSize entry == 24 && resume == transitionEnd) "task storage adapter boundary differs"
  let rustStart = F.symbolAddress rust
      rustEnd = toInteger rustStart + toInteger (F.symbolSize rust)
  require (rustStart >= start + 24 && rustStart .&. 3 == 0 && F.symbolSize rust > 0 && rustEnd <= toInteger end) "task storage Rust body leaves its linked image"
  call <- branch 0xeb000000 (start + 12) rustStart
  exit <- branch 0xea000000 (start + 20) resume
  let expected = [0xe1a00004,0xe1a01007,0xe1a02008,call,0xe1a04000,exit]
  require (map (U.word32At image) [0,4 .. 20] == expected) "task storage adapter register or return ABI differs"
  pure (LinkedArtifact image (identity image) (identity symbolBytes) (GuestAddress rustStart) (GuestAddress (fromInteger rustEnd)))

stageImage4 :: CanonicalSource -> LinkedArtifact -> StagedImage4
stageImage4 source@(CanonicalSource _ original _ _ _ _) artifact@(LinkedArtifact linked _ _ _ _) =
  let offset = fromIntegral (transitionStart - imageBase)
      extent = fromIntegral (transitionEnd - transitionStart)
      padding = B.concat (replicate ((extent - B.length linked) `div` 4) (U.putWord32 0xe320f000))
      staged = B.take offset original <> linked <> padding <> B.drop (offset + extent) original
  in StagedImage4 source artifact staged

stagedBytes :: StagedImage4 -> B.ByteString
stagedBytes (StagedImage4 _ _ bytes) = bytes

stageCandidate :: Image4.Tools -> Image4.ScratchDirectory -> StagedImage4 -> IO StagedCandidate
stageCandidate tools scratch staged@(StagedImage4 (CanonicalSource original _ _ _ _ _) _ image) = do
  candidate <- Image4.runImage4Staging tools scratch original original image
  pure (StagedCandidate staged candidate)

candidateBytes :: StagedCandidate -> B.ByteString
candidateBytes (StagedCandidate _ candidate) = Image4.candidateDat candidate

candidateContractValue :: StagedCandidate -> Value
candidateContractValue (StagedCandidate staged candidate) = object
  [ "schema" .= (1 :: Int)
  , "qualification" .= ("source-only" :: String)
  , "task_storage" .= contractValue staged
  , "candidate_dat_sha256" .= Image4.candidateSha256 candidate
  , "candidate_image4_sha256" .= Image4.candidateImage4Sha256 candidate
  ]

addressValue :: GuestAddress -> Word32
addressValue (GuestAddress address) = address

aliasValue :: GuestAddress -> Value
aliasValue guest = object ["guest" .= addressValue guest, "candidate_physical" .= (addressValue guest .&. 0xbfffffff)]

contractValue :: StagedImage4 -> Value
contractValue (StagedImage4 (CanonicalSource _ _ source original primary secondary) (LinkedArtifact _ linked symbols rustStart rustEnd) staged) =
  let TaskIndex taskIndex = TaskIndex 102
      RequestedBytes requested = RequestedBytes 0x400
  in object
    [ "schema" .= (1 :: Int), "qualification" .= ("source-only" :: String)
    , "source_sha256" .= identityText source, "image4_sha256" .= identityText original
    , "loader_a_sha256" .= identityText primary, "loader_b_sha256" .= identityText secondary
    , "linked_sha256" .= identityText linked, "symbols_sha256" .= identityText symbols
    , "staged_image4_sha256" .= identityText (identity staged)
    , "patch" .= object ["start" .= transitionStart, "end" .= transitionEnd]
    , "rust" .= [addressValue rustStart,addressValue rustEnd]
    , "retained_helper" .= ([0x725c6c,0x725cc4,0x725da8,0x725dd8] :: [Word32])
    , "retained_caller" .= ([0x725dd8,0x726098] :: [Word32])
    , "synchronization" .= object
        [ "selector_root" .= (0x96bd20 :: Word32), "selector_slot" .= (0x1618 :: Word32)
        , "irq_slots" .= ([0x164,0x168] :: [Word32]), "try_acquire" .= (0x427d4 :: Word32)
        , "release" .= (0x427fc :: Word32), "lock" .= aliasValue (GuestAddress 0x409c280c)
        , "acquisition_success_assumed" .= False
        ]
    , "conditional_task103" .= object
        [ "index" .= taskIndex, "requested_bytes" .= requested, "selector" .= (2 :: Word32)
        , "caller" .= (0x69cf34 :: Word32), "helper_call" .= (0x725e74 :: Word32)
        , "wrapper" .= aliasValue (GuestAddress 0x40a5b534)
        , "state" .= aliasValue (GuestAddress 0x40a1010c)
        , "cursor" .= aliasValue (GuestAddress 0x4089ea98)
        , "pool" .= aliasValue (GuestAddress 0x40a10200)
        ]
    , "open_premises" .=
        ([ "natural-caller qualification and runtime receipts"
         , "mapped aliases, compiler scratch extent and separation from guest writes"
         , "runtime wrapper/state/cursor inputs and descriptor word +0x18"
         , "indexed-record, cursor and reserved-storage lifetime and concurrent writers"
         , "dynamic service bindings and later installed-service/task behavior"
         ] :: [String])
    ]
