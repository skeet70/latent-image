{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
-- | Checked parsing of the native runner's output.
--
-- The parser owns the wire-independent checks shared by every consumer:
-- status/exit/engine consistency, the register set, region order and sizes,
-- and a complete provenance log. It does not decide what a case means.
module Emulation.NativeObservation
  ( RegionId (..)
  , GuestAddress (..)
  , Register (..)
  , registerLabel
  , registerFromLabel
  , registerLabels
  , RunnerStatus (..)
  , runnerStatusFromLabel
  , runnerStatusLabel
  , InstructionCount (..)
  , EngineError (..)
  , ProvenanceTotal (..)
  , WriteBytes (..)
  , Write (..)
  , Provenance (..)
  , provenanceWrites
  , provenanceCount
  , provenanceRetained
  , NativeObservation (..)
  , lookupRegister
  , ObservationError (..)
  , observeNative
  , renderObservationError
  , regionIdOf
  , regionBounds
  , regionInitialBytes
  , parseWord32
  , hexWord32
  ) where

import Control.Monad (forM, when)
import Data.Aeson (Value (..))
import qualified Data.ByteString as B
import Data.Char (isHexDigit)
import qualified Data.Map.Strict as M
import Data.Scientific (floatingOrInteger)
import Data.Word (Word32, Word64, Word8)
import Emulation.NativeCase (Region (..))
import FujiAF.Host (get, keys, list, str)
import Numeric (readHex, showHex)
import System.Exit (ExitCode (..))

-- | The region identifier from the case description.
newtype RegionId = RegionId {regionName :: String}
  deriving (Eq, Ord, Show)

-- | A 32-bit guest address, widened for arithmetic.
newtype GuestAddress = GuestAddress {guestAddress :: Word64}
  deriving (Eq, Ord, Show)

data Register
  = R0 | R1 | R2 | R3 | R4 | R5 | R6 | R7 | R8 | R9 | R10 | R11 | R12
  | Sp | Lr | Cpsr | Pc
  deriving (Bounded, Enum, Eq, Ord, Show)

registerLabels :: [Register]
registerLabels = [minBound .. maxBound]

registerLabel :: Register -> String
registerLabel R0 = "r0"
registerLabel R1 = "r1"
registerLabel R2 = "r2"
registerLabel R3 = "r3"
registerLabel R4 = "r4"
registerLabel R5 = "r5"
registerLabel R6 = "r6"
registerLabel R7 = "r7"
registerLabel R8 = "r8"
registerLabel R9 = "r9"
registerLabel R10 = "r10"
registerLabel R11 = "r11"
registerLabel R12 = "r12"
registerLabel Sp = "sp"
registerLabel Lr = "lr"
registerLabel Cpsr = "cpsr"
registerLabel Pc = "pc"

registerFromLabel :: String -> Maybe Register
registerFromLabel text =
  case [register | register <- registerLabels, registerLabel register == text] of
    (register : _) -> Just register
    [] -> Nothing

data RunnerStatus
  = Returned
  | InstructionBudget
  | WallBudget
  | MemoryGuard
  | FetchGuard
  | UnknownCall
  | UnsupportedInstruction
  | StubContract
  | RunnerEngineError
  | InvalidCase
  | HostTimeout
  | RunnerFailure
  | InvalidOutput
  | OtherStatus String
  deriving (Eq, Show)

runnerStatusFromLabel :: String -> RunnerStatus
runnerStatusFromLabel "returned" = Returned
runnerStatusFromLabel "instruction_budget" = InstructionBudget
runnerStatusFromLabel "wall_budget" = WallBudget
runnerStatusFromLabel "memory_guard" = MemoryGuard
runnerStatusFromLabel "fetch_guard" = FetchGuard
runnerStatusFromLabel "unknown_call" = UnknownCall
runnerStatusFromLabel "unsupported_instruction" = UnsupportedInstruction
runnerStatusFromLabel "stub_contract" = StubContract
runnerStatusFromLabel "engine_error" = RunnerEngineError
runnerStatusFromLabel "invalid_case" = InvalidCase
runnerStatusFromLabel "host_timeout" = HostTimeout
runnerStatusFromLabel "runner_failure" = RunnerFailure
runnerStatusFromLabel "invalid_output" = InvalidOutput
runnerStatusFromLabel other = OtherStatus other

runnerStatusLabel :: RunnerStatus -> String
runnerStatusLabel Returned = "returned"
runnerStatusLabel InstructionBudget = "instruction_budget"
runnerStatusLabel WallBudget = "wall_budget"
runnerStatusLabel MemoryGuard = "memory_guard"
runnerStatusLabel FetchGuard = "fetch_guard"
runnerStatusLabel UnknownCall = "unknown_call"
runnerStatusLabel UnsupportedInstruction = "unsupported_instruction"
runnerStatusLabel StubContract = "stub_contract"
runnerStatusLabel RunnerEngineError = "engine_error"
runnerStatusLabel InvalidCase = "invalid_case"
runnerStatusLabel HostTimeout = "host_timeout"
runnerStatusLabel RunnerFailure = "runner_failure"
runnerStatusLabel InvalidOutput = "invalid_output"
runnerStatusLabel (OtherStatus other) = other

newtype InstructionCount = InstructionCount {instructionCount :: Word64}
  deriving (Eq, Show)

newtype EngineError = EngineError {engineError :: Word32}
  deriving (Eq, Show)

newtype ProvenanceTotal = ProvenanceTotal {provenanceTotal :: Word64}
  deriving (Eq, Show)

newtype WriteBytes = WriteBytes {writeBytes :: Word8}
  deriving (Eq, Show)

data Write = Write
  { writeAddress :: GuestAddress
  , writeLength :: WriteBytes
  }
  deriving (Eq, Show)

data Provenance
  = Complete ProvenanceTotal [Write]
  | Truncated ProvenanceTotal
  deriving (Eq, Show)

provenanceWrites :: Provenance -> Maybe [Write]
provenanceWrites (Complete _ writes) = Just writes
provenanceWrites (Truncated _) = Nothing

provenanceCount :: Provenance -> ProvenanceTotal
provenanceCount (Complete total _) = total
provenanceCount (Truncated total) = total

provenanceRetained :: Provenance -> Int
provenanceRetained (Complete _ writes) = length writes
provenanceRetained (Truncated _) = 4096

data NativeObservation = NativeObservation
  { nativeStatus :: RunnerStatus
  , nativeRegisters :: M.Map Register Word32
  , nativeMemory :: [(RegionId, B.ByteString)]
  , nativeProvenance :: Provenance
  , nativeInstructions :: InstructionCount
  , nativeEngineError :: EngineError
  }
  deriving (Eq, Show)

data ObservationError
  = MissingStatus
  | EmptyStatus
  | ReturnedWithNonzeroExit
  | MissingEngineError
  | MalformedEngineError
  | EngineErrorOutOfRange
  | ReturnedWithEngineError Word32
  | MissingInstructions
  | MalformedInstructionCount
  | MissingProvenanceTotal
  | MalformedProvenanceTotal
  | MissingRegisters
  | UnknownRegister String
  | MalformedRegister Register
  | MissingRegister Register
  | MissingMemory
  | MalformedMemoryAddress RegionId
  | MemoryRegionCountMismatch Int Int
  | MemoryRegionAddressMismatch RegionId
  | MalformedMemoryBytes RegionId
  | MemoryRegionSizeMismatch RegionId
  | MissingProvenance
  | MalformedProvenanceKind
  | MalformedProvenanceAddress
  | MalformedProvenanceSize
  | ProvenanceRetentionMismatch Int
  | ProvenanceCountMismatch ProvenanceTotal Int
  deriving (Eq, Show)

lookupRegister :: Register -> NativeObservation -> Maybe Word32
lookupRegister register observation = M.lookup register (nativeRegisters observation)

observeNative :: ExitCode -> [Region] -> Value -> Either ObservationError NativeObservation
observeNative exit regions value = do
  status <- parseStatus value
  when (status == Returned && exit /= ExitSuccess) (Left ReturnedWithNonzeroExit)
  engineError <- parseEngineError (get value "unicorn_error")
  when (status == Returned && engineError /= EngineError 0) (Left (ReturnedWithEngineError (engineError engineError)))
  instructions <- requireCount "instructions" MissingInstructions MalformedInstructionCount value
  total <- requireCount "memory_provenance_total" MissingProvenanceTotal MalformedProvenanceTotal value
  registers <- parseRegisters (get value "registers")
  memory <- parseMemory regions (get value "memory")
  provenance <- parseProvenance total (get value "memory_provenance")
  pure
    NativeObservation
      { nativeStatus = status
      , nativeRegisters = registers
      , nativeMemory = memory
      , nativeProvenance = provenance
      , nativeInstructions = InstructionCount instructions
      , nativeEngineError = engineError
      }

renderObservationError :: ObservationError -> String
renderObservationError errorValue = case errorValue of
  MissingStatus -> "missing runner status"
  EmptyStatus -> "empty runner status"
  ReturnedWithNonzeroExit -> "status returned with nonzero process exit"
  MissingEngineError -> "missing unicorn_error"
  MalformedEngineError -> "malformed unicorn_error"
  EngineErrorOutOfRange -> "unicorn_error out of range"
  ReturnedWithEngineError value -> "status returned with unicorn_error " ++ show value
  MissingInstructions -> "missing instructions"
  MalformedInstructionCount -> "malformed instructions"
  MissingProvenanceTotal -> "missing memory_provenance_total"
  MalformedProvenanceTotal -> "malformed memory_provenance_total"
  MissingRegisters -> "missing registers"
  UnknownRegister name -> "unknown register " ++ name
  MalformedRegister register -> "malformed register " ++ registerLabel register
  MissingRegister register -> "missing register " ++ registerLabel register
  MissingMemory -> "missing memory"
  MalformedMemoryAddress ident -> "malformed address for region " ++ regionName ident
  MemoryRegionCountMismatch expected actual ->
    "memory region count " ++ show actual ++ " differs from " ++ show expected
  MemoryRegionAddressMismatch ident -> "memory region " ++ regionName ident ++ " address mismatch"
  MalformedMemoryBytes ident -> "malformed bytes for region " ++ regionName ident
  MemoryRegionSizeMismatch ident -> "memory region " ++ regionName ident ++ " size mismatch"
  MissingProvenance -> "missing memory provenance"
  MalformedProvenanceKind -> "malformed provenance kind"
  MalformedProvenanceAddress -> "malformed provenance address"
  MalformedProvenanceSize -> "provenance size out of range"
  ProvenanceRetentionMismatch retained ->
    "provenance retention count " ++ show retained ++ " differs from 4096"
  ProvenanceCountMismatch total retained ->
    "provenance count " ++ show retained ++ " differs from " ++ show (provenanceTotal total)

parseStatus :: Value -> Either ObservationError RunnerStatus
parseStatus value =
  case get value "status" of
    String _ ->
      let label = str (get value "status")
       in if null label then Left EmptyStatus else Right (runnerStatusFromLabel label)
    _ -> Left MissingStatus

requireCount :: String -> ObservationError -> ObservationError -> Value -> Either ObservationError Word64
requireCount key missing malformed value =
  case get value key of
    String _ -> maybe (Left malformed) Right (readDecimal (str (get value key)))
    _ -> Left missing

readDecimal :: String -> Maybe Word64
readDecimal text = case reads text :: [(Integer, String)] of
  [(value, "")]
    | value >= 0 && value <= 18446744073709551615 -> Just (fromIntegral value)
  _ -> Nothing

parseEngineError :: Value -> Either ObservationError EngineError
parseEngineError value = case value of
  Number number -> case floatingOrInteger number :: Either Double Integer of
    Right exact
      | exact >= 0 && exact <= 4294967295 -> Right (EngineError (fromIntegral exact))
      | otherwise -> Left EngineErrorOutOfRange
    Left _ -> Left MalformedEngineError
  _ -> Left MissingEngineError

parseRegisters :: Value -> Either ObservationError (M.Map Register Word32)
parseRegisters value = case value of
  Object _ -> do
    pairs <- forM (keys value) $ \name -> do
      register <- maybe (Left (UnknownRegister name)) Right (registerFromLabel name)
      word <- maybe (Left (MalformedRegister register)) Right (parseWord32 (str (get value name)))
      pure (register, word)
    let registers = M.fromList pairs
        missing = [register | register <- registerLabels, M.notMember register registers]
    case missing of
      (register : _) -> Left (MissingRegister register)
      [] -> Right registers
  _ -> Left MissingRegisters

parseMemory :: [Region] -> Value -> Either ObservationError [(RegionId, B.ByteString)]
parseMemory regions value = case value of
  Array _ -> do
    let entries = list value
    when (length entries /= length regions) (Left (MemoryRegionCountMismatch (length regions) (length entries)))
    pairs <- forM (zip regions entries) $ \(region, entry) -> do
      let ident = regionIdOf region
          expectedAddress = fromIntegral (baseAddress region) :: Word64
          expectedSize = fromIntegral (regionSize region) :: Int
      address <- maybe (Left (MalformedMemoryAddress ident)) (Right . fromIntegral) (parseWord32 (str (get entry "address")))
      when (address /= expectedAddress) (Left (MemoryRegionAddressMismatch ident))
      bytes <- maybe (Left (MalformedMemoryBytes ident)) Right (unhex (str (get entry "bytes")))
      when (B.length bytes /= expectedSize) (Left (MemoryRegionSizeMismatch ident))
      pure (ident, bytes)
    pure pairs
  _ -> Left MissingMemory

parseProvenance :: Word64 -> Value -> Either ObservationError Provenance
parseProvenance total value = case value of
  Array _ -> do
    let entries = list value
    parsed <- forM entries $ \entry -> do
      kind <- case str (get entry "kind") of
        "write" -> Right True
        "read" -> Right False
        _ -> Left MalformedProvenanceKind
      address <- maybe (Left MalformedProvenanceAddress) (Right . GuestAddress . fromIntegral) (parseWord32 (str (get entry "address")))
      size <- case get entry "size" of
        Number number -> case floatingOrInteger number :: Either Double Integer of
          Right exact
            | exact >= 1 && exact <= 8 -> Right (WriteBytes (fromIntegral exact))
          _ -> Left MalformedProvenanceSize
        _ -> Left MalformedProvenanceSize
      pure (kind, Write address size)
    let writes = [write | (True, write) <- parsed]
    if total > 4096
      then do
        when (length entries /= 4096) (Left (ProvenanceRetentionMismatch (length entries)))
        pure (Truncated (ProvenanceTotal total))
      else do
        when (fromIntegral (length entries) /= total) (Left (ProvenanceCountMismatch (ProvenanceTotal total) (length entries)))
        pure (Complete (ProvenanceTotal total) writes)
  _ -> Left MissingProvenance

regionIdOf :: Region -> RegionId
regionIdOf region = RegionId (str (get (description region) "id"))

regionBounds :: [Region] -> [(RegionId, GuestAddress, Int)]
regionBounds regions =
  [ (regionIdOf region, GuestAddress (fromIntegral (baseAddress region)), fromIntegral (regionSize region))
  | region <- regions
  ]

regionInitialBytes :: [Region] -> [(RegionId, B.ByteString)]
regionInitialBytes regions = [(regionIdOf region, regionBytes region) | region <- regions]

parseWord32 :: String -> Maybe Word32
parseWord32 ('0' : 'x' : digits) = parseDigits digits
parseWord32 digits = parseDigits digits

parseDigits :: String -> Maybe Word32
parseDigits digits
  | null digits || length digits > 8 || not (all isHexDigit digits) = Nothing
  | otherwise =
      case readHex digits :: [(Word32, String)] of
        [(value, "")] -> Just value
        _ -> Nothing

unhex :: String -> Maybe B.ByteString
unhex text
  | odd (length text) = Nothing
  | otherwise = B.pack <$> go text
  where
    go [] = Just []
    go (highChar : lowChar : rest) = do
      high <- digit highChar
      low <- digit lowChar
      ((high * 16 + low) :) <$> go rest
    go _ = Nothing
    digit char
      | char >= '0' && char <= '9' = Just (fromIntegral (fromEnum char - fromEnum '0'))
      | char >= 'a' && char <= 'f' = Just (fromIntegral (fromEnum char - fromEnum 'a' + 10))
      | otherwise = Nothing

hexWord32 :: Word32 -> String
hexWord32 value = "0x" ++ replicate (8 - length text) '0' ++ text
  where
    text = showHex value ""
