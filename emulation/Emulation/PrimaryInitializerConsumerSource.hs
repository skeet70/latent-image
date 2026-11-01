{-# LANGUAGE OverloadedStrings #-}
module Emulation.PrimaryInitializerConsumerSource
  ( ConsumerSource, DescriptorId, DescriptorAccess (..), descriptorId, descriptorIdentifier
  , CodeSite (..), CodeRegion (..), consumerCode, consumerSource, consumerImageDigest
  , bindConsumerSource, descriptorAccess, consumerSourceValue
  ) where

import Control.Monad (forM_, unless)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import Data.Word (Word32)
import Emulation.FirstStageWitness (leWordAt, sha256Hex)
import Emulation.PrimaryInitializerSource
import qualified FujiAF.FirstStage as F

newtype DescriptorId = DescriptorId Word32 deriving (Eq, Ord, Show)

descriptorIdentifier :: DescriptorId -> Word32
descriptorIdentifier (DescriptorId ident) = ident

data CodeSite = Installation | Descriptor | Selector | Identity deriving (Eq, Ord, Show)

data CodeRegion = CodeRegion
  { codeSite :: CodeSite
  , codeStart :: Guest
  , codeWords :: [Word32]
  } deriving (Eq, Show)

consumerCode :: ConsumerSource -> [CodeRegion]
consumerCode _ = codeRegions

codeRegions :: [CodeRegion]
codeRegions =
  [ CodeRegion Installation (Guest 0x725a04)
      [0xe92d4800,0xe28db004,0xe30b0d20,0xe3400096,0xe3003000,0xe3403004
      ,0xe5933010,0xe5803000,0xe3003000,0xe340300d,0xe5933010,0xe5803004]
  , CodeRegion Descriptor (Guest 0x725b74)
      [0xe92d48f0,0xe28db014,0xe1a05000,0xe30b4d20,0xe3404096,0xe5942000
      ,0xe3013618,0xe7923003,0xe12fff33,0xe1a06000,0xe7942100,0xe2453001
      ,0xe3a0102c,0xe0212391,0xe5911184,0xe5911000]
  , CodeRegion Selector (Guest 0x412bc) [0xe92d4800,0xe28db004,0xeb00053b,0xe8bd8800]
  , CodeRegion Identity (Guest 0x427b8)
      [0xee121e10,0xe3a02901,0xe2822003,0xe1510002,0x03a00000,0x13a00001,0xe12fff1e]
  ]
data DescriptorAccess = DescriptorAccess
  { descriptorTableWord :: Guest
  , descriptorPoolWord :: Guest
  } deriving (Eq, Show)

data RuntimePremise
  = NaturalPrimaryArrival
  | OwnedCarrierCommittedAndImmutable
  | CurrentDecoderAndBothCoreAliases
  | BothCoreTcmExclusions
  | CallerFrameAndDerivedScratchBacking
  | ConsumerCallerStackBacking
  | RootPairInstallationObserved
  | PrimaryCp14SelectionObserved
  | DescriptorIdentifierBounded
  | RetainedPoolBackingAndContents
  | ConsumerCodeFetchable
  | NoInterveningSelectedWordOrSelectorWriter
  | AlternateAndComputedCallerClosureOpen

data ConsumerSource = ConsumerSource Source F.Sha256 Guest
consumerSource :: ConsumerSource -> Source
consumerSource (ConsumerSource source _ _) = source

consumerImageDigest :: ConsumerSource -> F.Sha256
consumerImageDigest (ConsumerSource _ digest _) = digest

require :: Bool -> String -> Either String ()
require condition message = unless condition (Left message)

descriptorId :: Word32 -> Either String DescriptorId
descriptorId value = do
  require (value >= 1 && value <= 120) "primary consumer descriptor identifier leaves 1..120"
  pure (DescriptorId value)

bindConsumerSource :: Source -> B.ByteString -> Either String ConsumerSource
bindConsumerSource source image4 = do
  canonical <- F.parseSha256 "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
  imageDigest <- F.parseSha256 (sha256Hex image4)
  require (imageDigest == canonical) "primary consumer requires canonical decoded image4"
  let wordsAt :: Word32 -> B.ByteString -> Word32 -> [Word32] -> Either String ()
      wordsAt origin bytes start expected = forM_ (zip [0,4..] expected) $ \(offset,value) ->
        require (leWordAt bytes (fromIntegral (start - origin) + offset) == Right value)
          "primary consumer installation, selector or descriptor source differs"
  forM_ codeRegions $ \(CodeRegion site (Guest start) words') ->
    case site of
      Installation -> wordsAt 0x61c400 image4 start words'
      Descriptor -> wordsAt 0x61c400 image4 start words'
      Selector -> wordsAt 0x40000 (sourceLoader source) start words'
      Identity -> wordsAt 0x40000 (sourceLoader source) start words'
  let RootPair (Guest primary) (Guest secondary) = sourceRoots source
      selector = [publicationValue row | row <- sourcePublications source,publicationGuest row == Guest (primary + 0x1618)]
  require (primary == 0x4004dae0 && secondary == 0x400ddaa0 && selector == [0x412bc])
    "primary consumer root pair or selector publication differs"
  let Guest pool = returnR0 (sourceReturn source)
      descriptorRows =
        [ row | row@(Publication (Guest guest) _ _) <- sourcePublications source
        , guest >= primary + 0x184 && guest <= primary + 0x184 + 119 * 44
        , (guest - primary - 0x184) `mod` 44 == 0
        ]
  require (length descriptorRows == 120) "primary consumer descriptor field coverage differs"
  forM_ (zip [0..] descriptorRows) $ \(index,Publication guest pointer _) ->
    require (guest == Guest (primary + 0x184 + index * 44) && pointer == pool + index * 40 + 8)
      "primary consumer descriptor does not select its source pool word"
  pure (ConsumerSource source imageDigest (Guest pool))

descriptorAccess :: ConsumerSource -> DescriptorId -> DescriptorAccess
descriptorAccess (ConsumerSource source _ (Guest pool)) (DescriptorId ident) =
  let RootPair (Guest root) _ = sourceRoots source
   in DescriptorAccess (Guest (root + (ident - 1) * 44 + 0x184)) (Guest (pool + (ident - 1) * 40 + 8))

premiseName :: RuntimePremise -> String
premiseName premise = case premise of
  NaturalPrimaryArrival -> "natural-primary-arrival"
  OwnedCarrierCommittedAndImmutable -> "owned-carrier-committed-and-immutable"
  CurrentDecoderAndBothCoreAliases -> "current-decoder-and-both-core-aliases"
  BothCoreTcmExclusions -> "both-core-tcm-exclusions"
  CallerFrameAndDerivedScratchBacking -> "caller-frame-and-derived-scratch-backing"
  ConsumerCallerStackBacking -> "consumer-caller-stack-backing"
  RootPairInstallationObserved -> "root-pair-installation-observed"
  PrimaryCp14SelectionObserved -> "primary-cp14-selection-observed"
  DescriptorIdentifierBounded -> "descriptor-identifier-in-1..120"
  RetainedPoolBackingAndContents -> "retained-pool-backing-and-contents"
  ConsumerCodeFetchable -> "consumer-code-fetchable"
  NoInterveningSelectedWordOrSelectorWriter -> "no-intervening-selected-word-or-selector-writer"
  AlternateAndComputedCallerClosureOpen -> "alternate-and-computed-caller-closure-open"

consumerSourceValue :: ConsumerSource -> Value
consumerSourceValue consumer@(ConsumerSource source imageDigest _) =
  let RootPair (Guest primary) (Guest secondary) = sourceRoots source
      accessValue ident =
        let DescriptorAccess (Guest tableWord) (Guest poolWord) = descriptorAccess consumer (DescriptorId ident)
         in object ["identifier" .= ident,"table_word" .= tableWord,"pool_word" .= poolWord]
   in object
      [ "kind" .= ("primary-root0" :: String)
      , "code" .= [object ["start" .= start,"words" .= words'] | CodeRegion _ (Guest start) words' <- codeRegions]
      , "evidence" .= ("source-only" :: String)
      , "source_sha256" .= F.sha256Hex (sourceDigest source)
      , "loader_a_sha256" .= F.sha256Hex (sourceLoaderDigest source)
      , "image4_sha256" .= F.sha256Hex imageDigest
      , "installation" .= object
          [ "code" .= ([0x725a04,0x725a34] :: [Word32])
          , "header_words" .= ([0x40010,0xd0010] :: [Word32])
          , "root_words" .= ([0x96bd20,0x96bd24] :: [Word32])
          , "root_values" .= [primary,secondary]
          ]
      , "selection" .= object
          [ "extension_offset" .= (0x1618 :: Word32), "service" .= (0x412bc :: Word32)
          , "identity_helper" .= (0x427b8 :: Word32), "cp14_identity" .= (0x4003 :: Word32)
          , "root_index" .= (0 :: Word32)
          ]
      , "descriptor" .= object
          [ "entry" .= (0x725b74 :: Word32), "pointer_read" .= (0x725bac :: Word32)
          , "pool_read" .= (0x725bb0 :: Word32), "after_pool_read" .= (0x725bb4 :: Word32)
          , "identifiers" .= ([1,120] :: [Word32]), "record_bytes" .= (44 :: Word32)
          , "root_relative_field" .= (0x184 :: Word32), "retained_pool_word_value" .= ("requires-live-preimage" :: String)
          , "published_accesses" .= map accessValue [1..120]
          ]
      , "consumer_stack_source" .= object
          [ "saved_bytes" .= (24 :: Word32), "selector_saved_bytes" .= (8 :: Word32)
          , "entry_sp_premise" .= ("live-consumer-context" :: String)
          ]
      , "unproved_runtime_premises" .= map premiseName
          [ NaturalPrimaryArrival,OwnedCarrierCommittedAndImmutable,CurrentDecoderAndBothCoreAliases,BothCoreTcmExclusions
          , CallerFrameAndDerivedScratchBacking,ConsumerCallerStackBacking,RootPairInstallationObserved,PrimaryCp14SelectionObserved
          , DescriptorIdentifierBounded,RetainedPoolBackingAndContents,ConsumerCodeFetchable
          , NoInterveningSelectedWordOrSelectorWriter,AlternateAndComputedCallerClosureOpen
          ]
      ]
