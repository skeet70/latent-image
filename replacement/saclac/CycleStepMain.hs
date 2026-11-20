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
import qualified Data.ByteString.Char8 as BC
import Data.Bits (complement, shiftL, shiftR, (.&.), (.|.))
import Data.IORef (IORef, newIORef, readIORef, writeIORef)
import Data.List (nub, sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, isJust, mapMaybe)
import Data.Word (Word32, Word8)
import Emulation.NativeCase (loadCase, runCase)
import Emulation.NativeObservation (Coverage (..), InstructionCount (..), NativeObservation (..), RegionId (..), Register (..), lookupRegister, nativeInstructions, nativeMemory, nativeStatus, observeNative, renderObservationError, reportRunnerExit, runnerStatusLabel)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (arr, b, ensure, failH, get, jsonRead, list, n, num, obj, readRegular, s, str)
import FujiAF.JSON (encodeCanonical)
import Numeric (showHex)
import Prelude hiding (cycle)
import Replacement.Saclac.CycleStep
import qualified Replacement.Saclac.PayloadProjection as Payload
import Replacement.Saclac.StateMap
import qualified Data.Text as T
import qualified FujiAF.FirstStage as F
import System.Directory (createDirectoryIfMissing, doesFileExist, listDirectory, removeFile, renameFile)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))

-- ---------------------------------------------------------------------------
-- Options

-- | One compared lane; an owned lane names its linked symbol table, which resolves the owned
-- identities the payload contract declares.
data LaneSpec = LaneSpec {laneName :: String, laneResult :: FilePath, laneCensus :: FilePath, laneInputs :: FilePath, laneSymbols :: Maybe FilePath}

data Options = Options
  { optionRunner :: FilePath
  , optionId5 :: FilePath
  , optionId4 :: FilePath
  , optionLoader :: FilePath
  , optionSites :: FilePath
  , optionWork :: FilePath
  , optionOut :: FilePath
  , optionLanes :: [LaneSpec]
  , optionChain :: Maybe String
  , optionStubs :: [(Word32, Word32)]
  , optionRounds :: Int
  , optionDiscoverData :: Bool
  , optionClosure :: Maybe FilePath
  , optionServices :: [Word32]
  , optionCoverage :: Bool
  , optionFields :: FilePath
  , optionQualification :: Maybe (FilePath, FilePath)
  }

usage :: String
usage = "usage: saclac-cycle-step --runner RUNNER --id5 IMAGE5 --id4 IMAGE4 --loader LOADER_A --sites SITES --fields PAYLOAD_FIELDS --work DIR --out JSON --lane NAME:RESULT_JSON:CENSUS_JSON:INPUTS_DIR[:SYMBOLS] [--lane ...] [--chain NAME] [--stub ADDRESS=VALUE]... [--record-service ADDRESS]... [--coverage] [--qualification BIN:SYMBOLS] [--rounds N] [--discover-data] [--closure FILE]"

parseOptions :: [String] -> IO Options
parseOptions arguments = go empty arguments
  where
    empty = Options "" "" "" "" "" "" "" [] Nothing [] 6 False Nothing [] False "" Nothing
    go options ("--runner" : value : rest) = go options {optionRunner = value} rest
    go options ("--id5" : value : rest) = go options {optionId5 = value} rest
    go options ("--id4" : value : rest) = go options {optionId4 = value} rest
    go options ("--loader" : value : rest) = go options {optionLoader = value} rest
    go options ("--sites" : value : rest) = go options {optionSites = value} rest
    go options ("--fields" : value : rest) = go options {optionFields = value} rest
    go options ("--work" : value : rest) = go options {optionWork = value} rest
    go options ("--out" : value : rest) = go options {optionOut = value} rest
    go options ("--chain" : value : rest) = go options {optionChain = Just value} rest
    go options ("--rounds" : value : rest) = go options {optionRounds = read value} rest
    go options ("--closure" : value : rest) = go options {optionClosure = Just value} rest
    go options ("--record-service" : value : rest) = go options {optionServices = optionServices options ++ [fromIntegral (readAddress value)]} rest
    go options ("--coverage" : rest) = go options {optionCoverage = True} rest
    go options ("--qualification" : value : rest) = case splitOn ':' value of
      [bin, symbols] -> go options {optionQualification = Just (bin, symbols)} rest
      _ -> failH "--qualification takes BIN:SYMBOLS"
    go options ("--discover-data" : rest) = go options {optionDiscoverData = True} rest
    go options ("--stub" : value : rest) = case break (== '=') value of
      (address, '=' : number) -> go options {optionStubs = optionStubs options ++ [(fromIntegral (readAddress address), fromIntegral (readAddress number))]} rest
      _ -> failH "--stub takes ADDRESS=VALUE"
    go options ("--lane" : value : rest) = case splitOn ':' value of
      [name, result, census, inputs] -> go options {optionLanes = optionLanes options ++ [LaneSpec name result census inputs Nothing]} rest
      [name, result, census, inputs, symbols] -> go options {optionLanes = optionLanes options ++ [LaneSpec name result census inputs (Just symbols)]} rest
      _ -> failH "--lane takes NAME:RESULT_JSON:CENSUS_JSON:INPUTS_DIR[:SYMBOLS]"
    go _ (other : _) = failH ("unknown option " ++ other)
    go options [] = do
      forM_ [("--runner", optionRunner options), ("--id5", optionId5 options), ("--id4", optionId4 options), ("--loader", optionLoader options), ("--sites", optionSites options), ("--fields", optionFields options), ("--work", optionWork options), ("--out", optionOut options)] $ \(name, value) ->
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

-- | The pinned code images: image 5, image 4 and loader A at 0x00040000, whose kernel byte
-- copy 0x0004357c the step calls.
data Images = Images {image5 :: B.ByteString, image4 :: B.ByteString, image0 :: B.ByteString}

loaderBase :: Word32
loaderBase = 0x00040000

imageWordAt :: Word32 -> B.ByteString -> Word32 -> Maybe Word32
imageWordAt base image address
  | address >= base && fromIntegral (address - base) + 4 <= B.length image = Just (wordAt image (fromIntegral (address - base)))
  | otherwise = Nothing

-- | The direct branch target of the instruction at a guest address, when the image holds it.
branchTarget :: Images -> Word32 -> Maybe Word32
branchTarget images address
  | Just word <- imageWordAt id5Base (image5 images) address = decode address word
  | Just word <- imageWordAt id4Base (image4 images) address = decode address word
  | Just word <- imageWordAt loaderBase (image0 images) address = decode address word
  | otherwise = Nothing
  where
    decode pc word
      | word .&. 0x0E000000 == 0x0A000000 =
          let immediate = word .&. 0x00FFFFFF
              offset = if immediate .&. 0x00800000 /= 0 then fromIntegral immediate - 0x1000000 else fromIntegral immediate
           in Just (fromIntegral (fromIntegral pc + 8 + 4 * offset :: Integer) :: Word32)
      | otherwise = Nothing

imageSlice :: Word32 -> B.ByteString -> Window -> Either String B.ByteString
imageSlice base image (Window low size)
  | low < base = Left ("window starts before its image at " ++ hexText (fromIntegral low))
  | fromIntegral (low - base) + fromIntegral size > B.length image = Left ("window ends after its image at " ++ hexText (fromIntegral low))
  | otherwise = Right (B.take (fromIntegral size) (B.drop (fromIntegral (low - base)) image))

-- ---------------------------------------------------------------------------
-- Lane inputs

data Lane = Lane
  { laneSpec :: LaneSpec
  , laneDumps :: [Dump]
  , laneEvents :: [TimedEvent]
  , laneFrameWrites :: [Integer]
  , laneError :: Maybe String
  , laneRole :: Payload.Role
  , laneSymbolsDigest :: Maybe String
  , laneOwnedSymbols :: Maybe [Symbol]
  }

loadLane :: LaneSpec -> IO Lane
loadLane spec = do
  (role, digest, owned) <- case laneSymbols spec of
    Nothing -> pure (Payload.StockRole, Nothing, Nothing)
    Just path -> do
      bytes <- readRegular (16 * 1024 * 1024) path
      symbols <- either failH pure (F.parseSymbols bytes)
      rows <- either failH pure (parseSymbols (BC.unpack bytes))
      pure (Payload.OwnedRole (\name -> F.symbolAddress <$> F.lookupSymbol symbols (T.unpack name)), Just (sha256Hex (sha256 bytes)), Just rows)
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
    , laneRole = role
    , laneSymbolsDigest = digest
    , laneOwnedSymbols = owned
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

-- | The closure the stock step needs: the mapped image windows, the discovered call targets
-- (each a declared function start) and the captured-state granules it touches. It only grows;
-- one closure serves every cycle and lane and `--closure FILE` keeps it across runs for the
-- same pinned images. `closureScratch` lists the zero-filled pages a `--discover-data` run maps
-- where the capture has no bytes; only such a run maps them, and it never yields a verdict.
data Closure = Closure
  { closureWindows5 :: [Window]
  , closureWindows4 :: [Window]
  , closureWindows0 :: [Window]
  , closureSplits :: [Word32]
  , closureData :: [Word32]
  , closureScratch :: [Word32]
  }
  deriving (Eq)

initialClosure :: Closure
initialClosure = Closure initialWindows5 initialWindows4 [] [] [] []

-- | Captured state is mapped in 64 KiB granules: the case map holds at most 8 MiB, less
-- than one dump.
granuleBytes :: Word32
granuleBytes = 0x10000

granuleOf :: Integer -> Word32
granuleOf address = fromIntegral address .&. complement (granuleBytes - 1)

mappedLimit :: Integer
mappedLimit = 8 * 1024 * 1024

regionLimit :: Int
regionLimit = 128

data PlanInput = PlanInput
  { planState :: State
  , planSeeds :: [Integer]
  , planMessage :: (Int, Int, [Word8])
  , planClosure :: Closure
  , planId :: String
  , planExtraStubs :: [(Word32, Word32)]
  , planServices :: [Word32]
  , planCoverage :: Bool
  }

exitAddress :: Word32
exitAddress = 0x00511000

entryFunctionSize :: Word32
entryFunctionSize = 0x80

-- | The mapped runs of captured state: the closure's granules and the plan's seed addresses,
-- adjacent granules of one segment merged, each clipped to its segment.
dataRuns :: PlanInput -> [(Word32, Word32)]
dataRuns input = foldr merge [] (mapMaybe clip granules)
  where
    state = planState input
    granules = nub (sortOn id (closureData (planClosure input) ++ map granuleOf (planSeeds input)))
    clip granule = case [segment | segment <- stateSegments state, stateHolds (State [segment]) (fromIntegral granule)] of
      segment : _ ->
        let end = min (fromIntegral granule + fromIntegral granuleBytes) (segmentBase segment + fromIntegral (B.length (segmentBytes segment)))
         in Just (granule, fromIntegral end - granule)
      [] -> Nothing
    merge (base, size) ((next, nextSize) : rest)
      | base + size == next && sameSegment base next = (base, size + nextSize) : rest
    merge run rest = run : rest
    sameSegment low high = any (\segment -> stateHolds (State [segment]) (fromIntegral low) && stateHolds (State [segment]) (fromIntegral high)) (stateSegments state)

dataRegionId :: Word32 -> String
dataRegionId base = "data-" ++ drop 2 (hexText (fromIntegral base))

regionsOf :: Images -> PlanInput -> Either String [(RegionPlan, Maybe B.ByteString)]
regionsOf images input = do
  windows5 <- mapM (windowPlan (image5 images) id5Base "id5") (zip [0 :: Int ..] (closureWindows5 (planClosure input)))
  windows4 <- mapM (windowPlan (image4 images) id4Base "id4") (zip [0 :: Int ..] (clearHarness (planServices input) (closureWindows4 (planClosure input))))
  windows0 <- mapM (windowPlan (image0 images) loaderBase "ld") (zip [0 :: Int ..] (closureWindows0 (planClosure input)))
  state <- mapM dataRegion (dataRuns input)
  let regions =
        [ (RegionPlan "record" recordBase (fromIntegral recordSize) "rw" Nothing, Nothing)
        , (RegionPlan "exit" exitAddress 4 "rx" Nothing, Nothing)
        , (RegionPlan "stack" stackBase (fromIntegral stackSize) "rw" Nothing, Nothing)
        , (RegionPlan "tramp" trampolineBase (fromIntegral trampolineSize) "rx" Nothing, Just trampolineBytes)
        ]
          ++ [ (RegionPlan ("svc-" ++ drop 2 (hexText (fromIntegral entry))) entry (fromIntegral (4 * length (serviceRecorder entry))) "rx" Nothing, Just (B.concat (map (leWord . fromIntegral) (serviceRecorder entry))))
             | entry <- planServices input
             ]
          ++ state
          ++ [ (RegionPlan ("scratch-" ++ show index) page 0x1000 "rw" Nothing, Nothing)
             | (index, page) <- zip [0 :: Int ..] (sortOn id (filter (not . stateHolds (planState input) . fromIntegral) (closureScratch (planClosure input))))
             ]
          ++ windows5
          ++ windows4
          ++ windows0
      mapped = sum [fromIntegral (regionSize region) | (region, _) <- regions] :: Integer
  if mapped > mappedLimit || length regions > regionLimit
    then Left ("the case exceeds resource8: " ++ show mapped ++ " mapped bytes in " ++ show (length regions) ++ " regions")
    else Right regions
  where
    trampolineBytes = B.concat (map leWord (map fromIntegral trampoline))
    windowPlan image base tag (index, window) = do
      bytes <- imageSlice base image window
      pure (RegionPlan (tag ++ "-" ++ show index) (windowBase window) (windowSize window) "rx" Nothing, Just bytes)
    dataRegion (base, size) = case stateSlice (planState input) (fromIntegral base) (fromIntegral size) of
      Just bytes -> Right (RegionPlan (dataRegionId base) base size "rw" Nothing, Just bytes)
      Nothing -> Left ("state granule " ++ hexText (fromIntegral base) ++ " is not captured")

-- | The input state with every mapped state region's final bytes written over it.
predictedState :: PlanInput -> RunResult -> Either String State
predictedState input run = foldl' apply (Right (planState input)) (dataRuns input)
  where
    apply acc (base, size) = do
      state <- acc
      case M.lookup (dataRegionId base) (runMemory run) of
        Just bytes | B.length bytes == fromIntegral size -> overlayState state (fromIntegral base) bytes
        _ -> Left ("the runner did not report state region " ++ dataRegionId base)

-- | Every window cut at the entry, the end of the entry's own partition and each discovered
-- call target, so every call the step makes lands on a declared function start. A stub's
-- word is cut out: the runner admits no stub inside a declared function.
functionsOf :: PlanInput -> [(Word32, Word32)]
functionsOf input =
  filter ((`notElem` stubs) . fst) (concatMap partitions (closureWindows5 closure ++ clearHarness (planServices input) (closureWindows4 closure) ++ closureWindows0 closure))
    ++ [(trampolineBase, fromIntegral trampolineSize)]
    ++ [(entry, fromIntegral (4 * length (serviceRecorder entry))) | entry <- planServices input]
  where
    closure = planClosure input
    stubs = taskIdAddress : receiveAddress : map fst (planExtraStubs input)
    cuts = entryAddress : entryAddress + entryFunctionSize : closureSplits closure ++ stubs ++ map (+ 4) stubs
    partitions window =
      let inside = nub (sortOn id [cut | cut <- cuts, windowContains window cut, cut /= windowBase window])
          starts = windowBase window : inside
          ends = inside ++ [windowBase window + windowSize window]
       in zip starts (zipWith (-) ends starts)

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

-- | Region bytes packed into three artifacts, captured state, code and the trampoline: a case
-- names at most 20 artifacts, and a region reads its bytes at an offset into one.
packArtifacts :: [(RegionPlan, Maybe B.ByteString)] -> ([RegionPlan], [(String, B.ByteString)])
packArtifacts regions = (reverse placed, [(name, B.concat (reverse parts)) | (name, parts) <- M.toList packed])
  where
    (placed, packed) = foldl' place ([], M.empty) regions
    place (done, artifacts) (region, Nothing) = (region {regionArtifact = Nothing} : done, artifacts)
    place (done, artifacts) (region, Just bytes) =
      let name = artifactOf (regionId region)
          parts = M.findWithDefault [] name artifacts
          offset = fromIntegral (sum (map B.length parts))
       in (region {regionArtifact = Just (name, offset)} : done, M.insert name (bytes : parts) artifacts)
    artifactOf ident
      | take 5 ident == "data-" = "state"
      | ident == "tramp" = "tramp"
      | otherwise = "code"

planCase :: Images -> PlanInput -> Either String (CasePlan, [(String, B.ByteString)])
planCase images input = do
  (regions, artifacts) <- packArtifacts <$> regionsOf images input
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
        , casePlanRegions = regions
        , casePlanFunctions = functionsOf input
        , casePlanStubs =
            [ ScriptStub taskIdAddress (replicate 32 (StubCall [] [("r0", 43)] []))
            , ScriptStub receiveAddress [receiveCall (planMessage input)]
            ]
              ++ [ScriptStub address (replicate extraCalls (StubCall [] [("r0", value)] [])) | (address, value) <- planExtraStubs input]
        , casePlanArtifacts = artifacts
        , casePlanBudget = 16000000
        , casePlanCoverage = planCoverage input
        }
    , artifacts
    )
  where
    -- A case's stubs hold at most 128 script calls; the task-id stub takes 32 and receive 1.
    extraCalls = (128 - 33) `div` max 1 (length (planExtraStubs input))

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
  , runCoverage :: M.Map String (B.ByteString, B.ByteString, B.ByteString)
  , runRegionBases :: M.Map String Word32
  , runPc :: Word32
  , runR0 :: Word32
  , runAccesses :: [Value]
  , runExecuted :: [Value]
  , runCaseSha :: String
  , runRunnerSha :: String
  , runProvenance :: [Value]
  , runProvenanceTotal :: Integer
  , runTrace :: [Value]
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
              , runCoverage = M.fromList [(regionName ident, (coverageRead c, coverageWritten c, coverageExposed c)) | (ident, c) <- fromMaybe [] (nativeCoverage observation)]
              , runRegionBases = M.fromList [(regionId region, regionBase region) | region <- casePlanRegions plan]
              , runPc = fromMaybe 0 (lookupRegister Pc observation)
              , runR0 = fromMaybe 0 (lookupRegister R0 observation)
              , runAccesses = list (get runner "accesses")
              , runExecuted = list (get runner "executed")
              , runCaseSha = digest
              , runRunnerSha = str (get report "runner_sha256")
              , runProvenance = list (get runner "memory_provenance")
              , runProvenanceTotal = intOf (get runner "memory_provenance_total")
              , runTrace = list (get runner "trace")
              })

-- ---------------------------------------------------------------------------
-- The abstraction map

-- | The qualification ELF: Root's declared stock encoding, built beside Root's types with
-- `--cfg saclac_qualification` and never linked into a production image. It reads an owned
-- lane's Root state with the layouts of the exact qualified build (the DWARF layout check
-- binds them) and writes the stock-layout overlay its field table declares.
data Qualification = Qualification
  { qualImage :: B.ByteString
  , qualBase :: Word32
  , qualTextEnd :: Word32
  , qualEnd :: Word32
  , qualEncode :: Word32
  , qualRoundtrip :: Word32
  , qualFields :: [Field]
  , qualFunctions :: [(Word32, Word32)]
  , qualSha :: String
  , qualSymbolsSha :: String
  }

-- | Harness scratch for one map call: the input overlay at +0, the output at +0x1000 and the
-- refusal detail word at +0x1ff0.
qualIoBase :: Word32
qualIoBase = 0x02100000

qualIoSize :: Word32
qualIoSize = 0x2000

loadQualification :: (FilePath, FilePath) -> IO Qualification
loadQualification (binPath, symbolsPath) = do
  image <- readRegular (1024 * 1024) binPath
  symbolBytes <- readRegular (16 * 1024 * 1024) symbolsPath
  symbols <- either failH pure (parseSymbols (BC.unpack symbolBytes))
  let named name = either failH (pure . symbolAddress) (symbolNamed symbols name)
  base <- named "__xm1_qualification_start"
  textEnd <- named "__xm1_qualification_text_end"
  imageEnd <- named "__xm1_qualification_image_end"
  end <- named "__xm1_qualification_end"
  encode <- named "xm1_saclac_qualification_encode"
  roundtrip <- named "xm1_saclac_qualification_roundtrip"
  count <- named "XM1_SACLAC_QUALIFICATION_FIELD_COUNT"
  table <- named "XM1_SACLAC_QUALIFICATION_FIELDS"
  ensure (base == 0x02000000 && base < textEnd && textEnd <= imageEnd && imageEnd <= end && end <= qualIoBase && textEnd .&. 0xfff == 0)
    "the qualification image leaves its declared window"
  ensure (fromIntegral (B.length image) == imageEnd - base) "the qualification .bin does not span its image symbols"
  fields <- either failH pure (parseFields base image count table)
  let starts = [symbolAddress symbol | symbol <- symbols, symbolType symbol `elem` ("tT" :: String), symbolAddress symbol >= base, symbolAddress symbol < textEnd]
      cuts = nub (sortOn id (base : starts))
      functions = zip cuts (zipWith (-) (drop 1 cuts ++ [textEnd]) cuts)
  ensure (encode `elem` cuts && roundtrip `elem` cuts) "the qualification entries are not function starts"
  pure Qualification
    { qualImage = image
    , qualBase = base
    , qualTextEnd = textEnd
    , qualEnd = end
    , qualEncode = encode
    , qualRoundtrip = roundtrip
    , qualFields = fields
    , qualFunctions = functions
    , qualSha = sha256Hex (sha256 image)
    , qualSymbolsSha = sha256Hex (sha256 symbolBytes)
    }

-- | Encode reads Root's state at the `OWNER`/`READY` addresses of the lane's build;
-- round trip decodes a stock overlay into Root's typed state and encodes it back.
data QualCall = QualEncode Word32 Word32 | QualRoundtrip B.ByteString

data QualRun = QualRun
  { qualCode :: Word32
  , qualGap :: Word32
  , qualOut :: B.ByteString
  , qualReads :: [Integer]
  , qualInstructions :: Integer
  , qualCaseSha :: String
  }

-- | The 64 KiB granule of captured state around an address, clipped to its dumped range.
granuleAround :: State -> Integer -> Maybe (Word32, Word32)
granuleAround state address = case [segment | segment <- stateSegments state, stateHolds (State [segment]) address] of
  segment : _ ->
    let granule = fromIntegral (granuleOf address) :: Integer
        low = max granule (segmentBase segment)
        high = min (granule + fromIntegral granuleBytes) (segmentBase segment + fromIntegral (B.length (segmentBytes segment)))
     in Just (fromIntegral low, fromIntegral (high - low))
  [] -> Nothing

-- | One map call in its own native case. Captured state is mapped by granule on demand, as
-- the step's is; the run's complete coverage gives the state it read.
runQualification :: Options -> Qualification -> String -> State -> QualCall -> IO (Either String QualRun)
runQualification options qual ident state call = go (0 :: Int) (mapMaybe (granuleAround state) seeds)
  where
    seeds = case call of
      QualEncode owner ready -> [fromIntegral owner, fromIntegral ready]
      QualRoundtrip _ -> []
    input = case call of
      QualRoundtrip bytes -> bytes
      QualEncode _ _ -> B.empty
    entry = case call of
      QualEncode _ _ -> qualEncode qual
      QualRoundtrip _ -> qualRoundtrip qual
    arguments = case call of
      QualEncode owner ready -> [owner, ready, qualIoBase + 0x1000, qualIoBase + 0x1ff0]
      QualRoundtrip _ -> [qualIoBase, qualIoBase + 0x1000, qualIoBase + 0x1ff0, 0]
    textSize = qualTextEnd qual - qualBase qual
    dataSize = qualEnd qual - qualTextEnd qual
    dataBytes = B.drop (fromIntegral textSize) (qualImage qual)
    go round' granules = case mapM granuleRegion (nub (sortOn id granules)) of
      Left problem -> pure (Left problem)
      Right stateRegions -> do
        let regions =
              [ (RegionPlan "q-text" (qualBase qual) textSize "rx" Nothing, Just (B.take (fromIntegral textSize) (qualImage qual)))
              , (RegionPlan "q-data" (qualTextEnd qual) dataSize "rw" Nothing, Just (dataBytes <> B.replicate (fromIntegral dataSize - B.length dataBytes) 0))
              , (RegionPlan "q-io" qualIoBase qualIoSize "rw" Nothing, Just (input <> B.replicate (fromIntegral qualIoSize - B.length input) 0))
              , (RegionPlan "exit" exitAddress 4 "rx" Nothing, Nothing)
              , (RegionPlan "stack" stackBase (fromIntegral stackSize) "rw" Nothing, Nothing)
              ]
                ++ stateRegions
            (placed, artifacts) = packArtifacts regions
            plan = CasePlan
              { casePlanId = ident
              , casePlanProvenance = "Root's declared stock encoding (qualification ELF " ++ qualSha qual ++ ", never linked into a production image) run on a lane's captured state."
              , casePlanExclusions = ["The qualification code is not firmware; it reads the captured state read-only and writes only its own data and the harness scratch at 0x02100000."]
              , casePlanEntry = entry
              , casePlanExit = exitAddress
              , casePlanRegisters =
                  zip ["r0", "r1", "r2", "r3"] arguments
                    ++ [(register, 0) | register <- ["r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11", "r12"]]
                    ++ [("sp", harnessSp), ("lr", exitAddress), ("cpsr", 0x10)]
              , casePlanRegions = placed
              , casePlanFunctions = qualFunctions qual
              , casePlanStubs = []
              , casePlanArtifacts = artifacts
              , casePlanBudget = 4000000
              , casePlanCoverage = True
              }
        result <- runPlanned options plan
        case result of
          Left problem -> pure (Left problem)
          Right run -> case runStatus run of
            "returned" -> pure $ case M.lookup "q-io" (runMemory run) of
              Just io | B.length io == fromIntegral qualIoSize -> Right QualRun
                { qualCode = runR0 run
                , qualGap = wordAt io 0x1ff0
                , qualOut = B.take (fieldsBytes (qualFields qual)) (B.drop 0x1000 io)
                , qualReads = coverageRead' (coverageAddresses isStateRegion run)
                , qualInstructions = runInstructions run
                , qualCaseSha = runCaseSha run
                }
              _ -> Left "the runner did not report the map's scratch region"
            "memory_guard" -> case reverse (runAccesses run) of
              row : _ ->
                let address = (addressOf (get row "address")) .&. complement 0x40000000
                 in case granuleAround state address of
                      Just granule
                        | granule `notElem` granules && round' < 64 -> go (round' + 1) (granule : granules)
                      _ -> pure (Left ("the map refused an access at " ++ hexText address ++ " outside the captured state"))
              [] -> pure (Left "the map hit memory_guard without a recorded access")
            other -> pure (Left ("the map ended " ++ other ++ " at " ++ hexText (fromIntegral (runPc run)) ++ ": " ++ runDetail run))
    granuleRegion (base, size) = case stateSlice state (fromIntegral base) (fromIntegral size) of
      Just bytes -> Right (RegionPlan (dataRegionId base) base size "rw" Nothing, Just bytes)
      Nothing -> Left ("state granule " ++ hexText (fromIntegral base) ++ " is not captured")

-- | What one cycle compares after the map: the step's input, the completion state it is held
-- against, and whether stock-private scratch outside the fields leaves the comparison.
data Mapping = Mapping {mappingInput :: State, mappingObserved :: State, mappingAbstract :: Bool, mappingValue :: Value}

-- | The map for one cycle. Without `--qualification` the lane's states are compared as
-- captured. The stock lane is the map's validation: its wake and completion fields go
-- through decode and encode, and the step runs from the round-tripped wake state. An owned
-- lane's wake and completion Root state are encoded into the stock fields.
mapCycle :: Options -> Maybe Qualification -> [Span] -> Lane -> Int -> State -> State -> State -> IO (Either String Mapping)
mapCycle options qualification shared lane index wake input completion = case qualification of
  Nothing -> pure (Right (Mapping input completion False Null))
  Just qual -> case laneOwnedSymbols lane of
    Nothing -> stockRoundtrip qual
    Just symbols -> ownedEncode qual symbols
  where
    ident suffix = "map-" ++ laneName (laneSpec lane) ++ "-" ++ show index ++ "-" ++ suffix
    refused label run
      | qualCode run == 0 = Right ()
      | otherwise = Left ("refused: the map's " ++ label ++ " call returned " ++ show (qualCode run) ++ " for field " ++ show (qualGap run `shiftR` 16) ++ " detail " ++ show (qualGap run .&. 0xffff))
    stockRoundtrip qual = case (extractOverlay (qualFields qual) input, extractOverlay (qualFields qual) completion) of
      (Right before, Right after) -> do
        wakeRun <- runQualification options qual (ident "wake") input (QualRoundtrip before)
        completionRun <- runQualification options qual (ident "completion") completion (QualRoundtrip after)
        pure $ do
          wakeMap <- wakeRun
          completionMap <- completionRun
          refused "wake round trip" wakeMap
          refused "completion round trip" completionMap
          input' <- applyOverlay (qualFields qual) (qualOut wakeMap) input
          pure Mapping
            { mappingInput = input'
            , mappingObserved = completion
            , mappingAbstract = False
            , mappingValue = obj
                [ ("mode", s "stock-round-trip")
                , ("wake_identical", b (qualOut wakeMap == before))
                , ("completion_identical", b (qualOut completionMap == after))
                , ("wake_changed", rangesValue (overlayChanges qual before (qualOut wakeMap)))
                , ("completion_changed", rangesValue (overlayChanges qual after (qualOut completionMap)))
                , ("instructions", arr [n (qualInstructions wakeMap), n (qualInstructions completionMap)])
                , ("case_sha256", arr [s (qualCaseSha wakeMap), s (qualCaseSha completionMap)])
                ]
            }
      (Left problem, _) -> pure (Left problem)
      (_, Left problem) -> pure (Left problem)
    ownedEncode qual symbols = case (,) <$> symbolPath symbols rootOwner <*> symbolPath symbols rootReady of
      Left problem -> pure (Left problem)
      Right (owner, ready) -> do
        let call = QualEncode (symbolAddress owner) (symbolAddress ready)
            rootData = rootDataSpan symbols
        wakeRun <- runQualification options qual (ident "wake") wake call
        completionRun <- runQualification options qual (ident "completion") completion call
        pure $ do
          wakeMap <- wakeRun
          completionMap <- completionRun
          refused "wake encode" wakeMap
          refused "completion encode" completionMap
          let outside = [address | address <- qualReads wakeMap ++ qualReads completionMap, not (any (`spanAt` address) (rootData : shared))]
          if null outside
            then Right ()
            else Left ("refused: the map read outside Root's data and the shared spans: " ++ unwords [hexText low ++ ".." ++ hexText high | (low, high) <- take 16 (ranges (nub (sortOn id outside)))])
          input' <- applyOverlay (qualFields qual) (qualOut wakeMap) input
          observed' <- applyOverlay (qualFields qual) (qualOut completionMap) completion
          pure Mapping
            { mappingInput = input'
            , mappingObserved = observed'
            , mappingAbstract = True
            , mappingValue = obj
                [ ("mode", s "owned-encode")
                , ("owner", s (hexText (fromIntegral (symbolAddress owner))))
                , ("root_data", arr [s (hexText (spanLow rootData)), s (hexText (spanHigh rootData))])
                , ("wake_overlay_sha256", s (sha256Hex (sha256 (qualOut wakeMap))))
                , ("completion_overlay_sha256", s (sha256Hex (sha256 (qualOut completionMap))))
                , ("wake_machine_states", machineStates input')
                , ("completion_machine_states", machineStates observed')
                , ("map_read", rangesValue (nub (sortOn id (qualReads wakeMap ++ qualReads completionMap))))
                , ("instructions", arr [n (qualInstructions wakeMap), n (qualInstructions completionMap)])
                , ("case_sha256", arr [s (qualCaseSha wakeMap), s (qualCaseSha completionMap)])
                ]
            }
    machineStates state = obj [(show machine, maybe Null (n . fromIntegral) (halfword state (0x0169d4b4 + 2 * machine))) | machine <- [0, 1, 8, 11, 12, 13, 24, 29 :: Integer]]
    halfword state address = (\bytes -> fromIntegral (B.index bytes 0) + 256 * fromIntegral (B.index bytes 1) :: Integer) <$> stateSlice state address 2

rootOwner, rootReady :: [String]
rootOwner = ["xm1_saclac_root", "stock", "composition", "OWNER"]
rootReady = ["xm1_saclac_root", "stock", "composition", "READY"]

-- | Root's data: every data and bss symbol of the lane's own image.
rootDataSpan :: [Symbol] -> Span
rootDataSpan symbols = Span (minimum (0xffffffff : map fst rows)) (maximum (0 : map snd rows))
  where
    rows = [(fromIntegral (symbolAddress symbol), fromIntegral (symbolAddress symbol) + fromIntegral size) | symbol <- symbols, symbolType symbol `elem` ("bBdD" :: String), Just size <- [symbolSize symbol]]

-- | The stock addresses whose overlay byte the round trip changed.
overlayChanges :: Qualification -> B.ByteString -> B.ByteString -> [Integer]
overlayChanges qual before after =
  [ address
  | (address, old, new) <- zip3 (concat [[fieldAddress field .. fieldAddress field + fromIntegral (fieldLength field) - 1] | field <- qualFields qual]) (B.unpack before) (B.unpack after)
  , old /= new
  ]

-- | The shared closure and how a growth is kept: in memory, and in `--closure FILE` when given.
data ClosureStore = ClosureStore {storeRef :: IORef Closure, storeSave :: Closure -> IO ()}

grow :: ClosureStore -> Closure -> IO ()
grow store closure = writeIORef (storeRef store) closure >> storeSave store closure

-- | Runs one cycle, growing the shared closure whenever the runner refuses a fetch, a data
-- access or a call outside it, up to the declared round limit. Every growth is kept at once,
-- so a cycle that fails later still leaves its discovery behind. Only a discovery run maps
-- scratch pages.
runCycle :: Options -> Images -> ClosureStore -> PlanInput -> Word32 -> IO (Either String (RunResult, Int))
runCycle options images store input sp = go 0
  where
    go round' = do
      closure <- readIORef (storeRef store)
      let planned = input {planClosure = if optionDiscoverData options then closure else closure {closureScratch = []}}
      case planCase images planned of
        Left problem -> pure (Left problem)
        Right (plan, _) -> do
          result <- runPlanned options (withStack sp plan)
          case result of
            Left problem -> pure (Left problem)
            Right run -> case extension (optionDiscoverData options) images planned closure run of
              Nothing -> pure (Right (run, round'))
              Just (Left problem) -> pure (Left problem)
              Just (Right closure')
                | round' >= optionRounds options ->
                    pure (Left ("closure did not close after " ++ show round' ++ " rounds: " ++ runStatus run ++ " at " ++ hexText (fromIntegral (runPc run)) ++ " " ++ runDetail run))
                | otherwise -> grow store closure' >> go (round' + 1)

-- | The closure growth a runner refusal asks for, or Nothing when the run ended. A refused data
-- access is resolved through the bit-30 uncached alias to the address it names: inside the
-- captured state it maps that granule; inside the pinned images it maps the surrounding image
-- pages; with `discover`, anywhere else it maps a zero-filled scratch page. Mapped regions
-- carry both views. An unknown call stops at the call instruction: its direct target inside
-- the pinned images becomes a declared function start, mapped first when outside the windows;
-- any other target needs a declared stub. A refusal inside memory already mapped is an error,
-- never another round.
extension :: Bool -> Images -> PlanInput -> Closure -> RunResult -> Maybe (Either String Closure)
extension discover images input closure run = case runStatus run of
  "fetch_guard" -> Just (extend (runPc run))
  "memory_guard" -> case reverse (runAccesses run) of
    row : _ ->
      let address = fromIntegral (addressOf (get row "address")) .&. complement 0x40000000
          granule = granuleOf (fromIntegral address)
          page = address .&. 0xfffff000
       in Just $
            if stateHolds (planState input) (fromIntegral address)
              then if any (\(base, size) -> base <= address && address < base + size) (dataRuns input)
                then Left ("refused access at " ++ hexText (fromIntegral address) ++ " inside a mapped state granule")
                else Right closure {closureData = granule : closureData closure}
              else if inImage5 address || inImage4 address || inImage0 address
                then extend address
                else if discover
                  then if page `elem` closureScratch closure
                    then Left ("refused access at " ++ hexText (fromIntegral address) ++ " inside a scratch page")
                    else Right closure {closureScratch = page : closureScratch closure}
                  else Left ("refused access at " ++ hexText (fromIntegral address) ++ " outside the captured state and the pinned images")
    [] -> Just (Left "memory_guard without a recorded access")
  "unknown_call" -> Just $ case branchTarget images (runPc run) of
    Just target
      | not (inImage5 target || inImage4 target || inImage0 target) ->
          Left ("call at " ++ hexText (fromIntegral (runPc run)) ++ " to " ++ hexText (fromIntegral target) ++ " outside the pinned images: declare --stub")
      | target `elem` closureSplits closure ->
          Left ("unknown call at " ++ hexText (fromIntegral (runPc run)) ++ " to declared start " ++ hexText (fromIntegral target))
      | otherwise -> (\grown -> grown {closureSplits = target : closureSplits grown}) <$> (if mapped target then Right closure else extend target)
    Nothing -> Left ("unknown control transfer at " ++ hexText (fromIntegral (runPc run)) ++ " " ++ runDetail run)
  _ -> Nothing
  where
    mapped target = any (`windowContains` target) (closureWindows5 closure ++ closureWindows4 closure ++ closureWindows0 closure)
    extend address
      | mapped address = Left ("refused access at " ++ hexText (fromIntegral address) ++ " inside a mapped code window")
      | inImage5 address = Right closure {closureWindows5 = mergeWindows (growWindow id5Base (fromIntegral (B.length (image5 images))) address (closureWindows5 closure))}
      | inImage4 address = Right closure {closureWindows4 = clearTrampoline (mergeWindows (growWindow id4Base (fromIntegral (B.length (image4 images))) address (closureWindows4 closure)))}
      | inImage0 address = Right closure {closureWindows0 = mergeWindows (growWindow loaderBase (fromIntegral (B.length (image0 images))) address (closureWindows0 closure))}
      | otherwise = Left ("refused access at " ++ hexText (fromIntegral address) ++ " outside the pinned images")
    inImage5 address = address >= id5Base && fromIntegral address < fromIntegral id5Base + B.length (image5 images)
    inImage4 address = address >= id4Base && fromIntegral address < fromIntegral id4Base + B.length (image4 images)
    inImage0 address = address >= loaderBase && fromIntegral address < fromIntegral loaderBase + B.length (image0 images)
    -- Four KiB either side: the case map holds at most 8 MiB.
    growWindow :: Word32 -> Int -> Word32 -> [Window] -> [Window]
    growWindow base length' address windows =
      let low = max base ((address - 0x1000) .&. 0xfffff000)
          high = min (base + fromIntegral length') ((address + 0x2000) .&. 0xfffff000)
       in Window low (high - low) : windows

-- | The recording trampoline owns the kernel post bytes; no image window may map them.
clearTrampoline :: [Window] -> [Window]
clearTrampoline = clearHarness []

-- | No image window may map the bytes a harness trampoline or a service recorder owns.
clearHarness :: [Word32] -> [Window] -> [Window]
clearHarness services windows = foldl' cut windows spans
  where
    spans = (trampolineBase, trampolineBase + fromIntegral trampolineSize) : [(entry, entry + fromIntegral (4 * length (serviceRecorder entry))) | entry <- services]
    cut current (low', high') = concatMap (split low' high') current
    split low' high' window =
      let low = windowBase window
          high = low + windowSize window
       in if high' <= low || high <= low'
            then [window]
            else [Window low (low' - low) | low' > low] ++ [Window high' (high - high') | high > high']

-- ---------------------------------------------------------------------------
-- Comparison

data CycleRun = CycleRun
  { cycleRunCycle :: Cycle
  , cycleRunInput :: State
  , cycleRunPredicted :: State
  , cycleRunObserved :: State
  , cycleRunReport :: CycleReport
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
  , reportNativePc :: Word32
  , reportNativeDetail :: String
  , reportRunnerSha :: String
  , reportAccessTotal :: Integer
  , reportTrace :: [Value]
  , reportStateRead :: [Integer]
  , reportStateWritten :: [Integer]
  , reportImageRead :: [Integer]
  , reportStateExposed :: [Integer]
  , reportNativeInstructions :: Integer
  , reportExecuted :: Integer
  , reportLevelPredicted :: Maybe Word32
  , reportLevelObserved :: Maybe Word32
  , reportCaseSha :: String
  , reportRounds :: Int
  , reportComparedBytes :: Int
  , reportMap :: Value
  }

declaredSpans :: Value -> [Span]
declaredSpans sites =
  [ Span (addressOf (get row "start")) (addressOf (get row "end"))
  | row <- list (get (get sites "interface") "shared")
  ]
    ++ [Span 0x0169d374 0x0169d4fe, Span groupBase 0x00919a20]

-- | The step's outputs the recording trampolines kept in the record region, in call order: a
-- post with its payload as copied at the post, or a recorded service call (queue -1, the entry
-- as its operation) with its first declared argument words as little-endian payload bytes.
postRecords :: M.Map Integer Int -> RunResult -> [(Int, Int, [Word8], String)]
postRecords arity run = case M.lookup "record" (runMemory run) of
  Just record | B.length record >= 4 -> mapMaybe (slot record) [1 .. min maxSlots (fromIntegral (wordAt record 0))]
  _ -> []
  where
    maxSlots = recordSize `div` recordSlotBytes - 1
    slot record index
      | recordSlotBytes * (index + 1) <= B.length record =
          let base = recordSlotBytes * index
              at offset = wordAt record (base + offset)
              header = at 16
              length' = fromIntegral (at 20) :: Int
              copied = min length' recordPayloadBytes
              payload = B.unpack (B.take copied (B.drop (base + 32) record))
              problem = if length' > recordPayloadBytes then "payload of " ++ show length' ++ " bytes exceeds the record slot" else ""
           in Just $ if at 0 == serviceMarker
                then
                  let count = M.findWithDefault 4 (fromIntegral header) arity
                   in (-1, fromIntegral header, take (4 * count) payload, "")
                else (fromIntegral (at 0), fromIntegral (header .&. 0xffff), payload, problem)
      | otherwise = Nothing

-- | A lane's outputs between the wake and the completion dump, in execution order: its posts and
-- its calls to the recorded services.
observedPosts :: [Word32] -> Cycle -> [(Int, Int, [Word8])]
observedPosts services cycle =
  [ output
  | TimedEvent _ event <- sortOn eventInstructions (cyclePosts cycle ++ cycleOthers cycle)
  , output <- case event of
      EventPeerRequest queue operation payload -> [(queue, operation, payload)]
      EventServiceRequest entry arguments
        | fromIntegral entry `elem` services -> [(-1, fromIntegral entry, concatMap (B.unpack . leWord . fromIntegral) arguments)]
      _ -> []
  ]

-- | The word-argument counts the census sites declare for each service-request entry.
serviceArity :: Value -> M.Map Integer Int
serviceArity sites = M.fromList
  [ (addressOf (get row "address"), length [() | argument <- list (get row "arguments"), str (get argument "kind") == "word"])
  | row <- list (get (get sites "interface") "entries")
  , str (get row "kind") == "service-request"
  ]

-- | Posts projected through the payload contract as the interface comparison projects them:
-- the native step's in the stock role, a lane's observed posts in that lane's role.
projectNative :: Payload.Declaration -> [(Int, Int, [Word8], String)] -> [(Int, Int, [Word8], String)]
projectNative declaration = map $ \(queue, operation, payload, problem) ->
  if queue < 0 then (queue, operation, payload, problem) else
  case projectPost Payload.StockRole declaration queue operation payload of
    Right projected -> (queue, operation, projected, problem)
    Left reason -> (queue, operation, payload, if null problem then "native payload: " ++ reason else problem)

projectObserved :: Payload.Role -> Payload.Declaration -> [(Int, Int, [Word8])] -> Either String [(Int, Int, [Word8])]
projectObserved role declaration = mapM $ \(queue, operation, payload) ->
  if queue < 0 then Right (queue, operation, payload) else
  (\projected -> (queue, operation, projected)) <$> projectPost role declaration queue operation payload

projectPost :: Payload.Role -> Payload.Declaration -> Int -> Int -> [Word8] -> Either String [Word8]
projectPost role declaration queue operation =
  Payload.projectPayload role declaration Payload.Outgoing (Payload.QueueId (fromIntegral queue)) (Payload.Operation (fromIntegral operation))

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
  , laneOutputSymbols :: Maybe String
  , laneOutputQualified :: Bool
  , laneOutputCycles :: [Value]
  , laneOutputRefusals :: [Value]
  , laneOutputMetrics :: [Value]
  , laneOutputClosure :: Value
  , laneOutputRuns :: [CycleRun]
  , laneOutputError :: Maybe String
  }

runLane :: Options -> Images -> Maybe Qualification -> [Span] -> M.Map Integer Int -> Payload.Declaration -> ClosureStore -> Lane -> IO LaneOutput
runLane options images qualification spans arity declaration store lane = case laneError lane of
  Just problem -> pure (LaneOutput (laneName (laneSpec lane)) (laneSymbolsDigest lane) False [] [] [] (obj []) [] (Just problem))
  Nothing -> do
    let (cycles, refusals) = pairCycles (laneDumps lane) (laneEvents lane)
        refusalValues = [obj [("cycle", s (refusalWhere refusal)), ("reason", s (refusalReason refusal))] | refusal <- refusals]
    runs <- forM (zip [0 :: Int ..] cycles) $ \(index, cycle) -> (,) index <$> runCycleOf index cycle
    let successes = [run | (_, Right run) <- runs]
        failures = [(index, problem) | (index, Left problem) <- runs]
        metrics = map (metricOf lane) (zip [0 :: Int ..] cycles)
        qualified = null failures && not (null successes) && all (\run -> reportStateEqual (cycleRunReport run) && reportOutputsEqual (cycleRunReport run)) successes
    closure <- readIORef (storeRef store)
    pure LaneOutput
      { laneOutputName = laneName (laneSpec lane)
      , laneOutputSymbols = laneSymbolsDigest lane
      , laneOutputQualified = qualified
      , laneOutputCycles = map (cycleValue . cycleRunReport) successes ++ [obj [("cycle", n (fromIntegral index)), ("error", s problem)] | (index, problem) <- failures]
      , laneOutputRefusals = refusalValues
      , laneOutputMetrics = metrics
      , laneOutputClosure = obj
          [ ("windows", arr ([obj [("image", s "image5"), ("base", s (hexText (fromIntegral (windowBase window)))), ("size", n (fromIntegral (windowSize window)))] | window <- closureWindows5 closure]
              ++ [obj [("image", s "image4"), ("base", s (hexText (fromIntegral (windowBase window)))), ("size", n (fromIntegral (windowSize window)))] | window <- closureWindows4 closure]
              ++ [obj [("image", s "loader-a"), ("base", s (hexText (fromIntegral (windowBase window)))), ("size", n (fromIntegral (windowSize window)))] | window <- closureWindows0 closure]))
          , ("function_starts", arr [s (hexText (fromIntegral start)) | start <- sortOn id (closureSplits closure)])
          , ("state_granules", arr [s (hexText (fromIntegral granule)) | granule <- sortOn id (closureData closure)])
          , ("scratch_pages", arr [s (hexText (fromIntegral page)) | page <- sortOn id (closureScratch closure)])
          , ("executed_bytes", n (sum [reportExecuted (cycleRunReport run) | run <- successes]))
          , ("cycles", n (fromIntegral (length successes)))
          , ("stubs", arr (map s ["0x00723cb8 task id returns 43", "0x00725318 receive is scripted", "0x0072503c post is the recording trampoline"] ++ [s (hexText (fromIntegral address) ++ " returns " ++ show value) | (address, value) <- optionStubs options]))
          ]
      , laneOutputRuns = successes
      , laneOutputError = Nothing
      }
  where
    runCycleOf index cycle = do
      loaded <- cycleInput lane cycle
      wakeResult <- dumpState lane (cycleWake cycle)
      completionResult <- dumpState lane (cycleCompletion cycle)
      mapped <- case (loaded, wakeResult, completionResult) of
        (Right input, Right wake, Right completion) -> mapCycle options qualification spans lane index wake input completion
        (Left problem, _, _) -> pure (Left problem)
        (_, Left problem, _) -> pure (Left problem)
        (_, _, Left problem) -> pure (Left problem)
      case mapped of
        Left problem -> pure (Left problem)
        Right mapping -> let sp = harnessSp; input = mappingInput mapping; observed = mappingObserved mapping in do
            let planInput = PlanInput
                  { planState = input
                  , planSeeds = seedsOf spans ++ maybe [] (map fieldAddress . qualFields) qualification
                  , planMessage = messageOf cycle
                  , planClosure = initialClosure
                  , planId = "cycle-" ++ laneName (laneSpec lane) ++ "-" ++ show index
                  , planExtraStubs = optionStubs options
                  , planServices = optionServices options
                  , planCoverage = optionCoverage options || isJust qualification
                  }
            outcome <- runCycle options images store planInput sp
            case outcome of
              Left problem -> pure (Left problem)
              Right (run, rounds) -> do
                closure <- readIORef (storeRef store)
                pure $ do
                  predicted <- predictedState planInput {planClosure = closure} run
                  if not (sameLayout predicted observed)
                    then Left "the completion dump's ranges differ from the wake dump's"
                    else do
                      let changed = netChanged input predicted
                          exposed = coverageExposed' (coverageAddresses isStateRegion run)
                          unmapped = maybe [] (\qual -> unmappedExposed spans (qualFields qual) exposed) qualification
                          compared = compareState spans changed predicted observed
                          differences = case (qualification, mappingAbstract mapping) of
                            (Just qual, True) -> filter (abstractDifference spans (qualFields qual) . differenceAddress) compared
                            _ -> compared
                          posts = projectNative declaration (postRecords arity run)
                      if not (null unmapped)
                        then Left ("refused: the step read stock-private state no field maps: " ++ unwords [hexText low ++ ".." ++ hexText high | (low, high) <- take 16 (ranges unmapped)])
                        else Right ()
                      observed' <- projectObserved (laneRole lane) declaration (observedPosts (optionServices options) cycle)
                      let report = CycleReport
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
                            , reportNativePc = runPc run
                            , reportNativeDetail = runDetail run
                            , reportRunnerSha = runRunnerSha run
                            , reportAccessTotal = runProvenanceTotal run
                            , reportTrace = if runStatus run == "stub_contract" then [] else runTrace run
                            , reportStateRead = coverageRead' (coverageAddresses isStateRegion run)
                            , reportStateWritten = coverageWritten' (coverageAddresses isStateRegion run)
                            , reportImageRead = coverageRead' (coverageAddresses isImageRegion run)
                            , reportStateExposed = exposed
                            , reportNativeInstructions = runInstructions run
                            , reportExecuted = executedBytes (runExecuted run)
                            , reportLevelPredicted = stateWord predicted levelAddress
                            , reportLevelObserved = stateWord observed levelAddress
                            , reportCaseSha = runCaseSha run
                            , reportRounds = rounds
                            , reportComparedBytes = length changed + sum [fromIntegral (spanBytes span') | span' <- spans]
                            , reportMap = mappingValue mapping
                            }
                      Right CycleRun
                        { cycleRunCycle = cycle
                        , cycleRunInput = input
                        , cycleRunPredicted = predicted
                        , cycleRunObserved = observed
                        , cycleRunReport = report
                        }
    messageOf cycle = case cycleWakeEvent cycle of
      TimedEvent _ (EventQueueIn operation sender payload) -> (operation, sender, payload)
      _ -> (0, 0, [])

-- | A dump split into the ranges its plan declares.
dumpState :: Lane -> Dump -> IO (Either String State)
dumpState lane dump = (>>= splitState (dumpRanges dump)) <$> dumpBytes lane dump

-- | The step's input: the wake state with the sampled frame (snapshot slots and image plane,
-- dumped at the primary capture) written over it.
cycleInput :: Lane -> Cycle -> IO (Either String State)
cycleInput lane cycle = do
  wake <- dumpState lane (cycleWake cycle)
  frame <- dumpState lane (cycleFrame cycle)
  pure (do w <- wake; f <- frame; overlayStates w f)

-- | Addresses whose granules every case maps: the receive staging the stub writes and the
-- declared retained spans.
seedsOf :: [Span] -> [Integer]
seedsOf spans = 0x0169d374 : concat [[spanLow span', spanHigh span' - 1] | span' <- spans]

harnessSp :: Word32
harnessSp = stackBase + fromIntegral stackSize

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
  , ("differing_ranges", arr [obj [("start", s (hexText low)), ("end", s (hexText high))] | (low, high) <- take 64 (ranges (map differenceAddress (reportDifferences report)))])
  , ("native_status", s (reportNativeStatus report))
  , ("native_pc", s (hexText (fromIntegral (reportNativePc report))))
  , ("native_detail", s (reportNativeDetail report))
  , ("runner_sha256", s (reportRunnerSha report))
  , ("access_total", n (reportAccessTotal report))
  , ("native_trace", arr (reportTrace report))
  , ("coverage", obj
      [ ("state_read_bytes", n (fromIntegral (length (reportStateRead report))))
      , ("state_written_bytes", n (fromIntegral (length (reportStateWritten report))))
      , ("state_exposed_bytes", n (fromIntegral (length (reportStateExposed report))))
      , ("image_read_bytes", n (fromIntegral (length (reportImageRead report))))
      , ("state_read", rangesValue (reportStateRead report))
      , ("state_written", rangesValue (reportStateWritten report))
      , ("state_exposed", rangesValue (reportStateExposed report))
      , ("image_read", rangesValue (reportImageRead report))
      ])
  , ("native_instructions", n (reportNativeInstructions report))
  , ("executed_bytes", n (reportExecuted report))
  , ("level_predicted", maybe Null (s . hexText . fromIntegral) (reportLevelPredicted report))
  , ("level_observed", maybe Null (s . hexText . fromIntegral) (reportLevelObserved report))
  , ("case_sha256", s (reportCaseSha report))
  , ("closure_rounds", n (fromIntegral (reportRounds report)))
  , ("map", reportMap report)
  ]

coverageRead', coverageWritten', coverageExposed' :: ([Integer], [Integer], [Integer]) -> [Integer]
coverageRead' (value, _, _) = value
coverageWritten' (_, value, _) = value
coverageExposed' (_, _, value) = value

-- | The addresses a run read, wrote and read before writing, through regions whose id the
-- predicate admits, ascending, from the runner's complete access coverage.
coverageAddresses :: (String -> Bool) -> RunResult -> ([Integer], [Integer], [Integer])
coverageAddresses admit run =
  ( concat [bits base readBits | (name, (readBits, _, _)) <- selected, Just base <- [M.lookup name (runRegionBases run)]]
  , concat [bits base writtenBits | (name, (_, writtenBits, _)) <- selected, Just base <- [M.lookup name (runRegionBases run)]]
  , concat [bits base exposedBits | (name, (_, _, exposedBits)) <- selected, Just base <- [M.lookup name (runRegionBases run)]]
  )
  where
    selected = [(name, bitmaps) | (name, bitmaps) <- M.toAscList (runCoverage run), admit name]
    bits base bitmap =
      [ fromIntegral base + fromIntegral (8 * index + bit)
      | (index, byte) <- zip [0 :: Int ..] (B.unpack bitmap), byte /= 0
      , bit <- [0 .. 7], byte .&. (1 `shiftL` bit) /= 0
      ]

-- | Captured-state regions: what the abstraction map must provide or a cycle input supplies.
isStateRegion :: String -> Bool
isStateRegion name = take 5 name == "data-"

-- | Pinned code windows: data loads from image bytes.
isImageRegion :: String -> Bool
isImageRegion name = take 4 name `elem` ["id5-", "id4-"] || take 3 name == "ld-"

rangesValue :: [Integer] -> Value
rangesValue addresses = arr [arr [s (hexText low), s (hexText high)] | (low, high) <- ranges addresses]

-- | Ascending addresses grouped into half-open runs of consecutive bytes.
ranges :: [Integer] -> [(Integer, Integer)]
ranges = foldr add []
  where
    add address ((low, high) : rest) | address + 1 == low = (address, high) : rest
    add address runs = (address, address + 1) : runs

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
runChain :: Options -> Images -> [Span] -> ClosureStore -> Lane -> [CycleRun] -> IO ChainResult
runChain options images spans store lane runs = go 0 Nothing [] runs
  where
    go :: Int -> Maybe ChainRun -> [Value] -> [CycleRun] -> IO ChainResult
    go _ _ entries [] = pure (ChainResult (reverse entries) (all entryMatches (reverse entries)) Nothing)
    go index carried entries (run : rest) = case carried of
      Nothing -> runStep (cycleRunInput run)
      Just previous -> do
        nextWake <- dumpState lane (cycleWake (cycleRunCycle run))
        case nextWake of
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
        runStep input = let sp = harnessSp in do
            let planInput = PlanInput input (seedsOf spans) (0x3b4b, 47, wakePayload (cycleRunCycle run)) initialClosure ("chain-" ++ laneName (laneSpec lane) ++ "-" ++ show index) (optionStubs options) (optionServices options) (optionCoverage options)
            outcome <- runCycle options images store planInput sp
            closure <- readIORef (storeRef store)
            case outcome >>= \(result, _) -> predictedState planInput {planClosure = closure} result of
              Left problem -> pure (ChainResult (reverse entries) False (Just problem))
              Right predicted -> do
                let observed = cycleRunObserved run
                    predictedLevel = stateWord predicted levelAddress
                    observedLevel = stateWord observed levelAddress
                    entry = obj
                      [ ("cycle", n (fromIntegral (reportIndex (cycleRunReport run))))
                      , ("level_predicted", maybe Null (s . hexText . fromIntegral) predictedLevel)
                      , ("level_observed", maybe Null (s . hexText . fromIntegral) observedLevel)
                      , ("decade_predicted", maybe Null (s . hexText . fromIntegral) (stateWord predicted decadeAddress))
                      , ("decade_observed", maybe Null (s . hexText . fromIntegral) (stateWord observed decadeAddress))
                      , ("match", b (predictedLevel == observedLevel))
                      , ("native_status", s (either (const "") (runStatus . fst) outcome))
                      ]
                go (index + 1) (Just (ChainRun input predicted observed)) (entry : entries) rest
        overlayFrame state = do
          frame <- dumpState lane (cycleFrame (cycleRunCycle run))
          pure (frame >>= overlayStates state)
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
-- The persisted closure

imageIdentity :: Images -> Value
imageIdentity images = obj
  [ ("image5", s (sha256Hex (sha256 (image5 images))))
  , ("image4", s (sha256Hex (sha256 (image4 images))))
  , ("loader_a", s (sha256Hex (sha256 (image0 images))))
  ]

closureValue :: Value -> Closure -> Value
closureValue identity closure = obj
  [ ("schema", n 1)
  , ("identity", identity)
  , ("windows5", windows (closureWindows5 closure))
  , ("windows4", windows (closureWindows4 closure))
  , ("windows0", windows (closureWindows0 closure))
  , ("function_starts", addresses (closureSplits closure))
  , ("state_granules", addresses (closureData closure))
  , ("scratch_pages", addresses (closureScratch closure))
  ]
  where
    windows list' = arr [obj [("base", s (hexText (fromIntegral (windowBase window)))), ("size", n (fromIntegral (windowSize window)))] | window <- list']
    addresses list' = arr [s (hexText (fromIntegral address)) | address <- sortOn id list']

closureFrom :: Value -> Closure
closureFrom value = Closure (windows "windows5") (windows "windows4") (windows "windows0") (addresses "function_starts") (addresses "state_granules") (addresses "scratch_pages")
  where
    windows key = [Window (fromIntegral (addressOf (get row "base"))) (fromIntegral (num (get row "size"))) | row <- list (get value key)]
    addresses key = [fromIntegral (addressOf row) | row <- list (get value key)]

-- | The shared closure, read from `--closure FILE` when the file exists and belongs to the same
-- pinned images, and written back after every growth.
closureStore :: Options -> Images -> IO ClosureStore
closureStore options images = do
  let identity = imageIdentity images
  initial <- case optionClosure options of
    Nothing -> pure initialClosure
    Just path -> do
      present <- doesFileExist path
      if not present
        then pure initialClosure
        else do
          value <- jsonRead =<< readRegular (64 * 1024 * 1024) path
          ensure (get value "schema" == n 1 && get value "identity" == identity) (path ++ " holds the closure of other images")
          pure (closureFrom value)
  ref <- newIORef initial
  pure ClosureStore
    { storeRef = ref
    , storeSave = case optionClosure options of
        Nothing -> const (pure ())
        Just path -> \closure -> do
          B.writeFile (path ++ ".tmp") (encodeCanonical (closureValue identity closure))
          renameFile (path ++ ".tmp") path
    }

-- ---------------------------------------------------------------------------
-- Main

main :: IO ()
main = do
  options <- parseOptions =<< getArgs
  image5Bytes <- readRegular (16 * 1024 * 1024) (optionId5 options)
  image4Bytes <- readRegular (16 * 1024 * 1024) (optionId4 options)
  loaderBytes <- readRegular (16 * 1024 * 1024) (optionLoader options)
  sitesBytes <- readRegular (4 * 1024 * 1024) (optionSites options)
  sites <- jsonRead sitesBytes
  let images = Images image5Bytes image4Bytes loaderBytes
      spans = declaredSpans sites
  createDirectoryIfMissing True (optionWork options)
  lanes <- mapM loadLane (optionLanes options)
  declaration <- either failH pure . Payload.parseDeclaration =<< readRegular (4 * 1024 * 1024) (optionFields options)
  store <- closureStore options images
  qualification <- mapM loadQualification (optionQualification options)
  outputs <- forM lanes $ \lane -> runLane options images qualification spans (serviceArity sites) declaration store lane
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
      result <- runChain options images spans store lane (laneOutputRuns output)
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
        , ("discovery", b (optionDiscoverData options))
        , ("qualification", case qualification of
            Nothing -> Null
            Just qual -> obj
              [ ("image_sha256", s (qualSha qual))
              , ("symbols_sha256", s (qualSymbolsSha qual))
              , ("fields", arr [arr [s (hexText (fieldAddress field)), n (fromIntegral (fieldLength field))] | field <- qualFields qual])
              , ("overlay_bytes", n (fromIntegral (fieldsBytes (qualFields qual))))
              , ("stock_private", arr [arr [s (hexText (spanLow span')), s (hexText (spanHigh span'))] | span' <- stockPrivate])
              ])
        , ("frame_identity_slip", arr slip)
        , ("exclusions", arr (map s
            [ "The record region 0x00500000 and the stack 0x00580000 are harness regions, not retained state: the stock loop at 0xf01be4 never reads its frame, so the step runs on a fresh stack."
            , "Outside the declared retained spans only the step's net changes are compared; the runner's provenance log truncates at 4096 accesses, so a byte the step wrote and restored is not compared."
            , "Code bytes come from the pinned image 5, image 4 and loader A; captured state is mapped in 64 KiB granules the step touches, and an unmapped byte is unchanged by construction."
            , "Posts are compared through payload-fields.json as saclac-interface-compare compares them: the native step's in the stock role, each lane's in its own role; undeclared payload bytes are not compared."
            , "The kernel receive, post, task id, cache maintenance and critical sections are outside the step: receive and post are scripted, the others are declared --stub returns."
            ]))
        ]
  B.writeFile (optionOut options) (encodeCanonical overall)
  -- A discovery run reads zero-filled scratch where the capture has no bytes; it never qualifies.
  unless (stockQualified && chainQualified && not (optionDiscoverData options)) exitFailure
  where
    laneValue output = obj
      [ ("lane", s (laneOutputName output))
      , ("symbols_sha256", maybe Null s (laneOutputSymbols output))
      , ("qualified", b (laneOutputQualified output))
      , ("error", maybe Null s (laneOutputError output))
      , ("refusals", arr (laneOutputRefusals output))
      , ("closure", laneOutputClosure output)
      , ("cycles", arr (laneOutputCycles output))
      , ("metrics", arr (laneOutputMetrics output))
      ]
