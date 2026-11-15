{-# LANGUAGE OverloadedStrings #-}

-- | The linker script and the census watch ranges, from the one declared extent
-- list in "Replacement.Saclac.Artifact".
module Replacement.Saclac.Placement
  ( renderLinkerScript
  , watchRanges
  , insertWatchRanges
  , hexAddress
  ) where

import qualified Data.Aeson as J
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.List (isPrefixOf)
import Data.Word (Word32)
import Numeric (showHex)
import Replacement.Saclac.Artifact (Entry (..), Extent (..), Retained (..), entries, extentBytes, extents, retainedData, retainedExternals)
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

renderLinkerScript :: String
renderLinkerScript = case (entries, extents) of
  (firstEntry : _, _ : _) -> unlines $
    [ "ENTRY(" ++ entryVeneer firstEntry ++ ")"
    , ""
    , "/* Generated from Replacement.Saclac.Artifact.extents. */"
    , "EXTERN(XM1_KERNEL_BOUNDARY XM1_SACLAC_TASK_STACK xm1_saclac_create_task)"
    ]
      ++ [ retainedName external ++ " = " ++ hexAddress (retainedAddress external) ++ ";"
         | external <- retainedExternals ++ retainedData,
           "xm1_kernel_" `isPrefixOf` retainedName external || "__saclac_runtime_" `isPrefixOf` retainedName external
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
      ++ [ "  .text : { __fixed_body_start = .; *(.text*) } > OWNED_FIXED"
         , "  .rodata : { KEEP(*(.rodata.XM1_KERNEL_BOUNDARY)) KEEP(*(.rodata.XM1_SACLAC_TASK_STACK)) KEEP(*(.rodata.XM1_REQUIRES_LENS_SAFETY_CONTRACT)) *(.rodata*) . = ALIGN(4); } > OWNED_FIXED"
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
