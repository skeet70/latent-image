{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}

module Main where

import Control.Exception (IOException, try)
import Control.Monad (forM, when)
import Data.Aeson (Value, encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
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

stateBase, stackBase, planBase, checkCountBase, checkStatusBase, traceBase, traceCountBase, waitSignalBase, releaseBase, candidateBase, depsHighBase, depsLowBase, exitAddress, stackTop, contextBase :: Word32
stateBase = 0x009a0000
stackBase = 0x009a2000
planBase = 0x009a1000
checkCountBase = 0x009a1100
checkStatusBase = 0x009a1104
traceBase = 0x009a1200
traceCountBase = 0x009a1300
waitSignalBase = 0x009a1400
releaseBase = 0x009a1600
candidateBase = 0x009c0000
depsHighBase = 0x00e91810
depsLowBase = 0x007243a8
exitAddress = 0x009c7ff0
stackTop = 0x009a2ff0
contextBase = 0x009a0000

stateSize, stackSize, candidateSize, depsHighSize, depsLowSize, stockWaitOffset :: Int
stateSize = 0x2000
stackSize = 0x1000
candidateSize = 0x8000
depsHighSize = 0x20000
depsLowSize = 0x4000
stockWaitOffset = fromIntegral (0x00e9e038 - depsHighBase)

hex :: Word32 -> String
hex value = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex value ""

u32le :: Word32 -> B.ByteString
u32le value = B.pack [fromIntegral value, fromIntegral (value `shiftR` 8), fromIntegral (value `shiftR` 16), fromIntegral (value `shiftR` 24)]

putU32 :: Int -> Word32 -> B.ByteString -> B.ByteString
putU32 offset value bytes = B.take offset bytes <> u32le value <> B.drop (offset + 4) bytes

putWords :: Int -> [Word32] -> B.ByteString -> B.ByteString
putWords offset values bytes =
  foldl (\current (index, value) -> putU32 (offset + index * 4) value current) bytes (zip [0 :: Int ..] values)

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

data Outcome = Outcome
  { outcomeStatus :: Word32
  , outcomeTrace :: [Word32]
  , outcomeChecks :: Int
  , outcomeRelease :: Word32
  , outcomeSignals :: Word32
  }

clearEntry, advanceEntry, logEntry, waitEntry :: Word32
clearEntry = 0x0100_0000
advanceEntry = 0x0200_0000
logEntry = 0x0300_0000
waitEntry = 0x0400_0000

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
  | otherwise = signals .&. ((1 :: Word32) `shiftL` fromIntegral (planCancelBit plan .&. 31)) /= 0

nextCheck :: [Word32] -> (Word32, [Word32])
nextCheck [] = (0, [])
nextCheck (value : rest) = (value, rest)

model :: Scenario -> Outcome
model scenario
  | entryStatus /= 0 = Outcome entryStatus [] 1 0 (scenarioSignals scenario)
  | cancelRequested plan (scenarioSignals scenario) = cancelPath [clearEntry .|. planEntryClear plan] afterEntry 2 (scenarioSignals scenario)
  | advanceStatus /= 0 = Outcome advanceStatus trace1 2 1 (scenarioSignals scenario)
  | cancelRequested plan afterWait = cancelPath trace2 afterAdvance 3 afterWait
  | otherwise = Outcome finalStatus trace2 3 1 afterWait
  where
    plan = scenarioPlan scenario
    (entryStatus, afterEntry) = nextCheck (scenarioChecks scenario)
    (advanceStatus, afterAdvance) = nextCheck afterEntry
    (finalStatus, _) = nextCheck afterAdvance
    trace1 = [clearEntry .|. planEntryClear plan, advanceEntry .|. planNormalState plan]
    afterWait = if scenarioWaitSignal scenario /= 0 then scenarioWaitSignal scenario else scenarioSignals scenario
    trace2 = trace1 ++ [logEntry .|. 5, waitEntry .|. planWaitEvent plan, logEntry .|. 6]
    cancelPath trace rest count signals =
      let (status, _) = nextCheck rest
       in Outcome status (trace ++ [advanceEntry .|. planCancelState plan, clearEntry .|. planCancelClear plan]) count 1 signals

initialState :: Scenario -> B.ByteString
initialState scenario =
  putWords (stateOffset planBase) (planWords (scenarioPlan scenario))
    (putWords (stateOffset checkStatusBase) (scenarioChecks scenario)
      (putU32 (stateOffset waitSignalBase) (scenarioWaitSignal scenario)
        (putU32 (stateOffset (contextBase + 0x4c)) (scenarioSignals scenario) (B.replicate stateSize 0))))

expectedState :: Scenario -> B.ByteString
expectedState scenario =
  putWords (stateOffset traceBase) (outcomeTrace outcome)
    (putU32 (stateOffset traceCountBase) (fromIntegral (length (outcomeTrace outcome)))
      (putU32 (stateOffset releaseBase) (outcomeRelease outcome)
        (putU32 (stateOffset checkCountBase) (fromIntegral (outcomeChecks outcome))
          (putU32 (stateOffset (contextBase + 0x4c)) (outcomeSignals outcome) (initialState scenario)))))
  where
    outcome = model scenario

scenarios :: [Scenario]
scenarios =
  [ Scenario "normal" base 0 [0, 0, 0] 0
  , Scenario "cancel-before" base 1 [0, 0] 0
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

data Artifacts = Artifacts
  { artifactState :: B.ByteString
  , artifactHigh :: B.ByteString
  , artifactLow :: B.ByteString
  , artifactCandidate :: B.ByteString
  , artifactCandidateLength :: Int
  }

loadArtifacts :: FilePath -> Int -> IO Artifacts
loadArtifacts build candidateLength = do
  high <- B.readFile (build </> "capture-deps-high.bin")
  low <- B.readFile (build </> "capture-deps-low.bin")
  candidate <- B.readFile (build </> "capture-candidate.bin")
  stock <- B.readFile (build </> "stock-wait.bin")
  let highStock = overlay stockWaitOffset stock (ensure (stockWaitOffset + B.length stock) high)
  pure
    Artifacts
      { artifactState = B.empty
      , artifactHigh = pad depsHighSize highStock
      , artifactLow = pad depsLowSize low
      , artifactCandidate = pad candidateSize candidate
      , artifactCandidateLength = candidateLength
      }
  where
    overlay offset bytes base = B.take offset base <> bytes <> B.drop (offset + B.length bytes) base
    ensure size bytes = if B.length bytes >= size then bytes else bytes <> B.replicate (size - B.length bytes) 0
    pad size bytes = B.take size (bytes <> B.replicate size 0)

candidateLengthFrom :: FilePath -> IO Int
candidateLengthFrom build = read <$> readFile (build </> "capture-candidate.length")

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

caseJson :: Scenario -> Side -> Artifacts -> Value
caseJson scenario side artifacts =
  let state = artifactState artifacts
      entry = if side == Original then 0x00e9e038 else candidateBase
      argument = 1
      candidateSpan = ((artifactCandidateLength artifacts + 3) `div` 4) * 4
      spans = syntheticSpans ++ [(entry, if side == Original then 0x11c else candidateSpan)]
      origin = if side == Original then "stock" else "synthetic"
   in object
        [ "schema_version" .= (1 :: Int)
        , "id" .= (scenarioId scenario ++ "-" ++ sideLabel side)
        , "origin" .= origin
        , "provenance" .= ("X-M1 1.50 image 5 FUN_00e9e038 develop step wait; recording dependencies at the stock call addresses")
        , "cpu" .= object ["architecture" .= ("arm" :: String), "model" .= ("cortex-r5f" :: String), "mode" .= ("arm" :: String), "endian" .= ("little" :: String)]
        , "artifacts" .=
            [ artifactJson "state" state
            , artifactJson "deps-high" (artifactHigh artifacts)
            , artifactJson "deps-low" (artifactLow artifacts)
            , artifactJson "candidate" (artifactCandidate artifacts)
            ]
        , "entry" .= hex entry
        , "exit" .= hex exitAddress
        , "registers" .=
            object
              [ "r0" .= hex contextBase
              , "r1" .= hex argument
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
            , regionJson "deps-high" depsHighBase depsHighSize "rx" (artifactHigh artifacts)
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
            [ "The dependency functions record through fixed RAM cells; they model the stock scheduler and state operations, not their full implementations."
            , "No stock table lookup is executed; the plan is supplied at its case address."
            , "The stack region is execution scratch and is not compared."
            ]
        ]

data Side = Original | Candidate deriving (Eq, Show)

sideLabel :: Side -> String
sideLabel Original = "original"
sideLabel Candidate = "candidate"

contains :: N.GuestAddress -> Int -> N.GuestAddress -> Int -> Bool
contains (N.GuestAddress base) extent (N.GuestAddress address) size =
  address >= base && address + fromIntegral size <= base + fromIntegral extent

checkCapture :: [Region] -> Scenario -> N.NativeObservation -> [String]
checkCapture regions scenario observation =
  case N.nativeStatus observation of
    N.Returned -> registers ++ state ++ writes
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
    state =
      case lookup (N.RegionId "state") (N.nativeMemory observation) of
        Nothing -> ["state region missing"]
        Just actual
          | actual /= expectedState scenario -> ["state differs from the source model"]
          | otherwise -> []
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
runSide runner root artifactsTemplate scenario side = do
  let dir = root </> (scenarioId scenario ++ "-" ++ sideLabel side)
      artifacts = artifactsTemplate {artifactState = initialState scenario}
      value = caseJson scenario side artifacts
  createDirectoryIfMissing True dir
  B.writeFile (dir </> "state.bin") (artifactState artifacts)
  B.writeFile (dir </> "deps-high.bin") (artifactHigh artifacts)
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

controlChecks :: Scenario -> N.NativeObservation -> [Value]
controlChecks scenario observation =
  [ control "release-twice" (mutateState (putU32 (stateOffset releaseBase) 2) observation)
  , control "missing-release" (mutateState (putU32 (stateOffset releaseBase) 0) observation)
  , control "extra-trace" (mutateState addTrace observation)
  , control "wrong-status" (observation {N.nativeRegisters = M.insert N.R0 (outcomeStatus (model scenario) + 1) (N.nativeRegisters observation)})
  ]
  where
    trace = outcomeTrace (model scenario)
    addTrace bytes =
      putU32 (stateOffset traceCountBase) (fromIntegral (length trace) + 1)
        (putU32 (stateOffset traceBase + length trace * 4) 0x9 bytes)
    control name changed =
      let problems = checkCapture [] scenario changed
       in object ["name" .= name, "status" .= (if null problems then "accepted" else "refused"), "problems" .= problems]

report :: [(Scenario, Result, Result, [Value])] -> Value
report results =
  object
    [ "schema_version" .= (1 :: Int)
    , "profile" .= ("capture-develop-wait" :: String)
    , "cases" .= [caseReport scenario original candidate controls | (scenario, original, candidate, controls) <- results]
    , "status" .= (if capability results then "qualified" else "failed")
    , "capability" .= capability results
    ]
  where
    caseReport scenario original candidate controls =
      object
        [ "id" .= scenarioId scenario
        , "model" .=
            object
              [ "status" .= outcomeStatus (model scenario)
              , "trace" .= outcomeTrace (model scenario)
              , "checks" .= outcomeChecks (model scenario)
              , "release" .= outcomeRelease (model scenario)
              ]
        , "original" .= resultJson original
        , "candidate" .= resultJson candidate
        , "controls" .= controls
        , "status" .= (if resultStatus original == "passed" && resultStatus candidate == "passed" then "passed" else "failed")
        ]
    resultJson result =
      object ["status" .= resultStatus result, "problems" .= resultProblems result]
    capability =
      all (\(_, original, candidate, controls) ->
        resultStatus original == "passed"
          && resultStatus candidate == "passed"
          && not (null controls)
          && all (\control -> str (get control "status") == "refused") controls)
      results

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [runner, build, out] -> run runner build out
    _ -> putStrLn "usage: capture-qualification RUNNER BUILD_DIR OUT_DIR" >> exitFailure

run :: FilePath -> FilePath -> FilePath -> IO ()
run runner build out = do
  candidateLength <- candidateLengthFrom build
  when (candidateLength <= 0 || candidateLength >= fromIntegral (exitAddress - candidateBase)) $
    error "candidate text reaches the case exit"
  artifacts <- loadArtifacts build candidateLength
  createDirectoryIfMissing True out
  results <- forM scenarios $ \scenario -> do
    original <- runSide runner out artifacts scenario Original
    candidate <- runSide runner out artifacts scenario Candidate
    let controls = maybe [] (controlChecks scenario) (resultObservation candidate)
    pure (scenario, original, candidate, controls)
  BL.writeFile (out </> "results.json") (encode (report results))
  let passed = length [() | (_, original, candidate, _) <- results, resultStatus original == "passed", resultStatus candidate == "passed"]
  putStrLn ("capture develop wait: " ++ show passed ++ "/" ++ show (length results) ++ " differential cases passed")
