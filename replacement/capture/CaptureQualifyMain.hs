{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}

module Main where

import Control.Exception (IOException, try)
import Control.Monad (forM, when)
import Data.Aeson (Value (Null), encode, object, toJSON, (.=))
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.Either (isLeft)
import Data.List (isPrefixOf)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.NativeCase (Region, loadCase)
import qualified Emulation.NativeObservation as N
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host hiding (sha256)
import Numeric (showHex)
import System.Directory (canonicalizePath, createDirectoryIfMissing)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.IO.Error (userError)

stateBase, stackBase, planBase, checkStatusBase, clearCountBase, clearTraceBase, advanceCountBase, advanceTraceBase, waitFlagBase, waitStateBase, waitClearCountBase, waitClearTraceBase, waitEventBase, waitAdvanceCountBase, waitAdvanceTraceBase, finalFlagBase, finalStateBase, finalClearCountBase, finalClearTraceBase, finalAdvanceCountBase, finalAdvanceTraceBase, waitSignalBase, releaseBase, candidateBase, depsHighBase, depsLowBase, exitAddress, stackTop, contextBase :: Word32
stateBase = 0x009a0000
stackBase = 0x009a2000
planBase = 0x009a1000
checkStatusBase = 0x009a1104
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

data Plan = Plan
  { planEvent :: Word32
  , planEntryClear :: Word32
  , planCancelClear :: Word32
  , planNormalState :: Word32
  , planCancelState :: Word32
  , planWaitEvent :: Word32
  , planCancelBit :: Word32
  }

data Scenario = Scenario
  { scenarioId :: String
  , scenarioPlan :: Plan
  , scenarioSignals :: Word32
  , scenarioChecks :: [Word32]
  , scenarioWaitSignal :: Word32
  }

data Boundary = Boundary
  { boundaryState :: Word32
  , boundaryCleared :: [Word32]
  , boundaryAdvanced :: [Word32]
  , boundaryWait :: Maybe Word32
  }
  deriving (Eq, Show)

data Outcome = Outcome
  { outcomeStatus :: Word32
  , outcomeWait :: Maybe Boundary
  , outcomeFinal :: Maybe Boundary
  , outcomeRelease :: Word32
  }

planWords :: Plan -> [Word32]
planWords plan =
  [ planEvent plan
  , planEntryClear plan
  , planCancelClear plan
  , planNormalState plan
  , planCancelState plan
  , 1
  , 0
  , 0xd
  , planWaitEvent plan
  , planCancelBit plan
  , 0
  , 0
  , 0
  ]

cancelRequested :: Plan -> Word32 -> Bool
cancelRequested plan signals
  | planCancelBit plan == 0xc = signals /= 0
  | otherwise = signals .&. ((1 :: Word32) `shiftL` fromIntegral (planCancelBit plan)) /= 0

nextCheck :: [Word32] -> (Word32, [Word32])
nextCheck [] = (0, [])
nextCheck (value : rest) = (value, rest)

model :: Scenario -> Outcome
model scenario
  | entryStatus /= 0 = Outcome entryStatus Nothing Nothing 0
  | cancelRequested plan (scenarioSignals scenario) =
      let (cancelStatus, _) = nextCheck afterEntry
          final = Boundary cancelState [entryClear, cancelClear] [cancelState] Nothing
       in Outcome cancelStatus Nothing (Just final) 1
  | advanceStatus /= 0 =
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
    afterWait = if scenarioWaitSignal scenario /= 0 then scenarioWaitSignal scenario else scenarioSignals scenario

initialState :: Scenario -> B.ByteString
initialState scenario =
  putWords (stateOffset planBase) (planWords (scenarioPlan scenario))
    (putWords (stateOffset checkStatusBase) (scenarioChecks scenario)
      (putU32 (stateOffset waitSignalBase) (scenarioWaitSignal scenario)
        (putU32 (stateOffset (contextBase + 0x4c)) (scenarioSignals scenario) (B.replicate stateSize 0))))

scenarios :: [Scenario]
scenarios =
  [ Scenario "normal" base 0 [0, 0, 0] 0
  , Scenario "cancel-before" base 1 [0, 0] 0
  , Scenario "cancel-before-final-error" base 1 [0, 7] 0
  , Scenario "cancel-wait" base 0 [0, 0, 0] 1
  , Scenario "entry-error" base 0 [5] 0
  , Scenario "advance-error" base 0 [0, 6, 0] 0
  , Scenario "final-error" base 0 [0, 0, 5] 0
  , Scenario "bit3-cancel" base {planCancelBit = 3} 0x8 [0, 0] 0
  , Scenario "bit3-other" base {planCancelBit = 3} 0x4 [0, 0, 0] 0
  , Scenario "all-bit-other" base 0x4 [0, 0] 0
  , Scenario "wait-mask-20" base {planWaitEvent = 0x20} 0 [0, 0, 0] 0
  , Scenario "cancel-bit-zero" base {planCancelBit = 0} 1 [0, 0, 0] 0
  ]
  where
    base = Plan 0 2 3 4 5 0x30 0xc

invalidScenarios :: [Scenario]
invalidScenarios =
  [ Scenario "invalid-cancel-bit-32" base {planCancelBit = 32} 0 [0, 0, 0] 0
  , Scenario "invalid-cancel-bit-64" base {planCancelBit = 64} 0 [0, 0, 0] 0
  ]
  where
    base = Plan 0 2 3 4 5 0x30 0xc

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

validateScenario :: Scenario -> Either String ()
validateScenario scenario
  | cancel /= 0xc && cancel > 31 = Left "descriptor cancel bit is outside 0xc or 0..=31"
  | length (scenarioChecks scenario) > 8 = Left "check script exceeds the case status array"
  | otherwise = validatePointer contextBase >> validatePointer planBase
  where
    cancel = planCancelBit (scenarioPlan scenario)

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
  let stock = B.take stockWaitSize (B.drop stockWaitElfOffset elf)
  case readU32 stock 0 of
    Right 0xe92d4870 -> pure ()
    other -> ioError (userError ("stock wait entry differs: " ++ either id hex other))
  let highStock = overlay stockWaitOffset stock (ensure (stockWaitOffset + B.length stock) high)
      guard = B.concat (replicate ((stockWaitSize + 3) `div` 4) (u32le 0xe7f000f0))
      highGuard = overlay stockWaitOffset guard (ensure (stockWaitOffset + B.length guard) high)
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
    ensure size bytes = if B.length bytes >= size then bytes else bytes <> B.replicate (size - B.length bytes) 0
    pad size bytes = B.take size (bytes <> B.replicate size 0)

syntheticSpans :: [(Word32, Int)]
syntheticSpans =
  [ (0x00e91810, 0x100)
  , (0x00e93664, 0x100)
  , (0x00e9390c, 0x100)
  , (0x00e93bdc, 0x100)
  , (0x00e9db80, 0x2b8)
  , (0x00eb0ea0, 0x100)
  , (0x007243a8, 0x100)
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
      origin = if side == Original then "stock" else "synthetic"
   in object
        [ "schema_version" .= (1 :: Int)
        , "id" .= (scenarioId scenario ++ "-" ++ sideLabel side)
        , "origin" .= origin
        , "provenance" .= ("X-M1 1.50 image 5 FUN_00e9e038 develop step wait; recording dependencies at the stock call addresses")
        , "cpu" .= object ["architecture" .= ("arm" :: String), "model" .= ("cortex-r5f" :: String), "mode" .= ("arm" :: String), "endian" .= ("little" :: String)]
        , "artifacts" .=
            [ artifactJson "state" state
            , artifactJson "deps-high" high
            , artifactJson "deps-low" (artifactLow artifacts)
            , artifactJson "candidate" (artifactCandidate artifacts)
            ]
        , "entry" .= hex entry
        , "exit" .= hex exitAddress
        , "registers" .=
            object
              [ "r0" .= hex contextBase
              , "r1" .= hex 1
              , "r2" .= hex 0
              , "r3" .= hex 0
              , "r4" .= hex 0
              , "r5" .= hex 0
              , "r6" .= hex 0
              , "r7" .= hex 0
              , "r8" .= hex 0
              , "r9" .= hex 0
              , "r10" .= hex 0
              , "r11" .= hex 0
              , "r12" .= hex 0
              , "sp" .= hex stackTop
              , "lr" .= hex exitAddress
              , "cpsr" .= hex 0x10
              ]
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
                    [ "r0" .= hex (outcomeStatus (model scenario))
                    , "sp" .= hex stackTop
                    , "pc" .= hex exitAddress
                    ]
              , "memory" .= ([] :: [Value])
              , "events" .= ([] :: [Value])
              ]
        , "exclusions" .=
            [ "Wait and release boundaries compare the current state, clear sequence and advance sequence; diagnostic log and check counts are not asserted."
            , "The stack region is execution scratch and is not compared."
            ]
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
      state <- readU32 bytes (stateOffset waitStateBase)
      cleared <- readValues bytes waitClearCountBase waitClearTraceBase
      advanced <- readValues bytes waitAdvanceCountBase waitAdvanceTraceBase
      event <- readU32 bytes (stateOffset waitEventBase)
      Right (Just (Boundary state cleared advanced (Just event)))
    other -> Left ("invalid wait boundary flag: " ++ show other)

readFinalBoundary :: B.ByteString -> Either String (Maybe Boundary)
readFinalBoundary bytes = do
  flag <- readU32 bytes (stateOffset finalFlagBase)
  case flag of
    0 -> Right Nothing
    1 -> do
      state <- readU32 bytes (stateOffset finalStateBase)
      cleared <- readValues bytes finalClearCountBase finalClearTraceBase
      advanced <- readValues bytes finalAdvanceCountBase finalAdvanceTraceBase
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
    registers =
      register N.R0 (outcomeStatus expected)
        ++ register N.Sp stackTop
        ++ register N.Pc exitAddress
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
  { resultStatus :: String
  , resultProblems :: [String]
  , resultObservation :: Maybe N.NativeObservation
  }

runSide :: FilePath -> FilePath -> Artifacts -> Scenario -> Side -> IO Result
runSide runner root artifactsTemplate scenario side =
  case validateScenario scenario of
    Left reason -> pure (Result "invalid_case" [reason] Nothing)
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
        Left (message :: IOException) -> pure (Result "invalid_case" [show message] Nothing)
        Right (_, regions, wire, _) -> do
          executable <- canonicalizePath runner
          (timed, exit, out, err) <- runBounded 8000000 (8 * 1024 * 1024) executable [] wire
          if timed
            then pure (Result "host_timeout" [BC.unpack err] Nothing)
            else do
              parsed <- try (jsonRead out)
              case parsed of
                Left (message :: IOException) -> pure (Result "invalid_output" [show message, BC.unpack err] Nothing)
                Right runnerValue ->
                  case N.observeNative exit regions runnerValue of
                    Left problem -> pure (Result "invalid_output" [N.renderObservationError problem, BC.unpack err] Nothing)
                    Right observation ->
                      let problems = checkCapture regions scenario observation
                       in pure (Result (if null problems then "passed" else "failed") problems (Just observation))

mutateState :: (B.ByteString -> B.ByteString) -> N.NativeObservation -> N.NativeObservation
mutateState change observation =
  observation
    { N.nativeMemory =
        [ (ident, if ident == N.RegionId "state" then change bytes else bytes)
        | (ident, bytes) <- N.nativeMemory observation
        ]
    }

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
    , "status" .= (if controlStatus control == Refused then "refused" else "accepted")
    , "problems" .= controlProblems control
    ]

controlChecks :: Scenario -> N.NativeObservation -> [Control]
controlChecks scenario observation =
  [ control "release-twice" (mutateState (putU32 (stateOffset releaseBase) 2) observation)
  , control "missing-release" (mutateState (putU32 (stateOffset releaseBase) 0) observation)
  , control "missing-wait" (mutateState (putU32 (stateOffset waitFlagBase) 0) observation)
  , control "wrong-status" (observation {N.nativeRegisters = M.insert N.R0 (outcomeStatus (model scenario) + 1) (N.nativeRegisters observation)})
  ]
  where
    control name changed =
      let problems = checkCapture [] scenario changed
       in Control name (if null problems then Accepted else Refused) problems

witnessControl :: String -> (Result -> Bool) -> Result -> Control
witnessControl name witness result =
  Control name (if witness result then Refused else Accepted) (resultProblems result)

producerWitness :: Result -> Bool
producerWitness result =
  resultStatus result == "failed"
    && maybe False ((== N.Returned) . N.nativeStatus) (resultObservation result)
    && any ("release count expected" `isPrefixOf`) (resultProblems result)

guardWitness :: Result -> Bool
guardWitness result =
  resultStatus result == "failed"
    && "runner status unsupported_instruction" `elem` resultProblems result

invalidValue :: String -> Either String () -> Control
invalidValue name result =
  Control name (if isLeft result then Refused else Accepted) [either id (const "") result]

boundaryJson :: Maybe Boundary -> Value
boundaryJson Nothing = Null
boundaryJson (Just boundary) =
  object
    [ "state" .= boundaryState boundary
    , "cleared" .= boundaryCleared boundary
    , "advanced" .= boundaryAdvanced boundary
    , "wait" .= maybe Null (toJSON :: Word32 -> Value) (boundaryWait boundary)
    ]

report :: [(Scenario, Result, Result, [Control])] -> Control -> Control -> [Control] -> Value
report results producerControl guardControl invalidControls =
  object
    [ "schema_version" .= (1 :: Int)
    , "profile" .= ("capture-develop-wait" :: String)
    , "cases" .= [caseReport scenario original candidate controls | (scenario, original, candidate, controls) <- results]
    , "producer_controls" .= [controlJson producerControl]
    , "guard_controls" .= [controlJson guardControl]
    , "invalid_inputs" .= map controlJson invalidControls
    , "status" .= (if capability results producerControl guardControl invalidControls then "qualified" else "failed")
    , "capability" .= capability results producerControl guardControl invalidControls
    ]
  where
    caseReport scenario original candidate controls =
      object
        [ "id" .= scenarioId scenario
        , "model" .=
            object
              [ "status" .= outcomeStatus (model scenario)
              , "wait_boundary" .= boundaryJson (outcomeWait (model scenario))
              , "final_boundary" .= boundaryJson (outcomeFinal (model scenario))
              , "release" .= outcomeRelease (model scenario)
              ]
        , "original" .= resultJson original
        , "candidate" .= resultJson candidate
        , "controls" .= map controlJson controls
        , "status" .= (if resultStatus original == "passed" && resultStatus candidate == "passed" then "passed" else "failed")
        ]
    resultJson result =
      object ["status" .= resultStatus result, "problems" .= resultProblems result]
    capability cases producer guard invalid =
      all
        (\(_, original, candidate, controls) ->
          resultStatus original == "passed"
            && resultStatus candidate == "passed"
            && not (null controls)
            && all ((== Refused) . controlStatus) controls)
        cases
        && controlStatus producer == Refused
        && controlStatus guard == Refused
        && all ((== Refused) . controlStatus) invalid

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
    let controls = maybe [] (controlChecks scenario) (resultObservation candidate)
    pure (scenario, original, candidate, controls)
  let normalScenario = head [scenario | scenario <- scenarios, scenarioId scenario == "normal"]
      producerRoot = out </> "producer"
      guardRoot = out </> "guard"
  createDirectoryIfMissing True producerRoot
  createDirectoryIfMissing True guardRoot
  producer <- runSide runner producerRoot (artifacts {artifactLow = artifactLowFaulty artifacts}) normalScenario Candidate
  guard <- runSide runner guardRoot artifacts normalScenario ForbiddenOriginal
  let invalidControls =
        [ invalidValue (scenarioId scenario) (validateScenario scenario) | scenario <- invalidScenarios ]
          ++ [ invalidValue name (validatePointer address) | (name, address) <- invalidPointers ]
      producerControl = witnessControl "double-release" producerWitness producer
      guardControl = witnessControl "forbidden-stock-entry" guardWitness guard
  BL.writeFile (out </> "results.json") (encode (report results producerControl guardControl invalidControls))
  let passed = length [() | (_, original, candidate, _) <- results, resultStatus original == "passed", resultStatus candidate == "passed"]
  putStrLn ("capture develop wait: " ++ show passed ++ "/" ++ show (length results) ++ " differential cases passed")
