{-# LANGUAGE OverloadedStrings #-}

-- | An owned unit of the X-M1 1.50 plain image, read from its `plain-overlay-unit.json`: the
-- overlay it lives in, the stock extents its ELF replaces, the stock table words outside them it
-- republishes, its entries, the RAM it owns and the census words reviewed as data.
module Replacement.PlainOverlay.Unit
  ( Overlay (..)
  , overlaySpan
  , overlayNumber
  , overlayOf
  , plainBase
  , Span (..)
  , inside
  , Row (..)
  , rowSpan
  , Entry (..)
  , Scanned (..)
  , scannedName
  , ReviewedData (..)
  , Unit (..)
  , unitSpans
  , parseUnit
  , unitValue
  , hexWord
  ) where

import Control.Monad (forM_, unless, when)
import Data.Aeson (FromJSON (..), Value, object, withObject, (.:), (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Types as A
import qualified Data.ByteString as B
import Data.List (sortOn, tails)
import qualified Data.Text as T
import Data.Word (Word32)
import Numeric (readHex, showHex)

-- | The three overlays of the plain image, each linked at its own address.
data Overlay = Overlay44 | Overlay46 | Overlay47
  deriving (Eq, Ord, Show, Enum, Bounded)

overlayNumber :: Overlay -> Int
overlayNumber overlay = case overlay of
  Overlay44 -> 44
  Overlay46 -> 46
  Overlay47 -> 47

overlaySpan :: Overlay -> Span
overlaySpan overlay = case overlay of
  Overlay44 -> Span 0x01100000 0x011ac5a0
  Overlay46 -> Span 0x011c0000 0x011dcca0
  Overlay47 -> Span 0x011e0000 0x0144a5e0

-- | The overlay holding a plain-image address.
overlayOf :: Word32 -> Maybe Overlay
overlayOf address = case [o | o <- [minBound .. maxBound], inside (overlaySpan o) address] of
  [o] -> Just o
  _ -> Nothing

plainBase :: Word32
plainBase = 0x01100000

-- | A half-open guest address range.
data Span = Span {spanStart :: Word32, spanEnd :: Word32}
  deriving (Eq, Ord, Show)

inside :: Span -> Word32 -> Bool
inside (Span s e) a = s <= a && a < e

-- | A stock table word the ELF republishes, with the stock value it replaces.
data Row = Row {rowAddress :: Word32, rowStock :: Word32}
  deriving (Eq, Show)

rowSpan :: Row -> Span
rowSpan (Row a _) = Span a (a + 4)

-- | A stock address callers enter; the ELF holds an A32 branch there into the unit. The entry
-- reads `entryArguments` registers from r0, returns a value in r0 when `entryReturns`, and writes
-- the buffers `entryOutputs` names: (argument register, byte count).
data Entry = Entry {entryName :: String, entryAddress :: Word32, entryArguments :: Int, entryReturns :: Bool, entryOutputs :: [(Int, Int)]}
  deriving (Eq, Show)

-- | The images the reference census scans.
data Scanned = Image4 | Image5 | Plain | LoaderA | LoaderB | Image2 | Image3
  deriving (Eq, Ord, Show, Enum, Bounded)

scannedName :: Scanned -> String
scannedName scanned = case scanned of
  Image4 -> "image4"
  Image5 -> "image5"
  Plain -> "plain"
  LoaderA -> "loader_a"
  LoaderB -> "loader_b"
  Image2 -> "image2"
  Image3 -> "image3"

-- | A census site whose word was read and judged data.
data ReviewedData = ReviewedData {reviewedImage :: Scanned, reviewedSite :: Word32, reviewedWord :: Word32, reviewedReason :: String}
  deriving (Eq, Show)

data Unit = Unit
  { unitName :: String
  , unitOverlay :: Overlay
  , unitExtents :: [Span]
  , unitRows :: [Row]
  , unitEntries :: [Entry]
  , unitState :: [Span]
  , unitReviewed :: [ReviewedData]
  }
  deriving (Eq, Show)

-- | Every byte range the unit's ELF writes: the extents and the rows.
unitSpans :: Unit -> [Span]
unitSpans unit = unitExtents unit ++ map rowSpan (unitRows unit)

hexWord :: Word32 -> String
hexWord w = "0x" ++ showHex w ""

newtype Hex = Hex Word32

instance FromJSON Hex where
  parseJSON = A.withText "hex word" $ \text -> case T.unpack <$> T.stripPrefix "0x" text of
    Just digits | [(v, "")] <- readHex digits, v <= toInteger (maxBound :: Word32) -> pure (Hex (fromInteger v))
    _ -> fail ("not a 0x-prefixed 32-bit hex word: " ++ T.unpack text)

pairSpan :: [Hex] -> A.Parser Span
pairSpan [Hex s, Hex e] = pure (Span s e)
pairSpan _ = fail "a span is [start, end]"

instance FromJSON Unit where
  parseJSON = withObject "plain overlay unit" $ \o -> do
    schema <- o .: "schema"
    unless (schema == (1 :: Int)) (fail "schema must be 1")
    name <- o .: "unit"
    number <- o .: "overlay"
    overlay <- case [v | v <- [minBound .. maxBound], overlayNumber v == number] of
      [v] -> pure v
      _ -> fail ("no overlay " ++ show number)
    extents <- mapM pairSpan =<< o .: "extents"
    rows <- mapM (withObject "row" (\r -> (\(Hex a) (Hex v) -> Row a v) <$> r .: "address" <*> r .: "stock")) =<< o .: "rows"
    entries <- mapM entry =<< o .: "entries"
    state <- mapM pairSpan =<< o .: "state"
    reviewed <- mapM reviewedEntry =<< o .: "reviewed_data"
    pure (Unit name overlay extents rows entries state reviewed)
    where
      entry = withObject "entry" $ \r -> do
        Hex address <- r .: "address"
        outputs <- mapM (withObject "output" (\b -> (,) <$> b .: "register" <*> b .: "bytes")) =<< r .: "outputs"
        (\n a ret -> Entry n address a ret outputs) <$> r .: "name" <*> r .: "arguments" <*> r .: "returns"
      reviewedEntry = withObject "reviewed word" $ \r -> do
        imageText <- r .: "image"
        image <- case [s | s <- [minBound .. maxBound], scannedName s == imageText] of
          [s] -> pure s
          _ -> fail ("no scanned image " ++ imageText)
        Hex site <- r .: "site"
        Hex word <- r .: "word"
        ReviewedData image site word <$> r .: "reason"

-- | A unit manifest, refused unless its extents and rows are word-aligned, inside its overlay and
-- disjoint, and every entry is a word inside an extent.
parseUnit :: B.ByteString -> Either String Unit
parseUnit bytes = do
  unit <- A.eitherDecodeStrict' bytes
  let name = unitName unit
      refuse message = Left (name ++ ": " ++ message)
      overlay = overlaySpan (unitOverlay unit)
      aligned w = w `mod` 4 == 0
      within (Span s e) = spanStart overlay <= s && e <= spanEnd overlay
      spans = sortOn spanStart (unitSpans unit)
  when (null (unitExtents unit)) (refuse "declares no extent")
  forM_ (unitSpans unit) $ \span'@(Span s e) -> do
    unless (s < e && aligned s && aligned e) (refuse ("span " ++ hexWord s ++ ".." ++ hexWord e ++ " is empty or unaligned"))
    unless (within span') (refuse ("span " ++ hexWord s ++ " leaves overlay " ++ show (overlayNumber (unitOverlay unit))))
  forM_ [(a, b) | (a : rest) <- tails spans, b <- take 1 rest] $ \(Span _ e, Span s' _) ->
    unless (e <= s') (refuse ("spans overlap at " ++ hexWord s'))
  forM_ (unitEntries unit) $ \(Entry entry address arguments _ outputs) -> do
    unless (aligned address && any (`inside` address) (unitExtents unit)) (refuse ("entry " ++ entry ++ " is not a word inside an extent"))
    unless (0 <= arguments && arguments <= 4 && all (\(r, n) -> 0 <= r && r < arguments && n > 0) outputs) (refuse ("entry " ++ entry ++ " has register arguments outside r0..r3 or an empty output"))
  forM_ (unitState unit) $ \(Span s e) -> unless (s < e) (refuse "empty state span")
  pure unit

unitValue :: Unit -> Value
unitValue unit =
  object
    [ "schema" .= (1 :: Int)
    , "unit" .= unitName unit
    , "overlay" .= overlayNumber (unitOverlay unit)
    , "extents" .= map spanValue (unitExtents unit)
    , "rows" .= [object ["address" .= hexWord a, "stock" .= hexWord v] | Row a v <- unitRows unit]
    , "entries" .=
        [ object ["name" .= n, "address" .= hexWord a, "arguments" .= k, "returns" .= ret, "outputs" .= [object ["register" .= r, "bytes" .= b] | (r, b) <- outs]]
        | Entry n a k ret outs <- unitEntries unit
        ]
    , "state" .= map spanValue (unitState unit)
    , "reviewed_data" .= [object ["image" .= scannedName i, "site" .= hexWord s, "word" .= hexWord w, "reason" .= r] | ReviewedData i s w r <- unitReviewed unit]
    ]
  where
    spanValue (Span s e) = [hexWord s, hexWord e]
