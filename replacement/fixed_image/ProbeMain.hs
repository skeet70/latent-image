{-# LANGUAGE OverloadedStrings #-}
module Main where

import Data.Aeson (encode, object, (.=))
import Data.Bits ((.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.Word (Word32)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Replacement.FixedImage.Record (corruptRecord)
import Replacement.FixedImage.Layout
import System.Environment (getArgs)
import System.Exit (die)

script :: String
script = unlines
  [ "ENTRY(xm1_fixed_test_entry)", "MEMORY {", linkerMemory, "}", "SECTIONS {"
  , ".text : { __fixed_body_start = .; KEEP(*(.text.entry)) *(.text*) *(.rodata*) } > OWNED_FIXED"
  , ".data : { *(.data*) . = ALIGN(4); __fixed_body_end = .; } > OWNED_FIXED"
  , ".bss (NOLOAD) : { __fixed_bss_start = .; *(.bss*) *(COMMON) . = ALIGN(4); __fixed_bss_end = .; } > OWNED_FIXED"
  , "/DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.note*) *(.comment*) }", "}"
  , "xm1_fixed_test_resume = 0x69cdfc;"
  , "ASSERT(__fixed_bss_end <= ORIGIN(OWNED_FIXED) + LENGTH(OWNED_FIXED), \"fixed image exceeds reservation\")"
  ]

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["script"] -> putStr script
    ["corrupt", source, output] -> do
      parsed <- H.readRegular (64 * 1024 * 1024) source >>= either die pure . U.parseContainer
      changed <- either die pure (corruptRecord (U.parsedParts parsed))
      restored <- either die pure (corruptRecord changed)
      inverse <- either die pure (U.rebuildContainer parsed restored)
      original <- H.readRegular (64 * 1024 * 1024) source
      H.ensure (inverse == original) "record control changed more than its reversible bit"
      either die (B.writeFile output) (U.rebuildContainer parsed changed)
    ["publication", path] -> do
      symbols <- H.readRegular 0x400000 path >>= either die pure . F.parseSymbols
      entry <- F.symbolAddress <$> either die pure (F.lookupSymbol symbols "xm1_fixed_test_entry")
      let delta = toInteger entry - 0x69cdf8 - 8
      if delta `mod` 4 /= 0 || delta < -0x2000000 || delta >= 0x2000000
        then die "probe entry is outside A32 branch reach"
        else do
          let branch = 0xea000000 .|. (fromInteger (delta `div` 4) .&. 0xffffff) :: Word32
          L.putStr (encode [object ["address" .= (0x69cdf8 :: Word32), "old" .= (0xe92d4830 :: Word32), "new" .= branch]])
    _ -> die "usage: fixed-image-probe script|publication SYMBOLS"
