{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}

module Main where

import Control.Exception (IOException, try)
import Control.Monad (forM, unless, when)
import Data.Aeson (Value (Null), encode, object, toJSON, (.=))
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.Either (isLeft)
import Data.List (isPrefixOf)
import qualified Data.Map.Strict as M
import Data.Word (Word8, Word32)
import Emulation.NativeCase (Region, loadCase)
import qualified Emulation.NativeObservation as N
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host hiding (sha256)
import Numeric (showHex)
import System.Directory (canonicalizePath, createDirectoryIfMissing)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))

stateBase, stackBase, planBase, checkStatusBase, admittedStepBase, clearCountBase, clearTraceBase, advanceCountBase, advanceTraceBase, waitFlagBase, waitStateBase, waitClearCountBase, waitClearTraceBase, waitEventBase, waitAdvanceCountBase, waitAdvanceTraceBase, finalFlagBase, finalStateBase, finalClearCountBase, finalClearTraceBase, finalAdvanceCountBase, finalAdvanceTraceBase, waitSignalBase, releaseBase, candidateBase, depsHighBase, depsLowBase, exitAddress, stackTop, contextBase :: Word32
stateBase = 0x009a0000
stackBase = 0x009a2000
planBase = 0x009a1000
checkStatusBase = 0x009a1104
admittedStepBase = 0x009a1180
clearCountBase = 0x009a1200
clearTraceBase = 0x009a1204
advanceCountBase = 0x009a1300
advanceTraceBase = 0x009a1304
waitFlagBase = 0x009a1344
waitStateBase = 0x009a1348
waitClearCountBase = 0x009a134c
waitClearTraceBase = 0x009a1350
waitEventBase = 0x009a1370
waitAdvanceCountBase = 0x009a13a0
waitAdvanceTraceBase = 0x009a13a4
finalFlagBase = 0x009a1374
finalStateBase = 0x009a1378
finalClearCountBase = 0x009a137c
finalClearTraceBase = 0x009a1380
finalAdvanceCountBase = 0x009a13c8
finalAdvanceTraceBase = 0x009a13cc
waitSignalBase = 0x009a1400
releaseBase = 0x009a1600
candidateBase = 0x009c0000
depsHighBase = 0x00e91810
depsLowBase = 0x007243a8
exitAddress = 0x009c7ff0
stackTop = 0x009a2ff0
contextBase = 0x009a0000

stateSize, stackSize, candidateSize, depsHighSize, depsLowSize, stockWaitOffset, stockWaitElfOffset, stockWaitSize :: Int
stateSize = 0x2000
stackSize = 0x1000
candidateSize = 0x8000
depsHighSize = 0x20000
depsLowSize = 0x4000
stockWaitOffset = fromIntegral (0x00e9e038 - depsHighBase)
stockWaitElfOffset = 0x2ac720
stockWaitSize = 0x11c

hex :: Word32 -> String
hex value = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex value ""

u32le :: Word32 -> B.ByteString
u32le value = B.pack [fromIntegral value, fromIntegral (value `shiftR` 8), fromIntegral (value `shiftR` 16), fromIntegral (value `shiftR` 24)]

putU32 :: Int -> Word32 -> B.ByteString -> B.ByteString
putU32 offset value bytes = B.take offset bytes <> u32le value <> B.drop (offset + 4) bytes

putWords :: Int -> [Word32] -> B.ByteString -> B.ByteString
putWords offset values bytes =
  foldl (\current (index, value) -> putU32 (offset + index * 4) value current) bytes (zip [0 :: Int ..] values)

readU32 :: B.ByteString -> Int -> Either String Word32
readU32 bytes offset
  | offset < 0 || offset + 4 > B.length bytes = Left ("read outside state at " ++ show offset)
  | otherwise =
      Right
        ( fromIntegral (B.index bytes offset)
            .|. (fromIntegral (B.index bytes (offset + 1)) `shiftL` 8)
            .|. (fromIntegral (B.index bytes (offset + 2)) `shiftL` 16)
            .|. (fromIntegral (B.index bytes (offset + 3)) `shiftL` 24)
        )

stateOffset :: Word32 -> Int
stateOffset address = fromIntegral (address - stateBase)

newtype EventId = EventId Word32 deriving (Eq, Show)
newtype StepState = StepState Word32 deriving (Eq, Show)
newtype RequestedEvents = RequestedEvents Word32 deriving (Eq, Show)
newtype StockStatus = StockStatus Word32 deriving (Eq, Show)

data CancelSelector = AllEvents | CancelBit Word8 deriving (Eq, Show)

eventWord :: EventId -> Word32
eventWord (EventId value) = value

stateWord :: StepState -> Word32
stateWord (StepState value) = value

requestedWord :: RequestedEvents -> Word32
requestedWord (RequestedEvents value) = value

stockStatusWord :: StockStatus -> Word32
stockStatusWord (StockStatus value) = value

cancelWord :: CancelSelector -> Word32
cancelWord AllEvents = 0xc
cancelWord (CancelBit bit) = fromIntegral bit

data Plan = Plan
  { planEvent :: EventId
  , planEntryClear :: EventId
  , planCancelClear :: EventId
  , planNormalState :: StepState
  , planCancelState :: StepState
  , planWaitEvent :: EventId
  , planCancelSelector :: CancelSelector
  }

data Scenario = Scenario
  { scenarioId :: String
  , scenarioPlan :: Plan
  , scenarioSignals :: RequestedEvents
  , scenarioChecks :: [StockStatus]
  , scenarioWaitSignal :: Maybe RequestedEvents
  }

data Boundary = Boundary
  { boundaryState :: StepState
  , boundaryCleared :: [EventId]
  , boundaryAdvanced :: [StepState]
  , boundaryWait :: Maybe EventId
  }
  deriving (Eq, Show)

data Outcome = Outcome
  { outcomeStatus :: StockStatus
  , outcomeWait :: Maybe Boundary
  , outcomeFinal :: Maybe Boundary
  , outcomeRelease :: Word32
  }

planWords :: Plan -> [Word32]
planWords plan =
  [ eventWord (planEvent plan)
  , eventWord (planEntryClear plan)
  , eventWord (planCancelClear plan)
  , stateWord (planNormalState plan)
  , stateWord (planCancelState plan)
  , 1
  , 0
  , 0xd
  , eventWord (planWaitEvent plan)
  , cancelWord (planCancelSelector plan)
  , 0
  , 0
  , 0
  ]

cancelRequested :: Plan -> RequestedEvents -> Bool
cancelRequested plan signals =
  case planCancelSelector plan of
    AllEvents -> signals /= RequestedEvents 0
    CancelBit bit -> requestedWord signals .&. ((1 :: Word32) `shiftL` fromIntegral bit) /= 0

nextCheck :: [StockStatus] -> (StockStatus, [StockStatus])
nextCheck [] = (StockStatus 0, [])
nextCheck (value : rest) = (value, rest)

model :: Scenario -> Outcome
model scenario
  | entryStatus /= StockStatus 0 = Outcome entryStatus Nothing Nothing 0
  | cancelRequested plan (scenarioSignals scenario) =
      let (cancelStatus, _) = nextCheck afterEntry
          final = Boundary cancelState [entryClear, cancelClear] [cancelState] Nothing
       in Outcome cancelStatus Nothing (Just final) 1
  | advanceStatus /= StockStatus 0 =
      Outcome advanceStatus Nothing (Just (Boundary normalState [entryClear] [normalState] Nothing)) 1
  | otherwise =
      let (finalStatus, _) = nextCheck afterAdvance
          waitBoundary = Boundary normalState [entryClear] [normalState] (Just waitEvent)
          final
            | cancelRequested plan afterWait = Boundary cancelState [entryClear, cancelClear] [normalState, cancelState] Nothing
            | otherwise = Boundary normalState [entryClear] [normalState] Nothing
       in Outcome finalStatus (Just waitBoundary) (Just final) 1
  where
    plan = scenarioPlan scenario
    entryClear = planEntryClear plan
    cancelClear = planCancelClear plan
    normalState = planNormalState plan
    cancelState = planCancelState plan
    waitEvent = planWaitEvent plan
    (entryStatus, afterEntry) = nextCheck (scenarioChecks scenario)
    (advanceStatus, afterAdvance) = nextCheck afterEntry
    afterWait = maybe (scenarioSignals scenario) id (scenarioWaitSignal scenario)

initialState :: Scenario -> B.ByteString
initialState scenario =
  putWords (stateOffset planBase) (planWords (scenarioPlan scenario))
    (putWords (stateOffset checkStatusBase) (map stockStatusWord (scenarioChecks scenario))
      (putU32 (stateOffset admittedStepBase) 2
        (putU32 (stateOffset waitSignalBase) (maybe 0 requestedWord (scenarioWaitSignal scenario))
          (putU32 (stateOffset (contextBase + 0x4c)) (requestedWord (scenarioSignals scenario)) (B.replicate stateSize 0)))))

scenarios :: [Scenario]
scenarios =
  [ Scenario "normal" base (RequestedEvents 0) [StockStatus 0, StockStatus 0, StockStatus 0] Nothing
  , Scenario "cancel-before" base (RequestedEvents 1) [StockStatus 0, StockStatus 0] Nothing
  , Scenario "cancel-before-final-error" base (RequestedEvents 1) [StockStatus 0, StockStatus 7] Nothing
  , Scenario "cancel-wait" base (RequestedEvents 0) [StockStatus 0, StockStatus 0, StockStatus 0] (Just (RequestedEvents 1))
  , Scenario "entry-error" base (RequestedEvents 0) [StockStatus 5] Nothing
  , Scenario "advance-error" base (RequestedEvents 0) [StockStatus 0, StockStatus 6, StockStatus 0] Nothing
  , Scenario "final-error" base (RequestedEvents 0) [StockStatus 0, StockStatus 0, StockStatus 5] Nothing
  , Scenario "bit3-cancel" base {planCancelSelector = CancelBit 3} (RequestedEvents 0x8) [StockStatus 0, StockStatus 0] Nothing
  , Scenario "bit3-other" base {planCancelSelector = CancelBit 3} (RequestedEvents 0x4) [StockStatus 0, StockStatus 0, StockStatus 0] Nothing
  , Scenario "all-bit-other" base (RequestedEvents 0x4) [StockStatus 0, StockStatus 0] Nothing
  , Scenario "wait-mask-20" base {planWaitEvent = EventId 0x20} (RequestedEvents 0) [StockStatus 0, StockStatus 0, StockStatus 0] Nothing
  , Scenario "cancel-bit-zero" base {planCancelSelector = CancelBit 0} (RequestedEvents 1) [StockStatus 0, StockStatus 0, StockStatus 0] Nothing
  ]
  where
    base = Plan (EventId 2) (EventId 4) (EventId 5) (StepState 4) (StepState 5) (EventId 0xffffffff) AllEvents

invalidCancelFixtures :: [(String, Word32)]
invalidCancelFixtures =
  [ ("invalid-cancel-bit-32", 32)
  , ("invalid-cancel-bit-64", 64)
  ]

invalidPointers :: [(String, Word32)]
invalidPointers =
  [ ("invalid-plan-null", 0)
  , ("invalid-plan-misaligned", planBase + 2)
  , ("invalid-context-null", 0)
  , ("invalid-context-misaligned", contextBase + 2)
  ]

validatePointer :: Word32 -> Either String ()
validatePointer address
  | address == 0 = Left "pointer is null"
  | address .&. 3 /= 0 = Left "pointer is not word aligned"
  | otherwise = Right ()

validateCancelWord :: Word32 -> Either String ()
validateCancelWord word
  | word == 0xc || word <= 31 = Right ()
  | otherwise = Left "descriptor cancel bit is outside 0xc or 0..=31"

validateScenario :: Scenario -> Either String ()
validateScenario scenario
  | length (scenarioChecks scenario) > 8 = Left "check script exceeds the case status array"
  | otherwise = validatePointer contextBase >> validatePointer planBase

data ResultStatus = ResultPassed | ResultFailed | ResultInvalidCase | ResultInvalidOutput | ResultHostTimeout deriving (Eq, Show)

resultStatusLabel :: ResultStatus -> String
resultStatusLabel ResultPassed = "passed"
resultStatusLabel ResultFailed = "failed"
resultStatusLabel ResultInvalidCase = "invalid_case"
resultStatusLabel ResultInvalidOutput = "invalid_output"
resultStatusLabel ResultHostTimeout = "host_timeout"

data ControlStatus = Refused | Accepted deriving (Eq, Show)

data Control = Control
  { controlName :: String
  , controlStatus :: ControlStatus
  , controlProblems :: [String]
  }

controlJson :: Control -> Value
controlJson control =
  object
    [ "name" .= controlName control
    , "status" .= ((if controlStatus control == Refused then "refused" else "accepted") :: String)
    , "problems" .= controlProblems control
    ]

data Artifacts = Artifacts
  { artifactState :: B.ByteString
  , artifactHigh :: B.ByteString
  , artifactHighForbidden :: B.ByteString
  , artifactLow :: B.ByteString
  , artifactLowFaulty :: B.ByteString
  , artifactCandidate :: B.ByteString
  , artifactCandidateLength :: Int
  }

loadArtifacts :: FilePath -> FilePath -> IO Artifacts
loadArtifacts build stockElf = do
  candidate <- B.readFile (build </> "replacement/capture/capture-candidate.bin")
  high <- B.readFile (build </> "replacement/capture/capture-deps-high.bin")
  low <- B.readFile (build </> "replacement/capture/capture-deps-low.bin")
  faulty <- B.readFile (build </> "replacement/capture/capture-deps-low-faulty.bin")
  elf <- B.readFile stockElf
  when (B.length elf < stockWaitElfOffset + stockWaitSize) $
    ioError (userError "stock ELF is too short for the wait entry")
  when (B.length high > depsHighSize || B.length low > depsLowSize || B.length faulty > depsLowSize || B.length candidate > candidateSize) $
    ioError (userError "artifact exceeds its declared region")
  let stock = B.take stockWaitSize (B.drop stockWaitElfOffset elf)
  case readU32 stock 0 of
    Right 0xe92d4870 -> pure ()
    other -> ioError (userError ("stock wait entry differs: " ++ either id hex other))
  let highStock = overlay stockWaitOffset stock (ensureLength (stockWaitOffset + B.length stock) high)
      guard = B.concat (replicate ((stockWaitSize + 3) `div` 4) (u32le 0xeafffffe))
      highGuard = overlay stockWaitOffset guard (ensureLength (stockWaitOffset + B.length guard) high)
  when (B.length highStock > depsHighSize || B.length highGuard > depsHighSize) $
    ioError (userError "high dependency artifact exceeds its declared region")
  case readU32 highGuard stockWaitOffset of
    Right 0xeafffffe -> pure ()
    other -> ioError (userError ("guard opcode differs: " ++ either id hex other))
  pure
    Artifacts
      { artifactState = B.empty
      , artifactHigh = pad depsHighSize highStock
      , artifactHighForbidden = pad depsHighSize highGuard
      , artifactLow = pad depsLowSize low
      , artifactLowFaulty = pad depsLowSize faulty
      , artifactCandidate = pad candidateSize candidate
      , artifactCandidateLength = B.length candidate
      }
  where
    overlay offset bytes base = B.take offset base <> bytes <> B.drop (offset + B.length bytes) base
    ensureLength size bytes = if B.length bytes >= size then bytes else bytes <> B.replicate (size - B.length bytes) 0
    pad size bytes = bytes <> B.replicate (size - B.length bytes) 0

syntheticSpans :: [(Word32, Int)]
syntheticSpans =
  [ (0x00e91810, 0x180)
  , (0x00e93664, 0x180)
  , (0x00e9390c, 0x180)
  , (0x00e93bdc, 0x180)
  , (0x00e9db80, 0x2b8)
  , (0x00eb0ea0, 0x100)
  , (0x007243a8, 0x280)
  , (0x00724638, 0x100)
  ]

regionJson :: String -> Word32 -> Int -> String -> B.ByteString -> Value
regionJson ident base size permissions initial =
  object
    [ "id" .= ident
    , "address" .= hex base
    , "size" .= size
    , "permissions" .= permissions
    , "initial" .=
        if B.null initial
          then object ["zero" .= True]
          else object ["artifact" .= ident, "offset" .= (0 :: Int), "length" .= B.length initial]
    ]

artifactJson :: String -> B.ByteString -> Value
artifactJson ident bytes =
  object
    [ "id" .= ident
    , "path" .= (ident ++ ".bin")
    , "size" .= B.length bytes
    , "sha256" .= sha256Hex (sha256 bytes)
    ]

data Side = Original | Candidate | ForbiddenOriginal deriving (Eq, Show)

sideLabel :: Side -> String
sideLabel Original = "original"
sideLabel Candidate = "candidate"
sideLabel ForbiddenOriginal = "forbidden-original"

sideHigh :: Artifacts -> Side -> B.ByteString
sideHigh artifacts Original = artifactHigh artifacts
sideHigh artifacts Candidate = artifactHighForbidden artifacts
sideHigh artifacts ForbiddenOriginal = artifactHighForbidden artifacts

caseJson :: Scenario -> Side -> Artifacts -> Value
caseJson scenario side artifacts =
  let state = artifactState artifacts
      high = sideHigh artifacts side
      entry = if side == Candidate then candidateBase else 0x00e9e038
      candidateSpan = ((artifactCandidateLength artifacts + 3) `div` 4) * 4
      spans = syntheticSpans ++ [(entry, if side == Candidate then candidateSpan else 0x11c)]
      origin = (if side == Original then "stock" else "synthetic") :: String
   in object
        [ "schema_version" .= (1 :: Int)
        , "id" .= (scenarioId scenario ++ "-" ++ sideLabel side)
        , "origin" .= origin
        , "provenance" .= ("X-M1 1.50 image 5 FUN_00e9e038 develop step wait; recording dependencies at the stock call addresses" :: String)
        , "cpu" .= object ["architecture" .= ("arm" :: String), "model" .= ("cortex-r5f" :: String), "mode" .= ("arm" :: String), "endian" .= ("little" :: String)]
        , "artifacts" .=
            [ artifactJson "state" state
            , artifactJson "deps-high" high
            , artifactJson "deps-low" (artifactLow artifacts)
            , artifactJson "candidate" (artifactCandidate artifacts)
            ]
        , "entry" .= hex entry
        , "exit" .= hex exitAddress
        , "registers" .= initialRegisters contextBase 2
        , "regions" .=
            [ regionJson "state" stateBase stateSize "rw" state
            , regionJson "stack" stackBase stackSize "rw" B.empty
            , regionJson "deps-high" depsHighBase depsHighSize "rx" high
            , regionJson "deps-low" depsLowBase depsLowSize "rx" (artifactLow artifacts)
            , regionJson "candidate" candidateBase candidateSize "rx" (artifactCandidate artifacts)
            ]
        , "functions" .= [object ["address" .= hex address, "size" .= size] | (address, size) <- spans]
        , "stubs" .= ([] :: [Value])
        , "limits" .= object ["instructions" .= (4000000 :: Int), "wall_ms" .= (4000 :: Int)]
        , "expected" .=
            object
              [ "status" .= ("returned" :: String)
              , "registers" .=
                  object
                    [ "r0" .= hex (stockStatusWord (outcomeStatus (model scenario)))
                    , "sp" .= hex stackTop
                    , "pc" .= hex exitAddress
                    ]
              , "memory" .= ([] :: [Value])
              , "events" .= ([] :: [Value])
              ]
        , "exclusions" .=
            ( [ "Wait and release boundaries compare the current state, clear sequence and advance sequence; diagnostic log and check counts are not asserted."
              , "The stack region is execution scratch and is not compared."
              ] :: [String] )
        ]

initialRegisters :: Word32 -> Word32 -> Value
initialRegisters first second =
  object
    [ "r0" .= hex first
    , "r1" .= hex second
    , "r2" .= hex 0
    , "r3" .= hex 0
    , "r4" .= hex 0x4a4a4a4a
    , "r5" .= hex 0x5b5b5b5b
    , "r6" .= hex 0x6c6c6c6c
    , "r7" .= hex 0x7d7d7d7d
    , "r8" .= hex 0x8e8e8e8e
    , "r9" .= hex 0x9f9f9f9f
    , "r10" .= hex 0xa0a0a0a0
    , "r11" .= hex 0xb1b1b1b1
    , "r12" .= hex 0
    , "sp" .= hex stackTop
    , "lr" .= hex exitAddress
    , "cpsr" .= hex 0x10
    ]

lookupGuardJson :: String -> Word32 -> Word32 -> Artifacts -> Value
lookupGuardJson ident first second artifacts =
  let state = artifactState artifacts
      high = artifactHigh artifacts
   in object
        [ "schema_version" .= (1 :: Int)
        , "id" .= ident
        , "origin" .= ("synthetic" :: String)
        , "provenance" .= ("X-M1 1.50 FUN_00e91810 group and step contract" :: String)
        , "cpu" .= object ["architecture" .= ("arm" :: String), "model" .= ("cortex-r5f" :: String), "mode" .= ("arm" :: String), "endian" .= ("little" :: String)]
        , "artifacts" .=
            [ artifactJson "state" state
            , artifactJson "deps-high" high
            , artifactJson "deps-low" (artifactLow artifacts)
            , artifactJson "candidate" (artifactCandidate artifacts)
            ]
        , "entry" .= hex 0x00e91810
        , "exit" .= hex exitAddress
        , "registers" .= initialRegisters first second
        , "regions" .=
            [ regionJson "state" stateBase stateSize "rw" state
            , regionJson "stack" stackBase stackSize "rw" B.empty
            , regionJson "deps-high" depsHighBase depsHighSize "rx" high
            , regionJson "deps-low" depsLowBase depsLowSize "rx" (artifactLow artifacts)
            , regionJson "candidate" candidateBase candidateSize "rx" (artifactCandidate artifacts)
            ]
        , "functions" .= [object ["address" .= hex address, "size" .= size] | (address, size) <- syntheticSpans]
        , "stubs" .= ([] :: [Value])
        , "limits" .= object ["instructions" .= (4000000 :: Int), "wall_ms" .= (4000 :: Int)]
        , "expected" .=
            object
              [ "status" .= ("returned" :: String)
              , "registers" .= object ["r0" .= hex 0, "sp" .= hex stackTop, "pc" .= hex exitAddress]
              , "memory" .= ([] :: [Value])
              , "events" .= ([] :: [Value])
              ]
        , "exclusions" .=
            ( [ "The lookup guard must fault when the group is not 0x80." ] :: [String] )
        ]

contains :: N.GuestAddress -> Int -> N.GuestAddress -> Int -> Bool
contains (N.GuestAddress base) extent (N.GuestAddress address) size =
  address >= base && address + fromIntegral size <= base + fromIntegral extent

readValues :: B.ByteString -> Word32 -> Word32 -> Either String [Word32]
readValues bytes countBase traceBase = do
  count <- readU32 bytes (stateOffset countBase)
  if count > 8
    then Left ("trace count exceeds the case capacity: " ++ show count)
    else mapM (\index -> readU32 bytes (stateOffset traceBase + index * 4)) [0 .. fromIntegral count - 1]

readWaitBoundary :: B.ByteString -> Either String (Maybe Boundary)
readWaitBoundary bytes = do
  flag <- readU32 bytes (stateOffset waitFlagBase)
  case flag of
    0 -> Right Nothing
    1 -> do
      state <- StepState <$> readU32 bytes (stateOffset waitStateBase)
      cleared <- map EventId <$> readValues bytes waitClearCountBase waitClearTraceBase
      advanced <- map StepState <$> readValues bytes waitAdvanceCountBase waitAdvanceTraceBase
      event <- EventId <$> readU32 bytes (stateOffset waitEventBase)
      Right (Just (Boundary state cleared advanced (Just event)))
    other -> Left ("invalid wait boundary flag: " ++ show other)

readFinalBoundary :: B.ByteString -> Either String (Maybe Boundary)
readFinalBoundary bytes = do
  flag <- readU32 bytes (stateOffset finalFlagBase)
  case flag of
    0 -> Right Nothing
    1 -> do
      state <- StepState <$> readU32 bytes (stateOffset finalStateBase)
      cleared <- map EventId <$> readValues bytes finalClearCountBase finalClearTraceBase
      advanced <- map StepState <$> readValues bytes finalAdvanceCountBase finalAdvanceTraceBase
      Right (Just (Boundary state cleared advanced Nothing))
    other -> Left ("invalid final boundary flag: " ++ show other)

compareBoundary :: String -> Maybe Boundary -> Maybe Boundary -> [String]
compareBoundary label expected actual =
  case (expected, actual) of
    (Nothing, Nothing) -> []
    (Just _, Nothing) -> [label ++ " boundary missing"]
    (Nothing, Just observed) -> [label ++ " boundary unexpected: " ++ show observed]
    (Just expectedBoundary, Just observed)
      | expectedBoundary == observed -> []
      | otherwise -> [label ++ " boundary differs: expected " ++ show expectedBoundary ++ ", observed " ++ show observed]

checkCapture :: [Region] -> Scenario -> N.NativeObservation -> [String]
checkCapture regions scenario observation =
  case N.nativeStatus observation of
    N.Returned -> registers ++ boundaries ++ writes
    status -> ["runner status " ++ N.runnerStatusLabel status]
  where
    expected = model scenario
    registersOf = N.nativeRegisters observation
    register which expectedValue =
      case M.lookup which registersOf of
        Nothing -> ["register " ++ N.registerLabel which ++ ": missing"]
        Just actual
          | actual /= expectedValue -> ["register " ++ N.registerLabel which ++ ": expected " ++ N.hexWord32 expectedValue ++ ", observed " ++ N.hexWord32 actual]
          | otherwise -> []
    abiPreserved =
      [ (N.R4, 0x4a4a4a4a)
      , (N.R5, 0x5b5b5b5b)
      , (N.R6, 0x6c6c6c6c)
      , (N.R7, 0x7d7d7d7d)
      , (N.R8, 0x8e8e8e8e)
      , (N.R9, 0x9f9f9f9f)
      , (N.R10, 0xa0a0a0a0)
      , (N.R11, 0xb1b1b1b1)
      ]
    cpsrControl =
      case M.lookup N.Cpsr registersOf of
        Nothing -> ["register cpsr: missing"]
        Just cpsr
          | cpsr .&. 0x3ff /= 0x10 -> ["cpsr control bits expected 0x00000010, observed " ++ N.hexWord32 cpsr]
          | otherwise -> []
    registers =
      register N.R0 (stockStatusWord (outcomeStatus expected))
        ++ register N.Sp stackTop
        ++ register N.Pc exitAddress
        ++ concat [register which value | (which, value) <- abiPreserved]
        ++ cpsrControl
    boundaries =
      case lookup (N.RegionId "state") (N.nativeMemory observation) of
        Nothing -> ["state region missing"]
        Just bytes ->
          let observed = do
                wait <- readWaitBoundary bytes
                final <- readFinalBoundary bytes
                release <- readU32 bytes (stateOffset releaseBase)
                pure (wait, final, release)
           in case observed of
                Left problem -> [problem]
                Right (wait, final, release) ->
                  compareBoundary "wait" (outcomeWait expected) wait
                    ++ compareBoundary "final" (outcomeFinal expected) final
                    ++ [ "release count expected " ++ show (outcomeRelease expected) ++ ", observed " ++ show release
                       | release /= outcomeRelease expected
                       ]
    writes =
      case N.nativeProvenance observation of
        N.Truncated _ -> ["provenance truncated"]
        N.Complete _ entries ->
          concat
            [ case [ident | (ident, base, extent) <- N.regionBounds regions, contains base extent address (fromIntegral size)] of
                [] -> ["write outside every declared region at " ++ show (N.guestAddress address)]
                (ident : _) | ident `elem` map N.RegionId ["state", "stack"] -> []
                (ident : _) -> ["write to " ++ N.regionName ident ++ " at " ++ show (N.guestAddress address)]
            | N.Write address (N.WriteBytes size) <- entries
            ]

data Result = Result
  { resultStatus :: ResultStatus
  , resultProblems :: [String]
  , resultObservation :: Maybe N.NativeObservation
  }

runSide :: FilePath -> FilePath -> Artifacts -> Scenario -> Side -> IO Result
runSide runner root artifactsTemplate scenario side =
  case validateScenario scenario of
    Left reason -> pure (Result ResultInvalidCase [reason] Nothing)
    Right () -> do
      let dir = root </> (scenarioId scenario ++ "-" ++ sideLabel side)
          artifacts = artifactsTemplate {artifactState = initialState scenario}
          value = caseJson scenario side artifacts
      createDirectoryIfMissing True dir
      B.writeFile (dir </> "state.bin") (artifactState artifacts)
      B.writeFile (dir </> "deps-high.bin") (sideHigh artifacts side)
      B.writeFile (dir </> "deps-low.bin") (artifactLow artifacts)
      B.writeFile (dir </> "candidate.bin") (artifactCandidate artifacts)
      BL.writeFile (dir </> "case.json") (encode value)
      loaded <- try (loadCase (dir </> "case.json"))
      case loaded of
        Left (message :: IOException) -> pure (Result ResultInvalidCase [show message] Nothing)
        Right (_, regions, wire, _) -> do
          executable <- canonicalizePath runner
          (timed, exit, out, err) <- runBounded 8000000 (8 * 1024 * 1024) executable [] wire
          if timed
            then pure (Result ResultHostTimeout [BC.unpack err] Nothing)
            else do
              parsed <- try (jsonRead out)
              case parsed of
                Left (message :: IOException) -> pure (Result ResultInvalidOutput [show message, BC.unpack err] Nothing)
                Right runnerValue ->
                  case N.observeNative exit regions runnerValue of
                    Left problem -> pure (Result ResultInvalidOutput [N.renderObservationError problem, BC.unpack err] Nothing)
                    Right observation ->
                      let problems = checkCapture regions scenario observation
                       in pure (Result (if null problems then ResultPassed else ResultFailed) problems (Just observation))

runLookupGuard :: FilePath -> FilePath -> Artifacts -> Scenario -> String -> Word32 -> Word32 -> IO Result
runLookupGuard runner root artifactsTemplate scenario ident first second = do
  let dir = root </> ident
      artifacts = artifactsTemplate {artifactState = initialState scenario}
      value = lookupGuardJson ident first second artifacts
  createDirectoryIfMissing True dir
  B.writeFile (dir </> "state.bin") (artifactState artifacts)
  B.writeFile (dir </> "deps-high.bin") (artifactHigh artifacts)
  B.writeFile (dir </> "deps-low.bin") (artifactLow artifacts)
  B.writeFile (dir </> "candidate.bin") (artifactCandidate artifacts)
  BL.writeFile (dir </> "case.json") (encode value)
  loaded <- try (loadCase (dir </> "case.json"))
  case loaded of
    Left (message :: IOException) -> pure (Result ResultInvalidCase [show message] Nothing)
    Right (_, regions, wire, _) -> do
      executable <- canonicalizePath runner
      (timed, exit, out, err) <- runBounded 8000000 (8 * 1024 * 1024) executable [] wire
      if timed
        then pure (Result ResultHostTimeout [BC.unpack err] Nothing)
        else do
          parsed <- try (jsonRead out)
          case parsed of
            Left (message :: IOException) -> pure (Result ResultInvalidOutput [show message, BC.unpack err] Nothing)
            Right runnerValue ->
              case N.observeNative exit regions runnerValue of
                Left problem -> pure (Result ResultInvalidOutput [N.renderObservationError problem, BC.unpack err] Nothing)
                Right observation ->
                  pure
                    ( Result
                        (if N.nativeStatus observation == N.Returned then ResultPassed else ResultFailed)
                        []
                        (Just observation)
                    )

witnessControl :: String -> (Result -> Bool) -> Result -> Control
witnessControl name witness result =
  Control name (if witness result then Refused else Accepted) (resultProblems result)

producerWitness :: Result -> Bool
producerWitness result =
  resultStatus result == ResultFailed
    && maybe False ((== N.Returned) . N.nativeStatus) (resultObservation result)
    && any ("release count expected" `isPrefixOf`) (resultProblems result)

guardWitness :: Result -> Bool
guardWitness result =
  resultStatus result == ResultFailed
    && maybe
      False
      (\observation ->
        N.nativeStatus observation == N.InstructionBudget
          && M.lookup N.Pc (N.nativeRegisters observation) == Just 0x00e9e038)
      (resultObservation result)

lookupWitness :: Result -> Bool
lookupWitness result =
  resultStatus result == ResultFailed
    && maybe
      False
      (\observation ->
        N.nativeStatus observation == N.InstructionBudget
          && maybe False (\pc -> pc >= 0x00e91810 && pc < 0x00e91910) (M.lookup N.Pc (N.nativeRegisters observation)))
      (resultObservation result)

invalidValue :: String -> Either String () -> Control
invalidValue name result =
  Control name (if isLeft result then Refused else Accepted) [either id (const "") result]

boundaryJson :: Maybe Boundary -> Value
boundaryJson Nothing = Null
boundaryJson (Just boundary) =
  object
    [ "state" .= stateWord (boundaryState boundary)
    , "cleared" .= map eventWord (boundaryCleared boundary)
    , "advanced" .= map stateWord (boundaryAdvanced boundary)
    , "wait" .= maybe Null (toJSON . eventWord) (boundaryWait boundary)
    ]

qualifiedResults :: [(Scenario, Result, Result)] -> Control -> Control -> [Control] -> [Control] -> Bool
qualifiedResults cases producer guard lookupControls invalid =
  all
    (\(_, original, candidate) ->
      resultStatus original == ResultPassed
        && resultStatus candidate == ResultPassed)
    cases
    && controlStatus producer == Refused
    && controlStatus guard == Refused
    && all ((== Refused) . controlStatus) lookupControls
    && all ((== Refused) . controlStatus) invalid

report :: [(Scenario, Result, Result)] -> Control -> Control -> [Control] -> [Control] -> Value
report results producerControl guardControl lookupControls invalidControls =
  object
    [ "schema_version" .= (1 :: Int)
    , "profile" .= ("capture-develop-wait" :: String)
    , "cases" .= [caseReport scenario original candidate | (scenario, original, candidate) <- results]
    , "producer_controls" .= [controlJson producerControl]
    , "guard_controls" .= [controlJson guardControl]
    , "lookup_controls" .= map controlJson lookupControls
    , "invalid_inputs" .= map controlJson invalidControls
    , "status" .= ((if qualifiedResults results producerControl guardControl lookupControls invalidControls then "qualified" else "failed") :: String)
    , "capability" .= qualifiedResults results producerControl guardControl lookupControls invalidControls
    ]
  where
    caseReport scenario original candidate =
      object
        [ "id" .= scenarioId scenario
        , "model" .=
            object
              [ "status" .= stockStatusWord (outcomeStatus (model scenario))
              , "wait_boundary" .= boundaryJson (outcomeWait (model scenario))
              , "final_boundary" .= boundaryJson (outcomeFinal (model scenario))
              , "release" .= outcomeRelease (model scenario)
              ]
        , "original" .= resultJson original
        , "candidate" .= resultJson candidate
        , "status" .= ((if resultStatus original == ResultPassed && resultStatus candidate == ResultPassed then "passed" else "failed") :: String)
        ]
    resultJson result =
      object ["status" .= resultStatusLabel (resultStatus result), "problems" .= resultProblems result]

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [runner, build, out, stockElf] -> run runner build out stockElf
    _ -> putStrLn "usage: capture-qualification RUNNER BAZEL_BIN OUT_DIR STOCK_ELF" >> exitFailure

run :: FilePath -> FilePath -> FilePath -> FilePath -> IO ()
run runner build out stockElf = do
  artifacts <- loadArtifacts build stockElf
  let candidateLength = artifactCandidateLength artifacts
  when (candidateLength <= 0 || candidateLength >= fromIntegral (exitAddress - candidateBase)) $
    error "candidate text reaches the case exit"
  createDirectoryIfMissing True out
  results <- forM scenarios $ \scenario -> do
    original <- runSide runner out artifacts scenario Original
    candidate <- runSide runner out artifacts scenario Candidate
    pure (scenario, original, candidate)
  let normalScenario = case [scenario | scenario <- scenarios, scenarioId scenario == "normal"] of
        (scenario : _) -> scenario
        [] -> error "normal capture scenario missing"
      producerRoot = out </> "producer"
      guardRoot = out </> "guard"
      lookupRoot = out </> "lookup"
  createDirectoryIfMissing True producerRoot
  createDirectoryIfMissing True guardRoot
  createDirectoryIfMissing True lookupRoot
  producer <- runSide runner producerRoot (artifacts {artifactLow = artifactLowFaulty artifacts}) normalScenario Candidate
  guard <- runSide runner guardRoot artifacts normalScenario ForbiddenOriginal
  lookupGroup <- runLookupGuard runner lookupRoot artifacts normalScenario "lookup-wrong-group" 0 2
  lookupStep <- runLookupGuard runner lookupRoot artifacts normalScenario "lookup-wrong-step" 0x80 0
  let invalidControls =
        [ invalidValue name (validateCancelWord word) | (name, word) <- invalidCancelFixtures ]
          ++ [ invalidValue name (validatePointer address) | (name, address) <- invalidPointers ]
      producerControl = witnessControl "double-release" producerWitness producer
      guardControl = witnessControl "forbidden-stock-entry" guardWitness guard
      lookupControls =
        [ witnessControl "lookup-wrong-group" lookupWitness lookupGroup
        , witnessControl "lookup-wrong-step" lookupWitness lookupStep
        ]
      output = report results producerControl guardControl lookupControls invalidControls
      qualified = qualifiedResults results producerControl guardControl lookupControls invalidControls
  BL.writeFile (out </> "results.json") (encode output)
  let passed = length [() | (_, original, candidate) <- results, resultStatus original == ResultPassed, resultStatus candidate == ResultPassed]
  putStrLn ("capture develop wait: " ++ show passed ++ "/" ++ show (length results) ++ " differential cases passed")
  unless qualified exitFailure
