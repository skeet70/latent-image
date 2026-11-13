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
import Data.Word (Word32)
import Numeric (showHex)
import Replacement.Saclac.Artifact (Entry (..), Extent (..), entries, extentBytes, extents)

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
sectionOf _ = "router"

-- | One MEMORY region per declared extent. The two owned sections take the
-- first two regions; every assertion is stated in terms of the same extents.
renderLinkerScript :: String
renderLinkerScript = case (entries, extents) of
  (firstEntry : _, _ : _) -> unlines $
    [ "ENTRY(" ++ entryVeneer firstEntry ++ ")"
    , ""
    , "/* Generated from Replacement.Saclac.Artifact.extents. */"
    , "MEMORY {"
    ]
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
            ++ (if index == 0 then [] else ["    *(.text*)", "    *(.rodata*)", "    . = ALIGN(4);"])
            ++ [ "  } > " ++ regionName index ]
        | (index, entry) <- zip [0 :: Int ..] entries
        ]
      ++ [ "  .data : { *(.data*) *(.sdata*) *(.got*) } > " ++ regionName (length extents - 1)
         , "  .bss : { *(.bss*) *(.sbss*) *(COMMON) } > " ++ regionName (length extents - 1)
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
      ++ [ "ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, \"saclac has writable statics\")"
         , ""
         ]
  _ -> ""

-- | The census watch ranges: the same extents, in the shape the census tool
-- reads. The label is the entry's name without its prefix and suffix.
watchRanges :: [J.Value]
watchRanges =
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
