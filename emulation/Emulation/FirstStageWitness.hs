{-# LANGUAGE OverloadedStrings #-}
-- Typed receipt and expected-evidence checks for the first-stage witness.
module Emulation.FirstStageWitness
  ( StopLabel (..)
  , ReadLabel (..)
  , Register (..)
  , Registers (..)
  , Core (..)
  , Cpsr (..)
  , GuestAddress (..)
  , WitnessResult (..)
  , Hit (..)
  , ReadRow (..)
  , Receipt (..)
  , Expected (..)
  , ExpectedRead (..)
  , parseReceipt
  , validateReceipt
  , stopName
  , readName
  , decodeLeWord
  , leWordAt
  , encodeLeWord
  , sha256Hex
  ) where

import Control.Monad (forM_, unless, when)
import qualified Crypto.Hash.SHA256 as SHA256
import Data.Aeson (FromJSON, Object, Result (..), Value (..), fromJSON)
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KeyMap
import qualified Data.ByteString as B
import Data.Bits ((.&.), (.|.), shiftL)
import Data.List (nub, sort)
import qualified Data.Text as T
import Data.Word (Word32, Word64, Word8)
import Emulation.NativeObservation (GuestAddress (..))
import FujiAF.FirstStage (Sha256, parseSha256)
import FujiAF.JSON (decodeStrictIntegers)
import Numeric (readHex)

data StopLabel
  = OwnedEntry
  | StorageInit
  | TrailerGate
  | StageImages
  | HandoffVector
  deriving (Eq, Ord, Show, Enum, Bounded)

data ReadLabel
  = OwnedImage
  | HandoffWords
  | SkipList
  | LoaderA
  | LoaderB
  | SegmentTable
  | Image4
  | Image4Bss
  | Image4Arg
  | Image4Publication
  | PeerCells
  | RetainedEventWord
  deriving (Eq, Ord, Show, Enum, Bounded)

data Register = R0 | R1 | R2 | R3
  deriving (Eq, Ord, Show, Enum, Bounded)

data Registers = Registers
  { registerR0 :: Word32
  , registerR1 :: Word32
  , registerR2 :: Word32
  , registerR3 :: Word32
  }
  deriving (Eq, Show)

data Core = Core0 | Core1
  deriving (Eq, Show)

newtype Cpsr = Cpsr Word32
  deriving (Eq, Show)

cpsrWord :: Cpsr -> Word32
cpsrWord (Cpsr value) = value

cpsrMode :: Cpsr -> Word32
cpsrMode value = cpsrWord value .&. 0x1f

cpsrHasIf :: Cpsr -> Bool
cpsrHasIf value = cpsrWord value .&. 0xc0 == 0xc0

cpsrIsA32Little :: Cpsr -> Bool
cpsrIsA32Little value = cpsrWord value .&. 0x0100_0220 == 0

data WitnessResult = Passed | Failed
  deriving (Eq, Show)

data Hit = Hit
  { hitLabel :: StopLabel
  , hitAddress :: GuestAddress
  , hitCore :: Core
  , hitInstructions :: Word64
  , hitRegisters :: Registers
  , hitSp :: GuestAddress
  , hitLr :: GuestAddress
  , hitCpsr :: Cpsr
  }
  deriving (Eq, Show)

data ReadRow = ReadRow
  { rowLabel :: ReadLabel
  , rowAddress :: GuestAddress
  , rowLength :: Int
  , rowSha256 :: Sha256
  , rowHex :: Maybe B.ByteString
  }
  deriving (Eq, Show)

data Receipt = Receipt
  { receiptResult :: WitnessResult
  , receiptError :: Maybe String
  , receiptMachineFailure :: Maybe String
  , receiptInstructions :: Word64
  , receiptStops :: [Hit]
  , receiptReads :: [ReadRow]
  }
  deriving (Eq, Show)

data Expected = Expected
  { expectedStops :: [(StopLabel, GuestAddress)]
  , expectedReads :: [ExpectedRead]
  , expectedHandoffTarget :: GuestAddress
  , expectedHandoffArgument :: GuestAddress
  }
  deriving (Eq, Show)

data ExpectedRead = ExpectedRead
  { expectedLabel :: ReadLabel
  , expectedAddress :: GuestAddress
  , expectedContent :: B.ByteString
  }
  deriving (Eq, Show)

stopName :: StopLabel -> String
stopName label = case label of
  OwnedEntry -> "owned-first-stage-entry"
  StorageInit -> "retained-storage-init"
  TrailerGate -> "retained-trailer-gate"
  StageImages -> "retained-stage-images"
  HandoffVector -> "owned-handoff-vector"

readName :: ReadLabel -> String
readName label = case label of
  OwnedImage -> "owned-image"
  HandoffWords -> "handoff-words"
  SkipList -> "skip-list"
  LoaderA -> "loader-a"
  LoaderB -> "loader-b"
  SegmentTable -> "segment-table"
  Image4 -> "image-4"
  Image4Bss -> "image-4-bss"
  Image4Arg -> "image-4-arg"
  Image4Publication -> "image-4-publication"
  PeerCells -> "peer-cells"
  RetainedEventWord -> "retained-event-word"

parseStopLabel :: String -> Either String StopLabel
parseStopLabel text =
  case [label | label <- [minBound .. maxBound], stopName label == text] of
    [label] -> Right label
    [] -> Left ("unknown witness stop: " ++ text)
    _ -> Left ("ambiguous witness stop: " ++ text)

parseReadLabel :: String -> Either String ReadLabel
parseReadLabel text =
  case [label | label <- [minBound .. maxBound], readName label == text] of
    [label] -> Right label
    [] -> Left ("unknown witness read: " ++ text)
    _ -> Left ("ambiguous witness read: " ++ text)

hexBytes :: String -> Either String B.ByteString
hexBytes text
  | odd (length text) = Left "hex field has odd length"
  | otherwise = B.pack <$> mapM chunk [0, 2 .. length text - 1]
  where
    chunk index =
      case readHex (take 2 (drop index text)) of
        [(value, "")] | value >= 0 && value <= 0xff -> Right (fromIntegral (value :: Integer) :: Word8)
        _ -> Left ("hex field is not hexadecimal: " ++ text)

decodeLeWord :: String -> Either String Word32
decodeLeWord text = do
  bytes <- hexBytes text
  if B.length bytes /= 4
    then Left "little-endian word field must be four bytes"
    else leWordAt bytes 0

leWordAt :: B.ByteString -> Int -> Either String Word32
leWordAt bytes offset
  | offset < 0 || offset + 4 > B.length bytes = Left "little-endian word is outside the extent"
  | otherwise =
      Right
        ( fromIntegral (B.index bytes offset)
            .|. (fromIntegral (B.index bytes (offset + 1)) `shiftL` 8)
            .|. (fromIntegral (B.index bytes (offset + 2)) `shiftL` 16)
            .|. (fromIntegral (B.index bytes (offset + 3)) `shiftL` 24)
        )

encodeLeWord :: Word32 -> B.ByteString
encodeLeWord value =
  B.pack
    [ fromIntegral (value `mod` 0x100)
    , fromIntegral ((value `div` 0x100) `mod` 0x100)
    , fromIntegral ((value `div` 0x10000) `mod` 0x100)
    , fromIntegral ((value `div` 0x1000000) `mod` 0x100)
    ]

hexText :: B.ByteString -> String
hexText bytes = concatMap byte (B.unpack bytes)
  where
    byte value =
      let digits = "0123456789abcdef"
       in [digits !! fromIntegral (value `div` 16), digits !! fromIntegral (value `mod` 16)]

sha256Hex :: B.ByteString -> String
sha256Hex bytes = hexText (SHA256.hash bytes)

exactFields :: Object -> [String] -> Either String ()
exactFields object allowed =
  let actual = sort (map Key.toString (KeyMap.keys object))
      wanted = sort allowed
   in if actual == wanted then Right () else Left ("unexpected witness fields: " ++ show (actual, wanted))

asObject :: String -> Value -> Either String Object
asObject _ (Object object) = Right object
asObject name value = Left (name ++ " is not an object: " ++ show value)

lookupField :: Object -> String -> Either String Value
lookupField object name =
  maybe (Left ("missing witness field: " ++ name)) Right (KeyMap.lookup (Key.fromString name) object)

textField :: Object -> String -> Either String String
textField object name = jsonField object name

optionalTextField :: Object -> String -> Either String (Maybe String)
optionalTextField object name =
  case KeyMap.lookup (Key.fromString name) object of
    Nothing -> Right Nothing
    Just Null -> Right Nothing
    Just (String text) -> Right (Just (T.unpack text))
    Just _ -> Left ("witness field is not a string or null: " ++ name)

jsonField :: FromJSON a => Object -> String -> Either String a
jsonField object name = do
  value <- lookupField object name
  case fromJSON value of
    Error message -> Left ("witness field " ++ name ++ ": " ++ message)
    Success parsed -> Right parsed

parseAddress :: Object -> String -> Either String GuestAddress
parseAddress object name = do
  value <- jsonField object name :: Either String Word64
  if value <= 0xffffffff
    then Right (GuestAddress value)
    else Left ("witness address is outside 32 bits: " ++ name)

parseCore :: Object -> String -> Either String Core
parseCore object name = do
  value <- jsonField object name :: Either String Int
  case value of
    0 -> Right Core0
    1 -> Right Core1
    _ -> Left ("witness core is not 0 or 1: " ++ show value)

parseCpsr :: Object -> String -> Either String Cpsr
parseCpsr object name = Cpsr <$> (jsonField object name :: Either String Word32)

parseRegisters :: Object -> Either String Registers
parseRegisters object = do
  exactFields object ["r0", "r1", "r2", "r3"]
  Registers
    <$> (jsonField object "r0" :: Either String Word32)
    <*> (jsonField object "r1" :: Either String Word32)
    <*> (jsonField object "r2" :: Either String Word32)
    <*> (jsonField object "r3" :: Either String Word32)

parseResult :: String -> Either String WitnessResult
parseResult text = case text of
  "passed" -> Right Passed
  "failed" -> Right Failed
  _ -> Left ("unknown witness result: " ++ text)

parseHit :: Value -> Either String Hit
parseHit value = do
  object <- asObject "hit" value
  exactFields object ["label", "address", "core", "instructions", "registers", "sp", "lr", "cpsr"]
  siteText <- textField object "label"
  site <- parseStopLabel siteText
  address <- parseAddress object "address"
  core <- parseCore object "core"
  instructions <- jsonField object "instructions" :: Either String Word64
  registerObject <- lookupField object "registers" >>= asObject "registers"
  registers <- parseRegisters registerObject
  sp <- parseAddress object "sp"
  lr <- parseAddress object "lr"
  cpsr <- parseCpsr object "cpsr"
  pure (Hit site address core instructions registers sp lr cpsr)

parseRead :: Value -> Either String ReadRow
parseRead value = do
  object <- asObject "read" value
  exactFields object ["label", "address", "length", "sha256", "hex"]
  labelText <- textField object "label"
  label <- parseReadLabel labelText
  address <- parseAddress object "address"
  lengthValue <- jsonField object "length" :: Either String Int
  if lengthValue < 0
    then Left "negative witness read length"
    else pure ()
  digestText <- textField object "sha256"
  digest <- parseSha256 digestText
  hexField <- optionalTextField object "hex"
  hexValue <- case hexField of
    Nothing -> pure Nothing
    Just text -> do
      bytes <- hexBytes text
      if B.length bytes == lengthValue
        then pure (Just bytes)
        else Left "witness hex length differs from length field"
  pure (ReadRow label address lengthValue digest hexValue)

parseReceipt :: B.ByteString -> Either String Receipt
parseReceipt bytes = do
  value <- decodeStrictIntegers bytes
  object <- asObject "receipt" value
  exactFields object ["schema", "result", "error", "instructions", "machine_failure", "stops", "reads"]
  schema <- jsonField object "schema" :: Either String Int
  if schema /= 1 then Left "witness receipt schema must be 1" else pure ()
  resultText <- textField object "result"
  result <- parseResult resultText
  errorText <- optionalTextField object "error"
  failureText <- optionalTextField object "machine_failure"
  instructions <- jsonField object "instructions" :: Either String Word64
  stops <- jsonField object "stops" >>= mapM parseHit
  readRows <- jsonField object "reads" >>= mapM parseRead
  pure (Receipt result errorText failureText instructions stops readRows)

validateReceipt :: Expected -> Receipt -> Either String ()
validateReceipt expected receipt = do
  unless (receiptResult receipt == Passed) (Left "witness result is not passed")
  when (receiptError receipt /= Nothing) (Left "passed witness carries an error")
  when (receiptMachineFailure receipt /= Nothing) (Left "passed witness carries a machine failure")
  let stops = receiptStops receipt
      expectedOrder = map fst (expectedStops expected)
      actualOrder = map hitLabel stops
  unless (actualOrder == expectedOrder) (Left ("witness stop order differs: " ++ show actualOrder))
  unless (nub actualOrder == actualOrder) (Left "witness stops repeat")
  forM_ (zip stops (expectedStops expected)) $ \(hitValue, (_, address)) -> do
    unless (hitCore hitValue == Core0) (Left "witness stop is not on core 0")
    unless (hitAddress hitValue == address) (Left "witness stop address differs")
  let positions = map hitInstructions stops
  unless (and (zipWith (<=) positions (drop 1 positions))) (Left "witness stops are not in instruction order")
  entry <- findStop OwnedEntry stops
  unless (hitSp entry == GuestAddress 0xfff00f00) (Left "owned entry did not see the ROM handoff stack")
  unless (cpsrMode (hitCpsr entry) == 0x13) (Left "owned entry was not in SVC")
  unless (cpsrHasIf (hitCpsr entry)) (Left "owned entry did not see IF set")
  unless (cpsrIsA32Little (hitCpsr entry)) (Left "owned entry was not A32 little-endian")
  unless (register entry R0 == Just 0) (Left "owned entry r0 is not the ROM handoff 0")
  unless (register entry R1 == Just 2) (Left "owned entry r1 is not the ROM handoff 2")
  stage <- findStop StageImages stops
  unless (register stage R0 == Just 0) (Left "stage entry did not receive status 0")
  unless (register stage R1 == Just 0) (Left "stage entry did not receive core 0")
  handoff <- findStop HandoffVector stops
  unless (cpsrMode (hitCpsr handoff) == 0x13) (Left "handoff vector was not in SVC")
  unless (cpsrHasIf (hitCpsr handoff)) (Left "handoff vector did not see IF set")
  unless (cpsrIsA32Little (hitCpsr handoff)) (Left "handoff vector was not A32 little-endian")
  unless (register handoff R0 == Just (fromIntegral (guestAddress (expectedHandoffArgument expected)))) (Left "handoff r0 differs from the argument")
  unless (register handoff R2 == Just (fromIntegral (guestAddress (expectedHandoffTarget expected)))) (Left "handoff r2 differs from the target")
  unless (register handoff R3 == Just 0x4300) (Left "handoff r3 is not the 0x4300 word address")
  let expectedLabels = map expectedLabel (expectedReads expected)
      actualLabels = map rowLabel (receiptReads receipt)
  unless (sort actualLabels == sort expectedLabels) (Left ("witness reads differ: " ++ show actualLabels))
  unless (nub actualLabels == actualLabels) (Left "witness reads repeat")
  forM_ (expectedReads expected) $ \expectedRead -> do
    row <- findRead (expectedLabel expectedRead) (receiptReads receipt)
    unless (rowAddress row == expectedAddress expectedRead) (Left ("witness read address differs: " ++ readName (expectedLabel expectedRead)))
    unless (rowLength row == B.length (expectedContent expectedRead)) (Left ("witness read length differs: " ++ readName (expectedLabel expectedRead)))
    expectedDigest <- either (Left . ("expected digest: " ++)) Right (parseSha256 (sha256Hex (expectedContent expectedRead)))
    unless (rowSha256 row == expectedDigest) (Left ("witness read digest differs: " ++ readName (expectedLabel expectedRead)))
    case rowHex row of
      Nothing -> pure ()
      Just bytes -> unless (bytes == expectedContent expectedRead) (Left ("witness read bytes differ: " ++ readName (expectedLabel expectedRead)))
  where
    findStop label stops = maybe (Left ("witness stop missing: " ++ stopName label)) Right (findHit label stops)
    findHit _ [] = Nothing
    findHit label (hitValue : rest)
      | hitLabel hitValue == label = Just hitValue
      | otherwise = findHit label rest
    findRead label rows = maybe (Left ("witness read missing: " ++ readName label)) Right (findRow label rows)
    findRow _ [] = Nothing
    findRow label (row : rest)
      | rowLabel row == label = Just row
      | otherwise = findRow label rest
    register hitValue registerValue =
      case registerValue of
        R0 -> Just (registerR0 (hitRegisters hitValue))
        R1 -> Just (registerR1 (hitRegisters hitValue))
        R2 -> Just (registerR2 (hitRegisters hitValue))
        R3 -> Just (registerR3 (hitRegisters hitValue))
