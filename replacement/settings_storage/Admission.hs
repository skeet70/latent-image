{-# LANGUAGE GHC2021 #-}
module Admission (AdmissionError(..), admitArtifact) where

import qualified A32 as A
import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.OwnedCase (Symbol(..), readWordAt)
import Native (entryNames)
import Replacement.PropertyService.Artifact (storageStoreName, storageStoreWords, storageOutputName, storageOutputWords, storageStatusWords, mirrorBoundaries, kernelBoundaries)

newtype SymbolName = SymbolName String deriving (Eq, Show)
data AdmissionError
  = EmptyArtifact
  | UnalignedArtifact
  | ArtifactOutsideCarrier
  | MissingSymbol SymbolName
  | InvalidFunctionExtent SymbolName
  | InvalidDefaultExtent SymbolName
  | DefaultBytesDiffer SymbolName
  | UndeclaredExternal SymbolName
  | BoundaryEncodingDiffer SymbolName
  deriving (Eq, Show)

admitArtifact :: B.ByteString -> B.ByteString -> M.Map String Symbol -> Either AdmissionError ()
admitArtifact image binary symbols = do
  unless (not (B.null binary)) (Left EmptyArtifact)
  unless (B.length binary `mod` 4 == 0) (Left UnalignedArtifact)
  unless (B.length binary <= 0x61c400-0x5c0000) (Left ArtifactOutsideCarrier)
  forM_ entryNames $ \(_,name) -> do
    Symbol address size <- required name
    unless (size > 0 && inside address size && address `mod` 4 == 0) (Left (InvalidFunctionExtent (SymbolName name)))
  forM_ (M.toList symbols) $ \(name,Symbol address size) ->
    if name `elem` ["TAIL_BYTES","SLOT_BYTES"] then pure ()
    else if size > 0 then unless (inside address size && address `mod` 4 == 0 && size `mod` 4 == 0) (Left (InvalidFunctionExtent (SymbolName name)))
    else unless (inside address 0 || lookup name externalSymbols == Just address) (Left (UndeclaredExternal (SymbolName name)))
  forM_ [("TAIL_BYTES",0x81b074,40),("SLOT_BYTES",0x7cb7dc,200)] $ \(name,source,size) -> do
    Symbol address declared <- required name
    unless (declared == fromIntegral size && inside address declared) (Left (InvalidDefaultExtent (SymbolName name)))
    let expected = B.take size (B.drop (source-0x61c400) image)
        actual = B.take size (B.drop (fromIntegral (address-0x5c0000)) binary)
    unless (B.length expected == size && actual == expected) (Left (DefaultBytesDiffer (SymbolName name)))
  forM_ [minBound .. maxBound] $ \width -> exact (storageStoreName width) (storageStoreWords width)
  forM_ [minBound .. maxBound] $ \width -> exact (storageOutputName width) (storageOutputWords width)
  exact "xm1_storage_status32" storageStatusWords
  forM_ mirrorBoundaries $ \(name,_,target) -> do
    Symbol address size <- required name
    unless (size == 16 && inside address size && wordsAt address 2 == [0xe92d4800,0xe1a0b00d]
      && A.decode (address+8) (wordAt (address+8)) == A.Branch (A.Cond 14) True target
      && wordAt (address+12) == 0xe8bd8800) (Left (BoundaryEncodingDiffer (SymbolName name)))
  where
    required name = maybe (Left (MissingSymbol (SymbolName name))) Right (M.lookup name symbols)
    inside :: Word32 -> Word32 -> Bool
    inside address size = address >= 0x5c0000 && fromIntegral address + fromIntegral size <= (0x5c0000 + toInteger (B.length binary))
    wordAt address = readWordAt binary (fromIntegral (address-0x5c0000))
    wordsAt address count = map wordAt (take count [address,address+4..])
    exact name expected = do
      Symbol address size <- required name
      unless (size == 4 * fromIntegral (length expected) && inside address size && wordsAt address (length expected) == expected) (Left (BoundaryEncodingDiffer (SymbolName name)))

externalSymbols :: [(String,Word32)]
externalSymbols =
  [("xm1_stock_61df44",0x61df44),("xm1_stock_61d8a4",0x61d8a4),("xm1_stock_61d59c",0x61d59c),("xm1_stock_61d618",0x61d618),("xm1_stock_61d5f0",0x61d5f0),("xm1_stock_61d65c",0x61d65c)
  ,("xm1_bank_property_set",0x624694),("xm1_bank_current_selection",0x6439e0),("xm1_bank_short_read",0x72b3a4)
  ,("xm1_bank_custom_refresh",0x6a86c4),("xm1_bank_record_append",0x6ac7d4)]
  ++ [(name,address) | (_,name,address) <- mirrorBoundaries]
  ++ kernelBoundaries
