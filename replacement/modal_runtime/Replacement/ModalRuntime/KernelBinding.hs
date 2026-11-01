{-# LANGUAGE GHC2021 #-}
module Replacement.ModalRuntime.KernelBinding
  ( KernelDigests (..)
  , KernelSymbol (..)
  , BoundKernel (..)
  , bindElfText
  , bindKernel
  ) where

import qualified ArmElf as E
import Control.Monad (foldM, forM_, unless)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.NativeObservation (parseWord32)
import Emulation.Sha256 (Sha256, sha256)

data KernelDigests = KernelDigests
  { expectedElf :: Sha256
  , expectedText :: Sha256
  , expectedSymbols :: Sha256
  }

data KernelSymbol = KernelSymbol
  { kernelSymbolAddress :: Word32
  , kernelSymbolSize :: Word32
  } deriving (Eq, Show)

data BoundKernel = BoundKernel
  { boundTextAddress :: Word32
  , boundText :: B.ByteString
  , boundSymbols :: M.Map String KernelSymbol
  }

bindKernel :: KernelDigests -> [String] -> B.ByteString -> B.ByteString -> B.ByteString -> Either String BoundKernel
bindKernel expected functions elfBytes suppliedText symsBytes = do
  (elf, section, text) <- bindElfText (expectedElf expected) elfBytes
  unless (sha256 suppliedText == expectedText expected) (Left "kernel-text.bin digest mismatch")
  unless (sha256 symsBytes == expectedSymbols expected) (Left "kernel.syms digest mismatch")
  unless (text == suppliedText) (Left "kernel-text.bin differs from bound ELF .text")
  let base = E.sectionAddress section
      names = "__rb_image_start" : functions
  symbols <- foldM (addSymbol names) M.empty (lines (C.unpack symsBytes))
  forM_ names $ \name -> do
    supplied <- maybe (Left ("kernel symbol missing: " ++ name)) Right (M.lookup name symbols)
    case [s | s <- E.elfSymbols elf, E.symbolName s == name] of
      [s] -> do
        unless (KernelSymbol (E.symbolValue s) (E.symbolSize s) == supplied) (Left ("kernel.syms differs from bound ELF: " ++ name))
        unless (E.symbolSection s /= E.Undefined) (Left ("undefined ELF symbol: " ++ name))
        unless (name == "__rb_image_start" || E.symbolKind s == E.FunctionSymbol) (Left ("ELF function symbol required: " ++ name))
      _ -> Left ("unique ELF symbol required: " ++ name)
  unless (fmap kernelSymbolAddress (M.lookup "__rb_image_start" symbols) == Just base) (Left "__rb_image_start differs from ELF .text")
  forM_ functions $ \name -> case M.lookup name symbols of
    Just (KernelSymbol address size) ->
      unless (size > 0 && toInteger base <= toInteger address && toInteger address + toInteger size <= toInteger base + toInteger (B.length text)) (Left ("owned function outside ELF .text: " ++ name))
    Nothing -> Left ("kernel function missing: " ++ name)
  pure (BoundKernel base text symbols)
  where
    addSymbol names symbols row = case words row of
      [address, size, _, name] -> insert names symbols name address size
      [address, _, name] -> insert names symbols name address "0"
      _ -> Right symbols
    insert names symbols name address size
      | name `notElem` names = Right symbols
      | M.member name symbols = Left ("duplicate symbol: " ++ name)
      | otherwise = case (parseWord32 address, parseWord32 size) of
          (Just a, Just s) -> Right (M.insert name (KernelSymbol a s) symbols)
          _ -> Left ("invalid symbol: " ++ name)

bindElfText :: Sha256 -> B.ByteString -> Either String (E.Elf, E.Section, B.ByteString)
bindElfText expected elfBytes = do
  unless (sha256 elfBytes == expected) (Left "kernel.elf digest mismatch")
  elf <- E.parseElf elfBytes
  section <- case [s | s <- E.elfSections elf, E.sectionName s == ".text", E.sectionExecutable s, E.sectionType s == 1] of
    [s] -> Right s
    _ -> Left "exactly one executable PROGBITS .text section required"
  let text = B.take (fromIntegral (E.sectionSize section)) (B.drop (fromIntegral (E.sectionOffset section)) elfBytes)
  unless (not (B.null text) && B.length text <= 0xa300) (Left "ELF .text size outside (0,0xa300]")
  pure (elf, section, text)
