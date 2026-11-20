-- | The declared abstraction map between Root's owned state and the stock saclac state the
-- stock 0x3b4b wake step reads (issue 117, X-M1 1.50).
--
-- The map itself is qualification-only Rust code beside Root's types, linked into its own
-- ELF (`//replacement/saclac:saclac-qualification`) and never into a production image. This
-- module holds what the harness needs to drive it: the qualification ELF's symbols and field
-- table, the stock overlay the map writes, and the stock-private data the overlay must cover.
module Replacement.Saclac.StateMap
  ( -- * Symbols
    Symbol (..)
  , parseSymbols
  , symbolNamed
  , symbolPath
    -- * Fields
  , Field (..)
  , parseFields
  , fieldsBytes
  , extractOverlay
  , applyOverlay
  , fieldAt
    -- * Stock-private data
  , stockPrivate
  , unmappedExposed
  , abstractDifference
  ) where

import qualified Data.ByteString as B
import Data.Bits (shiftL, (.|.))
import Data.Char (isHexDigit)
import Data.List (isSuffixOf)
import Data.Word (Word32)
import Numeric (readHex)
import Replacement.Saclac.CycleStep (Span (..), State, overlayState, spanAt, stateSlice)

-- ---------------------------------------------------------------------------
-- Symbols

-- | One `llvm-nm -S --defined-only --numeric-sort` row; absolute and other unsized symbols
-- carry no size.
data Symbol = Symbol {symbolAddress :: Word32, symbolSize :: Maybe Word32, symbolType :: Char, symbolName :: String}
  deriving (Eq, Show)

parseSymbols :: String -> Either String [Symbol]
parseSymbols text = mapM row (filter (not . null . words) (lines text))
  where
    row line = case words line of
      [address, size, [kind], name] -> Symbol <$> hex address <*> (Just <$> hex size) <*> pure kind <*> pure name
      [address, [kind], name] -> Symbol <$> hex address <*> pure Nothing <*> pure kind <*> pure name
      _ -> Left ("unreadable symbol row: " ++ line)
    hex digits
      | not (null digits) && all isHexDigit digits, [(value, "")] <- readHex digits, value <= (0xffffffff :: Integer) = Right (fromIntegral value)
      | otherwise = Left ("unreadable symbol value " ++ digits)

-- | The one symbol with this exact name.
symbolNamed :: [Symbol] -> String -> Either String Symbol
symbolNamed symbols name = case [symbol | symbol <- symbols, symbolName symbol == name] of
  [symbol] -> Right symbol
  [] -> Left ("no symbol " ++ name)
  _ -> Left ("symbol " ++ name ++ " is defined more than once")

-- | The one v0-mangled Rust item whose path ends in these components, as in
-- `xm1_saclac_root::stock::composition::OWNER`.
symbolPath :: [Symbol] -> [String] -> Either String Symbol
symbolPath symbols components = case [symbol | symbol <- symbols, take 2 (symbolName symbol) == "_R", suffix `isSuffixOf` symbolName symbol] of
  [symbol] -> Right symbol
  [] -> Left ("no item " ++ path)
  _ -> Left ("item " ++ path ++ " names more than one symbol")
  where
    suffix = concat [show (length component) ++ component | component <- components]
    path = foldr1 (\a rest -> a ++ "::" ++ rest) components

-- ---------------------------------------------------------------------------
-- Fields

-- | One mapped stock field: a stock-layout byte range the map writes.
data Field = Field {fieldAddress :: Integer, fieldLength :: Int}
  deriving (Eq, Show)

-- | The field table the qualification ELF exports: `count` (address, length) word pairs.
parseFields :: Word32 -> B.ByteString -> Word32 -> Word32 -> Either String [Field]
parseFields base image countAddress tableAddress = do
  count <- word countAddress
  if count == 0 || count > 512 then Left ("the field table declares " ++ show count ++ " fields") else Right ()
  fields <- mapM (\index -> Field <$> (fromIntegral <$> word (tableAddress + 8 * index)) <*> (fromIntegral <$> word (tableAddress + 8 * index + 4))) [0 .. count - 1]
  if any ((<= 0) . fieldLength) fields then Left "a field is empty" else Right ()
  if fieldsBytes fields > 4096 then Left "the overlay exceeds 4096 bytes" else Right ()
  if overlapping fields then Left "two fields overlap" else Right fields
  where
    word address
      | address >= base && fromIntegral (address - base) + 4 <= B.length image =
          let offset = fromIntegral (address - base)
              byte k = fromIntegral (B.index image (offset + k)) :: Word32
           in Right (byte 0 .|. byte 1 `shiftL` 8 .|. byte 2 `shiftL` 16 .|. byte 3 `shiftL` 24)
      | otherwise = Left ("field table word " ++ show address ++ " lies outside the qualification image")
    overlapping fields = or [overlap a c | (i, a) <- zip [0 :: Int ..] fields, (j, c) <- zip [0 ..] fields, i < j]
    overlap a c = fieldAddress a < fieldAddress c + fromIntegral (fieldLength c) && fieldAddress c < fieldAddress a + fromIntegral (fieldLength a)

fieldsBytes :: [Field] -> Int
fieldsBytes = sum . map fieldLength

-- | The stock-layout bytes of every field, concatenated in table order.
extractOverlay :: [Field] -> State -> Either String B.ByteString
extractOverlay fields state = B.concat <$> mapM slice fields
  where
    slice field = maybe (Left ("the state does not hold field " ++ show (fieldAddress field))) Right (stateSlice state (fieldAddress field) (fieldLength field))

-- | The state with an overlay's bytes written at their fields.
applyOverlay :: [Field] -> B.ByteString -> State -> Either String State
applyOverlay fields overlay state
  | B.length overlay /= fieldsBytes fields = Left "the overlay does not match the field table"
  | otherwise = go state fields overlay
  where
    go acc [] _ = Right acc
    go acc (field : rest) bytes = do
      next <- overlayState acc (fieldAddress field) (B.take (fieldLength field) bytes)
      go next rest (B.drop (fieldLength field) bytes)

fieldAt :: [Field] -> Integer -> Bool
fieldAt fields address = any (\field -> fieldAddress field <= address && address < fieldAddress field + fromIntegral (fieldLength field)) fields

-- ---------------------------------------------------------------------------
-- Stock-private data

-- | The data the replaced stock saclac code (0x00ef8ad0..0x00f473a4) owns: the x70view
-- 55436fc85c0560dd252b0410af6f2427fe198cd3 X-M1 1.50 data symbols whose translation unit lies
-- in that code range run contiguously from 0x0169cf00 to 0x016c73f0, with two further runs at
-- 0x016caf00 (mode_rate_controller) and 0x016cd228 (s54u_rate_workspace). Root keeps none of
-- it except the census shared spans; every byte of it the stock step reads before writing must
-- come from the map.
stockPrivate :: [Span]
stockPrivate = [Span 0x0169cf00 0x016c73f0, Span 0x016caf00 0x016cc960, Span 0x016cd228 0x016cd2a0]

private :: [Span] -> Integer -> Bool
private shared address = any (`spanAt` address) stockPrivate && not (any (`spanAt` address) shared)

-- | The exposed reads (read before the step wrote them) of stock-private data outside the
-- census shared spans that no field supplies: each is state the map leaves undefined.
unmappedExposed :: [Span] -> [Field] -> [Integer] -> [Integer]
unmappedExposed shared fields = filter (\address -> private shared address && not (fieldAt fields address))

-- | Whether a next-state difference at this address belongs to the abstract state: retained
-- memory and mapped fields do; stock-private scratch outside the fields does not, because the
-- stock step writes it before reading it (checked per cycle through `unmappedExposed`).
abstractDifference :: [Span] -> [Field] -> Integer -> Bool
abstractDifference shared fields address = fieldAt fields address || not (private shared address)
