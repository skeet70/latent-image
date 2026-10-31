{-# LANGUAGE OverloadedStrings #-}
-- | Schema-3 peer startup witness: contract, owned-candidate facts derived from
-- linked bytes, receipt acceptance and retained/owned comparison.
module Emulation.PeerStartupWitness
  ( Endpoint (..), Scenario (..), Program (..), OwnedFacts (..), Sites (..), Interval (..)
  , RegisterIndex, registerIndex, registerIndexValue, PollPeriod (..)
  , StagedRecord, stagedRecord, stagedRecordBytes
  , scenarioBudget, scenarioValue, programName, programRecordHex
  , forbidFetchIntervals, forbidReadIntervals, retainedSites
  , contractValue, parseStartupContract
  , candidateEntry, deriveOwnedFacts, verifyStagedOwned
  , StartupReceipt, ValidatedStartupReceipt, parseStartupReceipt, validateStartupReceipt, compareStartupReceipts, startupArgument
  , maxStartupReceiptBytes
  ) where

import A32 (BlockMode (..), Cond (..), DataOp (..), Forbidden (Barrier), Indexing (..), Instruction (..), Offset (..), Operand (..), Reg (..), Shift (..), Width (..))
import qualified A32
import Control.Monad (foldM, forM, forM_, unless, when)
import Data.Aeson (FromJSON, Object, Result (..), Value (..), fromJSON, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (bit, complement, rotateR, shiftL, shiftR, testBit, xor, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Char (digitToInt)
import Data.Int (Int32)
import Data.List (nub, sort, stripPrefix)
import qualified Data.Map.Strict as M
import Data.Maybe (isJust, isNothing)
import Data.Word (Word32, Word64, Word8)
import Emulation.FirstStageWitness (Core (..), Cpsr (..), GuestAddress (..), WitnessResult (..), encodeLeWord, leWordAt, sha256Hex)
import Emulation.PeerStageWitness (Argument (..), PeerState (..), Registers (..))
import qualified Emulation.PeerStageWitness as P
import Emulation.WatchObservation (Instructions (..))
import qualified Emulation.WatchObservation as W
import qualified FujiAF.FirstStage as F
import FujiAF.JSON (decodeStrictIntegers)
import qualified FujiAF.PeerWaitControl as C
import FujiAF.PeerWaitControlImage (veneerBytes)
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)

-- * Contract

data Endpoint = Handoff | LoaderEntry deriving (Eq, Show)

data Scenario = Natural Endpoint | HeldSentinel | DelayedRelease Endpoint deriving (Eq, Show)

-- | Half-open.
data Interval = Interval Word64 Word64 deriving (Eq, Ord, Show)

-- | Index into r0..r12.
newtype RegisterIndex = RegisterIndex Int deriving (Eq, Show)

-- | Core-1 instructions entered from one arrival at @peer_loaded@ to the next.
newtype PollPeriod = PollPeriod Word64 deriving (Eq, Show)

-- | Guest bytes [0x10000,0x14c80) of record 2 as staged.
newtype StagedRecord = StagedRecord B.ByteString deriving (Eq, Show)

data Sites = Sites
  { siteEntry :: GuestAddress, siteMain :: GuestAddress, sitePeerLoad :: GuestAddress
  , sitePeerLoaded :: GuestAddress, siteClearStore :: GuestAddress, siteTargetStore :: GuestAddress
  , siteArgumentStore :: GuestAddress, siteTargetLoad :: GuestAddress, siteArgumentLoad :: GuestAddress
  , siteHandoff :: GuestAddress
  } deriving (Eq, Show)

data OwnedFacts = OwnedFacts
  { ownedRecord :: F.Sha256, ownedImage :: Interval, ownedSites :: Sites
  , ownedLoadedRegister :: RegisterIndex, ownedPollPeriod :: PollPeriod
  , ownedForbidFetch :: [Interval], ownedForbidRead :: [Interval]
  } deriving (Eq, Show)

data Program = Retained | Owned OwnedFacts deriving (Eq, Show)

registerIndex :: Int -> Either String RegisterIndex
registerIndex value
  | value >= 0 && value <= 12 = Right (RegisterIndex value)
  | otherwise = Left "loaded register must be r0..r12"

registerIndexValue :: RegisterIndex -> Int
registerIndexValue (RegisterIndex value) = value

registerAt :: RegisterIndex -> Registers -> Word32
registerAt (RegisterIndex value) = case value of
  0 -> r0
  1 -> r1
  2 -> r2
  3 -> r3
  4 -> r4
  5 -> r5
  6 -> r6
  7 -> r7
  8 -> r8
  9 -> r9
  10 -> r10
  11 -> r11
  _ -> r12

stagedRecord :: B.ByteString -> Either String StagedRecord
stagedRecord bytes
  | B.length bytes == recordLength = Right (StagedRecord bytes)
  | otherwise = Left "staged record must hold guest [0x10000,0x14c80)"

stagedRecordBytes :: StagedRecord -> B.ByteString
stagedRecordBytes (StagedRecord bytes) = bytes

recordLength :: Int
recordLength = 0x4c80

scenarioBudget :: Scenario -> Instructions
scenarioBudget (Natural _) = Instructions 20000000
scenarioBudget _ = Instructions 10000000

forbidFetchIntervals, forbidReadIntervals :: [Interval]
forbidFetchIntervals =
  [ Interval 0x11800 0x11854, Interval 0x119e4 0x11abc, Interval 0x11cc0 0x11fe4, Interval 0x12960 0x12974
  , Interval 0x12abc 0x12ad0, Interval 0x13490 0x13574, Interval 0x1386c 0x138ac ]
forbidReadIntervals = [Interval 0x11ac4 0x11acc, Interval 0x11ad4 0x11ae8, Interval 0x11fe4 0x11fec]

retainedSites :: Sites
retainedSites = Sites
  (GuestAddress 0x11800) (GuestAddress 0x11808) (GuestAddress 0x11f74) (GuestAddress 0x11f78) (GuestAddress 0x11f4c)
  (GuestAddress 0x11f98) (GuestAddress 0x11fb0) (GuestAddress 0x11844) (GuestAddress 0x1184c) (GuestAddress 0x11850)

retainedRecordHex :: String
retainedRecordHex = "2f1bf3f0e483b892d2890599679c1b515cfc1be8b8c27cbb06d4d913e428abb4"

loaderBDigestHex :: String
loaderBDigestHex = "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"

programName :: Program -> String
programName Retained = "retained"
programName (Owned _) = "owned"

programRecordHex :: Program -> String
programRecordHex Retained = retainedRecordHex
programRecordHex (Owned facts) = F.sha256Hex (ownedRecord facts)

siteNames :: [(String, Sites -> GuestAddress)]
siteNames =
  [ ("entry", siteEntry), ("main", siteMain), ("peer_load", sitePeerLoad), ("peer_loaded", sitePeerLoaded)
  , ("clear_store", siteClearStore), ("target_store", siteTargetStore), ("argument_store", siteArgumentStore)
  , ("target_load", siteTargetLoad), ("argument_load", siteArgumentLoad), ("handoff", siteHandoff) ]

siteList :: Sites -> [GuestAddress]
siteList sites = [select sites | (_, select) <- siteNames]

endpointName :: Endpoint -> String
endpointName Handoff = "handoff"
endpointName LoaderEntry = "loader-entry"

scenarioValue :: Scenario -> Value
scenarioValue scenario = case scenario of
  Natural endpoint -> object ["kind" .= ("natural" :: String), "endpoint" .= endpointName endpoint]
  HeldSentinel -> object ["kind" .= ("held-sentinel" :: String)]
  DelayedRelease endpoint -> object ["kind" .= ("delayed-release" :: String), "endpoint" .= endpointName endpoint]

intervalValue :: Interval -> [Word64]
intervalValue (Interval low high) = [low, high]

addressWord :: GuestAddress -> Word64
addressWord (GuestAddress value) = value

programValue :: Program -> Value
programValue Retained = object ["kind" .= ("retained" :: String)]
programValue (Owned facts) = let PollPeriod period = ownedPollPeriod facts in object
  [ "kind" .= ("owned" :: String), "record_sha256" .= F.sha256Hex (ownedRecord facts)
  , "image" .= intervalValue (ownedImage facts)
  , "sites" .= object [K.fromString name .= addressWord (select (ownedSites facts)) | (name, select) <- siteNames]
  , "loaded_register" .= registerIndexValue (ownedLoadedRegister facts), "poll_period" .= period
  , "forbid_fetch" .= map intervalValue (ownedForbidFetch facts), "forbid_read" .= map intervalValue (ownedForbidRead facts)
  ]

contractValue :: Scenario -> Program -> Value
contractValue scenario program = let Instructions budget = scenarioBudget scenario in object
  ["schema" .= (3 :: Int), "budget" .= budget, "scenario" .= scenarioValue scenario, "program" .= programValue program]

fields :: [String] -> Value -> Either String Object
fields names (Object row) = do
  unless (sort names == sort (map K.toString (KM.keys row))) (Left ("startup witness fields differ from schema 3: " ++ unwords names))
  pure row
fields _ _ = Left "startup witness value is not an object"

field :: Object -> String -> Either String Value
field row name = maybe (Left ("missing startup witness field: " ++ name)) Right (KM.lookup (K.fromString name) row)

json :: FromJSON a => Value -> Either String a
json value = case fromJSON value of
  Error problem -> Left problem
  Success parsed -> Right parsed

get :: FromJSON a => Object -> String -> Either String a
get row name = field row name >>= json

nullable :: (Value -> Either String a) -> Value -> Either String (Maybe a)
nullable _ Null = Right Nothing
nullable parse value = Just <$> parse value

addressField :: Object -> String -> Either String GuestAddress
addressField row name = GuestAddress . fromIntegral <$> (get row name :: Either String Word32)

require :: Bool -> String -> Either String ()
require condition message = unless condition (Left message)

parseInterval :: Value -> Either String Interval
parseInterval value = do
  bounds <- json value :: Either String [Word32]
  case bounds of
    [low, high] | low < high -> Right (Interval (fromIntegral low) (fromIntegral high))
    _ -> Left "interval must be an increasing [low, high] pair"

parseEndpoint :: Object -> Either String Endpoint
parseEndpoint row = do
  name <- get row "endpoint" :: Either String String
  case name of
    "handoff" -> Right Handoff
    "loader-entry" -> Right LoaderEntry
    _ -> Left "unknown startup endpoint"

parseScenario :: Value -> Either String Scenario
parseScenario value = do
  kind <- case value of
    Object row -> get row "kind" :: Either String String
    _ -> Left "scenario is not an object"
  case kind of
    "natural" -> fields ["kind", "endpoint"] value >>= fmap Natural . parseEndpoint
    "held-sentinel" -> HeldSentinel <$ fields ["kind"] value
    "delayed-release" -> fields ["kind", "endpoint"] value >>= fmap DelayedRelease . parseEndpoint
    _ -> Left "unknown startup scenario"

parseSites :: Value -> Either String Sites
parseSites value = do
  row <- fields (map fst siteNames) value
  let site name = addressField row name
  Sites <$> site "entry" <*> site "main" <*> site "peer_load" <*> site "peer_loaded" <*> site "clear_store"
    <*> site "target_store" <*> site "argument_store" <*> site "target_load" <*> site "argument_load" <*> site "handoff"

parseProgram :: Value -> Either String Program
parseProgram value = do
  kind <- case value of
    Object row -> get row "kind" :: Either String String
    _ -> Left "program is not an object"
  case kind of
    "retained" -> Retained <$ fields ["kind"] value
    "owned" -> do
      row <- fields ["kind", "record_sha256", "image", "sites", "loaded_register", "poll_period", "forbid_fetch", "forbid_read"] value
      facts <- OwnedFacts <$> (get row "record_sha256" >>= F.parseSha256) <*> (field row "image" >>= parseInterval)
        <*> (field row "sites" >>= parseSites) <*> (get row "loaded_register" >>= registerIndex)
        <*> (PollPeriod <$> get row "poll_period")
        <*> (field row "forbid_fetch" >>= json >>= mapM parseInterval)
        <*> (field row "forbid_read" >>= json >>= mapM parseInterval)
      checkOwnedFacts facts
      pure (Owned facts)
    _ -> Left "unknown startup program"

checkOwnedFacts :: OwnedFacts -> Either String ()
checkOwnedFacts facts = do
  let Interval low high = ownedImage facts
      sites = siteList (ownedSites facts)
      PollPeriod period = ownedPollPeriod facts
  require (low >= 0x103e8 && high <= 0x11800 && low < high && low .&. 3 == 0 && high .&. 3 == 0)
    "owned image lies outside [0x103e8,0x11800) or is not word aligned"
  require (all (\(GuestAddress site) -> site >= low && site < high && site .&. 3 == 0) sites) "owned site lies outside the image or is not word aligned"
  require (length (nub sites) == length sites) "owned sites are not distinct"
  require (period > 0 && period <= 100000) "owned poll period lies outside 1..100000"
  require (sort (ownedForbidFetch facts) == sort forbidFetchIntervals) "forbid_fetch differs from the fixed intervals"
  require (sort (ownedForbidRead facts) == sort forbidReadIntervals) "forbid_read differs from the fixed intervals"

parseStartupContract :: B.ByteString -> Either String (Scenario, Program)
parseStartupContract bytes = do
  require (B.length bytes <= 1024 * 1024) "startup contract exceeds 1 MiB"
  value <- decodeStrictIntegers bytes
  row <- fields ["schema", "budget", "scenario", "program"] value
  schema <- get row "schema" :: Either String Word32
  require (schema == 3) "unsupported startup contract schema"
  scenario <- field row "scenario" >>= parseScenario
  budget <- get row "budget" :: Either String Word64
  require (Instructions budget == scenarioBudget scenario) "startup contract budget differs from its scenario"
  program <- field row "program" >>= parseProgram
  pure (scenario, program)

-- * Owned facts from linked bytes

data CodeImage = CodeImage Word32 B.ByteString

imageWord :: CodeImage -> Word32 -> Maybe Word32
imageWord (CodeImage base bytes) location
  | location < base || location .&. 3 /= 0 = Nothing
  | otherwise = either (const Nothing) Just (leWordAt bytes (fromIntegral (location - base)))

hex :: Word32 -> String
hex value = "0x" ++ showHex value ""

candidateEntry :: B.ByteString -> Either String GuestAddress
candidateEntry symbols = do
  rows <- F.parseSymbols symbols
  GuestAddress . fromIntegral . F.symbolAddress <$> F.lookupSymbol rows "xm1_secondary_entry"

deriveOwnedFacts :: B.ByteString -> B.ByteString -> StagedRecord -> Either String OwnedFacts
deriveOwnedFacts bin symbolBytes (StagedRecord staged) = do
  rows <- F.parseSymbols symbolBytes
  let symbolAt name = F.symbolAddress <$> F.lookupSymbol rows name
      extent name = do
        symbol <- F.lookupSymbol rows name
        let low = F.symbolAddress symbol
        require (F.symbolSize symbol > 0 && low .&. 3 == 0) (name ++ " has no size or is not word aligned")
        pure (low, low + F.symbolSize symbol)
  entry <- symbolAt "xm1_secondary_entry"
  require (not (B.null bin) && B.length bin `mod` 4 == 0 && entry .&. 3 == 0) "candidate image is empty, unaligned or not whole words"
  let code = CodeImage entry bin
      imageEnd = entry + fromIntegral (B.length bin)
      wordAt location = maybe (Left ("candidate word outside the image at " ++ hex location)) Right (imageWord code location)
      decodeAt location = A32.decode location <$> wordAt location
      decodeExtent (low, high) = do
        require (low >= entry && high <= imageEnd) "candidate symbol extent lies outside the image"
        mapM (\location -> (,) location <$> decodeAt location) [low, low + 4 .. high - 4]
      single what candidates = case candidates of
        [found] -> Right found
        [] -> Left ("candidate has no " ++ what)
        _ -> Left ("candidate has several " ++ what)
      literalLoad location loaded expected = do
        previous <- decodeAt (location - 4)
        case previous of
          Load (Cond 14) Word False rt base (OffsetImmediate displacement) OffsetOnly
            | rt == loaded && base == A32.pc -> do
                literal <- wordAt (location - 4 + 8 + fromIntegral displacement)
                require (literal == expected) ("handoff literal differs from " ++ hex expected)
          _ -> Left "handoff load is not preceded by its PC-relative literal load"
  entryWords <- mapM decodeAt [entry, entry + 4, entry + 8]
  mainSymbol <- symbolAt "xm1_secondary_main"
  mainAddress <- case entryWords of
    [Branch (Cond 14) True 0x11854, Branch (Cond 14) True 0x118b8, Branch (Cond 14) True callee] -> Right callee
    _ -> Left "candidate entry is not BL 0x11854, BL 0x118b8, BL main"
  require (mainAddress == mainSymbol) "candidate entry calls a target other than xm1_secondary_main"
  peerCell <- extent "xm1_secondary_peer_cell"
  peerCellWords <- decodeExtent peerCell
  (peerLoad, loadedNumber) <- single "peer-cell word load"
    [(location, number) | (location, Load (Cond 14) Word False (Reg number) _ (OffsetImmediate 0) OffsetOnly) <- peerCellWords]
  loadedRegister <- registerIndex loadedNumber
  rendezvous <- extent "xm1_secondary_rendezvous"
  rendezvousWords <- decodeExtent rendezvous
  peerLoaded <- single "rendezvous call of xm1_secondary_peer_cell"
    [location + 4 | (location, Branch (Cond 14) True callee) <- rendezvousWords, callee == fst peerCell]
  afterLoad <- decodeAt (peerLoad + 4)
  require (returnsWithout (Reg loadedNumber) (Just afterLoad)) "peer-cell load is not followed by a return that keeps the loaded register"
  imageWords <- decodeExtent (entry, imageEnd)
  require (length [() | (_, Branch _ _ callee) <- imageWords, callee == fst peerCell] == 1)
    "candidate reaches xm1_secondary_peer_cell by more than its one rendezvous call"
  require (null [() | (_, Branch _ _ target) <- imageWords, target == peerLoaded]) "candidate branches directly to the peer-loaded site"
  clearStore <- single "rendezvous word store" [location | (location, Store (Cond 14) Word _ _ _ _) <- rendezvousWords]
  publishWords <- extent "xm1_secondary_publish" >>= decodeExtent
  (targetStore, argumentStore) <- case [location | (location, Store (Cond 14) Word _ _ _ _) <- publishWords] of
    firstStore : secondStore : _ -> Right (firstStore, secondStore)
    _ -> Left "xm1_secondary_publish has fewer than two word stores"
  handoffWords <- extent "xm1_secondary_handoff" >>= decodeExtent
  (targetLoad, argumentLoad) <-
    case [(location, rt) | (location, Load (Cond 14) Word False rt rn (OffsetImmediate 0) OffsetOnly) <- handoffWords, rt == rn, rt /= A32.pc] of
      [(firstLoad, firstRegister), (secondLoad, secondRegister)] -> do
        literalLoad firstLoad firstRegister 0x14c80
        literalLoad secondLoad secondRegister 0x14c84
        pure (firstLoad, secondLoad)
      _ -> Left "xm1_secondary_handoff does not hold exactly two self-based word loads"
  handoff <- symbolAt "xm1_secondary_handoff_vector"
  handoffWord <- wordAt handoff
  require (handoffWord == 0xe1a0f003) "candidate handoff vector is not MOV PC, R3"
  period <- pollPeriod code (fst rendezvous) peerLoaded
  fetchSet <- forbidSet rows "xm1_forbid_fetch_"
  readSet <- forbidSet rows "xm1_forbid_read_"
  let load = fromIntegral entry - 0x10000 :: Int
  require (leWordAt staged 0x20 == Right entry) "staged reset vector does not name the candidate entry"
  require (load >= 0x24 && B.take (B.length bin) (B.drop load staged) == bin) "staged record does not hold the candidate image at its entry"
  record <- F.parseSha256 (sha256Hex staged)
  let at location = GuestAddress (fromIntegral location)
      facts = OwnedFacts
        { ownedRecord = record
        , ownedImage = Interval (fromIntegral entry) (fromIntegral imageEnd)
        , ownedSites = Sites (at entry) (at mainAddress) (at peerLoad) (at peerLoaded) (at clearStore)
            (at targetStore) (at argumentStore) (at targetLoad) (at argumentLoad) (at handoff)
        , ownedLoadedRegister = loadedRegister, ownedPollPeriod = period
        , ownedForbidFetch = fetchSet, ownedForbidRead = readSet
        }
  checkOwnedFacts facts
  pure facts

forbidSet :: [F.Symbol] -> String -> Either String [Interval]
forbidSet rows prefix = do
  let tagged = [(stem, F.symbolAddress symbol) | symbol <- rows, Just stem <- [stripPrefix prefix (F.symbolName symbol)]]
      split suffix = [(stem, location) | (name, location) <- tagged, Just stem <- [stripSuffix suffix name]]
      starts = split "_start"
      ends = split "_end"
  require (length starts + length ends == length tagged) (prefix ++ " symbol lacks a _start or _end suffix")
  require (sort (map fst starts) == sort (map fst ends) && length (nub (map fst starts)) == length starts) (prefix ++ " symbols do not pair")
  forM starts $ \(stem, low) -> case lookup stem ends of
    Just high | low < high -> Right (Interval (fromIntegral low) (fromIntegral high))
    _ -> Left (prefix ++ stem ++ " is empty or unpaired")
  where
    stripSuffix suffix text = reverse <$> stripPrefix (reverse suffix) (reverse text)

-- ** Poll-period interpreter

-- | 'Nothing' is an incoming flag or one computed from an unknown value.
data Flags = Flags {flagN :: Maybe Bool, flagZ :: Maybe Bool, flagC :: Maybe Bool, flagV :: Maybe Bool}

-- | An absent register, or a 'Nothing' stack word, holds an incoming or derived unknown value.
data Machine = Machine
  { machinePc :: Word32
  , machineRegisters :: M.Map Int Word32
  , machineFlags :: Flags
  , machineStack :: M.Map Word32 (Maybe Word32)
  }

maxPollSteps :: Int
maxPollSteps = 100000

stackFloor, stackTop, peerCellAddress, peerCellWaiting :: Word32
stackFloor = 0x70000000
stackTop = 0x70000800
peerCellAddress = 0x40020000
peerCellWaiting = 0x22b

pollPeriod :: CodeImage -> Word32 -> Word32 -> Either String PollPeriod
pollPeriod code start loaded = go 0 Nothing initial
  where
    initial = Machine start (M.singleton 13 stackTop) (Flags Nothing Nothing Nothing Nothing) M.empty
    go steps counted machine = do
      let here = machinePc machine
      case counted of
        Just total | here == loaded -> Right (PollPeriod total)
        _ -> do
          require (steps < maxPollSteps) "poll-period interpretation exceeded 100000 steps"
          next <- execute code machine
          let counting = case counted of
                Just total -> Just (total + 1)
                Nothing | here == loaded -> Just 1
                Nothing -> Nothing
          go (steps + 1 :: Int) counting next

slot :: Machine -> Int -> Maybe Word32
slot machine number = M.lookup number (machineRegisters machine)

setSlot :: Int -> Maybe Word32 -> Machine -> Machine
setSlot number value machine = machine {machineRegisters = M.alter (const value) number (machineRegisters machine)}

regNumber :: Reg -> Int
regNumber (Reg number) = number

unsupportedAt :: Word32 -> Either String a
unsupportedAt location = Left ("poll-period interpreter does not support the instruction at " ++ hex location)

known :: Word32 -> Maybe a -> Either String a
known location = maybe (Left ("poll-period control flow or address depends on an unknown incoming value at " ++ hex location)) Right

conditionOf :: Instruction -> Maybe Cond
conditionOf instruction = case instruction of
  DataProcessing condition _ _ _ _ _ -> Just condition
  MoveWide condition _ _ -> Just condition
  MoveTop condition _ _ -> Just condition
  Load condition _ _ _ _ _ _ -> Just condition
  Store condition _ _ _ _ _ -> Just condition
  LoadMultiple condition _ _ _ _ -> Just condition
  StoreMultiple condition _ _ _ _ -> Just condition
  Branch condition _ _ -> Just condition
  BranchExchange condition _ _ -> Just condition
  Hint condition -> Just condition
  _ -> Nothing

holds :: Flags -> Cond -> Maybe Bool
holds flags (Cond condition) = case condition of
  0 -> z
  1 -> not <$> z
  2 -> c
  3 -> not <$> c
  4 -> n
  5 -> not <$> n
  6 -> v
  7 -> not <$> v
  8 -> (&&) <$> c <*> (not <$> z)
  9 -> (||) <$> (not <$> c) <*> z
  10 -> (==) <$> n <*> v
  11 -> (/=) <$> n <*> v
  12 -> (&&) <$> (not <$> z) <*> ((==) <$> n <*> v)
  13 -> (||) <$> z <*> ((/=) <$> n <*> v)
  _ -> Just (condition == 14)
  where
    n = flagN flags
    z = flagZ flags
    c = flagC flags
    v = flagV flags

execute :: CodeImage -> Machine -> Either String Machine
execute code machine = do
  let here = machinePc machine
  word <- maybe (Left ("poll-period fetch outside the candidate image at " ++ hex here)) Right (imageWord code here)
  let instruction = A32.decode here word
      advance state = state {machinePc = here + 4}
      readReg r = if r == A32.pc then Just (here + 8) else slot machine (regNumber r)
      carry = flagC (machineFlags machine)
      linked link = if link then setSlot 14 (Just (here + 4)) machine else machine
      branchTo destination state
        | destination .&. 3 /= 0 = unsupportedAt here
        | otherwise = Right state {machinePc = destination}
      transferAddress rn offset indexing = do
        displacement <- case offset of
          OffsetImmediate value -> Right (fromIntegral value)
          OffsetRegister up rm kind amount
            | rm == A32.pc -> unsupportedAt here
            | otherwise -> (\shifted -> if up then shifted else negate shifted) <$> known here (fst (shiftImmediateKnown kind amount (readReg rm) carry))
        base <- known here (readReg rn)
        let effective = base + displacement
        pure (if indexing == PostIndexed then base else effective, effective)
      blockAddresses base mode list =
        let size = 4 * fromIntegral (length list)
            lowest = case mode of
              IncrementAfter -> base
              IncrementBefore -> base + 4
              DecrementAfter -> base - size + 4
              DecrementBefore -> base - size
            final = if mode == IncrementAfter || mode == IncrementBefore then base + size else base - size
         in ([lowest + 4 * fromIntegral index | index <- [0 .. length list - 1]], final)
  case conditionOf instruction of
    Nothing -> case instruction of
      Rejected Barrier -> Right (advance machine)
      _ -> unsupportedAt here
    Just condition -> do
      taken <- known here (holds (machineFlags machine) condition)
      if not taken then Right (advance machine) else case instruction of
        DataProcessing _ op setFlags rd rn shifter -> do
          (operand2, shifterCarry) <- case shifter of
            Immediate value -> Right (Just value, if (word `shiftR` 8) .&. 0xf == 0 then carry else Just (testBit value 31))
            ShiftedImmediate rm kind amount -> Right (shiftImmediateKnown kind amount (readReg rm) carry)
            ShiftedRegister rm kind rs
              | rm == A32.pc || rs == A32.pc -> unsupportedAt here
              | otherwise -> Right (shiftRegisterKnown kind (readReg rs) (readReg rm) carry)
          let (result, flags) = alu op (readReg rn) operand2 shifterCarry (machineFlags machine)
              writes = op `notElem` [TST, TEQ, CMP, CMN]
          when (writes && rd == A32.pc) (unsupportedAt here)
          let written = if writes then setSlot (regNumber rd) result machine else machine
          Right (advance (if setFlags then written {machineFlags = flags} else written))
        MoveWide _ rd value
          | rd == A32.pc -> unsupportedAt here
          | otherwise -> Right (advance (setSlot (regNumber rd) (Just value) machine))
        MoveTop _ rd value
          | rd == A32.pc -> unsupportedAt here
          | otherwise -> Right (advance (setSlot (regNumber rd) ((\old -> (old .&. 0xffff) .|. (value `shiftL` 16)) <$> readReg rd) machine))
        Load _ Word False rt rn offset indexing -> do
          let writeback = indexing /= OffsetOnly
          when (writeback && (rn == A32.pc || rn == rt)) (unsupportedAt here)
          (location, effective) <- transferAddress rn offset indexing
          loaded <- loadWord code machine (rn == A32.pc) location here
          let based = if writeback then setSlot (regNumber rn) (Just effective) machine else machine
          if rt == A32.pc then known here loaded >>= (`branchTo` based) else Right (advance (setSlot (regNumber rt) loaded based))
        Store _ Word rt rn offset indexing -> do
          let writeback = indexing /= OffsetOnly
          when (rt == A32.pc || (writeback && (rn == A32.pc || rn == rt))) (unsupportedAt here)
          (location, effective) <- transferAddress rn offset indexing
          stored <- storeWord here location (readReg rt) machine
          Right (advance (if writeback then setSlot (regNumber rn) (Just effective) stored else stored))
        LoadMultiple _ rn mode writeback list -> do
          when (null list || rn == A32.pc || (writeback && rn `elem` list)) (unsupportedAt here)
          base <- known here (readReg rn)
          let (locations, final) = blockAddresses base mode list
          values <- mapM (\location -> loadWord code machine False location here) locations
          let pairs = zip list values
              loadedState = foldl (\state (r, value) -> if r == A32.pc then state else setSlot (regNumber r) value state) machine pairs
              based = if writeback then setSlot (regNumber rn) (Just final) loadedState else loadedState
          case lookup A32.pc pairs of
            Just destination -> known here destination >>= (`branchTo` based)
            Nothing -> Right (advance based)
        StoreMultiple _ rn mode writeback list -> do
          when (null list || rn == A32.pc || A32.pc `elem` list || (writeback && rn `elem` list)) (unsupportedAt here)
          base <- known here (readReg rn)
          let (locations, final) = blockAddresses base mode list
          stored <- foldM (\state (location, r) -> storeWord here location (readReg r) state) machine (zip locations list)
          Right (advance (if writeback then setSlot (regNumber rn) (Just final) stored else stored))
        Branch _ link destination -> branchTo destination (linked link)
        BranchExchange _ link rm
          | rm == A32.pc -> unsupportedAt here
          | otherwise -> known here (readReg rm) >>= (`branchTo` linked link)
        Hint _ -> Right (advance machine)
        _ -> unsupportedAt here

insideStack :: Word32 -> Bool
insideStack location = location >= stackFloor && location < stackTop

loadWord :: CodeImage -> Machine -> Bool -> Word32 -> Word32 -> Either String (Maybe Word32)
loadWord code machine literal location here
  | location .&. 3 /= 0 = unsupportedAt here
  | location == peerCellAddress = Right (Just peerCellWaiting)
  | insideStack location = maybe (Left ("poll-period interpreter reads an unwritten stack word at " ++ hex here)) Right (M.lookup location (machineStack machine))
  | literal, Just value <- imageWord code location = Right (Just value)
  | otherwise = Left ("poll-period interpreter load outside the modelled memory at " ++ hex here)

storeWord :: Word32 -> Word32 -> Maybe Word32 -> Machine -> Either String Machine
storeWord here location value machine
  | location .&. 3 /= 0 = unsupportedAt here
  | insideStack location = Right machine {machineStack = M.insert location value (machineStack machine)}
  | otherwise = Right machine

shiftImmediateKnown :: Shift -> Int -> Maybe Word32 -> Maybe Bool -> (Maybe Word32, Maybe Bool)
shiftImmediateKnown kind amount value carry
  | kind == LSL && amount == 0 = (value, carry)
  | kind == ROR && amount == 0 = split (shiftByImmediate kind amount <$> value <*> carry)
  | otherwise = split ((\word -> shiftByImmediate kind amount word False) <$> value)
  where
    split shifted = (fst <$> shifted, snd <$> shifted)

shiftRegisterKnown :: Shift -> Maybe Word32 -> Maybe Word32 -> Maybe Bool -> (Maybe Word32, Maybe Bool)
shiftRegisterKnown kind amountRegister value carry = case (.&. 0xff) <$> amountRegister of
  Nothing -> (Nothing, Nothing)
  Just 0 -> (value, carry)
  Just amount ->
    let shifted = (\word -> shiftByRegister kind (fromIntegral amount) word False) <$> value
     in (fst <$> shifted, snd <$> shifted)

alu :: DataOp -> Maybe Word32 -> Maybe Word32 -> Maybe Bool -> Flags -> (Maybe Word32, Flags)
alu op left right shifterCarry flags = case op of
  AND -> logical ((.&.) <$> left <*> right)
  EOR -> logical (xor <$> left <*> right)
  SUB -> arithmetic left (complement <$> right) (Just True)
  RSB -> arithmetic (complement <$> left) right (Just True)
  ADD -> arithmetic left right (Just False)
  ADC -> arithmetic left right (flagC flags)
  SBC -> arithmetic left (complement <$> right) (flagC flags)
  RSC -> arithmetic (complement <$> left) right (flagC flags)
  TST -> logical ((.&.) <$> left <*> right)
  TEQ -> logical (xor <$> left <*> right)
  CMP -> arithmetic left (complement <$> right) (Just True)
  CMN -> arithmetic left right (Just False)
  ORR -> logical ((.|.) <$> left <*> right)
  MOV -> logical right
  BIC -> logical ((\a b -> a .&. complement b) <$> left <*> right)
  MVN -> logical (complement <$> right)
  where
    logical result = (result, flags {flagN = (`testBit` 31) <$> result, flagZ = (== 0) <$> result, flagC = shifterCarry})
    arithmetic a b carryIn =
      let outcome = addWithCarry <$> a <*> b <*> carryIn
          result = (\(value, _, _) -> value) <$> outcome
       in (result, Flags ((`testBit` 31) <$> result) ((== 0) <$> result) ((\(_, carryOut, _) -> carryOut) <$> outcome) ((\(_, _, overflow) -> overflow) <$> outcome))

addWithCarry :: Word32 -> Word32 -> Bool -> (Word32, Bool, Bool)
addWithCarry a b carryIn =
  let incoming = if carryIn then 1 else 0 :: Integer
      unsigned = toInteger a + toInteger b + incoming
      result = fromInteger unsigned :: Word32
      signed value = toInteger (fromIntegral value :: Int32)
   in (result, unsigned /= toInteger result, signed a + signed b + incoming /= signed result)

arithmeticShift :: Word32 -> Int -> Word32
arithmeticShift value amount = fromIntegral ((fromIntegral value :: Int32) `shiftR` amount)

shiftByImmediate :: Shift -> Int -> Word32 -> Bool -> (Word32, Bool)
shiftByImmediate kind amount value carry = case kind of
  LSL
    | amount == 0 -> (value, carry)
    | otherwise -> (value `shiftL` amount, testBit value (32 - amount))
  LSR
    | amount == 0 -> (0, testBit value 31)
    | otherwise -> (value `shiftR` amount, testBit value (amount - 1))
  ASR
    | amount == 0 -> (if testBit value 31 then maxBound else 0, testBit value 31)
    | otherwise -> (arithmeticShift value amount, testBit value (amount - 1))
  ROR
    | amount == 0 -> ((if carry then bit 31 else 0) .|. (value `shiftR` 1), testBit value 0)
    | otherwise -> (value `rotateR` amount, testBit value (amount - 1))

shiftByRegister :: Shift -> Int -> Word32 -> Bool -> (Word32, Bool)
shiftByRegister kind amount value carry
  | amount == 0 = (value, carry)
  | otherwise = case kind of
      LSL
        | amount < 32 -> (value `shiftL` amount, testBit value (32 - amount))
        | amount == 32 -> (0, testBit value 0)
        | otherwise -> (0, False)
      LSR
        | amount < 32 -> (value `shiftR` amount, testBit value (amount - 1))
        | amount == 32 -> (0, testBit value 31)
        | otherwise -> (0, False)
      ASR
        | amount < 32 -> (arithmeticShift value amount, testBit value (amount - 1))
        | otherwise -> (if testBit value 31 then maxBound else 0, testBit value 31)
      ROR ->
        let rotation = amount `mod` 32
         in if rotation == 0 then (value, testBit value 31) else (value `rotateR` rotation, testBit value (rotation - 1))

-- ** Staged DAT

-- | Staged equals the original DAT, or the retained control built from it,
-- outside the main checksum word and the record-2 slot; inside the slot only
-- the reset vector word and the candidate image extent differ.
verifyStagedOwned :: B.ByteString -> B.ByteString -> B.ByteString -> GuestAddress -> Maybe C.PeerControl -> Either String StagedRecord
verifyStagedOwned original staged bin (GuestAddress entry) control = do
  reference <- maybe (Right original) (\selected -> C.controlDat <$> C.buildControl selected original) control
  referenceParsed <- U.parseContainer reference
  stagedParsed <- U.parseContainer staged
  forM_ (U.checks stagedParsed ++ U.carriedChecks stagedParsed) $ \check ->
    require (U.checkPassed check) ("staged container contract: " ++ U.checkField check)
  let mainOffset = U.hMainOffset . U.parsedHeader
      slotStart = U.stagingLength + fromIntegral (mainOffset referenceParsed) + F.recordSlotOffset F.SecondRecord
      slotLength = F.firstRecordSlotLength
      load = fromIntegral entry - F.recordGuestBase F.SecondRecord
      imageLength = B.length bin
      checksumEnd = U.mainChecksumOffset + 4
      slice start size = B.take size . B.drop start
  require (mainOffset referenceParsed == mainOffset stagedParsed) "staged main offset differs"
  require (B.length staged == B.length reference && slotStart + slotLength <= B.length reference && checksumEnd <= slotStart)
    "staged DAT length or record-2 slot extent differs"
  require (entry .&. 3 == 0 && imageLength > 0 && load >= F.resetVectorWordOffset + 4 && load + imageLength <= slotLength)
    "candidate image does not fit the record-2 slot after the reset vector"
  require (slice 0 U.mainChecksumOffset staged == slice 0 U.mainChecksumOffset reference
    && slice checksumEnd (slotStart - checksumEnd) staged == slice checksumEnd (slotStart - checksumEnd) reference
    && B.drop (slotStart + slotLength) staged == B.drop (slotStart + slotLength) reference)
    "staged DAT differs outside the main checksum word and the record-2 slot"
  let referenceSlot = slice slotStart slotLength reference
      stagedSlot = slice slotStart slotLength staged
      declared index = (index >= F.resetVectorWordOffset && index < F.resetVectorWordOffset + 4) || (index >= load && index < load + imageLength)
  require (and [declared index | (index, (before, after)) <- zip [0 :: Int ..] (B.zip referenceSlot stagedSlot), before /= after])
    "staged record 2 differs outside the reset vector word and the candidate image"
  require (fromIntegral (U.mediaWordAt stagedSlot F.resetVectorWordOffset) == entry) "staged reset vector does not name the candidate entry"
  require (B.map complement (slice load imageLength stagedSlot) == bin) "staged record-2 bytes differ from the candidate image"
  stagedRecord (B.map complement (B.take recordLength stagedSlot))

-- * Receipt

data EpochName = ResetEpoch | ReleasedEpoch | PublishedEpoch deriving (Eq, Show, Enum, Bounded)

data AccessKind = KindCpuRead | KindCpuWrite | KindCpuFetch | KindHostRead | KindHostWrite | KindModelBusRead | KindModelBusWrite
  deriving (Eq, Ord, Show, Enum, Bounded)

data Views = ObserveViews | FixedViews deriving (Eq, Show)

-- | Kinds sorted and distinct.
data WatchClass = WatchClass Interval [AccessKind] deriving (Eq, Ord, Show)

data Epoch = Epoch
  { epochName :: EpochName, epochStart :: Instructions, epochWatch :: W.WatchObservation
  , epochClasses :: [WatchClass], epochViews :: Views
  } deriving (Eq, Show)

data Label
  = HeldRelease | StartupEntry | StartupMain | PeerLoaded | PrimaryGate | PeerAcknowledged
  | PrimaryClear | ControlFailure | StartupHandoff | LoaderBEntry | ForbiddenFetch
  deriving (Eq, Show, Enum, Bounded)

-- | Row count of the stop's epoch watch at the snapshot.
newtype WatchCursor = WatchCursor Int deriving (Eq, Ord, Show)

-- | Per-core entered instruction counts, core 0 first.
data CoreCounts = CoreCounts Word64 Word64 deriving (Eq, Show)

-- | ATCM and BTCM region registers.
data Tcm = Tcm Word32 Word32 deriving (Eq, Show)

-- | Core 0, core 1.
data TcmBank = TcmBank Tcm Tcm deriving (Eq, Show)

-- | Physical [0x14c60,0x14ca0) and [0xa0000,0xa0548).
data ProgramState = ProgramState B.ByteString B.ByteString deriving (Eq, Show)

data StartupStop = StartupStop
  { stopLabel :: Label, stopAddress :: GuestAddress, stopCore :: Core, stopCount :: Instructions
  , stopCounts :: CoreCounts, stopRegisters :: Registers, stopSp :: GuestAddress, stopLr :: GuestAddress
  , stopCpsr :: Cpsr, stopState :: PeerState, stopTcm :: TcmBank, stopEpoch :: EpochName
  , stopCursor :: WatchCursor, stopProgramState :: Maybe ProgramState
  } deriving (Eq, Show)

-- | Core, guest address, physical address, size.
data MemoryMap = MemoryMap Core Word32 Word32 Word32 deriving (Eq, Show)

-- | Decoder choices, required maps, TCM registers at @held-release@.
data Bindings = Bindings [Word32] [MemoryMap] TcmBank deriving (Eq, Show)

-- | Root, index, record, header, entry, image-4 argument word, loader-B
-- digest, loader-B header word +0xc, and the core-1 mapping of the table
-- extent @[root, record + 36)@.
data Segment = Segment Word32 Word32 Word32 Word32 Word32 Word32 F.Sha256 Word32 MemoryMap deriving (Eq, Show)

-- | Record-2 digest and the site words read at @startup-entry@.
data RecordEvidence = RecordEvidence F.Sha256 [(GuestAddress, Word32)] deriving (Eq, Show)

newtype GateBindings = GateBindings Instructions deriving (Eq, Show)

-- | Counts before, counts after, cell 0 after.
data Microstep = Microstep CoreCounts CoreCounts Word32 deriving (Eq, Show)

data ClearReadback = ClearReadback
  { readbackCount :: Instructions, readbackAddress :: GuestAddress, readbackState :: PeerState
  , readbackCursor :: WatchCursor, readbackSteps :: [Microstep], readbackCpsr :: Cpsr
  } deriving (Eq, Show)

data PrimaryContext = PrimaryContext GuestAddress Registers GuestAddress GuestAddress Cpsr Instructions deriving (Eq, Show)

data ProgramKind = RetainedKind | OwnedKind deriving (Eq, Show)

data StartupReceipt = StartupReceipt
  { receiptScenario :: Scenario, receiptProgram :: ProgramKind, receiptResult :: WitnessResult
  , receiptError :: Maybe String, receiptMachineFailure :: Maybe String, receiptInstructions :: Instructions
  , receiptIdentities :: (Word32, Word32), receiptEpochs :: [Epoch], receiptStops :: [StartupStop]
  , receiptBindings :: Maybe Bindings, receiptGate :: Maybe GateBindings, receiptReadback :: Maybe ClearReadback
  , receiptFinalPrimary :: Maybe PrimaryContext, receiptSegment :: Maybe Segment, receiptRecord :: Maybe RecordEvidence
  } deriving (Eq, Show)

maxStartupReceiptBytes :: Int
maxStartupReceiptBytes = 16 * 1024 * 1024

maxWatchRows, maxStops, maxPairs :: Int
maxWatchRows = 12288
maxStops = 160
maxPairs = 128

epochText :: EpochName -> String
epochText ResetEpoch = "reset"
epochText ReleasedEpoch = "released"
epochText PublishedEpoch = "published"

kindText :: AccessKind -> String
kindText kind = case kind of
  KindCpuRead -> "cpu-read"
  KindCpuWrite -> "cpu-write"
  KindCpuFetch -> "cpu-fetch"
  KindHostRead -> "host-read"
  KindHostWrite -> "host-write"
  KindModelBusRead -> "model-bus-read"
  KindModelBusWrite -> "model-bus-write"

labelText :: Label -> String
labelText item = case item of
  HeldRelease -> "held-release"
  StartupEntry -> "startup-entry"
  StartupMain -> "startup-main"
  PeerLoaded -> "peer-loaded"
  PrimaryGate -> "primary-gate"
  PeerAcknowledged -> "peer-acknowledged"
  PrimaryClear -> "primary-clear"
  ControlFailure -> "control-failure"
  StartupHandoff -> "startup-handoff"
  LoaderBEntry -> "loader-b-entry"
  ForbiddenFetch -> "forbidden-fetch"

closed :: (Enum a, Bounded a) => (a -> String) -> String -> String -> Either String a
closed render what text = case [item | item <- [minBound .. maxBound], render item == text] of
  [found] -> Right found
  _ -> Left ("unknown " ++ what ++ ": " ++ text)

parseCore :: Object -> String -> Either String Core
parseCore row name = do
  number <- get row name :: Either String Word8
  case number of
    0 -> Right Core0
    1 -> Right Core1
    _ -> Left "startup witness core must be 0 or 1"

parseCursor :: Object -> String -> Either String WatchCursor
parseCursor row name = do
  value <- get row name :: Either String Word32
  require (value <= fromIntegral maxWatchRows) "watch cursor exceeds the collector limit"
  pure (WatchCursor (fromIntegral value))

parseCounts :: Value -> Either String CoreCounts
parseCounts value = do
  counts <- json value :: Either String [Word64]
  case counts of
    [first0, first1] -> Right (CoreCounts first0 first1)
    _ -> Left "counts must hold two per-core values"

parseTcm :: Value -> Either String TcmBank
parseTcm value = do
  registersValue <- json value :: Either String [[Word32]]
  case registersValue of
    [[atcm0, btcm0], [atcm1, btcm1]] -> Right (TcmBank (Tcm atcm0 btcm0) (Tcm atcm1 btcm1))
    _ -> Left "tcm must hold [[atcm0, btcm0], [atcm1, btcm1]]"

hexBytes :: String -> Either String B.ByteString
hexBytes text = do
  require (even (length text) && all (`elem` ("0123456789abcdef" :: String)) text) "program state is not lowercase hexadecimal"
  pure (B.pack (pairs text))
  where
    pairs (high : low : rest) = fromIntegral (digitToInt high * 16 + digitToInt low) : pairs rest
    pairs _ = []

parseProgramState :: Value -> Either String ProgramState
parseProgramState value = do
  row <- fields ["globals", "companion"] value
  globals <- get row "globals" >>= hexBytes
  companion <- get row "companion" >>= hexBytes
  require (B.length globals == 0x40 && B.length companion == 0x548) "program state extents differ"
  pure (ProgramState globals companion)

parseStop :: Value -> Either String StartupStop
parseStop value = do
  row <- fields ["label", "address", "core", "instructions", "counts", "registers", "sp", "lr", "cpsr", "state", "tcm", "epoch", "watch_records", "program_state"] value
  item <- get row "label" >>= closed labelText "startup stop label"
  StartupStop item <$> addressField row "address" <*> parseCore row "core" <*> (Instructions <$> get row "instructions")
    <*> (field row "counts" >>= parseCounts) <*> (field row "registers" >>= P.parseRegisters)
    <*> addressField row "sp" <*> addressField row "lr" <*> (Cpsr <$> get row "cpsr") <*> (field row "state" >>= P.parseState)
    <*> (field row "tcm" >>= parseTcm) <*> (get row "epoch" >>= closed epochText "epoch")
    <*> parseCursor row "watch_records" <*> (field row "program_state" >>= nullable parseProgramState)

parseClass :: Value -> Either String WatchClass
parseClass value = do
  row <- fields ["range", "kinds"] value
  extent <- field row "range" >>= parseInterval
  names <- get row "kinds" :: Either String [String]
  kinds <- mapM (closed kindText "watch kind") names
  require (not (null kinds) && length (nub kinds) == length kinds) "watch class kinds are empty or repeated"
  pure (WatchClass extent (sort kinds))

parseEpoch :: Value -> Either String Epoch
parseEpoch value = do
  row <- fields ["epoch", "instructions", "memory_watch"] value
  name <- get row "epoch" >>= closed epochText "epoch"
  start <- Instructions <$> get row "instructions"
  watchValue <- field row "memory_watch"
  watchRow <- fields ["enabled", "ranges", "records", "violation", "classes", "views"] watchValue
  classes <- field watchRow "classes" >>= json >>= mapM parseClass
  viewText <- get watchRow "views" :: Either String String
  views <- case viewText of
    "observe" -> Right ObserveViews
    "fixed" -> Right FixedViews
    _ -> Left "unknown watch view policy"
  limit <- W.watchLimit maxWatchRows
  watch <- W.parseWatchObservation limit (Object (KM.delete "classes" (KM.delete "views" watchRow)))
  pure (Epoch name start watch classes views)

parseBindings :: Value -> Either String Bindings
parseBindings value = do
  row <- fields ["decoder_choices", "maps", "tcm"] value
  maps <- field row "maps" >>= json >>= mapM parseMap
  Bindings <$> get row "decoder_choices" <*> pure maps <*> (field row "tcm" >>= parseTcm)

parseMap :: Value -> Either String MemoryMap
parseMap entry = do
  mapping <- fields ["core", "address", "physical", "size"] entry
  MemoryMap <$> parseCore mapping "core" <*> get mapping "address" <*> get mapping "physical" <*> get mapping "size"

parseSegment :: Value -> Either String Segment
parseSegment value = do
  row <- fields ["root", "index", "record", "header", "entry", "argument", "loader_sha256", "loader_entry", "table"] value
  Segment <$> get row "root" <*> get row "index" <*> get row "record" <*> get row "header" <*> get row "entry"
    <*> get row "argument" <*> (get row "loader_sha256" >>= F.parseSha256) <*> get row "loader_entry"
    <*> (field row "table" >>= parseMap)

parseRecord :: Value -> Either String RecordEvidence
parseRecord value = do
  row <- fields ["sha256", "site_words"] value
  digest <- get row "sha256" >>= F.parseSha256
  pairs <- get row "site_words" :: Either String [[Word32]]
  siteWords <- forM pairs $ \pair -> case pair of
    [location, word] -> Right (GuestAddress (fromIntegral location), word)
    _ -> Left "site word must be an [address, word] pair"
  pure (RecordEvidence digest siteWords)

gateCode :: C.PeerControl -> [(GuestAddress, B.ByteString)]
gateCode selected =
  [ (GuestAddress 0x100, if selected == C.HeldSentinel then B.replicate 104 0xff else veneerBytes)
  , (GuestAddress 0x1fe0, B.concat (map encodeLeWord
      [ 0xe3003000, 0xe344300a, 0xe3a02000, 0xe58320c4, 0xe3003000
      , if selected == C.HeldSentinel then 0xeafffffe else 0xeafff841, 0xe5832000, 0xf57ff04f ]))
  ]

parseGate :: Scenario -> Value -> Either String GateBindings
parseGate scenario value = do
  selected <- case scenario of
    Natural _ -> Left "natural receipt carries gate bindings"
    HeldSentinel -> Right C.HeldSentinel
    DelayedRelease _ -> Right C.DelayedRelease
  row <- fields ["core", "instructions", "decoder_choices", "low_ram", "atcm", "btcm", "stack_address", "stack", "code"] value
  gateCore <- get row "core" :: Either String Word32
  choices <- get row "decoder_choices" :: Either String [Word32]
  atcm <- get row "atcm" :: Either String Word32
  btcm <- get row "btcm" :: Either String Word32
  stackAddress <- addressField row "stack_address"
  stack <- get row "stack" :: Either String [Word8]
  require (gateCore == 0 && choices == [1] && atcm == 0x70000011 && btcm == 0 && stackAddress == GuestAddress 0x700007d8 && length stack == 40)
    "gate decoder/TCM/stack binding differs"
  low <- field row "low_ram" >>= fields ["address", "physical", "size", "permissions"]
  lowAddress <- addressField low "address"
  lowPhysical <- addressField low "physical"
  lowSize <- get low "size" :: Either String Word32
  permissions <- get low "permissions" :: Either String Word32
  require (lowAddress == GuestAddress 0 && lowPhysical == GuestAddress 0 && lowSize == 16384 && permissions == 7) "gate active low RAM binding differs"
  code <- get row "code" :: Either String [Value]
  require (length code == 2) "gate code binding extent count differs"
  forM_ (zip code (gateCode selected)) $ \(entry, (expectedAddress, expectedBytes)) -> do
    extent <- fields ["address", "virtual", "physical"] entry
    actualAddress <- addressField extent "address"
    virtual <- B.pack <$> get extent "virtual"
    physical <- B.pack <$> get extent "physical"
    require (actualAddress == expectedAddress && virtual == expectedBytes && physical == expectedBytes) "gate code or active alias bytes differ"
  GateBindings . Instructions <$> get row "instructions"

parseReadback :: Value -> Either String ClearReadback
parseReadback value = do
  row <- fields ["instructions", "primary_address", "primary_cpsr", "primary_instruction", "state", "watch_records", "microsteps"] value
  steps <- field row "microsteps" >>= json >>= mapM parseMicrostep
  require (not (null steps) && length steps <= 2048) "clear microstep bound exceeded"
  instruction <- get row "primary_instruction" :: Either String [Word8]
  require (instruction == [0, 32, 131, 229]) "clear instruction differs from the original STR"
  ClearReadback <$> (Instructions <$> get row "instructions") <*> addressField row "primary_address"
    <*> (field row "state" >>= P.parseState) <*> parseCursor row "watch_records" <*> pure steps
    <*> (Cpsr <$> get row "primary_cpsr")
  where
    parseMicrostep entry = do
      step <- fields ["before", "after", "cell0"] entry
      Microstep <$> (field step "before" >>= boundedCounts) <*> (field step "after" >>= boundedCounts) <*> get step "cell0"
    boundedCounts entry = do
      counts@(CoreCounts first0 first1) <- parseCounts entry
      require (first0 <= 10000000 && first1 <= 10000000) "microstep counts exceed the control budget"
      pure counts

parsePrimary :: Value -> Either String PrimaryContext
parsePrimary value = do
  row <- fields ["address", "registers", "sp", "lr", "cpsr", "instructions"] value
  PrimaryContext <$> addressField row "address" <*> (field row "registers" >>= P.parseRegisters)
    <*> addressField row "sp" <*> addressField row "lr" <*> (Cpsr <$> get row "cpsr") <*> (Instructions <$> get row "instructions")

parseStartupReceipt :: B.ByteString -> Either String StartupReceipt
parseStartupReceipt bytes = do
  require (B.length bytes <= maxStartupReceiptBytes) "startup receipt exceeds 16 MiB"
  value <- decodeStrictIntegers bytes
  row <- fields ["schema", "scenario", "program", "result", "error", "machine_failure", "instructions", "cp14_identities"
                , "epochs", "stops", "bindings", "gate", "clear_readback", "final_primary", "segment", "record"] value
  schema <- get row "schema" :: Either String Word32
  require (schema == 3) "unsupported startup receipt schema"
  scenario <- field row "scenario" >>= parseScenario
  programText <- get row "program" :: Either String String
  program <- case programText of
    "retained" -> Right RetainedKind
    "owned" -> Right OwnedKind
    _ -> Left "unknown startup receipt program"
  resultText <- get row "result" :: Either String String
  result <- case resultText of
    "passed" -> Right Passed
    "failed" -> Right Failed
    _ -> Left "unknown startup witness result"
  identities <- get row "cp14_identities" :: Either String [Word32]
  pair <- case identities of
    [identity0, identity1] -> Right (identity0, identity1)
    _ -> Left "startup receipt requires two CP14 identities"
  epochs <- field row "epochs" >>= json >>= mapM parseEpoch
  require (length epochs <= 3) "startup receipt holds more than three epochs"
  stops <- field row "stops" >>= json >>= mapM parseStop
  require (length stops <= maxStops) "startup receipt stop limit exceeded"
  StartupReceipt scenario program result <$> (field row "error" >>= nullable json)
    <*> (field row "machine_failure" >>= nullable json) <*> (Instructions <$> get row "instructions")
    <*> pure pair <*> pure epochs <*> pure stops
    <*> (field row "bindings" >>= nullable parseBindings)
    <*> (field row "gate" >>= nullable (parseGate scenario))
    <*> (field row "clear_readback" >>= nullable parseReadback)
    <*> (field row "final_primary" >>= nullable parsePrimary)
    <*> (field row "segment" >>= nullable parseSegment)
    <*> (field row "record" >>= nullable parseRecord)

-- * Acceptance

data Facts = Facts
  { factSites :: Sites, factRegister :: RegisterIndex, factPeriod :: PollPeriod, factCode :: Interval
  , factForbidFetch :: [Interval], factForbidRead :: [Interval]
  }

programFacts :: Program -> Facts
programFacts Retained = Facts retainedSites (RegisterIndex 3) (PollPeriod 304) (Interval 0x11800 0x11fec) [] []
programFacts (Owned facts) = Facts (ownedSites facts) (ownedLoadedRegister facts) (ownedPollPeriod facts)
  (ownedImage facts) (ownedForbidFetch facts) (ownedForbidRead facts)

programKind :: Program -> ProgramKind
programKind Retained = RetainedKind
programKind (Owned _) = OwnedKind

labelCore :: Label -> Core
labelCore item
  | item `elem` [PrimaryGate, PeerAcknowledged, PrimaryClear, ControlFailure] = Core0
  | otherwise = Core1

labelSite :: Facts -> Label -> Maybe GuestAddress
labelSite facts item = case item of
  HeldRelease -> Just (GuestAddress 0x3fff0000)
  StartupEntry -> Just (siteEntry sites)
  StartupMain -> Just (siteMain sites)
  PeerLoaded -> Just (sitePeerLoaded sites)
  PrimaryGate -> Just (GuestAddress 0x1ff4)
  PeerAcknowledged -> Just (GuestAddress 0x14c)
  PrimaryClear -> Just (GuestAddress 0x1ff8)
  ControlFailure -> Just (GuestAddress 0x148)
  StartupHandoff -> Just (siteHandoff sites)
  LoaderBEntry -> Just (GuestAddress 0xd0020)
  ForbiddenFetch -> Nothing
  where
    sites = factSites facts

endpointLabels :: Scenario -> [Label]
endpointLabels scenario = case scenario of
  Natural endpoint -> through endpoint
  HeldSentinel -> []
  DelayedRelease endpoint -> through endpoint
  where
    through endpoint = StartupHandoff : [LoaderBEntry | endpoint == LoaderEntry]

core0Labels :: Scenario -> [Label]
core0Labels scenario = case scenario of
  Natural _ -> []
  HeldSentinel -> [PrimaryGate]
  DelayedRelease _ -> [PrimaryGate, PeerAcknowledged, PrimaryClear]

protocolRanges :: [Interval]
protocolRanges = [Interval 0x14c80 0x14c88, Interval 0x20000 0x20008, Interval 0xa00c4 0xa00c8]

tablePhysical, tableGuest :: Segment -> Interval
tablePhysical (Segment _ _ _ _ _ _ _ _ (MemoryMap _ _ physical size)) = Interval (fromIntegral physical) (fromIntegral physical + fromIntegral size)
tableGuest (Segment _ _ _ _ _ _ _ _ (MemoryMap _ guest _ size)) = Interval (fromIntegral guest) (fromIntegral guest + fromIntegral size)

-- | A core-1 little-endian word read inside the table whose guest address the bound mapping translates to its physical address.
tableRead :: Segment -> W.WatchRecord -> Bool
tableRead found row = case row of
  W.CpuRecord {} ->
    let Interval low high = tablePhysical found
        Interval guest _ = tableGuest found
        physical = fromIntegral (W.watchAddressValue (W.cpuPhysical row)) :: Word64
        address = fromIntegral (W.watchAddressValue (W.cpuAddress row)) :: Word64
     in W.cpuCore row == W.Core1 && W.cpuAccess row == W.CpuRead && W.watchBytesValue (W.cpuBytes row) == 4
          && W.cpuByteOrder row == W.LittleEndian && physical >= low && physical + 4 <= high && address == guest + (physical - low)
          && maybe False (\value -> value <= 0xffffffff && W.cpuBefore row == Just (encodeLeWord (fromIntegral value))) (W.cpuValue row)
  W.TransferRecord {} -> False

expectedClasses :: Facts -> Maybe Segment -> EpochName -> Either String [WatchClass]
expectedClasses facts segment name = case name of
  ResetEpoch -> Right reset
  ReleasedEpoch -> Right released
  PublishedEpoch -> case segment of
    Just found ->
      Right (released ++ [WatchClass extent writes | extent <- [Interval 0xd0000 0xddaa0, Interval 0x61c40c 0x61c410, Interval 0x61c000 0x61c004]]
        ++ [WatchClass (tablePhysical found) (sort (KindCpuRead : writes))])
    Nothing -> Left "published epoch requires the segment binding"
  where
    allKinds = [minBound .. maxBound]
    writes = [KindCpuWrite, KindHostWrite, KindModelBusWrite]
    reset = [WatchClass extent allKinds | extent <- protocolRanges]
    released = reset
      ++ [WatchClass extent writes | extent <- [Interval 0x10000 0x14c60, Interval 0x14c6c 0x14c80]]
      ++ [WatchClass extent (sort (KindCpuRead : KindCpuFetch : KindModelBusRead : writes)) | extent <- factForbidFetch facts ++ factForbidRead facts]
      ++ [WatchClass extent [KindHostWrite, KindModelBusWrite] | extent <- [Interval 0x14c60 0x14c6c, Interval 0x14c88 0x14ca0, Interval 0xa0000 0xa00c4, Interval 0xa00c8 0xa0548]]

mergeIntervals :: [Interval] -> [Interval]
mergeIntervals = reverse . foldl step [] . sort
  where
    step (Interval low high : merged) (Interval nextLow nextHigh)
      | nextLow <= high = Interval low (max high nextHigh) : merged
    step merged next = next : merged

overlaps :: Interval -> Interval -> Bool
overlaps (Interval low high) (Interval otherLow otherHigh) = low < otherHigh && otherLow < high

rowExtent :: W.WatchRecord -> Interval
rowExtent row =
  let (physical, width) = case row of
        W.CpuRecord {} -> (W.cpuPhysical row, W.cpuBytes row)
        W.TransferRecord {} -> (W.transferPhysical row, W.transferBytes row)
      low = fromIntegral (W.watchAddressValue physical)
   in Interval low (low + fromIntegral (W.watchBytesValue width))

rowKind :: W.WatchRecord -> AccessKind
rowKind row = case row of
  W.CpuRecord {W.cpuAccess = access} -> case access of
    W.CpuRead -> KindCpuRead
    W.CpuWrite -> KindCpuWrite
    W.CpuFetch -> KindCpuFetch
  W.TransferRecord {W.transferAccess = access} -> case access of
    W.HostRead -> KindHostRead
    W.HostWrite -> KindHostWrite
    W.ModelBusRead -> KindModelBusRead
    W.ModelBusWrite -> KindModelBusWrite

nondecreasing :: Ord a => [a] -> Bool
nondecreasing values = and (zipWith (<=) values (drop 1 values))

sameSet :: Eq a => [a] -> [a] -> Bool
sameSet left right = length left == length right && length (nub left) == length left && all (`elem` right) left

lastOf :: [a] -> Maybe a
lastOf = foldl (\_ item -> Just item) Nothing

cpsrMasked :: Cpsr -> Bool
cpsrMasked (Cpsr value) = value .&. 0x010002ff == 0xd3

-- | Extents of the host reads the SDK takes at each endpoint stop.
endpointSnapshots :: [Interval]
endpointSnapshots = [Interval 0x14c60 0x14ca0, Interval 0xa0000 0xa0548]

-- | @snapshots@ bounds the endpoint snapshot reads of each extent in the epoch.
validateEpoch :: Maybe Segment -> [WatchClass] -> Instructions -> Int -> Epoch -> Either String ()
validateEpoch segment expected end snapshots epoch = do
  let watch = epochWatch epoch
      rows = W.watchRecords watch
      cpus = [row | row@W.CpuRecord {} <- rows]
      ranges = [Interval low high | extent <- W.watchRanges watch, let (low, high) = W.watchRangeBounds extent]
      name = epochText (epochName epoch)
      fixed = epochName epoch /= ResetEpoch
      afterStart row = not fixed || W.cpuInstructions row > epochStart epoch
      snapshot row = epochName epoch == PublishedEpoch && rowKind row == KindHostRead && rowExtent row `elem` endpointSnapshots
      -- The model records a reset-epoch bulk bus write at its whole transferred extent.
      resetBusWrite row = epochName epoch == ResetEpoch && rowKind row == KindModelBusWrite
      contained (Interval low high) = any (\(Interval rangeLow rangeHigh) -> rangeLow <= low && high <= rangeHigh) protocolRanges
      segmentRead row = epochName epoch == PublishedEpoch && maybe False (`tableRead` row) segment
  require (W.watchEnabled watch && isNothing (W.watchViolation watch)) (name ++ " watch is disabled or violated")
  require (sort (epochClasses epoch) == sort expected) (name ++ " watch classes differ")
  require (epochViews epoch == (if fixed then FixedViews else ObserveViews)) (name ++ " watch view policy differs")
  require (and (zipWith (\(Interval _ high) (Interval low _) -> high <= low) ranges (drop 1 ranges))
    && mergeIntervals ranges == mergeIntervals [extent | WatchClass extent _ <- expected]) (name ++ " watch ranges differ from the class union")
  require (nondecreasing (map W.cpuInstructions cpus) && nondecreasing (map W.cpuTick cpus)) (name ++ " CPU attempt chronology differs")
  require (all (\row -> afterStart row && W.cpuInstructions row <= end) cpus) (name ++ " CPU attempt lies outside its epoch")
  require (all (\extent -> length [() | row <- rows, snapshot row, rowExtent row == extent] <= snapshots) endpointSnapshots)
    (name ++ " endpoint snapshot reads exceed the endpoint stops")
  forM_ rows $ \row -> do
    let extent = rowExtent row
        kind = rowKind row
    require (any (\(WatchClass classExtent kinds) -> overlaps classExtent extent && kind `elem` kinds) expected) (name ++ " row is not watched by an overlapping class")
    require (kind /= KindHostWrite) "host write in startup receipt"
    require (snapshot row || resetBusWrite row || segmentRead row || contained extent) (name ++ " row lies outside a single protocol range")

validateCursor :: [W.WatchRecord] -> Instructions -> WatchCursor -> Either String ()
validateCursor rows time (WatchCursor offset) = do
  require (offset <= length rows) "watch cursor exceeds the epoch rows"
  require (all atOrBefore (take offset rows)) "watch cursor includes a later CPU attempt"
  require (not (any atOrBefore [row | row@W.CpuRecord {} <- drop offset rows])) "watch cursor omits an earlier CPU attempt"
  where
    atOrBefore row = case row of
      W.CpuRecord {} -> W.cpuInstructions row <= time
      W.TransferRecord {} -> True

data CpuRole
  = Sentinel0 | Sentinel1 | CompletionWrite | PrimaryClearWrite | VeneerPoll | VeneerRead
  | PeerPair | PeerClear | CellInit Word32 | TargetStore | ArgumentStore | TargetLoad | ArgumentLoad | TableRead
  deriving (Eq, Show)

data TransferRole = HostObservation | BusWrite | BusRead deriving (Eq, Show)

data Role = CpuRole CpuRole | TransferRole TransferRole deriving (Eq, Show)

data Located = Located {locatedEpoch :: EpochName, locatedIndex :: Int, locatedTime :: Instructions, locatedRow :: W.WatchRecord}

siteWatch :: GuestAddress -> W.WatchAddress
siteWatch (GuestAddress value) = W.watchAddress (fromIntegral value)

classifyRow :: Facts -> Maybe (Word32, Word32) -> Maybe Segment -> W.WatchRecord -> Either String Role
classifyRow facts publication segment row = case row of
  W.TransferRecord {W.transferAccess = access} -> case access of
    W.HostRead -> Right (TransferRole HostObservation)
    W.HostWrite -> Left "host write in startup receipt"
    W.ModelBusWrite -> Right (TransferRole BusWrite)
    W.ModelBusRead -> Right (TransferRole BusRead)
  W.CpuRecord {} | maybe False (`tableRead` row) segment -> Right (CpuRole TableRead)
  W.CpuRecord {} -> case [role | (role, matched) <- candidates, matched] of
    role : _ -> Right (CpuRole role)
    [] -> Left ("unexplained CPU attempt in protocol memory at pc " ++ hex (W.watchAddressValue (W.cpuPc row)))
  where
    sites = factSites facts
    Interval codeLow codeHigh = factCode facts
    word value = Just (fromIntegral (value :: Word32) :: Word64)
    attempt access cpu physical guest value before =
      W.cpuAccess row == access && W.cpuCore row == cpu && W.cpuPhysical row == W.watchAddress physical
        && W.cpuAddress row == W.watchAddress guest && W.watchBytesValue (W.cpuBytes row) == 4 && W.cpuByteOrder row == W.LittleEndian
        && W.cpuValue row == word value && W.cpuBefore row == Just (encodeLeWord before)
    at pc = W.cpuPc row == W.watchAddress pc
    atSite select = W.cpuPc row == siteWatch (select sites)
    pcValue = fromIntegral (W.watchAddressValue (W.cpuPc row)) :: Word64
    initialises physical = pcValue >= codeLow && pcValue < codeHigh && attempt W.CpuWrite W.Core1 physical physical 0 0xffffffff
    publishes check = maybe False check publication
    candidates =
      [ (Sentinel0, at 0x1e64 && attempt W.CpuWrite W.Core0 0x20000 0x40020000 0x22b 0x22b)
      , (Sentinel1, at 0x1e68 && attempt W.CpuWrite W.Core0 0x20004 0x40020004 0x22b 0x22b)
      , (CompletionWrite, at 0x1fec && attempt W.CpuWrite W.Core0 0xa00c4 0x400a00c4 0 0)
      , (PrimaryClearWrite, at 0x1ff8 && attempt W.CpuWrite W.Core0 0x20000 0x40020000 0 0x22b)
      , (VeneerPoll, at 0x114 && any (\value -> attempt W.CpuRead W.Core0 0x20004 0x40020004 value value) [0, 0x22b])
      , (VeneerRead, at 0x2020 && attempt W.CpuRead W.Core0 0x20004 0x40020004 0 0)
      , (PeerPair, atSite sitePeerLoad && W.cpuAccess row == W.CpuRead)
      , (PeerClear, atSite siteClearStore && attempt W.CpuWrite W.Core1 0x20004 0x40020004 0 0x22b)
      , (TargetStore, atSite siteTargetStore && publishes (\(entry, _) -> attempt W.CpuWrite W.Core1 0x14c80 0x14c80 entry 0))
      , (ArgumentStore, atSite siteArgumentStore && publishes (\(_, value) -> attempt W.CpuWrite W.Core1 0x14c84 0x14c84 value 0))
      , (TargetLoad, atSite siteTargetLoad && publishes (\(entry, _) -> attempt W.CpuRead W.Core1 0x14c80 0x14c80 entry entry))
      , (ArgumentLoad, atSite siteArgumentLoad && publishes (\(_, value) -> attempt W.CpuRead W.Core1 0x14c84 0x14c84 value value))
      , (CellInit 0x14c80, initialises 0x14c80)
      , (CellInit 0x14c84, initialises 0x14c84)
      ]

checkStartupReceipt :: Scenario -> Program -> StartupReceipt -> Either String ()
checkStartupReceipt scenario program receipt = do
  let facts = programFacts program
      sites = factSites facts
      held = scenario == HeldSentinel
      total = receiptInstructions receipt
      PollPeriod period = factPeriod facts
      stops = receiptStops receipt
      epochs = receiptEpochs receipt
      countOf = instructionsValue . stopCount
      core1Of (CoreCounts _ value) = value
  require (receiptScenario receipt == scenario) "receipt scenario differs from the contract"
  require (receiptProgram receipt == programKind program) "receipt program differs from the contract"
  require (receiptResult receipt == Passed && isNothing (receiptError receipt) && isNothing (receiptMachineFailure receipt)) "startup witness failed"
  require (receiptIdentities receipt == (0x4003, 0x5003)) "startup CP14 identities differ"
  require (total > Instructions 0 && total <= scenarioBudget scenario) "startup instruction budget exceeded"
  forM_ stops $ \stop -> do
    require (stopLabel stop /= ForbiddenFetch) "forbidden-fetch stop in startup receipt"
    require (stopLabel stop /= ControlFailure) "control-failure stop in startup receipt"
    require (stopCore stop == labelCore (stopLabel stop)) ("stop on the wrong core: " ++ labelText (stopLabel stop))
    require (Just (stopAddress stop) == labelSite facts (stopLabel stop)) ("stop address differs: " ++ labelText (stopLabel stop))
    let CoreCounts first0 first1 = stopCounts stop
    require (toInteger first0 + toInteger first1 == toInteger (countOf stop)) "stop per-core counts do not sum to its instructions"
  require (nondecreasing (map stopCount stops) && fmap stopCount (lastOf stops) == Just total) "stop chronology or final stop count differs"
  require (nondecreasing [value | CoreCounts value _ <- map stopCounts stops] && nondecreasing (map (core1Of . stopCounts) stops)) "per-core stop counts regress"
  require (map stopLabel (filter ((== Core0) . stopCore) stops) == core0Labels scenario) "core-0 stops differ from the scenario"
  (heldRelease, mainStop, loads, endpoints) <- case filter ((== Core1) . stopCore) stops of
    first : second : third : rest
      | map stopLabel [first, second, third] == [HeldRelease, StartupEntry, StartupMain] -> do
          let (peerLoads, after) = span ((== PeerLoaded) . stopLabel) rest
          require (not (null peerLoads) && length peerLoads <= maxPairs) "peer-loaded count lies outside 1..128"
          require (map stopLabel after == endpointLabels scenario) "core-1 stops after the peer loads differ from the scenario"
          when held (require (length peerLoads == 4) "held control requires exactly four peer-loaded stops")
          pure (first, third, peerLoads, after)
    _ -> Left "core-1 stops do not begin held-release, startup-entry, startup-main"
  let exitStop = if held then Nothing else lastOf loads
      releasedStart = stopCount heldRelease
      epochAt time
        | time <= releasedStart = ResetEpoch
        | maybe True ((time <=) . stopCount) exitStop = ReleasedEpoch
        | otherwise = PublishedEpoch
      -- A core-0 stop sharing a pause with an epoch-opening core-1 stop may lie on either side.
      epochsFor stop
        | stopCore stop == Core0 && stopCount stop == releasedStart = [ResetEpoch, ReleasedEpoch]
        | stopCore stop == Core0 && Just (stopCount stop) == fmap stopCount exitStop = [ReleasedEpoch, PublishedEpoch]
        | otherwise = [epochAt (stopCount stop)]
      rowsOf name = concat [W.watchRecords (epochWatch epoch) | epoch <- epochs, epochName epoch == name]
      startsExpected = [Instructions 0, releasedStart] ++ maybe [] (pure . stopCount) exitStop
  require (map epochName epochs == [ResetEpoch, ReleasedEpoch] ++ [PublishedEpoch | not held]) "startup epochs differ from the scenario"
  require (map epochStart epochs == startsExpected) "startup epoch start counts differ"
  segment <- case (held, receiptSegment receipt) of
    (True, Nothing) -> Right Nothing
    (False, Just found@(Segment root index record _ entry _ loaderDigest loaderEntry (MemoryMap tableCore tableGuestBase tablePhysicalBase tableSize))) -> do
      require (entry == 0xd0020 && loaderEntry == entry) "segment entry differs from the loader-B header entry 0xd0020"
      require (F.sha256Hex loaderDigest == loaderBDigestHex) "loader B digest differs"
      require (toInteger record == toInteger root + 36 * toInteger index) "segment record is not root + 36 * index"
      require (tableCore == Core1 && tableGuestBase == root && toInteger tableSize == 36 * (toInteger index + 1)
        && toInteger tableGuestBase + toInteger tableSize <= 2 ^ (32 :: Int) && toInteger tablePhysicalBase + toInteger tableSize <= 2 ^ (32 :: Int))
        "segment table mapping differs from [root, record + 36) on core 1"
      pure (Just found)
    _ -> Left "segment binding presence differs from the scenario"
  classesByEpoch <- forM epochs $ \epoch -> (,) epoch <$> expectedClasses facts segment (epochName epoch)
  forM_ segment $ \found ->
    require (and [not (overlaps (tablePhysical found) extent) | Right classes <- [expectedClasses facts Nothing ReleasedEpoch], WatchClass extent _ <- classes]
      && not (any (overlaps (tablePhysical found)) [Interval 0xd0000 0xddaa0, Interval 0x61c40c 0x61c410, Interval 0x61c000 0x61c004]))
      "segment table overlaps another watched extent"
  require (sum [length (W.watchRecords (epochWatch epoch)) | epoch <- epochs] <= maxWatchRows) "startup watch rows exceed the aggregate bound"
  let endpointStops name = length [() | stop <- stops, stopEpoch stop == name, stopLabel stop `elem` [StartupHandoff, LoaderBEntry]]
  forM_ (zip classesByEpoch (map epochStart (drop 1 epochs) ++ [total])) $ \((epoch, expected), end) ->
    validateEpoch segment expected end (endpointStops (epochName epoch)) epoch
  forM_ stops $ \stop -> do
    require (stopEpoch stop `elem` epochsFor stop) ("stop epoch differs: " ++ labelText (stopLabel stop))
    validateCursor (rowsOf (stopEpoch stop)) (stopCount stop) (stopCursor stop)
  forM_ [minBound .. maxBound] $ \name ->
    require (nondecreasing [stopCursor stop | stop <- stops, stopEpoch stop == name]) "stop cursors regress within an epoch"
  forM_ stops $ \stop ->
    require (isJust (stopProgramState stop) == (stopLabel stop `elem` [StartupHandoff, LoaderBEntry])) "program state presence differs"
  -- 1. held-release
  Bindings choices maps bindingTcm <- maybe (Left "held-release bindings missing") Right (receiptBindings receipt)
  let TcmBank heldCore0 (Tcm _ heldBtcm1) = stopTcm heldRelease
  require (choices == [1]) "decoder choices differ"
  require (sameSet maps requiredMaps) "held-release identity maps differ"
  require (bindingTcm == stopTcm heldRelease) "binding TCM registers differ from the held-release stop"
  -- 2. startup-entry record evidence
  RecordEvidence recordDigest siteWords <- maybe (Left "startup-entry record evidence missing") Right (receiptRecord receipt)
  require (F.sha256Hex recordDigest == programRecordHex program) "staged record-2 digest differs from the contract"
  let offsetSite offset select = let GuestAddress base = select sites in GuestAddress (base + offset)
      plain = offsetSite 0
      expectedSites = case program of
        Retained -> [plain siteEntry, offsetSite 4 siteEntry, plain sitePeerLoad] ++ common
        Owned _ -> [plain siteEntry, offsetSite 4 siteEntry, offsetSite 8 siteEntry, plain sitePeerLoad, GuestAddress (addressWord (sitePeerLoaded sites) - 4), offsetSite 4 sitePeerLoad] ++ common
      common = map plain [siteClearStore, siteTargetStore, siteArgumentStore, siteTargetLoad, siteArgumentLoad, siteHandoff]
      wordAt location = lookup location siteWords
      decodedAt location@(GuestAddress value) = A32.decode (fromIntegral value) <$> wordAt location
      isWordStore instruction = case instruction of
        Just (Store (Cond 14) Word _ _ _ _) -> True
        _ -> False
      isWordLoad instruction = case instruction of
        Just (Load (Cond 14) Word False _ _ _ _) -> True
        _ -> False
  require (map fst siteWords == expectedSites) "startup-entry site word addresses differ"
  case program of
    Retained -> require (wordAt (plain siteEntry) == Just 0xeb000013) "retained entry word differs"
    Owned _ -> do
      require (map decodedAt [plain siteEntry, offsetSite 4 siteEntry, offsetSite 8 siteEntry]
        == map (Just . Branch (Cond 14) True) [0x11854, 0x118b8, fromIntegral (addressWord (siteMain sites))]) "owned entry words are not BL 0x11854, BL 0x118b8, BL main"
      require (case decodedAt (GuestAddress (addressWord (sitePeerLoaded sites) - 4)) of
        Just (Branch (Cond 14) True _) -> True
        _ -> False) "owned word before peer_loaded is not a BL"
      require (returnsWithout (Reg (registerIndexValue (factRegister facts))) (decodedAt (offsetSite 4 sitePeerLoad)))
        "owned word after peer_load is not a return that keeps the loaded register"
  require (wordAt (plain siteHandoff) == Just 0xe1a0f003) "handoff word differs from MOV PC, R3"
  loadBase <- case decodedAt (plain sitePeerLoad) of
    Just (Load (Cond 14) Word False (Reg number) (Reg base) (OffsetImmediate 0) OffsetOnly)
      | number == registerIndexValue (factRegister facts) && base < 13 -> registerIndex base
    _ -> Left "peer_load is not a word LDR [rn] into the loaded register"
  require (all (isWordStore . decodedAt . plain) [siteClearStore, siteTargetStore, siteArgumentStore]) "store site word is not a word STR"
  require (all (isWordLoad . decodedAt . plain) [siteTargetLoad, siteArgumentLoad]) "load site word is not a word LDR"
  -- 3. startup-main
  let TcmBank mainCore0 (Tcm mainAtcm1 mainBtcm1) = stopTcm mainStop
      banks (TcmBank (Tcm atcm0 btcm0) (Tcm atcm1 btcm1)) = [atcm0, btcm0, atcm1, btcm1]
  require (cpsrMasked (stopCpsr mainStop)) "startup-main CPSR is not masked A32 little-endian SVC"
  require (stopSp mainStop == GuestAddress 0x70000800) "startup-main SP differs"
  require (mainAtcm1 == 0x70000011 && mainBtcm1 == heldBtcm1 && mainCore0 == heldCore0) "startup-main TCM transition differs"
  forM_ (drop 1 (dropWhile (/= heldRelease) (takeWhile (/= mainStop) stops))) $ \stop ->
    require (stopTcm stop == stopTcm heldRelease) ("TCM registers changed before startup-main: " ++ labelText (stopLabel stop))
  forM_ [stop | stop <- stops, stopCount stop > stopCount mainStop] $ \stop ->
    require (stopTcm stop == stopTcm mainStop) ("TCM registers changed after startup-main: " ++ labelText (stopLabel stop))
  -- Guest views of every guarded extent: physical identity, the 0x40000000 alias and every bound map.
  let guarded = concat [[extent, alias extent] | (_, expected) <- classesByEpoch, WatchClass extent _ <- expected]
        ++ [Interval (fromIntegral guest) (fromIntegral guest + fromIntegral size) | MemoryMap _ guest _ size <- maps]
        ++ [tableGuest found | Just found <- [segment]]
      alias (Interval low high) = Interval (low + 0x40000000) (high + 0x40000000)
  windows <- mapM tcmWindow (banks (stopTcm heldRelease) ++ banks (stopTcm mainStop))
  require (and [not (overlaps window extent) | Just window <- windows, extent <- guarded]) "enabled TCM window overlaps a guarded guest view"
  -- 4. peer-loaded pairs
  let releasedRows = rowsOf ReleasedEpoch
      preclearCount = [stopCount stop | stop <- stops, stopLabel stop == PrimaryClear]
      rearmed stop = case preclearCount of
        [count] -> stopCount stop > count && all (\earlier -> stopCount earlier <= count) (takeWhile (/= stop) loads)
        _ -> False
      attributed row = W.cpuCore row == W.Core1 && W.cpuAddress row == W.watchAddress 0x40020000 && W.cpuPhysical row == W.watchAddress 0x20000
        && W.watchBytesValue (W.cpuBytes row) == 4 && W.cpuByteOrder row == W.LittleEndian
  forM_ preclearCount $ \count ->
    require (length (filter ((== count) . stopCount) stops) == 1) "primary-clear shares its pause with another stop"
  pairing <- forM (zip (stopCursor mainStop : map stopCursor loads) loads) $ \(WatchCursor from, stop) -> do
    let WatchCursor to = stopCursor stop
        window = take (to - from) (drop from releasedRows)
        peerReads = [row | row@W.CpuRecord {W.cpuAccess = W.CpuRead} <- window, W.cpuPc row == siteWatch (sitePeerLoad sites)]
    (unstopped, row) <- case reverse peerReads of
      [found] -> Right ([], found)
      found : earlier@(_ : _) | rearmed stop -> Right (earlier, found)
      _ -> Left "peer-loaded stop lacks exactly one peer_load read since the previous cursor"
    require (all attributed (row : unstopped)) "peer_load read attribution differs"
    require (all ((== Just 0x22b) . W.cpuValue) unstopped) "peer_load read without a stop is not waiting"
    require (lastOf [cpu | cpu@W.CpuRecord {} <- window, W.cpuCore cpu == W.Core1] == Just row) "core-1 access intervenes between the peer load and its stop"
    require (W.cpuValue row == Just (fromIntegral (registerAt (factRegister facts) (stopRegisters stop)))) "loaded register differs from the read value"
    require (loadBase == factRegister facts || registerAt loadBase (stopRegisters stop) == W.watchAddressValue (W.cpuAddress row))
      "peer_load base register differs from the loaded peer-cell address"
    pure (W.cpuValue row == Just 0x22b, length unstopped)
  let loadedValues = map fst pairing
      pairCount = length loads + sum (map snd pairing)
  require (pairCount <= maxPairs) "peer pairs exceed 128"
  case (held, reverse loadedValues) of
    (True, _) -> require (and loadedValues) "held control observed a non-waiting load"
    (False, final : earlier) -> require (not final && and earlier) "exit pair is not exactly the last peer-loaded stop"
    (False, []) -> Left "peer-loaded stops missing"
  forM_ (zip (zip loads loadedValues) (drop 1 (zip loads loadedValues))) $ \((before, waitingBefore), (after, waitingAfter)) ->
    when (waitingBefore && waitingAfter && not (rearmed after))
      (require (core1Of (stopCounts after) - core1Of (stopCounts before) == period) "waiting stops differ from the poll period")
  -- 6, 7. endpoints
  publication <- case (endpoints, segment) of
    (_ : _, Just (Segment _ _ _ _ entry value _ _ _)) -> Right (Just (entry, value))
    ([], Nothing) -> Right Nothing
    _ -> Left "endpoint stops and segment binding disagree"
  forM_ endpoints $ \stop -> do
    let regs = stopRegisters stop
        state = stopState stop
        mainRegs = stopRegisters mainStop
    (entry, value) <- maybe (Left "endpoint lacks publication") Right publication
    require (cpsrMasked (stopCpsr stop)) "endpoint CPSR is not masked A32 little-endian SVC"
    require (r3 regs == entry && target state == GuestAddress (fromIntegral entry)) "endpoint target differs from the segment entry"
    require (r0 regs == value && argument state == Argument value) "endpoint argument differs"
    require (stopSp stop == stopSp mainStop) "endpoint SP differs from startup-main"
    require (map ($ regs) [r6, r7, r8, r9, r10, r11] == map ($ mainRegs) [r6, r7, r8, r9, r10, r11]) "endpoint r6-r11 differ from startup-main"
    require (cell0 state == 0 && cell1 state == 0) "endpoint peer cells are not clear"
  -- 8. protocol rows
  located <- fmap concat . forM epochs $ \epoch ->
    forM (zip [0 ..] (W.watchRecords (epochWatch epoch))) $ \(index, row) -> do
      role <- classifyRow facts publication segment row
      pure (role, epochName epoch, index, row)
  let having wanted = [Located name index (W.cpuInstructions row) row | (CpuRole role, name, index, row) <- located, role == wanted]
      transfers wanted = [(name, index) | (TransferRole role, name, index, _) <- located, role == wanted]
      unique wanted = case having wanted of
        [found] -> Right found
        _ -> Left ("missing or repeated protocol attempt: " ++ show wanted)
      absent wanted = require (null (having wanted)) ("protocol attempt not allowed in this scenario: " ++ show wanted)
  sentinel0 <- unique Sentinel0
  sentinel1 <- unique Sentinel1
  completionWrite <- unique CompletionWrite
  peerClear <- unique PeerClear
  initTarget <- unique (CellInit 0x14c80)
  initArgument <- unique (CellInit 0x14c84)
  firstLoad <- maybe (Left "peer-loaded stops missing") Right (lastOf (reverse loads))
  require (locatedEpoch sentinel0 == ResetEpoch && locatedEpoch sentinel1 == ResetEpoch && locatedTime sentinel0 < locatedTime sentinel1) "sentinel writes differ"
  forM_ (transfers BusWrite) $ \(name, index) ->
    require (name == ResetEpoch && index < locatedIndex sentinel1) "model-bus write after the 0x1e68 sentinel write"
  forM_ (transfers BusRead) $ \(name, _) -> require (name == ResetEpoch) "model-bus read in a fixed epoch"
  require (length (having PeerPair) == pairCount) "peer_load reads outside the peer-loaded pairs"
  require (locatedTime peerClear > stopCount mainStop && locatedTime peerClear < stopCount firstLoad) "peer clear lies outside startup-main..first peer-loaded"
  require (locatedTime initTarget < locatedTime peerClear && locatedTime initArgument < locatedTime peerClear) "publication cells initialised after the peer clear"
  case (scenario, exitStop, endpoints) of
    (HeldSentinel, _, _) -> mapM_ absent [PrimaryClearWrite, VeneerPoll, VeneerRead, TargetStore, ArgumentStore, TargetLoad, ArgumentLoad]
    (_, Just exit, handoffStop : _) -> do
      primaryClear <- unique PrimaryClearWrite
      targetStore <- unique TargetStore
      argumentStore <- unique ArgumentStore
      targetLoad <- unique TargetLoad
      argumentLoad <- unique ArgumentLoad
      require (locatedTime completionWrite < locatedTime primaryClear && locatedTime primaryClear < stopCount exit) "primary clear order differs"
      require (all ((> stopCount exit) . locatedTime) [targetStore, argumentStore]) "publication precedes the exit pair"
      require (locatedTime targetStore < locatedTime targetLoad && locatedTime argumentStore < locatedTime argumentLoad
        && all ((< stopCount handoffStop) . locatedTime) [targetLoad, argumentLoad]) "publication loads are out of order"
      bound@(Segment _ segmentIndex _ segmentHeader _ _ _ _ _) <- maybe (Left "non-held scenario lacks the segment binding") Right segment
      let Interval tableLow _ = tablePhysical bound
          recordPhysical = tableLow + 36 * fromIntegral segmentIndex
          tableValue physical value = any (\tableRow -> locatedEpoch tableRow == PublishedEpoch
              && locatedTime tableRow > stopCount exit && locatedTime tableRow < locatedTime targetStore
              && fromIntegral (W.watchAddressValue (W.cpuPhysical (locatedRow tableRow))) == physical
              && W.cpuValue (locatedRow tableRow) == Just (fromIntegral (value :: Word32)))
            (having TableRead)
      require (tableValue recordPhysical 1 && tableValue (recordPhysical + 4) segmentHeader)
        "segment-table id and header reads through the bound mapping are absent"
      case scenario of
        Natural _ -> mapM_ absent [VeneerPoll, VeneerRead]
        _ -> pure ()
    _ -> Left "non-held scenario lacks the exit pair or handoff"
  -- 9, 10. controls
  let gateContext stop = let Cpsr flags = stopCpsr stop in
        stopSp stop == GuestAddress 0x700007f0 && flags .&. 0x010002ff == 0xd3 && r2 (stopRegisters stop) == 0 && r3 (stopRegisters stop) == 0
      uniqueStop item = case filter ((== item) . stopLabel) stops of
        [found] -> Right found
        _ -> Left ("missing or repeated stop: " ++ labelText item)
      gateBound gate = do
        case receiptGate receipt of
          Just (GateBindings time) -> require (time == stopCount gate) "gate binding count differs from the gate stop"
          Nothing -> Left "gate bindings missing"
        require (within (Instructions 0) (stopCount gate) prefixBound) "prefix-to-gate bound exceeded"
  case scenario of
    Natural _ ->
      require (isNothing (receiptGate receipt) && isNothing (receiptReadback receipt) && isNothing (receiptFinalPrimary receipt)) "natural receipt carries control evidence"
    HeldSentinel -> do
      gate <- uniqueStop PrimaryGate
      gateBound gate
      require (gateContext gate) "primary gate live context differs"
      require (isNothing (receiptReadback receipt)) "held control carries clear readback"
      require (receiptFinalPrimary receipt == Just (PrimaryContext (GuestAddress 0x1ff4) (stopRegisters gate) (stopSp gate) (stopLr gate) (stopCpsr gate) total))
        "held primary context changed"
      final <- maybe (Left "held control lacks peer-loaded stops") Right (lastOf loads)
      require (within (stopCount gate) (stopCount final) heldBound) "gate-to-final peer-loaded bound exceeded"
    DelayedRelease _ -> do
      gate <- uniqueStop PrimaryGate
      ack <- uniqueStop PeerAcknowledged
      preclear <- uniqueStop PrimaryClear
      exit <- maybe (Left "delayed control lacks the exit pair") Right exitStop
      gateBound gate
      require (gateContext gate) "primary gate live context differs"
      require (isNothing (receiptFinalPrimary receipt)) "delayed control carries a held final context"
      require (stopRegisters preclear == (stopRegisters gate) {r3 = 0x40020000} && stopSp preclear == stopSp gate
        && stopLr preclear == stopLr gate && stopCpsr preclear == stopCpsr gate) "primary live context was not restored"
      require (length [() | earlier <- loads, stopCount earlier < stopCount preclear] >= 4) "delayed clear precedes four waiting peer-loaded stops"
      readback <- maybe (Left "committed clear readback missing") Right (receiptReadback receipt)
      CoreCounts final0 final1 <- validateMicrosteps preclear readback
      require (stopCount preclear < readbackCount readback && readbackCount readback < stopCount exit) "committed clear lies outside primary-clear..exit pair"
      endpoint <- maybe (Left "delayed control lacks its endpoint stop") Right (lastOf endpoints)
      require (within (stopCount gate) (readbackCount readback) delayedBound) "gate-to-committed-clear bound exceeded"
      require (within (readbackCount readback) (stopCount endpoint) releaseBound) "committed-clear-to-endpoint bound exceeded"
      require (readbackState readback == PeerState 0 0 0 (GuestAddress 0) (Argument 0) && readbackAddress readback == GuestAddress 0x1ffc
        && readbackCpsr readback == stopCpsr preclear) "committed original-store readback differs"
      require (stopEpoch preclear == ReleasedEpoch && stopCursor preclear < readbackCursor readback && readbackCursor readback <= stopCursor exit)
        "clear readback cursor order differs"
      validateCursor releasedRows (readbackCount readback) (readbackCursor readback)
      let WatchCursor preclearCursor = stopCursor preclear
          WatchCursor committedCursor = readbackCursor readback
          interval = take (committedCursor - preclearCursor) (drop preclearCursor releasedRows)
          physicalReads physical = length [() | W.TransferRecord W.HostRead guest actual width <- interval
            , guest == W.watchAddress physical, actual == W.watchAddress physical, W.watchBytesValue width == 4]
      require (physicalReads 0x20000 == length (readbackSteps readback) + 1 && all ((== 1) . physicalReads) [0x20004, 0xa00c4, 0x14c80, 0x14c84])
        "clear microsteps lack immediate physical scalar observations"
      primaryClear <- unique PrimaryClearWrite
      require (locatedEpoch primaryClear == ReleasedEpoch && locatedIndex primaryClear >= preclearCursor && locatedIndex primaryClear < committedCursor
        && locatedTime primaryClear == readbackCount readback) "original clear attempt lies outside the committed readback window"
      require (null [() | W.CpuRecord {} <- take (committedCursor - locatedIndex primaryClear - 1) (drop (locatedIndex primaryClear + 1) releasedRows)])
        "CPU attempt intervenes before the committed clear readback"
      let polls = having VeneerPoll
      require (length polls <= 8192 && all ((> stopCount gate) . locatedTime) polls) "veneer polls exceed their bound or precede the gate"
      require (all ((> readbackCount readback) . locatedTime) (having VeneerRead)) "veneer completion read precedes the committed clear"
      require (locatedTime peerClear < stopCount ack) "acknowledgment precedes the peer clear"
      require (any (\poll -> W.cpuValue (locatedRow poll) == Just 0 && locatedTime poll > locatedTime peerClear && locatedTime poll < stopCount ack) polls)
        "acknowledgment lacks a veneer zero load after the peer clear"
      let CoreCounts ack0 ack1 = stopCounts ack
      require (final0 >= ack0 && final1 >= ack1) "committed clear counts precede the acknowledgment"
      let delta0 = final0 - ack0
          delta1 = final1 - ack1
          pairsAfterAck = length [() | pair <- having PeerPair, locatedTime pair > stopCount ack, locatedTime pair < readbackCount readback]
      require (toInteger delta1 <= 1000 * ((toInteger delta0 + 999) `div` 1000 + 1)) "core-1 progress since acknowledgment exceeds its bound"
      require (fromIntegral pairsAfterAck <= delta1 `div` period + 1) "peer pairs since acknowledgment exceed the poll bound"

-- | Aggregate-instruction phase bounds shared with the SDK's schema-2 controls.
prefixBound, heldBound, delayedBound, releaseBound :: Word64
prefixBound = 4000000
heldBound = 4000000
delayedBound = 5100000
releaseBound = 20000

-- | The later count follows the earlier by at most the bound.
within :: Instructions -> Instructions -> Word64 -> Bool
within (Instructions earlier) (Instructions later) bound = earlier <= later && later - earlier <= bound

requiredMaps :: [MemoryMap]
requiredMaps =
  [ MemoryMap Core1 0x10000 0x10000 0x4ca0, MemoryMap Core1 0x40020000 0x20000 0x8, MemoryMap Core1 0xd0000 0xd0000 0xdaa0
  , MemoryMap Core1 0x61c000 0x61c000 0x4, MemoryMap Core1 0x61c40c 0x61c40c 0x4 ]

-- | @bx lr@, or @pop {..., pc}@ that leaves @register@ unwritten.
returnsWithout :: Reg -> Maybe Instruction -> Bool
returnsWithout register instruction = case instruction of
  Just (BranchExchange (Cond 14) False (Reg 14)) -> True
  Just (LoadMultiple (Cond 14) (Reg 13) IncrementAfter True list) -> A32.pc `elem` list && register `notElem` list
  _ -> False

-- | Cortex-R TCM region register: base [31:12], size code [6:2] (@0@ none, @3..14@ 4 KiB..8 MiB), enable [0].
-- The base must be aligned to the size.
tcmWindow :: Word32 -> Either String (Maybe Interval)
tcmWindow value
  | not (testBit value 0) || code == 0 = Right Nothing
  | code < 3 || code > 14 = Left "TCM region size encoding is reserved"
  | base `mod` size /= 0 || base + size > bit 32 = Left "TCM region is misaligned or extends past 4 GiB"
  | otherwise = Right (Just (Interval base (base + size)))
  where
    code = fromIntegral ((value `shiftR` 2) .&. 0x1f) :: Int
    base = fromIntegral (value .&. 0xfffff000) :: Word64
    size = bit (code + 9) :: Word64

validateMicrosteps :: StartupStop -> ClearReadback -> Either String CoreCounts
validateMicrosteps preclear readback = do
  let steps = readbackSteps readback
      aggregate (CoreCounts first0 first1) = first0 + first1
  forM_ steps $ \(Microstep before@(CoreCounts before0 before1) after@(CoreCounts after0 after1) _) ->
    require (aggregate after == aggregate before + 1 && ((after0 == before0 + 1 && after1 == before1) || (after0 == before0 && after1 == before1 + 1)))
      "microstep is not exactly one entered instruction"
  require (and (zipWith (\(Microstep _ after _) (Microstep before _ _) -> after == before) steps (drop 1 steps))) "microstep counts are not contiguous"
  require (all (\(Microstep _ _ scalar) -> scalar == 0x22b) (take (length steps - 1) steps)) "cell cleared before the final microstep"
  case (steps, reverse steps) of
    (Microstep initial _ _ : _, Microstep (CoreCounts before0 before1) final@(CoreCounts after0 after1) scalar : _) -> do
      require (initial == stopCounts preclear && aggregate final == instructionsValue (readbackCount readback)) "microstep boundary counts differ"
      require (scalar == 0 && after0 == before0 + 1 && after1 == before1) "final microstep did not clear on the primary core"
      pure final
    _ -> Left "clear microsteps missing"

-- * Comparison

-- | A receipt accepted by 'validateStartupReceipt' against its own contract.
newtype ValidatedStartupReceipt = ValidatedStartupReceipt StartupReceipt

validateStartupReceipt :: Scenario -> Program -> StartupReceipt -> Either String ValidatedStartupReceipt
validateStartupReceipt scenario program receipt = ValidatedStartupReceipt receipt <$ checkStartupReceipt scenario program receipt

startupArgument :: ValidatedStartupReceipt -> Either String Argument
startupArgument (ValidatedStartupReceipt receipt) = case filter ((== StartupHandoff) . stopLabel) (receiptStops receipt) of
  [stop] -> Right (argument (stopState stop))
  _ -> Left "receipt lacks exactly one startup-handoff stop"

-- | Zeroes the log timer words at physical 0xa0040 + 8k, k = 0..15.
maskTimers :: B.ByteString -> B.ByteString
maskTimers bytes = B.pack [if timer index then 0 else byte | (index, byte) <- zip [0 :: Int ..] (B.unpack bytes)]
  where
    timer index = index >= 0x40 && index < 0xc0 && (index - 0x40) `mod` 8 < 4

-- | NZCV cleared; private-data/analysis/x-m1-1.50/issue135/loaderb-first-overwrite.txt.
comparedCpsr :: Cpsr -> Word32
comparedCpsr (Cpsr value) = value .&. 0x0fffffff

compareStartupReceipts :: ValidatedStartupReceipt -> ValidatedStartupReceipt -> Either String ()
compareStartupReceipts (ValidatedStartupReceipt retained) (ValidatedStartupReceipt owned) = do
  require (receiptScenario retained == receiptScenario owned) "compared receipts differ in scenario"
  require (receiptProgram retained == RetainedKind && receiptProgram owned == OwnedKind) "compared receipts are not retained then owned"
  labels <- case endpointLabels (receiptScenario retained) of
    [] -> Left "held receipts have no endpoint stops to compare"
    found -> Right found
  forM_ labels $ \item -> do
    let pick receipt = case filter ((== item) . stopLabel) (receiptStops receipt) of
          [stop] -> Right stop
          _ -> Left ("compared receipt lacks exactly one " ++ labelText item)
        context = " at " ++ labelText item
    left <- pick retained
    right <- pick owned
    let leftRegs = stopRegisters left
        rightRegs = stopRegisters right
    require (comparedCpsr (stopCpsr left) == comparedCpsr (stopCpsr right)) ("CPSR differs outside NZCV" ++ context)
    require (map ($ leftRegs) [r0, r3, r6, r7, r8, r9, r10, r11] == map ($ rightRegs) [r0, r3, r6, r7, r8, r9, r10, r11]) ("r0, r3 or r6-r11 differ" ++ context)
    require (stopSp left == stopSp right) ("SP differs" ++ context)
    require (stopState left == stopState right) ("peer state differs" ++ context)
    case (stopProgramState left, stopProgramState right) of
      (Just (ProgramState leftGlobals leftCompanion), Just (ProgramState rightGlobals rightCompanion)) -> do
        require (leftGlobals == rightGlobals) ("program globals differ" ++ context)
        require (maskTimers leftCompanion == maskTimers rightCompanion) ("companion differs outside the log timer words" ++ context)
      _ -> Left ("program state missing" ++ context)
