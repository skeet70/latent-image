{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
-- | Per-cycle stock-step comparison for Root (issue 117, X-M1 1.50).
--
-- For every cycle a lane's wake dump, frame dump and window-completion dump bracket, the
-- tool runs the stock task-43 handling of the 0x3b4b wake from the lane's captured state
-- with the sampled frame overlaid, then compares the step's posts and next state with the
-- lane's observed interface events and completion dump. The owned lane is also chained:
-- the stock step applied to the owned sampled frames must reproduce the owned level.
module Main (main) where

import Control.Exception (IOException, try)
import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value (..))
import qualified Data.ByteString as B
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import Data.List (nub, sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, mapMaybe)
import Data.Word (Word32, Word8)
import Emulation.NativeCase (loadCase, runCase)
import Emulation.NativeObservation (InstructionCount (..), NativeObservation (..), RegionId (..), Register (..), lookupRegister, nativeInstructions, nativeMemory, nativeStatus, observeNative, renderObservationError, reportRunnerExit, runnerStatusLabel)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (arr, b, ensure, failH, get, jsonRead, list, n, num, obj, readRegular, s, str)
import FujiAF.JSON (encodeCanonical)
import Numeric (showHex)
import Prelude hiding (cycle)
import Replacement.Saclac.CycleStep
import System.Directory (createDirectoryIfMissing, listDirectory, removeFile)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))

-- ---------------------------------------------------------------------------
-- Options

data LaneSpec = LaneSpec {laneName :: String, laneResult :: FilePath, laneCensus :: FilePath, laneInputs :: FilePath}

data Options = Options
  { optionRunner :: FilePath
  , optionId5 :: FilePath
  , optionId4 :: FilePath
  , optionSites :: FilePath
  , optionWork :: FilePath
  , optionOut :: FilePath
  , optionLanes :: [LaneSpec]
  , optionChain :: Maybe String
  , optionStubs :: [(Word32, Word32)]
  , optionRounds :: Int
  }

usage :: String
usage = "usage: saclac-cycle-step --runner RUNNER --id5 IMAGE5 --id4 IMAGE4 --sites SITES --work DIR --out JSON --lane NAME:RESULT_JSON:CENSUS_JSON:INPUTS_DIR [--lane ...] [--chain NAME] [--stub ADDRESS=VALUE]... [--rounds N]"

parseOptions :: [String] -> IO Options
parseOptions arguments = go empty arguments
  where
    empty = Options "" "" "" "" "" "" [] Nothing [] 6
    go options ("--runner" : value : rest) = go options {optionRunner = value} rest
    go options ("--id5" : value : rest) = go options {optionId5 = value} rest
    go options ("--id4" : value : rest) = go options {optionId4 = value} rest
    go options ("--sites" : value : rest) = go options {optionSites = value} rest
    go options ("--work" : value : rest) = go options {optionWork = value} rest
    go options ("--out" : value : rest) = go options {optionOut = value} rest
    go options ("--chain" : value : rest) = go options {optionChain = Just value} rest
    go options ("--rounds" : value : rest) = go options {optionRounds = read value} rest
    go options ("--stub" : value : rest) = case break (== '=') value of
      (address, '=' : number) -> go options {optionStubs = optionStubs options ++ [(fromIntegral (readAddress address), fromIntegral (readAddress number))]} rest
      _ -> failH "--stub takes ADDRESS=VALUE"
    go options ("--lane" : value : rest) = case splitOn ':' value of
      [name, result, census, inputs] -> go options {optionLanes = optionLanes options ++ [LaneSpec name result census inputs]} rest
      _ -> failH "--lane takes NAME:RESULT_JSON:CENSUS_JSON:INPUTS_DIR"
    go _ (other : _) = failH ("unknown option " ++ other)
    go options [] = do
      forM_ [("--runner", optionRunner options), ("--id5", optionId5 options), ("--id4", optionId4 options), ("--sites", optionSites options), ("--work", optionWork options), ("--out", optionOut options)] $ \(name, value) ->
        ensure (not (null value)) ("missing " ++ name ++ "; " ++ usage)
      ensure (not (null (optionLanes options))) usage
      pure options
    splitOn separator text = case break (== separator) text of
      (part, []) -> [part]
      (part, _ : rest) -> part : splitOn separator rest
    readAddress text = case text of
      '0' : 'x' : _ -> hexToInteger text
      _ -> read text

hexToInteger :: String -> Integer
hexToInteger text = case text of
  '0' : 'x' : digits -> foldl' (\acc c -> acc * 16 + digit c) 0 digits
  _ -> 0
  where
    digit :: Char -> Integer
    digit c
      | c >= '0' && c <= '9' = fromIntegral (fromEnum c - fromEnum '0')
      | c >= 'a' && c <= 'f' = fromIntegral (fromEnum c - fromEnum 'a' + 10)
      | otherwise = 0

intOf :: Value -> Integer
intOf value = case value of
  Number _ -> num value
  String _ -> read (str value)
  _ -> 0

addressOf :: Value -> Integer
addressOf value = case value of
  Number _ -> num value
  String _ -> hexToInteger (str value)
  _ -> 0

hexBytesOf :: Value -> B.ByteString
hexBytesOf value = B.pack (pairs (str value))
  where
    pairs (a : c : rest) = fromIntegral (hexDigit a * 16 + hexDigit c) : pairs rest
    pairs _ = []
    hexDigit :: Char -> Word8
    hexDigit c
      | c >= '0' && c <= '9' = fromIntegral (fromEnum c - fromEnum '0')
      | c >= 'a' && c <= 'f' = fromIntegral (fromEnum c - fromEnum 'a' + 10)
      | otherwise = 0

hexText :: Integer -> String
hexText value = let digits = showHex value "" in "0x" ++ replicate (8 - length digits) '0' ++ digits

hexByte :: Word8 -> String
hexByte byte = let digits = showHex byte "" in replicate (2 - length digits) '0' ++ digits

hexList :: [Word8] -> String
hexList = concatMap hexByte

wordAt :: B.ByteString -> Int -> Word32
wordAt bytes offset =
  fromIntegral (B.index bytes offset)
    .|. (fromIntegral (B.index bytes (offset + 1)) `shiftL` 8)
    .|. (fromIntegral (B.index bytes (offset + 2)) `shiftL` 16)
    .|. (fromIntegral (B.index bytes (offset + 3)) `shiftL` 24)

leWord :: Integer -> B.ByteString
leWord value = B.pack [fromIntegral (value `shiftR` shift) | shift <- [0, 8, 16, 24]]

-- ---------------------------------------------------------------------------
-- Images

id5Base, id4Base :: Word32
id5Base = 0x00bf1a00
id4Base = 0x0061c400

initialWindows5, initialWindows4 :: [Window]
initialWindows5 = [Window 0x00ef8000 0x000ff000]
initialWindows4 = [Window 0x00740000 0x00040000]

data Images = Images {image5 :: B.ByteString, image4 :: B.ByteString}

imageWordAt :: Word32 -> B.ByteString -> Word32 -> Maybe Word32
imageWordAt base image address
  | address >= base && fromIntegral (address - base) + 4 <= B.length image = Just (wordAt image (fromIntegral (address - base)))
  | otherwise = Nothing

imageSlice :: Word32 -> B.ByteString -> Window -> Either String B.ByteString
imageSlice base image (Window low size)
  | low < base = Left ("window starts before its image at " ++ hexText (fromIntegral low))
  | fromIntegral (low - base) + fromIntegral size > B.length image = Left ("window ends after its image at " ++ hexText (fromIntegral low))
  | otherwise = Right (B.take (fromIntegral size) (B.drop (fromIntegral (low - base)) image))

-- | The direct branch target of the instruction at a guest address, when the image holds it.
branchTarget :: Images -> Word32 -> Maybe Word32
branchTarget images address
  | Just word <- imageWordAt id5Base (image5 images) address = decode address word
  | Just word <- imageWordAt id4Base (image4 images) address = decode address word
  | otherwise = Nothing
  where
    decode pc word
      | word .&. 0x0E000000 == 0x0A000000 =
          let immediate = word .&. 0x00FFFFFF
              offset = if immediate .&. 0x00800000 /= 0 then fromIntegral immediate - 0x1000000 else fromIntegral immediate
           in Just (fromIntegral (fromIntegral pc + 8 + 2 * offset :: Integer) :: Word32)
      | otherwise = Nothing

-- ---------------------------------------------------------------------------
-- Lane inputs

data Lane = Lane
  { laneSpec :: LaneSpec
  , laneDumps :: [Dump]
  , laneEvents :: [TimedEvent]
  , laneFrameWrites :: [Integer]
  , laneError :: Maybe String
  }

loadLane :: LaneSpec -> IO Lane
loadLane spec = do
  resultBytes <- readRegular (64 * 1024 * 1024) (laneResult spec)
  result <- jsonRead resultBytes
  censusBytes <- readRegular (64 * 1024 * 1024) (laneCensus spec)
  census <- jsonRead censusBytes
  let failure = case (get result "error", get result "machine_failure") of
        (Null, Null) -> Nothing
        (Null, other) -> Just ("machine_failure: " ++ show other)
        (other, _) -> Just ("error: " ++ show other)
      writes = [intOf (get row "instructions") | row <- list (get (get result "diagnostic") "writes"), addressOf (get row "address") == 0x016f673c]
  pure Lane
    { laneSpec = spec
    , laneDumps = parseDumps result
    , laneEvents = parseEvents census
    , laneFrameWrites = sortOn id writes
    , laneError = failure
    }

dumpBytes :: Lane -> Dump -> IO (Either String B.ByteString)
dumpBytes lane dump = do
  let path = laneInputs (laneSpec lane) </> dumpPath dump
  attempt <- try (readRegular (16 * 1024 * 1024) path) :: IO (Either IOException B.ByteString)
  pure $ case attempt of
    Left exception -> Left ("cannot read " ++ path ++ ": " ++ show exception)
    Right bytes
      | fromIntegral (B.length bytes) /= dumpByteCount dump -> Left ("dump " ++ dumpPath dump ++ " is " ++ show (B.length bytes) ++ " bytes, declared " ++ show (dumpByteCount dump))
      | sha256Hex (sha256 bytes) /= dumpSha256 dump -> Left ("dump " ++ dumpPath dump ++ " differs from its declared SHA-256")
      | otherwise -> Right bytes

-- ---------------------------------------------------------------------------
-- Case planning

data PlanInput = PlanInput
  { planState :: State
  , planMessage :: (Int, Int, [Word8])
  , planWindows5 :: [Window]
  , planWindows4 :: [Window]
  , planId :: String
  , planExtraStubs :: [(Word32, Word32)]
  }

exitAddress :: Word32
exitAddress = 0x00501000

entryFunctionSize :: Word32
entryFunctionSize = 0x80

regionsOf :: Images -> PlanInput -> Either String [(RegionPlan, Maybe B.ByteString)]
regionsOf images input = do
  windows5 <- mapM (windowPlan (image5 images) id5Base "id5") (zip [0 :: Int ..] (planWindows5 input))
  windows4 <- mapM (windowPlan (image4 images) id4Base "id4") (zip [0 :: Int ..] (planWindows4 input))
  pure
    ( [ (RegionPlan "ram-low" (fromIntegral lowBase) (fromIntegral lowSize) "rw" (Just "ram-low"), Just (stateLow (planState input)))
      , (RegionPlan "ram-high" (fromIntegral highBase) (fromIntegral highSize) "rw" (Just "ram-high"), Just (stateHigh (planState input)))
      , (RegionPlan "record" recordBase (fromIntegral recordSize) "rw" Nothing, Nothing)
      , (RegionPlan "exit" exitAddress 4 "rx" Nothing, Nothing)
      , (RegionPlan "tramp" trampolineBase (fromIntegral trampolineSize) "rx" (Just "tramp"), Just trampolineBytes)
      ]
      ++ windows5
      ++ windows4
    )
  where
    trampolineBytes = B.concat (map leWord (map fromIntegral trampoline))
    windowPlan image base tag (index, window) = do
      bytes <- imageSlice base image window
      pure (RegionPlan (tag ++ "-" ++ show index) (windowBase window) (windowSize window) "rx" (Just (tag ++ "-" ++ show index)), Just bytes)

functionsOf :: PlanInput -> [(Word32, Word32)]
functionsOf input =
  [(entryAddress, entryFunctionSize)]
    ++ concatMap partitions (planWindows5 input)
    ++ [(windowBase window, windowSize window) | window <- planWindows4 input]
    ++ [(trampolineBase, fromIntegral trampolineSize)]
  where
    partitions window
      | windowContains window entryAddress =
          [ (windowBase window, entryAddress - windowBase window)
          , (entryAddress + entryFunctionSize, windowBase window + windowSize window - (entryAddress + entryFunctionSize))
          ]
      | otherwise = [(windowBase window, windowSize window)]

receiveCall :: (Int, Int, [Word8]) -> StubCall
receiveCall (operation, sender, payload) = StubCall
  { stubInputs = [("r0", 43), ("r1", 0x0169d374), ("r2", 0xffffffff)]
  , stubOutputs = [("r0", 0)]
  , stubWrites =
      [ (0x0169d374, B.pack [fromIntegral (operation .&. 0xff), fromIntegral ((operation `shiftR` 8) .&. 0xff), fromIntegral sender, 43])
      , (0x0169d378, leWord (fromIntegral (length payload)))
      , (0x0169d37c, leWord 0x0169d380)
      , (0x0169d380, B.pack payload)
      ]
  }

planCase :: Images -> PlanInput -> Either String (CasePlan, [(String, B.ByteString)])
planCase images input = do
  regions <- regionsOf images input
  let artifacts = [(artifact, bytes) | (RegionPlan _ _ _ _ (Just artifact), Just bytes) <- regions]
  pure
    ( CasePlan
        { casePlanId = planId input
        , casePlanProvenance = "Stock X-M1 1.50 task-43 handling of the 0x3b4b wake entered at 0x00f01be4 from the lane's captured state; code bytes are the pinned image 4/5 bytes, the kernel post 0x0072503c is a recording trampoline and the kernel receive 0x00725318 and task-id 0x00723cb8 are stubs."
        , casePlanExclusions =
            [ "The kernel receive 0x00725318 is a scripted stub: its second call ends the step with stub_contract at the point task 43 would block."
            , "The kernel post 0x0072503c is replaced by a recording trampoline in the case's own region; its arguments are compared, its kernel effect is not executed."
            , "The record region 0x00500000 is harness scratch and is excluded from the retained state."
            ]
        , casePlanEntry = entryAddress
        , casePlanExit = exitAddress
        , casePlanRegisters =
            [ ("r0", 0), ("r1", 0), ("r2", 0), ("r3", 0), ("r4", 0)
            , ("r5", 0x0169d370), ("r6", 0x0169d380), ("r7", 0x0169d374)
            , ("r8", 0), ("r9", 0), ("r10", 0)
            , ("r11", 0)
            , ("r12", 0)
            , ("sp", 0)
            , ("lr", exitAddress)
            , ("cpsr", 0x10)
            ]
        , casePlanRegions = map fst regions
        , casePlanFunctions = functionsOf input
        , casePlanStubs =
            [ ScriptStub taskIdAddress (replicate 32 (StubCall [] [("r0", 43)] []))
            , ScriptStub receiveAddress [receiveCall (planMessage input)]
            ]
              ++ [ScriptStub address (replicate 32 (StubCall [] [("r0", value)] [])) | (address, value) <- planExtraStubs input]
        , casePlanArtifacts = artifacts
        , casePlanBudget = 16000000
        }
    , artifacts
    )

withStack :: Word32 -> CasePlan -> CasePlan
withStack sp plan = plan
  { casePlanRegisters = [(name, if name == "sp" then sp else if name == "r11" then sp + 20 else value) | (name, value) <- casePlanRegisters plan] }

-- ---------------------------------------------------------------------------
-- Running a case

data RunResult = RunResult
  { runStatus :: String
  , runDetail :: String
  , runInstructions :: Integer
  , runMemory :: M.Map String B.ByteString
  , runPc :: Word32
  , runAccesses :: [Value]
  , runExecuted :: [Value]
  , runCaseSha :: String
  }

runPlanned :: Options -> CasePlan -> IO (Either String RunResult)
runPlanned options plan = do
  let directory = optionWork options </> "case"
  createDirectoryIfMissing True directory
  existing <- listDirectory directory
  forM_ existing $ \name -> removeFile (directory </> name)
  forM_ (casePlanArtifacts plan) $ \(ident, bytes) -> B.writeFile (directory </> (ident ++ ".bin")) bytes
  let path = directory </> "case.json"
      json = encodeCanonical (caseValue plan)
  B.writeFile path json
  let digest = sha256Hex (sha256 json)
  (_, report) <- runCase (optionRunner options) path
  case get report "runner" of
    Null -> pure (Left ("case refused: " ++ str (get report "status") ++ ": " ++ str (get report "reason")))
    runner -> do
      loaded <- loadCase path
      let (_, regions, _, _) = loaded
      case reportRunnerExit report of
        Left problem -> pure (Left (renderObservationError problem))
        Right exit -> case observeNative exit regions runner of
          Left problem -> pure (Left (renderObservationError problem))
          Right observation -> do
            let InstructionCount count = nativeInstructions observation
            pure (Right RunResult
              { runStatus = runnerStatusLabel (nativeStatus observation)
              , runDetail = str (get runner "detail")
              , runInstructions = fromIntegral count
              , runMemory = M.fromList [(regionName ident, bytes) | (ident, bytes) <- nativeMemory observation]
              , runPc = fromMaybe 0 (lookupRegister Pc observation)
              , runAccesses = list (get runner "accesses")
              , runExecuted = list (get runner "executed")
              , runCaseSha = digest
              })

-- | Runs one cycle, extending the code windows when the runner refuses a fetch or a data
-- access outside them, up to the declared round limit.
runCycle :: Options -> Images -> PlanInput -> Word32 -> IO (Either String (RunResult, [Window], [Window], Int))
runCycle options images input sp = go 0 (planWindows5 input) (planWindows4 input)
  where
    go round' windows5 windows4 = case planCase images input {planWindows5 = windows5, planWindows4 = windows4} of
      Left problem -> pure (Left problem)
      Right (plan, _) -> do
        result <- runPlanned options (withStack sp plan)
        case result of
          Left problem -> pure (Left problem)
          Right run
            | round' >= optionRounds options ->
                pure (Left ("closure did not close after " ++ show round' ++ " rounds: " ++ runStatus run ++ " at " ++ hexText (fromIntegral (runPc run)) ++ " " ++ runDetail run))
            | otherwise -> case extension images windows5 windows4 run of
                Nothing -> pure (Right (run, windows5, windows4, round'))
                Just (Left problem) -> pure (Left problem)
                Just (Right (windows5', windows4')) -> go (round' + 1) windows5' windows4'

-- | The window extension a runner refusal asks for, or Nothing when the run ended.
extension :: Images -> [Window] -> [Window] -> RunResult -> Maybe (Either String ([Window], [Window]))
extension images windows5 windows4 run = case runStatus run of
  "fetch_guard" -> Just (extend (runPc run))
  "memory_guard" -> case reverse (runAccesses run) of
    row : _ -> Just (extend (fromIntegral (addressOf (get row "address"))))
    [] -> Just (Left "memory_guard without a recorded access")
  "unknown_call" ->
    let target = branchTarget images (runPc run)
     in Just (Left ("unmapped call at " ++ hexText (fromIntegral (runPc run))
          ++ maybe "" (\address -> " to " ++ hexText (fromIntegral address)) target
          ++ ": declare --stub or extend the windows"))
  _ -> Nothing
  where
    extend address
      | inImage5 address = Right (mergeWindows (grow id5Base (fromIntegral (B.length (image5 images))) address windows5), windows4)
      | inImage4 address = Right (windows5, mergeWindows (grow id4Base (fromIntegral (B.length (image4 images))) address windows4))
      | otherwise = Left ("refused access at " ++ hexText (fromIntegral address) ++ " outside the pinned images")
    inImage5 address = address >= id5Base && fromIntegral address < fromIntegral id5Base + B.length (image5 images)
    inImage4 address = address >= id4Base && fromIntegral address < fromIntegral id4Base + B.length (image4 images)
    grow :: Word32 -> Int -> Word32 -> [Window] -> [Window]
    grow base length' address windows =
      let low = max base (address - 0x10000)
          high = min (base + fromIntegral length') (address + 0x10000)
       in Window low (high - low) : windows

-- ---------------------------------------------------------------------------
-- Comparison

data CycleRun = CycleRun
  { cycleRunCycle :: Cycle
  , cycleRunInput :: State
  , cycleRunPredicted :: State
  , cycleRunObserved :: State
  , cycleRunReport :: CycleReport
  , cycleRunWindows5 :: [Window]
  , cycleRunWindows4 :: [Window]
  }

data CycleReport = CycleReport
  { reportIndex :: Int
  , reportWake :: Dump
  , reportFrame :: Dump
  , reportCompletion :: Dump
  , reportOutputsEqual :: Bool
  , reportNativePosts :: [(Int, Int, [Word8], String)]
  , reportObservedPosts :: [(Int, Int, [Word8])]
  , reportFirstDifference :: Maybe String
  , reportStateEqual :: Bool
  , reportDifferences :: [Difference]
  , reportNativeStatus :: String
  , reportNativeInstructions :: Integer
  , reportExecuted :: Integer
  , reportLevelPredicted :: Maybe Word32
  , reportLevelObserved :: Maybe Word32
  , reportCaseSha :: String
  , reportRounds :: Int
  , reportComparedBytes :: Int
  }

declaredSpans :: Value -> [Span]
declaredSpans sites =
  [ Span (addressOf (get row "start")) (addressOf (get row "end"))
  | row <- list (get (get sites "interface") "shared")
  ]
    ++ [Span 0x0169d374 0x0169d4fe, Span groupBase 0x00919a20]

postRecords :: State -> [(Int, Int, [Word8], String)]
postRecords state = case stateWord state (fromIntegral recordBase) of
  Nothing -> []
  Just count -> mapMaybe record [1 .. fromIntegral count]
  where
    record index = case stateSlice state (fromIntegral recordBase + 32 * index) 28 of
      Just slot ->
        let queue = fromIntegral (wordAt slot 0) :: Int
            header = wordAt slot 16
            length' = fromIntegral (wordAt slot 20) :: Int
            pointer = wordAt slot 24
         in Just (queue, fromIntegral (header .&. 0xffff), payloadAt pointer length', payloadProblem pointer length')
      Nothing -> Nothing
    payloadAt pointer length' = case stateSlice state (fromIntegral pointer) length' of
      Just bytes -> B.unpack bytes
      Nothing -> []
    payloadProblem pointer length' = case stateSlice state (fromIntegral pointer) length' of
      Just _ -> ""
      Nothing -> "payload at " ++ hexText (fromIntegral pointer) ++ " (" ++ show length' ++ " bytes) is outside the dumped state"

observedPosts :: Cycle -> [(Int, Int, [Word8])]
observedPosts cycle = [(queue, operation, payload) | TimedEvent _ (EventPeerRequest queue operation payload) <- cyclePosts cycle]

postsEqual :: [(Int, Int, [Word8], String)] -> [(Int, Int, [Word8])] -> Bool
postsEqual native observed = length native == length observed && and (zipWith same native observed)
  where
    same (queue, operation, payload, problem) (queue', operation', payload') =
      queue == queue' && operation == operation' && payload == payload' && null problem

firstDifference :: [(Int, Int, [Word8], String)] -> [(Int, Int, [Word8])] -> Maybe String
firstDifference native observed = go 0 native observed
  where
    go :: Int -> [(Int, Int, [Word8], String)] -> [(Int, Int, [Word8])] -> Maybe String
    go _ [] [] = Nothing
    go index [] ((queue, operation, payload) : _) =
      Just ("observed post " ++ show index ++ " (queue " ++ show queue ++ ", operation " ++ hexText (fromIntegral operation) ++ ", " ++ show (length payload) ++ " bytes) has no native post")
    go index ((queue, operation, _, problem) : _) [] =
      Just ("native post " ++ show index ++ " (queue " ++ show queue ++ ", operation " ++ hexText (fromIntegral operation) ++ ") has no observed post" ++ (if null problem then "" else ": " ++ problem))
    go index ((queue, operation, payload, problem) : rest) ((queue', operation', payload') : rest')
      | queue /= queue' = Just ("post " ++ show index ++ ": queue " ++ show queue ++ " vs observed " ++ show queue')
      | operation /= operation' = Just ("post " ++ show index ++ ": operation " ++ hexText (fromIntegral operation) ++ " vs observed " ++ hexText (fromIntegral operation'))
      | not (null problem) = Just ("post " ++ show index ++ ": " ++ problem)
      | payload /= payload' = Just ("post " ++ show index ++ ": payload differs: native " ++ hexList payload ++ " observed " ++ hexList payload')
      | otherwise = go (index + 1) rest rest'

executedBytes :: [Value] -> Integer
executedBytes rows = sum [4 * fromIntegral (popCount (hexBytesOf (get row "executed"))) | row <- rows]
  where
    popCount = sum . map bits . B.unpack
    bits byte = length [() | bit <- [0 :: Int .. 7], byte .&. (1 `shiftL` bit) /= 0]

-- ---------------------------------------------------------------------------
-- Lane execution

data LaneOutput = LaneOutput
  { laneOutputName :: String
  , laneOutputQualified :: Bool
  , laneOutputCycles :: [Value]
  , laneOutputRefusals :: [Value]
  , laneOutputMetrics :: [Value]
  , laneOutputClosure :: Value
  , laneOutputRuns :: [CycleRun]
  , laneOutputError :: Maybe String
  }

runLane :: Options -> Images -> [Span] -> Lane -> IO LaneOutput
runLane options images spans lane = case laneError lane of
  Just problem -> pure (LaneOutput (laneName (laneSpec lane)) False [] [] [] (obj []) [] (Just problem))
  Nothing -> do
    let (cycles, refusals) = pairCycles (laneDumps lane) (laneEvents lane)
        refusalValues = [obj [("cycle", s (refusalWhere refusal)), ("reason", s (refusalReason refusal))] | refusal <- refusals]
    runs <- forM (zip [0 :: Int ..] cycles) $ \(index, cycle) -> (,) index <$> runCycleOf index cycle
    let successes = [run | (_, Right run) <- runs]
        failures = [(index, problem) | (index, Left problem) <- runs]
        metrics = map (metricOf lane) (zip [0 :: Int ..] cycles)
        windows5 = nub (concatMap cycleRunWindows5 successes)
        windows4 = nub (concatMap cycleRunWindows4 successes)
        qualified = null failures && not (null successes) && all (\run -> reportStateEqual (cycleRunReport run) && reportOutputsEqual (cycleRunReport run)) successes
    pure LaneOutput
      { laneOutputName = laneName (laneSpec lane)
      , laneOutputQualified = qualified
      , laneOutputCycles = map (cycleValue . cycleRunReport) successes ++ [obj [("cycle", n (fromIntegral index)), ("error", s problem)] | (index, problem) <- failures]
      , laneOutputRefusals = refusalValues
      , laneOutputMetrics = metrics
      , laneOutputClosure = obj
          [ ("windows", arr ([obj [("image", s "image5"), ("base", s (hexText (fromIntegral (windowBase window)))), ("size", n (fromIntegral (windowSize window)))] | window <- (if null windows5 then initialWindows5 else windows5)]
              ++ [obj [("image", s "image4"), ("base", s (hexText (fromIntegral (windowBase window)))), ("size", n (fromIntegral (windowSize window)))] | window <- (if null windows4 then initialWindows4 else windows4)]))
          , ("executed_bytes", n (sum [reportExecuted (cycleRunReport run) | run <- successes]))
          , ("cycles", n (fromIntegral (length successes)))
          , ("stubs", arr (map s ["0x00723cb8 task id returns 43", "0x00725318 receive is scripted", "0x0072503c post is the recording trampoline"] ++ [s ("0x" ++ hexText (fromIntegral address) ++ " returns " ++ show value) | (address, value) <- optionStubs options]))
          ]
      , laneOutputRuns = successes
      , laneOutputError = Nothing
      }
  where
    runCycleOf index cycle = do
      loaded <- loadCycleDumps cycle
      case loaded of
        Left problem -> pure (Left problem)
        Right (state, frameBytes) -> case overlayState state frameLow frameBytes of
          Left problem -> pure (Left problem)
          Right input -> case findLoopStack input of
            Left problem -> pure (Left problem)
            Right sp -> do
              let planInput = PlanInput
                    { planState = input
                    , planMessage = messageOf cycle
                    , planWindows5 = initialWindows5
                    , planWindows4 = initialWindows4
                    , planId = "cycle-" ++ laneName (laneSpec lane) ++ "-" ++ show index
                    , planExtraStubs = optionStubs options
                    }
              outcome <- runCycle options images planInput sp
              case outcome of
                Left problem -> pure (Left problem)
                Right (run, windows5, windows4, rounds) -> do
                  observedResult <- completionState cycle
                  case observedResult of
                    Left problem -> pure (Left problem)
                    Right observed -> do
                      let predicted = State (M.findWithDefault B.empty "ram-low" (runMemory run)) (M.findWithDefault B.empty "ram-high" (runMemory run))
                      if B.length (stateLow predicted) /= fromIntegral lowSize || B.length (stateHigh predicted) /= fromIntegral highSize
                        then pure (Left "runner did not report the two dumped ranges")
                        else do
                          let changed = netChanged input predicted
                              differences = compareState spans changed predicted observed
                              posts = postRecords predicted
                              observed' = observedPosts cycle
                              report = CycleReport
                                { reportIndex = index
                                , reportWake = cycleWake cycle
                                , reportFrame = cycleFrame cycle
                                , reportCompletion = cycleCompletion cycle
                                , reportOutputsEqual = postsEqual posts observed'
                                , reportNativePosts = posts
                                , reportObservedPosts = observed'
                                , reportFirstDifference = firstDifference posts observed'
                                , reportStateEqual = null differences
                                , reportDifferences = differences
                                , reportNativeStatus = runStatus run
                                , reportNativeInstructions = runInstructions run
                                , reportExecuted = executedBytes (runExecuted run)
                                , reportLevelPredicted = stateWord predicted levelAddress
                                , reportLevelObserved = stateWord observed levelAddress
                                , reportCaseSha = runCaseSha run
                                , reportRounds = rounds
                                , reportComparedBytes = length changed + sum [fromIntegral (spanBytes span') | span' <- spans]
                                }
                          pure (Right CycleRun
                            { cycleRunCycle = cycle
                            , cycleRunInput = input
                            , cycleRunPredicted = predicted
                            , cycleRunObserved = observed
                            , cycleRunReport = report
                            , cycleRunWindows5 = windows5
                            , cycleRunWindows4 = windows4
                            })
    loadCycleDumps cycle = do
      wake <- dumpBytes lane (cycleWake cycle)
      frame <- dumpBytes lane (cycleFrame cycle)
      case (wake, frame) of
        (Left problem, _) -> pure (Left problem)
        (_, Left problem) -> pure (Left problem)
        (Right wakeBytes, Right frameBytes) -> case splitState wakeBytes of
          Left problem -> pure (Left problem)
          Right state
            | B.length frameBytes /= fromIntegral (frameHigh - frameLow) -> pure (Left ("frame dump is " ++ show (B.length frameBytes) ++ " bytes, expected " ++ show (frameHigh - frameLow)))
            | otherwise -> pure (Right (state, frameBytes))
    completionState cycle = do
      bytes <- dumpBytes lane (cycleCompletion cycle)
      pure (bytes >>= splitState)
    messageOf cycle = case cycleWakeEvent cycle of
      TimedEvent _ (EventQueueIn operation sender payload) -> (operation, sender, payload)
      _ -> (0, 0, [])

-- | Frame identity, latency and slip metrics for one cycle.
metricOf :: Lane -> (Int, Cycle) -> Value
metricOf lane (index, cycle) =
  let writes = laneFrameWrites lane
      frameInstruction = dumpInstructions (cycleFrame cycle)
      before = [write | write <- writes, write <= frameInstruction]
      identity' = length before - 1
      latency = case reverse before of
        write : _ -> frameInstruction - write
        [] -> -1
      period = median (zipWith (-) (drop 1 writes) writes)
   in obj
        [ ("cycle", n (fromIntegral index))
        , ("wake_instructions", n (dumpInstructions (cycleWake cycle)))
        , ("frame_instructions", n frameInstruction)
        , ("completion_instructions", n (dumpInstructions (cycleCompletion cycle)))
        , ("frame_identity", n (fromIntegral identity'))
        , ("latency_instructions", n latency)
        , ("frame_period_instructions", n period)
        , ("latency_frames", s (ratio latency period))
        ]

ratio :: Integer -> Integer -> String
ratio _ 0 = "n/a"
ratio numerator denominator = show (fromIntegral numerator / fromIntegral denominator :: Double)

median :: [Integer] -> Integer
median [] = 0
median values = let ordered = sortOn id values; count = length ordered in ordered !! (count `div` 2)

cycleValue :: CycleReport -> Value
cycleValue report = obj
  [ ("cycle", n (fromIntegral (reportIndex report)))
  , ("wake_instructions", n (dumpInstructions (reportWake report)))
  , ("frame_instructions", n (dumpInstructions (reportFrame report)))
  , ("completion_instructions", n (dumpInstructions (reportCompletion report)))
  , ("outputs_equal", b (reportOutputsEqual report))
  , ("native_posts", arr [obj [("queue", n (fromIntegral queue)), ("operation", s (hexText (fromIntegral operation))), ("payload", s (hexList payload)), ("unavailable", s problem)] | (queue, operation, payload, problem) <- reportNativePosts report])
  , ("observed_posts", arr [obj [("queue", n (fromIntegral queue)), ("operation", s (hexText (fromIntegral operation))), ("payload", s (hexList payload))] | (queue, operation, payload) <- reportObservedPosts report])
  , ("first_output_difference", maybe Null s (reportFirstDifference report))
  , ("state_equal", b (reportStateEqual report))
  , ("compared_bytes", n (fromIntegral (reportComparedBytes report)))
  , ("differing_count", n (fromIntegral (length (reportDifferences report))))
  , ("differing_bytes", arr (map differenceValue (take 64 (reportDifferences report))))
  , ("native_status", s (reportNativeStatus report))
  , ("native_instructions", n (reportNativeInstructions report))
  , ("executed_bytes", n (reportExecuted report))
  , ("level_predicted", maybe Null (s . hexText . fromIntegral) (reportLevelPredicted report))
  , ("level_observed", maybe Null (s . hexText . fromIntegral) (reportLevelObserved report))
  , ("case_sha256", s (reportCaseSha report))
  , ("closure_rounds", n (fromIntegral (reportRounds report)))
  ]

differenceValue :: Difference -> Value
differenceValue difference = obj
  [ ("address", s (hexText (differenceAddress difference)))
  , ("predicted", s (hexByte (differencePredicted difference)))
  , ("observed", s (hexByte (differenceObserved difference)))
  , ("declared", b (differenceDeclared difference))
  ]

-- ---------------------------------------------------------------------------
-- The chained owned lane

data ChainResult = ChainResult {chainEntries :: [Value], chainMatches :: Bool, chainError :: Maybe String}

data ChainRun = ChainRun {chainInput :: State, chainPredicted :: State, chainObserved :: State}

-- | Applies the stock step to the owned lane's sampled frames. The first cycle starts from
-- the owned wake state with its frame overlaid; each later cycle starts from the previous
-- prediction with the owned lane's observed changes since its completion applied outside
-- the step's own write set.
runChain :: Options -> Images -> Lane -> [CycleRun] -> IO ChainResult
runChain options images lane runs = go 0 Nothing [] runs
  where
    go :: Int -> Maybe ChainRun -> [Value] -> [CycleRun] -> IO ChainResult
    go _ _ entries [] = pure (ChainResult (reverse entries) (all entryMatches (reverse entries)) Nothing)
    go index carried entries (run : rest) = case carried of
      Nothing -> runStep (cycleRunInput run)
      Just previous -> do
        nextWake <- dumpBytes lane (cycleWake (cycleRunCycle run))
        case nextWake of
          Left problem -> pure (ChainResult (reverse entries) False (Just problem))
          Right raw -> case splitState raw of
            Left problem -> pure (ChainResult (reverse entries) False (Just problem))
            Right next ->
              let writeSet = netChanged (chainInput previous) (chainPredicted previous)
                  state = applyDeltas (chainObserved previous) next writeSet (chainPredicted previous)
              in do
                overlaid <- overlayFrame state
                case overlaid of
                  Left problem -> pure (ChainResult (reverse entries) False (Just problem))
                  Right input -> runStep input
      where
        runStep input = case findLoopStack input of
          Left problem -> pure (ChainResult (reverse entries) False (Just problem))
          Right sp -> do
            let planInput = PlanInput input (0x3b4b, 47, wakePayload (cycleRunCycle run)) initialWindows5 initialWindows4 ("chain-" ++ laneName (laneSpec lane) ++ "-" ++ show index) (optionStubs options)
            outcome <- runCycle options images planInput sp
            case outcome of
              Left problem -> pure (ChainResult (reverse entries) False (Just problem))
              Right (result, _, _, _) -> do
                let predicted = State (M.findWithDefault B.empty "ram-low" (runMemory result)) (M.findWithDefault B.empty "ram-high" (runMemory result))
                    observed = cycleRunObserved run
                    predictedLevel = stateWord predicted levelAddress
                    observedLevel = stateWord observed levelAddress
                    entry = obj
                      [ ("cycle", n (fromIntegral (reportIndex (cycleRunReport run))))
                      , ("level_predicted", maybe Null (s . hexText . fromIntegral) predictedLevel)
                      , ("level_observed", maybe Null (s . hexText . fromIntegral) observedLevel)
                      , ("decade_predicted", maybe Null (s . hexText . fromIntegral) (stateWord predicted decadeAddress))
                      , ("decade_observed", maybe Null (s . hexText . fromIntegral) (stateWord observed decadeAddress))
                      , ("match", b (predictedLevel == observedLevel))
                      , ("native_status", s (runStatus result))
                      ]
                go (index + 1) (Just (ChainRun input predicted observed)) (entry : entries) rest
        overlayFrame state = do
          frame <- dumpBytes lane (cycleFrame (cycleRunCycle run))
          pure (frame >>= overlayState state frameLow)
    entryMatches entry = get entry "match" == Bool True
    wakePayload cycle = case cycleWakeEvent cycle of
      TimedEvent _ (EventQueueIn _ _ payload) -> payload
      _ -> []

applyDeltas :: State -> State -> [Integer] -> State -> State
applyDeltas observed next writeSet predicted = foldl' apply predicted addresses
  where
    writeMembers = M.fromList [(address, ()) | address <- writeSet]
    addresses = [address | address <- netChanged observed next, not (M.member address writeMembers)]
    apply state address = case stateSlice next address 1 of
      Just byte -> either (const state) id (overlayState state address byte)
      Nothing -> state

-- ---------------------------------------------------------------------------
-- Main

main :: IO ()
main = do
  options <- parseOptions =<< getArgs
  image5Bytes <- readRegular (16 * 1024 * 1024) (optionId5 options)
  image4Bytes <- readRegular (16 * 1024 * 1024) (optionId4 options)
  sitesBytes <- readRegular (4 * 1024 * 1024) (optionSites options)
  sites <- jsonRead sitesBytes
  let images = Images image5Bytes image4Bytes
      spans = declaredSpans sites
  createDirectoryIfMissing True (optionWork options)
  lanes <- mapM loadLane (optionLanes options)
  outputs <- forM lanes $ \lane -> runLane options images spans lane
  let named name = case [output | output <- outputs, laneOutputName output == name] of
        output : _ -> Just output
        [] -> Nothing
      chainOutput = do
        name <- optionChain options
        lane <- case [lane | lane <- lanes, laneName (laneSpec lane) == name] of
          lane : _ -> Just lane
          [] -> Nothing
        output <- named name
        pure (lane, output)
  chained <- case chainOutput of
    Just (lane, output) -> do
      result <- runChain options images lane (laneOutputRuns output)
      pure (obj
        [ ("lane", s (laneName (laneSpec lane)))
        , ("cycles", arr (chainEntries result))
        , ("sequence_matches", b (chainMatches result))
        , ("error", maybe Null s (chainError result))
        ])
    Nothing -> pure Null
  let stockQualified = maybe False laneOutputQualified (named "stock")
      chainQualified = case chainOutput of
        Just (_, _) -> get chained "sequence_matches" == Bool True
        Nothing -> True
      -- Frame identity slip per cycle, paired by order from each lane's first common wake.
      slip = case named "stock" of
        Just stockOutput ->
          [ obj
              [ ("cycle", n (fromIntegral (index :: Int)))
              , ("lane", s (laneOutputName output))
              , ("stock_frame_identity", n (stockIdentity stockOutput index))
              , ("lane_frame_identity", n (identityAt output index))
              , ("slip", n (identityAt output index - stockIdentity stockOutput index))
              ]
          | output <- outputs
          , laneOutputName output /= "stock"
          , (index, _) <- zip [0 :: Int ..] (laneOutputMetrics output)
          ]
        Nothing -> []
      stockIdentity output index = identityAt output index
      identityAt output index = case drop index (laneOutputMetrics output) of
        entry : _ -> num (get entry "frame_identity")
        [] -> -1
      overall = obj
        [ ("schema", n 1)
        , ("tool", s "saclac-cycle-step")
        , ("stock_qualified", b stockQualified)
        , ("lanes", arr (map laneValue outputs))
        , ("chained", chained)
        , ("frame_identity_slip", arr slip)
        , ("exclusions", arr (map s
            [ "The record region 0x00500000 is the trampoline's harness scratch and is not retained state."
            , "Outside the declared retained spans only the step's net changes are compared; the runner's provenance log truncates at 4096 accesses, so a byte the step wrote and restored is not compared."
            , "Code bytes come from the pinned image 4 and image 5; the dumped ranges hold no code, so no per-function lane-code comparison applies."
            ]))
        ]
  B.writeFile (optionOut options) (encodeCanonical overall)
  unless (stockQualified && chainQualified) exitFailure
  where
    laneValue output = obj
      [ ("lane", s (laneOutputName output))
      , ("qualified", b (laneOutputQualified output))
      , ("error", maybe Null s (laneOutputError output))
      , ("refusals", arr (laneOutputRefusals output))
      , ("closure", laneOutputClosure output)
      , ("cycles", arr (laneOutputCycles output))
      , ("metrics", arr (laneOutputMetrics output))
      ]
