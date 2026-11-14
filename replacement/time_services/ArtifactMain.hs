{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (forM, unless)
import Data.Aeson (FromJSON(..), eitherDecodeStrict', encode, object, withObject, (.:), (.=))
import qualified Data.Aeson.Types as J
import Data.Bits ((.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.Word (Word32)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Numeric (readHex)
import Replacement.FixedImage.Layout (linkerMemory)
import System.Directory (canonicalizePath)
import System.Environment (getArgs)

newtype GuestAddress = GuestAddress Word32
newtype SymbolName = SymbolName String
newtype Contract = Contract [(GuestAddress, SymbolName)]
instance FromJSON GuestAddress where
  parseJSON value = do
    text <- parseJSON value
    case text of
      '0':'x':digits -> case readHex digits of
        [(word, "")] | word <= (0xffffffff :: Integer) -> pure (GuestAddress (fromInteger word))
        _ -> fail "invalid 32-bit guest address"
      _ -> fail "guest address lacks 0x prefix"
instance FromJSON Contract where
  parseJSON = withObject "time interface" $ \value -> do
    schema <- value .: "schema" :: J.Parser Int
    unless (schema == 1) (fail "unsupported time interface schema")
    rows <- value .: "entries"
    Contract <$> mapM (withObject "time entry" $ \row -> do
      address <- row .: "address"
      name <- row .: "symbol"
      pure (address, SymbolName name)) rows

script :: Contract -> String
script (Contract entries) = unlines
  [ "ENTRY(xm1_time_initialize)"
  , "EXTERN(" ++ unwords [name | (_, SymbolName name) <- entries] ++ ")"
  , "EXTERN(XM1_KERNEL_BOUNDARY)"
  , "MEMORY {", linkerMemory, "}", "SECTIONS {"
  , ".text : { __fixed_body_start = .; *(.text*) KEEP(*(.rodata.XM1_KERNEL_BOUNDARY)) KEEP(*(.rodata.XM1_REQUIRES_LENS_SAFETY_CONTRACT)) *(.rodata*) } > OWNED_FIXED"
  , ".data : { *(.data*) . = ALIGN(4); __fixed_body_end = .; } > OWNED_FIXED"
  , ".bss (NOLOAD) : { __fixed_bss_start = .; *(.bss*) *(COMMON) . = ALIGN(4); __fixed_bss_end = .; } > OWNED_FIXED"
  , "/DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.note*) *(.comment*) }", "}"
  , "xm1_kernel_console9 = 0x41a34;"
  , "xm1_kernel_critical_enter = 0x7235d8;"
  , "xm1_kernel_critical_exit = 0x7236b0;"
  , "ASSERT(__fixed_bss_end <= ORIGIN(OWNED_FIXED) + LENGTH(OWNED_FIXED), \"time services exceed fixed reservation\")"
  ]

readContract :: FilePath -> IO Contract
readContract path = canonicalizePath path >>= H.readRegular 0x100000 >>= either H.failH pure . eitherDecodeStrict'

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["script", contractPath] -> readContract contractPath >>= putStr . script
    ["publications", contractPath, imagePath, symbolsPath, output] -> do
      Contract entries <- readContract contractPath
      image <- H.readRegular (16 * 1024 * 1024) imagePath
      symbols <- H.readRegular 0x400000 symbolsPath >>= either H.failH pure . F.parseSymbols
      rows <- forM entries $ \(GuestAddress address, SymbolName name) -> do
        target <- F.symbolAddress <$> either H.failH pure (F.lookupSymbol symbols name)
        let delta = toInteger target - toInteger address - 8
            offset = fromIntegral (address - 0x61c400)
        H.ensure (address >= 0x61c400 && offset + 4 <= B.length image) "publication leaves image 4"
        H.ensure (delta `mod` 4 == 0 && delta >= -0x2000000 && delta < 0x2000000) "publication leaves A32 branch reach"
        let branch = 0xea000000 .|. (fromInteger (delta `div` 4) .&. 0xffffff) :: Word32
        pure (object ["address" .= address, "old" .= U.word32At image offset, "new" .= branch])
      B.writeFile output (L.toStrict (encode rows))
    _ -> H.failH "usage: time-services-artifact script CONTRACT | publications CONTRACT IMAGE4 SYMBOLS OUTPUT_JSON"
