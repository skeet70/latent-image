{-# LANGUAGE DeriveAnyClass, DeriveGeneric, GeneralizedNewtypeDeriving, DerivingStrategies, OverloadedStrings #-}
module Main where

import Control.Monad (unless)
import Data.Aeson (FromJSON(..), ToJSON(..), eitherDecodeStrict', encode, withObject, (.:))
import Data.Bits (testBit, (.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy.Char8 as BL
import Data.Char (digitToInt, intToDigit, isHexDigit)
import qualified Data.Text as T
import Data.Word (Word32)
import GHC.Generics (Generic)
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import System.Environment (getArgs)

newtype ObservedWord = ObservedWord Word32 deriving newtype (Eq, Show, ToJSON)
newtype CapturedBytes = CapturedBytes B.ByteString deriving stock (Eq, Show)
instance ToJSON CapturedBytes where
  toJSON (CapturedBytes bytes) = toJSON (B.foldr hex [] bytes)
    where hex byte rest = intToDigit (fromIntegral byte `div` 16) : intToDigit (fromIntegral byte `mod` 16) : rest

newtype Walk = Walk Word32 deriving newtype (ToJSON)
newtype Task = Task Word32 deriving newtype (ToJSON)
newtype Epoch = Epoch Word32 deriving newtype (ToJSON)
newtype Address = Address Word32 deriving newtype (ToJSON)
newtype ByteCount = ByteCount Word32 deriving newtype (ToJSON)
newtype Generation = Generation Word32 deriving newtype (ToJSON)

data Phase = Unused | Recording | Complete deriving stock (Eq, Enum, Bounded, Generic) deriving anyclass (ToJSON)
data File = FileA | FileB | NoFile deriving stock (Generic) deriving anyclass (ToJSON)
data FileClass = NotCandidate | Unsafe | Valid | UnexpectedClass ObservedWord deriving stock (Generic) deriving anyclass (ToJSON)
data BankStatus = Ok | NotReady | ReadOnly | Full | Io | Invalid | Unset | Range | AtC | NoBank | GenerationExhausted | TaskRefused | Missing | MediaChanged | ApplyRefused | UnsafeBank | ModeChanged | SlotSelected deriving stock (Enum, Bounded, Generic) deriving anyclass (ToJSON)
data Status = KnownStatus BankStatus | UnexpectedStatus ObservedWord deriving stock (Generic) deriving anyclass (ToJSON)
data Stage = Scan | Path | OpenReturn | OpenResult | StatReturn | StatSize | ReadReturn | ReadResult | BeforeClose | CloseReturn | CloseResult | ReadBytes | AfterClass | ClassResult | ScanResult deriving stock (Eq, Enum, Bounded, Generic) deriving anyclass (ToJSON)
data Flag = OtherWalk | UntaggedLoad | LoadStarted | LoadReturned | WinnerReached deriving stock (Eq, Enum, Bounded, Generic) deriving anyclass (ToJSON)

data SharedCompletion = SharedCompletion
  { producerReturn :: ObservedWord, sharedStatus :: ObservedWord, sharedValue :: ObservedWord, sharedDone :: ObservedWord
  } deriving stock (Generic) deriving anyclass (ToJSON)
data ScanContext = ScanContext
  { task :: Task, epoch :: Epoch, bufferAddress :: Address
  } deriving stock (Generic) deriving anyclass (ToJSON)
data PathObservation = PathObservation
  { pathBytes :: CapturedBytes, volume :: ObservedWord
  } deriving stock (Generic) deriving anyclass (ToJSON)
data OpenObservation = OpenObservation
  { openStatus :: Status, handle :: ObservedWord
  } deriving stock (Generic) deriving anyclass (ToJSON)
data ReadObservation = ReadObservation
  { readStatus :: Status, readCount :: ByteCount
  } deriving stock (Generic) deriving anyclass (ToJSON)
data Classification = Classification
  { fileClass :: FileClass, generation :: Generation
  } deriving stock (Generic) deriving anyclass (ToJSON)
data CandidateSnapshot = CandidateSnapshot
  { snapshotEpoch :: Epoch, candidateBytes :: CapturedBytes
  } deriving stock (Generic) deriving anyclass (ToJSON)
data Candidate = Candidate
  { stages :: [Stage]
  , scanContext :: Maybe ScanContext
  , path :: Maybe PathObservation
  , openSharedCompletion :: Maybe SharedCompletion
  , openResult :: Maybe OpenObservation
  , statReturn :: Maybe ObservedWord
  , statSize :: Maybe ByteCount
  , readSharedCompletion :: Maybe SharedCompletion
  , readResult :: Maybe ReadObservation
  , beforeCloseResult :: Maybe Status
  , closeSharedCompletion :: Maybe SharedCompletion
  , closeResult :: Maybe Status
  , classification :: Maybe Classification
  , scanResult :: Maybe Status
  , afterRead :: Maybe CandidateSnapshot
  , afterClass :: Maybe CandidateSnapshot
  , snapshotsEqual :: Maybe Bool
  } deriving stock (Generic) deriving anyclass (ToJSON)
data Discovery = Discovery
  { phase :: Phase, walk :: Walk, entryTask :: Task, discoveryEpoch :: Epoch
  , flags :: [Flag], loadReturn :: Maybe Status, loadOutcome :: Maybe Status
  , winner :: Maybe File, noneReturn :: Maybe Status, endEpoch :: Maybe Epoch
  , candidateA :: Candidate, candidateB :: Candidate
  } deriving stock (Generic) deriving anyclass (ToJSON)

snapshotSize :: Int
snapshotSize = 4424

checkedEnum :: (Enum a, Bounded a) => String -> Word32 -> Either String a
checkedEnum label value = checked
  where
    checked = if value <= fromIntegral (fromEnum (maxBound `asTypeOf` result)) then Right result else Left (label ++ ": unknown value")
    result = toEnum (fromIntegral value)

status :: Word32 -> Status
status value = either (const (UnexpectedStatus (ObservedWord value))) KnownStatus (checkedEnum "bank status" value)

flagList :: (Enum a, Bounded a) => Word32 -> Either String [a]
flagList bits = result
  where
    choices = [minBound .. maxBound]
    known = 2 ^ length choices - 1
    result = if bits .&. known == bits then Right [choice | choice <- choices, testBit bits (fromEnum choice)] else Left "unknown diagnostic flag bits"

observed :: Bool -> a -> Maybe a
observed True value = Just value
observed False _ = Nothing

parseCandidate :: B.ByteString -> Either String Candidate
parseCandidate bytes = do
  reached <- flagList (word 0)
  let seen stage = stage `elem` reached
      snapshot stage offset epochOffset =
        if not (seen stage) then Right Nothing else do
          unless (word 128 == 512 || word 128 == 1024) (Left "candidate snapshot size must be 512 or 1024")
          pure (Just (CandidateSnapshot (Epoch (word epochOffset)) (CapturedBytes (B.take (fromIntegral (word 128)) (B.drop offset bytes)))))
  before <- snapshot ReadBytes 140 132
  after <- snapshot AfterClass 1164 136
  pure Candidate
    { stages = reached
    , scanContext = observed (seen Scan) (ScanContext (Task (word 4)) (Epoch (word 8)) (Address (word 12)))
    , path = observed (seen Path) (PathObservation (CapturedBytes (B.take 16 (B.drop 16 bytes))) (raw 32))
    , openSharedCompletion = observed (seen OpenReturn) (completion 36)
    , openResult = observed (seen OpenResult) (OpenObservation (status (word 52)) (raw 56))
    , statReturn = observed (seen StatReturn) (raw 60)
    , statSize = observed (seen StatSize) (ByteCount (word 64))
    , readSharedCompletion = observed (seen ReadReturn) (completion 68)
    , readResult = observed (seen ReadResult) (ReadObservation (status (word 84)) (ByteCount (word 88)))
    , beforeCloseResult = observed (seen BeforeClose) (status (word 92))
    , closeSharedCompletion = observed (seen CloseReturn) (completion 96)
    , closeResult = observed (seen CloseResult) (status (word 112))
    , classification = observed (seen ClassResult) (Classification (case word 116 of 0 -> NotCandidate; 1 -> Unsafe; 2 -> Valid; other -> UnexpectedClass (ObservedWord other)) (Generation (word 120)))
    , scanResult = observed (seen ScanResult) (status (word 124))
    , afterRead = before
    , afterClass = after
    , snapshotsEqual = (==) <$> (candidateBytes <$> before) <*> (candidateBytes <$> after)
    }
  where
    word = U.word32At bytes
    raw = ObservedWord . word
    completion offset = SharedCompletion (raw offset) (raw (offset + 4)) (raw (offset + 8)) (raw (offset + 12))

parseDiscovery :: B.ByteString -> Either String Discovery
parseDiscovery bytes = do
  unless (B.length bytes == snapshotSize) (Left "DSC1 snapshot must be exactly 4424 bytes")
  unless (word 0 == 0x44534331 && word 4 == fromIntegral snapshotSize) (Left "unsupported discovery format or size")
  capturePhase <- checkedEnum "phase" (word 8)
  observedFlags <- flagList (word 24)
  chosen <- if WinnerReached `elem` observedFlags then Just <$> case word 36 of
    0 -> Right FileA
    1 -> Right FileB
    255 -> Right NoFile
    _ -> Left "unknown winning file"
    else Right Nothing
  first <- parseCandidate (B.take 2188 (B.drop 48 bytes))
  second <- parseCandidate (B.drop 2236 bytes)
  pure Discovery
    { phase = capturePhase, walk = Walk (word 12), entryTask = Task (word 16), discoveryEpoch = Epoch (word 20)
    , flags = observedFlags
    , loadReturn = observed (LoadReturned `elem` observedFlags) (status (word 28))
    , loadOutcome = observed (LoadReturned `elem` observedFlags) (status (word 32))
    , winner = chosen
    , noneReturn = observed (capturePhase == Complete) (status (word 40))
    , endEpoch = observed (capturePhase == Complete) (Epoch (word 44))
    , candidateA = first, candidateB = second
    }
  where word = U.word32At bytes

newtype Observation = Observation B.ByteString
newtype ObservationWindow = ObservationWindow (Maybe B.ByteString)
instance FromJSON ObservationWindow where
  parseJSON = withObject "observation read" $ \window -> do
    label <- window .: "label"
    if label /= ("discovery" :: T.Text) then pure (ObservationWindow Nothing) else do
      text <- window .: "bytes"
      unless (T.length text == snapshotSize * 2) (fail "discovery read has the wrong hex length")
      bytes <- either fail pure (hexBytes (T.unpack text))
      pure (ObservationWindow (Just bytes))
    where
      hexBytes [] = Right B.empty
      hexBytes text = B.pack <$> pairs text
      pairs [] = Right []
      pairs (a:b:rest) | isHexDigit a && isHexDigit b = ((fromIntegral (16 * digitToInt a + digitToInt b)) :) <$> pairs rest
      pairs _ = Left "invalid discovery hex bytes"
instance FromJSON Observation where
  parseJSON = withObject "emulator observation" $ \document -> do
    windows <- document .: "reads"
    case [bytes | ObservationWindow (Just bytes) <- windows] of
      [bytes] -> pure (Observation bytes)
      _ -> fail "observation must contain exactly one discovery read"

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["--help"] -> putStrLn "Usage: decode-recipe-discovery SNAPSHOT.bin | --observation E2E_OBSERVATION.json\nHost-only DSC1 decoder. Null fields were not observed. Shared completion words do not prove callback ownership; equal before/after bytes do not prove immutability during classification."
    _ -> do
      bytes <- case arguments of
        [pathName] -> H.readRegular snapshotSize pathName
        ["--observation", pathName] -> do
          document <- H.readRegular (16 * 1024 * 1024) pathName
          Observation captured <- either H.failH pure (eitherDecodeStrict' document)
          pure captured
        _ -> H.failH "expected SNAPSHOT.bin or --observation E2E_OBSERVATION.json"
      decoded <- either H.failH pure (parseDiscovery bytes)
      BL.putStrLn (encode decoded)
