{-# LANGUAGE OverloadedStrings #-}
-- | Bytes outside the task-storage carrier that the transition's admission
-- reads, or that its qualification assumes. Own carrier is subtracted when the
-- piece is derived.
module Replacement.StartupArtifact.Footprint.TaskStorage (footprint) where

import qualified Data.ByteString as B
import Data.Maybe (mapMaybe)
import Data.Text (Text)
import Data.Word (Word32)
import Replacement.StartupArtifact.Footprint.TaskSubmission (closure)
import Replacement.StartupArtifact.Types

entry :: FootprintBasis -> Target -> Word32 -> Word32 -> Text -> Maybe Footprint
entry basis target start end citation = (\range -> Footprint range basis (Citation citation)) <$> span' target start end

-- | Image-4 words the canonical-source admission pins (replacement/task_storage/Replacement/TaskStorage/Artifact.hs:72-88).
pinnedWords :: [Word32]
pinnedWords =
  [ 0x69cef8, 0x69cf14, 0x69cf34, 0x7caf34, 0x725c6c, 0x725c90, 0x725ca0, 0x725cc0, 0x725cd4, 0x725cf8, 0x725d14, 0x725d1c
  , 0x725d30, 0x725d4c, 0x725d50, 0x725d54, 0x725d60, 0x725d64, 0x725d70, 0x725d74, 0x725d7c, 0x725d80, 0x725d94, 0x725d98
  , 0x725da0, 0x725dac, 0x725dbc, 0x725dd0, 0x725dd4, 0x725df0, 0x725dfc, 0x725e04, 0x725e1c, 0x725e34, 0x725e50, 0x725e54
  , 0x725e68, 0x725e74, 0x725e84, 0x725ea0, 0x725efc, 0x725f54, 0x725f5c, 0x725f68, 0x725f78, 0x725f9c, 0x72601c, 0x7260fc
  , 0x72623c, 0x89ea98 ]

-- | Canonical loader A and loader B.
footprint :: B.ByteString -> B.ByteString -> [Footprint]
footprint loaderA loaderB = mapMaybe (\(basis, target, start, end, citation) -> entry basis target start end citation) $
  [ (AdmissionRead, Image4, word, word + 4, "replacement/task_storage/Replacement/TaskStorage/Artifact.hs:72-88 (pinned canonical word)") | word <- pinnedWords ]
  ++
  [ (AdmissionRead, LoaderA, 0x427d4, 0x42828, "replacement/task_storage/Replacement/TaskStorage/Artifact.hs:92-96 (lock services)")
  , (RuntimePremise, Image4, 0x725c6c, 0x725cc4, "replacement/task_storage/Replacement/TaskStorage/Artifact.hs:118 (adapter entry after the retained prelude)")
  , (RuntimePremise, Image4, 0x725da8, 0x725dd8, "replacement/task_storage/Replacement/TaskStorage/Artifact.hs:115,118 (resume into the retained epilogue; pool literal 0x725dd4)")
  , (RuntimePremise, Image4, 0x725dd8, 0x726098, "replacement/task_storage/Replacement/TaskStorage/Artifact.hs:205 (the wrapper, the prelude's only caller, supplies index, size and output word; storage-prelude-sole-caller)")
  , (RuntimePremise, Image4, 0x725a04, 0x725b54, "replacement/task_storage/Replacement/TaskStorage/Artifact.hs:207 (selector root 0x96bd20, which the installer stores)")
  , (RuntimePremise, LoaderA, 0x40430, 0x409ac, "replacement/task_storage/Replacement/TaskStorage/Artifact.hs:207-208 (root-0 slots 0x1618, 0x164, 0x168)")
  , (RuntimePremise, LoaderB, 0xd0430, 0xd0434, "replacement/task_storage/Replacement/TaskStorage/Artifact.hs:207-208 (root-1 slots 0x1618, 0x164, 0x168 behind the initializer entry)")
  ]
  ++ closure LoaderA 0x40000 loaderA [] [0x412bc, 0x41140, 0x41164]
       "replacement/task_storage/Replacement/TaskStorage/Artifact.hs:207-208 (root-0 slot targets; private-data/analysis/x-m1-1.50/decode/loader-40000.dis:518-536), closed over direct calls"
  ++ closure LoaderB 0xd0000 loaderB [] [0xd12bc, 0xd1140, 0xd1164]
       "replacement/task_storage/Replacement/TaskStorage/Artifact.hs:207-208 (root-1 slot targets; private-data/analysis/x-m1-1.50/decode/loader-d0000.dis:518-536), closed over direct calls"
