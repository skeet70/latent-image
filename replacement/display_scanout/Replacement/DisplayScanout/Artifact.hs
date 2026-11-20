{-# LANGUAGE OverloadedStrings #-}
-- | Entry publications and the observer contract of the fixed-range scanout image.
module Replacement.DisplayScanout.Artifact
  ( Carrier (..), Entry (..), Publication (..), ArtifactError (..), carriers, entries
  , canonicalImage4Sha256, publicationsFor, canonicalPublications, bodyBounds, publicationsValue, contractValue
  ) where

import Control.Monad (forM, unless)
import Data.Aeson (Value, object, toJSON, (.=))
import Data.Bits ((.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (isInfixOf)
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import Numeric (readHex)
import Replacement.FixedImage.Layout (headerBytes, imageBase, imageLimit)

-- | A retired stock body of image 4, half-open.
data Carrier = Carrier Word32 Word32 deriving (Eq, Show)

-- | A stock entry address that retained callers and registered callback words name, and its owned export.
data Entry = Entry String Word32 deriving (Eq, Show)

-- | One image-4 word: its address, canonical value and published value.
data Publication = Publication Word32 Word32 Word32 deriving (Eq, Show)

data ArtifactError
  = SourceMismatch
  | SymbolTableRejected String
  | MissingExport String
  | ExportOutsideFixed String
  | BodyBoundsRejected
  deriving (Eq, Show)

image4Base :: Word32
image4Base = 0x61c400

-- | Stock function extents of x70view 55436fc85c0560dd252b0410af6f2427fe198cd3 `function-ranges.json`.
carriers :: [Carrier]
carriers = [Carrier 0x6c6eb0 0x6c75f0, Carrier 0x6c926c 0x6c994c, Carrier 0x6c9cf4 0x6ca04c, Carrier 0x6ca3e0 0x6cb0ec, Carrier 0x6cb8b4 0x6cb9cc]

entries :: [Entry]
entries =
  [ Entry "xm1_scanout_isr_one" 0x6c6eb0, Entry "xm1_scanout_isr_zero" 0x6c70e4, Entry "xm1_scanout_scheduler_register" 0x6c7318
  , Entry "xm1_scanout_capture" 0x6c926c, Entry "xm1_scanout_item_configuration" 0x6c9cf4, Entry "xm1_scanout_range_packet" 0x6c9d44
  , Entry "xm1_scanout_range_enabled" 0x6c9e7c, Entry "xm1_scanout_group_options" 0x6c9f70, Entry "xm1_scanout_commit" 0x6ca3e0
  , Entry "xm1_scanout_commit_zero" 0x6cb0c4, Entry "xm1_scanout_commit_one" 0x6cb0d8, Entry "xm1_scanout_blank" 0x6cb8b4 ]

canonicalImage4Sha256 :: String
canonicalImage4Sha256 = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"

data Row = Row Word32 Word32 Char String

parseRows :: B.ByteString -> Either ArtifactError [Row]
parseRows bytes = forM (filter (not . C.null) (C.lines bytes)) $ \line -> case C.words line of
  [address, size, kind, name] | Just (letter, rest) <- C.uncons kind, C.null rest -> Row <$> hex address <*> hex size <*> pure letter <*> pure (C.unpack name)
  _ -> Left (SymbolTableRejected (C.unpack line))
  where
    hex text = case readHex (C.unpack text) of
      [(value, "")] | value <= (0xffffffff :: Integer) -> Right (fromIntegral value)
      _ -> Left (SymbolTableRejected (C.unpack text))

symbol :: [Row] -> String -> Maybe Word32
symbol rows name = case [address | Row address _ _ name' <- rows, name' == name] of
  [address] -> Just address
  _ -> Nothing

word :: B.ByteString -> Word32 -> Word32
word bytes address = let offset = fromIntegral (address - image4Base)
  in foldr (\index value -> value * 256 + fromIntegral (B.index bytes (offset + index))) 0 [0 .. 3]

-- | One A32 `b` at each stock entry to its export in the owned fixed range.
publicationsFor :: B.ByteString -> B.ByteString -> Either ArtifactError [Publication]
publicationsFor image4 symbolBytes = do
  rows <- parseRows symbolBytes
  forM entries $ \(Entry name address) -> do
    target <- maybe (Left (MissingExport name)) Right
      (case [value | Row value _ kind name' <- rows, name' == name, kind `elem` ("Tt" :: String)] of [value] -> Just value; _ -> Nothing)
    unless (imageBase + headerBytes <= target && target < imageLimit && target .&. 3 == 0) (Left (ExportOutsideFixed name))
    let delta = (toInteger target - toInteger address - 8) `div` 4
    unless (fromIntegral (B.length image4) >= address - image4Base + 4) (Left SourceMismatch)
    pure (Publication address (word image4 address) (0xea000000 .|. (fromInteger delta .&. 0xffffff)))

canonicalPublications :: B.ByteString -> B.ByteString -> Either ArtifactError [Publication]
canonicalPublications image4 symbolBytes = do
  unless (SHA.sha256Hex (SHA.sha256 image4) == canonicalImage4Sha256) (Left SourceMismatch)
  publicationsFor image4 symbolBytes

-- | The scanout unit's code in the fixed range: from its first text symbol to the end of its last. Only `core` and
-- `alloc` instantiations may lie between them; any other crate's code there is refused.
bodyBounds :: B.ByteString -> Either ArtifactError (Word32, Word32)
bodyBounds symbolBytes = do
  rows <- parseRows symbolBytes
  let text = [row | row@(Row address _ kind _) <- rows, kind `elem` ("Tt" :: String), imageBase + headerBytes <= address, address < imageLimit]
      named fragments (Row _ _ _ name) = any (`isInfixOf` name) fragments
      unit row@(Row _ _ _ name) = named ["_19xm1_display_scanout"] row || any (\(Entry export _) -> export == name) entries
      own = filter unit text
  unless (all (\(Entry export _) -> any (\(Row _ _ _ name) -> name == export) own) entries) (Left BodyBoundsRejected)
  let low = minimum [address | Row address _ _ _ <- own]
      high = maximum [address + size | Row address size _ _ <- own]
      between = [row | row@(Row address _ _ _) <- text, low <= address, address < high]
  unless (high <= imageLimit && all (\row -> unit row || named ["_4core", "_5alloc"] row) between) (Left BodyBoundsRejected)
  Right (low, high)

-- | The `stage-fixed-image` publication manifest.
publicationsValue :: [Publication] -> Value
publicationsValue rows = toJSON [object ["address" .= address, "old" .= old, "new" .= new] | Publication address old new <- rows]

-- | The scanout pair's contract: retired extents, entries, published words and the owned body range.
contractValue :: [Publication] -> (Word32, Word32) -> String -> Value
contractValue rows (low, high) symbolsSha256 = object
  [ "schema" .= (2 :: Int), "subsystem" .= ("display_scanout" :: String), "symbols_sha256" .= symbolsSha256
  , "carriers" .= [object ["low" .= start, "high" .= end] | Carrier start end <- carriers]
  , "entries" .= [object ["name" .= name, "address" .= address] | Entry name address <- entries]
  , "publications" .= [object ["address" .= address, "old" .= old, "new" .= new] | Publication address old new <- rows]
  , "body" .= object ["low" .= low, "high" .= high]
  ]
