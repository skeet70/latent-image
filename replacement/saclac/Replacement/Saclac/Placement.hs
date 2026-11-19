{-# LANGUAGE OverloadedStrings #-}

-- | The linker script and the census watch ranges, from the one declared extent
-- list in "Replacement.Saclac.Artifact".
module Replacement.Saclac.Placement
  ( renderLinkerScript
  , watchRanges
  , insertWatchRanges
  , bindOwnedPosts
  , hexAddress
  ) where

import Control.Monad (unless)
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.List (isInfixOf, isPrefixOf, isSuffixOf, nub)
import Data.Word (Word32)
import qualified FujiAF.FirstStage as F
import Numeric (showHex)
import qualified Replacement.RtosPlatform.Aggregate as Agg
import Replacement.Saclac.Artifact (Admitted (..), Entry (..), Extent (..), Retained (..), entries, extentBytes, extents, loadingEntries, retainedData, retainedExternals, wordAt)
import qualified Replacement.FixedImage.Layout as Fixed

hexAddress :: Word32 -> String
hexAddress value = "0x" ++ replicate (8 - length digits) '0' ++ digits
  where
    digits = showHex value ""

regionName :: Int -> String
regionName 0 = "DISPATCH"
regionName 1 = "ROUTER"
regionName index = "EXTENT" ++ show index

sectionOf :: Int -> String
sectionOf 0 = "dispatch"
sectionOf 1 = "router"
sectionOf _ = "driver"

-- | The Root script. The carrier union names the fixed-bank family bodies the link keeps and
-- the loader entries they call, as @<entry symbol>_<domain>@ at the stock entry address.
renderLinkerScript :: Agg.CarrierUnion -> String
renderLinkerScript union = case (entries, extents) of
  (firstEntry : _, _ : _) -> unlines $
    [ "ENTRY(" ++ entryVeneer firstEntry ++ ")"
    , ""
    , "/* Generated from Replacement.Saclac.Artifact.extents. */"
    , "EXTERN(XM1_KERNEL_BOUNDARY XM1_SACLAC_TASK_STACK XM1_SACLAC_CONSTRUCTOR_STACK xm1_saclac_create_task XM1_DECODER_STATE xm1_decoder_init xm1_decoder_feed xm1_decoder_progress xm1_decoder_complete xm1_decoder_finish)"
    , "/* Present only when the bank links the task60 loading owner. */"
    , "EXTERN(" ++ unwords (map snd loadingEntries) ++ ")"
    ]
      ++ [ retainedName external ++ " = " ++ hexAddress (retainedAddress external) ++ ";"
         | external <- retainedExternals ++ retainedData,
           any (`isPrefixOf` retainedName external) ["xm1_kernel_", "xm1_decoder_", "__saclac_runtime_", "xm1_loading_", "xm1_lower_read_"]
         ]
      ++ [ "EXTERN(" ++ Agg.carrierEntrySymbol entry ++ ")"
         | (_, domainEntries) <- domainUnion, entry <- domainEntries, Agg.carrierEntryBody entry == Agg.BankBody
         ]
      ++ [ "PROVIDE(" ++ Agg.carrierEntrySymbol entry ++ "_" ++ label ++ " = " ++ hexAddress (Agg.carrierEntryAddress entry) ++ ");"
         | (label, domainEntries) <- domainUnion, entry <- domainEntries, Agg.carrierEntryBody entry == Agg.LoaderBody
         ]
      ++ [ "MEMORY {"
    ]
      ++ [ "  " ++ Fixed.linkerMemory ]
      ++ [ "  " ++ regionName index ++ " (rx) : ORIGIN = " ++ hexAddress (extentLow extent)
             ++ ", LENGTH = " ++ hexAddress (fromIntegral (extentBytes extent))
         | (index, extent) <- zip [0 :: Int ..] extents
         ]
      ++ [ "}"
         , ""
         , "SECTIONS"
         , "{"
         ]
      ++ concat
        [ [ "  ." ++ sectionOf index ++ " : {"
          , "    KEEP(*(.text." ++ entryVeneer entry ++ "))"
          ]
            ++ [ "  } > " ++ regionName index ]
        | (index, entry) <- zip [0 :: Int ..] entries
        ]
      ++ [ "  .text : { __fixed_body_start = .; KEEP(*(.decoder.*)) *(.text*) } > OWNED_FIXED"
         , "  .rodata : { KEEP(*(.rodata.XM1_KERNEL_BOUNDARY)) KEEP(*(.rodata.XM1_SACLAC_TASK_STACK)) KEEP(*(.rodata.XM1_SACLAC_CONSTRUCTOR_STACK)) KEEP(*(.rodata.XM1_REQUIRES_LENS_SAFETY_CONTRACT)) *(.rodata*) . = ALIGN(4); } > OWNED_FIXED"
         , "  .data : { *(.data*) *(.sdata*) *(.got*) . = ALIGN(4); __fixed_body_end = .; } > OWNED_FIXED"
         , "  .bss (NOLOAD) : { __fixed_bss_start = .; *(.bss*) *(.sbss*) *(COMMON) . = ALIGN(4); __fixed_bss_end = .; } > OWNED_FIXED"
         , "  __power_cpu_start = __fixed_body_start;"
         , "  __power_cpu_end = __fixed_bss_end;"
         , "  /DISCARD/ : {"
         , "    *(.ARM.exidx*)"
         , "    *(.ARM.extab*)"
         , "    *(.eh_frame*)"
         , "    *(.ARM.attributes)"
         , "    *(.note*)"
         , "    *(.comment)"
         , "    *(.debug*)"
         , "    *(.llvm_addrsig)"
         , "  }"
         , "}"
         , ""
         ]
      ++ [ "ASSERT(" ++ entryVeneer entry ++ " == " ++ hexAddress (entryAddress entry)
             ++ ", \"" ++ entryVeneer entry ++ " moved\")"
         | entry <- entries
         ]
      ++ [ "ASSERT(SIZEOF(." ++ sectionOf index ++ ") <= " ++ hexAddress (fromIntegral (extentBytes extent))
             ++ ", \"" ++ sectionOf index ++ " exceeds its stock extent\")"
         | (index, extent) <- zip [0 :: Int ..] extents
         ]
      ++ [ "ASSERT(__fixed_bss_end <= " ++ hexAddress Fixed.imageLimit ++ ", \"fixed owned image exceeds reserved RAM\")"
         , ""
         ]
  _ -> ""
  where
    domainUnion = [("a", Agg.carrierEntriesA union), ("b", Agg.carrierEntriesB union)]

-- | The census watch ranges: the same extents, in the shape the census tool
-- reads. The label is the entry's name without its prefix and suffix.
watchRanges :: [J.Value]
watchRanges =
  J.object ["label" J..= ("fixed-body" :: String), "start" J..= hexAddress (Fixed.imageBase + Fixed.headerBytes), "end" J..= hexAddress Fixed.imageLimit] :
  [ J.object
      [ "label" J..= (dropSuffix "_entry" (dropPrefix "xm1_saclac_" (entryVeneer entry)) ++ "-extent")
      , "start" J..= hexAddress (extentLow extent)
      , "end" J..= hexAddress (extentHigh extent)
      ]
  | (entry, extent) <- zip entries extents
  ]
  where
    dropPrefix prefix text = case splitAt (length prefix) text of
      (start, rest) | start == prefix -> rest
      _ -> text
    dropSuffix suffix text =
      let count = length text - length suffix
       in if count >= 0 && drop count text == suffix then take count text else text

-- | A census sites file with its watch ranges replaced by the declared extents.
insertWatchRanges :: J.Value -> Either String J.Value
insertWatchRanges value = case value of
  J.Object fields -> Right (J.Object (KM.insert (K.fromText "watch") (J.toJSON watchRanges) fields))
  _ -> Left "the census sites file is not a JSON object"

-- | The census sites file with one owned post entry per linked Root image variant: each variant's
-- slot-table backend @message_post@ address and the first instruction word loaded there. The
-- observer enables an entry's ABI only while the loaded word matches, so one sites file serves
-- every compared lane. Binding refuses unless, in each variant, exactly one listed entry holds its
-- declared word and that entry is the variant's own; another variant's entry can never fire there.
bindOwnedPosts :: [Admitted] -> J.Value -> Either String J.Value
bindOwnedPosts images value = do
  posts <- mapM postOf images
  let distinct = nub posts
      holds image (address, word) = wordAt image address == word
  mapM_ (\(image, own) -> unless (filter (holds image) distinct == [own])
    (Left ("owned post " ++ hexAddress (fst own) ++ " is not the only declared post its image holds"))) (zip images posts)
  let owned = [J.object ["address" J..= hexAddress address, "instruction" J..= hexAddress word] | (address, word) <- distinct]
  case value of
    J.Object fields -> case KM.lookup (K.fromText "interface") fields of
      Just (J.Object interface) ->
        Right (J.Object (KM.insert (K.fromText "interface") (J.Object (KM.insert (K.fromText "owned_posts") (J.toJSON owned) interface)) fields))
      _ -> Left "the census sites file has no interface object"
    _ -> Left "the census sites file is not a JSON object"
  where
    postOf admitted = case filter isPost (admittedSymbols admitted) of
      [symbol] -> let address = F.symbolAddress symbol in Right (address, wordAt admitted address)
      [] -> Left "the linked image has no slot-table message_post"
      _ -> Left "the linked image has more than one slot-table message_post"
    isPost symbol = let name = F.symbolName symbol
                     in all (`isInfixOf` name) ["10xm1_kernel", "10slot_table", "9SlotTable"] && "7Backend12message_post" `isSuffixOf` name
