{-# LANGUAGE OverloadedStrings #-}
-- | A linked build's symbol table, as @llvm-nm --print-size@ writes it, and the one
-- lookup a trajectory read resolves through.
module Emulation.Symbols
  ( SymbolTable
  , parseSymbolTable
  , resolveSymbol
  ) where

import qualified Data.ByteString.Char8 as C
import Data.Word (Word32)
import Numeric (readHex)

newtype SymbolTable = SymbolTable [(String, Word32)]

parseSymbolTable :: C.ByteString -> SymbolTable
parseSymbolTable = SymbolTable . foldr row [] . C.lines
  where
    row line rest = case C.words line of
      fields@(address : _) | [(value, "")] <- readHex (C.unpack address) -> (C.unpack (last fields), value) : rest
      _ -> rest

-- | The address a name resolves to; a name the table lacks or repeats refuses.
resolveSymbol :: SymbolTable -> String -> Either String Word32
resolveSymbol (SymbolTable rows) name = case [address | (entry, address) <- rows, entry == name] of
  [address] -> Right address
  [] -> Left ("runtime symbol " ++ name ++ " is not in the loaded build's symbol table")
  _ -> Left ("runtime symbol " ++ name ++ " is not unique in the loaded build's symbol table")
