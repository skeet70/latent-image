{-# LANGUAGE ScopedTypeVariables #-}

-- | Original-versus-owned execution of the save facade against service models.
module Replacement.Save.Qualify
  ( Paths (..)
  , CaseStatus (..)
  , CaseResult (..)
  , runQualification
  ) where

import Control.Exception (IOException, try)
import Control.Monad (forM)
import Data.Aeson (Value (..))
import Data.Bits (shiftL, shiftR, (.&.))
import qualified Data.ByteString as B
import Data.List (find)
import Data.Maybe (fromMaybe)
import Data.Word (Word32, Word64, Word8)
import Emulation.NativeCase (Region (..), loadCase)
import Emulation.NativeObservation
  ( GuestAddress (..)
  , NativeObservation (..)
  , ProvenanceTotal (..)
  , RegionId (..)
  , Register (..)
  , RunnerStatus (..)
  , Write (..)
  , WriteBytes (..)
  , hexWord32
  , lookupRegister
  , observeNative
  , parseWord32
  , provenanceCount
  , provenanceWrites
  , regionBounds
  , renderObservationError
  , runnerStatusLabel
  )
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host hiding (sha256)
import FujiAF.JSON (encodeCanonical)
import Numeric (showHex)
import Replacement.Save.Domain
import System.Directory (createDirectoryIfMissing, doesPathExist)
import System.FilePath ((</>), takeDirectory)

data Paths = Paths
  { pathContract :: FilePath
  , pathOwned :: FilePath
  , pathFaulty :: FilePath
  , pathStorage :: FilePath
  , pathStorageFaulty :: FilePath
  , pathWait :: FilePath
  , pathStage :: FilePath
  , pathRunner :: FilePath
  , pathStockImage :: FilePath
  , pathOutput :: FilePath
  }

data Subject = Stock | Owned | FaultyGuest | FaultyProducer
  deriving (Eq, Show)

data CaseStatus = Passed | Failed | Blocked
  deriving (Eq, Show)

caseStatusLabel :: CaseStatus -> String
caseStatusLabel Passed = "passed"
caseStatusLabel Failed = "failed"
caseStatusLabel Blocked = "blocked"

data CaseProblem
  = ExecutionBlocked String
  | RunnerMismatch RunnerStatus
  | RegisterMismatch Register Word32 (Maybe Word32)
  | CpsrMismatch (Maybe Word32)
  | MemoryMismatch RegionId Int B.ByteString (Maybe B.ByteString)
  | EventsMismatch
  | ProviderAbiFault
  | ShortcutStage
  | StageStartMismatch Word32
  | StageEndMismatch Word32
  | UnexpectedStage
  | MissingMailbox
  | EffectsTruncated ProvenanceTotal
  | UndeclaredWrite GuestAddress
  | ForbiddenWrite RegionId GuestAddress
  deriving (Eq, Show)

renderProblem :: CaseProblem -> String
renderProblem problem = case problem of
  ExecutionBlocked message -> message
  RunnerMismatch status -> "runner status " ++ runnerStatusLabel status
  RegisterMismatch register wanted actual ->
    show register ++ ": expected " ++ hexWord32 wanted ++ ", observed " ++ maybe "missing" hexWord32 actual
  CpsrMismatch actual -> "cpsr control bits: observed " ++ maybe "missing" hexWord32 actual
  MemoryMismatch (RegionId ident) offset wanted actual ->
    "region " ++ ident ++ " offset " ++ show offset ++ ": expected " ++ hexBytes wanted ++ ", observed " ++ maybe "missing" hexBytes actual
  EventsMismatch -> "stub events differ"
  ProviderAbiFault -> "provider ABI fault"
  ShortcutStage -> "stage call on the admission shortcut"
  StageStartMismatch actual -> "stage start differs: " ++ hexWord32 actual
  StageEndMismatch actual -> "stage end differs: " ++ hexWord32 actual
  UnexpectedStage -> "unexpected stage point"
  MissingMailbox -> "mailbox region missing"
  EffectsTruncated count -> "effect evidence truncated at " ++ show (provenanceTotal count)
  UndeclaredWrite (GuestAddress address) -> "write outside declared regions at " ++ show address
  ForbiddenWrite ident (GuestAddress address) -> "write to " ++ show ident ++ " at " ++ show address ++ " outside its declared effect"

data CaseResult = CaseResult
  { caseSubject :: Subject
  , caseName :: String
  , caseStatus :: CaseStatus
  , caseProblems :: [CaseProblem]
  }

newtype Digest = Digest B.ByteString
  deriving (Eq, Show)

data Slice = Slice
  { sliceName :: String
  , sliceAddress :: Word32
  , sliceOffset :: Int
  , sliceLength :: Int
  , sliceSha :: Digest
  }

data Contract = Contract
  { contractImageSize :: Int
  , contractImageSha :: Digest
  , contractSlices :: [Slice]
  , contractStorage :: Word32
  , contractWait :: Word32
  , contractStage :: Word32
  }

data Env = Env
  { envPaths :: Paths
  , envContract :: Contract
  , envSlices :: [(String, B.ByteString)]
  , envOwned :: B.ByteString
  , envFaulty :: B.ByteString
  , envStorage :: B.ByteString
  , envStorageFaulty :: B.ByteString
  , envWait :: B.ByteString
  , envStage :: B.ByteString
  }

stateSize :: Int
stateSize = 0x10C + 0x4000

cpsrControlMask :: Word32
cpsrControlMask = 0x010002ff

runQualification :: Paths -> IO Value
runQualification paths = do
  contract <- loadContract (pathContract paths)
  image <- loadImage (pathStockImage paths) (contractImageSize contract) (contractImageSha contract)
  slices <- forM (contractSlices contract) $ \slice -> do
    let bytes = B.take (sliceLength slice) (B.drop (sliceOffset slice) image)
    ensure (B.length bytes == sliceLength slice) ("slice " ++ sliceName slice ++ ": length")
    ensure (sha256Hex (sha256 bytes) == renderDigest (sliceSha slice)) ("slice " ++ sliceName slice ++ ": digest")
    pure (sliceName slice, bytes)
  owned <- readBytes (pathOwned paths)
  faulty <- readBytes (pathFaulty paths)
  storage <- readBytes (pathStorage paths)
  storageFaulty <- readBytes (pathStorageFaulty paths)
  waitModel <- readBytes (pathWait paths)
  stageModel <- readBytes (pathStage paths)
  ensure (B.length owned > 0x300) "owned image lacks the probe extents"
  ensure (B.length faulty > 0x300) "faulty image lacks the probe extents"
  ensure (B.length storage <= 0x600) "storage model exceeds its region"
  ensure (B.length storageFaulty <= 0x600) "faulty storage model exceeds its region"
  ensure (B.length waitModel <= 0x100) "wait model exceeds its region"
  ensure (B.length stageModel <= 0x100) "stage model exceeds its region"
  let env =
        Env
          { envPaths = paths
          , envContract = contract
          , envSlices = slices
          , envOwned = owned
          , envFaulty = faulty
          , envStorage = storage
          , envStorageFaulty = storageFaulty
          , envWait = waitModel
          , envStage = stageModel
          }
  withOutputRoot (pathOutput paths) $ \root -> do
    (stockResults, stockFailure) <-
      runUntilFailure env root [(Stock, scenario) | scenario <- filter stockEligible scenarioDomain]
    (ownedResults, ownedFailure) <-
      case stockFailure of
        Just _ -> pure ([], Nothing)
        Nothing -> runUntilFailure env root [(Owned, scenario) | scenario <- scenarioDomain]
    let caseFailure = case stockFailure of
          Just result -> Just result
          Nothing -> ownedFailure
    (guestControl, producerControl, controlsExecuted) <-
      case caseFailure of
        Just _ -> pure (False, False, False)
        Nothing -> do
          guest <- controlGuest env root
          producer <- controlProducer env root
          pure (guest, producer, True)
    let failures = case caseFailure of
          Just result -> [failureObject result]
          Nothing -> []
        controlFailures =
          [s "faulty guest was accepted" | controlsExecuted, not guestControl]
            ++ [s "faulty storage producer was accepted" | controlsExecuted, not producerControl]
        unqualified = case caseFailure of
          Just _ -> False
          Nothing -> controlsExecuted && null controlFailures
    pure
      ( obj
          [ ("schema", n 1)
          , ("status", s (if unqualified then "qualified" else "failed"))
          , ("stock_cases", n (fromIntegral (length stockResults)))
          , ("owned_cases", n (fromIntegral (length ownedResults)))
          , ("scenario_cases", n (fromIntegral (length scenarioDomain)))
          , ("stock_comparison_cases", n (fromIntegral (length (filter stockEligible scenarioDomain))))
          , ("executed_cases", n (fromIntegral (length stockResults + length ownedResults)))
          , ("stopped_at_first_failure", b (case caseFailure of Just _ -> True; Nothing -> False))
          , ("controls_executed", b controlsExecuted)
          , ("output_root", s (pathOutput (envPaths env)))
          , ("artifacts", obj
              [ ("stock_image", s (renderDigest (contractImageSha contract)))
              , ("owned", s (sha256Hex (sha256 (envOwned env))))
              , ("faulty_guest", s (sha256Hex (sha256 (envFaulty env))))
              , ("storage_model", s (sha256Hex (sha256 (envStorage env))))
              , ("faulty_storage_model", s (sha256Hex (sha256 (envStorageFaulty env))))
              , ("wait_model", s (sha256Hex (sha256 (envWait env))))
              , ("stage_model", s (sha256Hex (sha256 (envStage env))))
              ])
          , ("failures", arr failures)
          , ("controls", arr
              [ obj [("name", s "faulty-guest"), ("executed", b controlsExecuted), ("refused", b guestControl)]
              , obj [("name", s "faulty-storage-producer"), ("executed", b controlsExecuted), ("refused", b producerControl)]
              ])
          , ("control_failures", arr controlFailures)
          , ("exclusions", arr (map s exclusions))
          ]
      )

withOutputRoot :: FilePath -> (FilePath -> IO a) -> IO a
withOutputRoot root action = do
  exists <- doesPathExist root
  ensure (not exists) "output root must be fresh"
  createDirectoryIfMissing True root
  action root

exclusions :: [String]
exclusions =
  [ "No develop step, card image, file, sensor, interrupt or hardware timing is executed"
  , "The retained storage machine, RTOS waits and signals and the metadata filler are boundary models"
  , "The stock facade runs with register-only stubs and guest models, not the stock service closures"
  ]

stockEligible :: Scenario -> Bool
stockEligible scenario =
  not (scenarioReentry scenario)
    && ( case scenarioLatePhaseOne scenario of
           Nothing -> True
           Just _ -> False
       )

subjectTag :: Subject -> String
subjectTag Stock = "stock"
subjectTag Owned = "owned"
subjectTag FaultyGuest = "faulty-guest"
subjectTag FaultyProducer = "faulty-producer"

controlScenario :: String -> Scenario
controlScenario identValue =
  fromMaybe (error ("missing control scenario " ++ identValue)) (find ((== identValue) . scenarioId) scenarioDomain)

controlGuest :: Env -> FilePath -> IO Bool
controlGuest env root = do
  let scenario = controlScenario "s-kind-k2-code0-interrupt-st0"
  result <- runScenario env root FaultyGuest scenario
  pure
    ( caseStatus result == Failed
        && caseProblems result == [RegisterMismatch R0 (expectedReturn (expectation scenario)) (Just 1)]
    )

controlProducer :: Env -> FilePath -> IO Bool
controlProducer env root = do
  let scenario = controlScenario "s-kind-k3-code0-st0"
      expected = expectation scenario
      descriptorField = frameOffset (0x27d0 + kindBank (scenarioKind scenario) * 0x150 + 0x14c)
  result <- runScenario env root FaultyProducer scenario
  pure
    ( caseStatus result == Failed
        && caseProblems result
          == [MemoryMismatch (RegionId "frame") descriptorField (wordBytes (fromMaybe 0 (expectedVariant expected))) (Just (wordBytes 0))]
    )

runRaw :: Env -> FilePath -> Subject -> Scenario -> IO (Either String (NativeObservation, Value, [Region]))
runRaw env root subject scenario = do
  let dir = root </> (scenarioId scenario ++ "-" ++ subjectTag subject)
  casePath <- buildCase env dir subject scenario
  runNative (pathRunner (envPaths env)) casePath

runScenario :: Env -> FilePath -> Subject -> Scenario -> IO CaseResult
runScenario env root subject scenario = do
  outcome <- runRaw env root subject scenario
  case outcome of
    Left message -> pure (CaseResult subject (scenarioId scenario) Blocked [ExecutionBlocked message])
    Right (observation, report, regions) -> do
      let problems = checkCase scenario observation report regions
          status = if null problems then Passed else Failed
      if null problems
        then pure ()
        else
          B.writeFile
            (caseDirectory root subject scenario </> "failure.json")
            ( encodeCanonical
                ( obj
                    [ ("kind", s "check")
                    , ("subject", s (subjectTag subject))
                    , ("case", s (scenarioId scenario))
                    , ("problems", arr (map (s . renderProblem) problems))
                    , ("runner", report)
                    ]
                )
            )
      pure (CaseResult subject (scenarioId scenario) status problems)

runUntilFailure :: Env -> FilePath -> [(Subject, Scenario)] -> IO ([CaseResult], Maybe CaseResult)
runUntilFailure env root = go []
  where
    go executed [] = pure (reverse executed, Nothing)
    go executed ((subject, scenario) : rest) = do
      result <- runScenario env root subject scenario
      if caseStatus result == Passed
        then go (result : executed) rest
        else pure (reverse (result : executed), Just result)

failureObject :: CaseResult -> Value
failureObject result =
  obj
    [ ("subject", s (subjectTag (caseSubject result)))
    , ("case", s (caseName result))
    , ("status", s (caseStatusLabel (caseStatus result)))
    , ("problems", arr (map (s . renderProblem) (caseProblems result)))
    ]

caseDirectory :: FilePath -> Subject -> Scenario -> FilePath
caseDirectory root subject scenario =
  root </> (scenarioId scenario ++ "-" ++ subjectTag subject)

loadContract :: FilePath -> IO Contract
loadContract path = do
  raw <- readRegularWith (pure ()) True (1024 * 1024) path
  value <- jsonRead raw
  ensure (num (get value "schema") == 1) "contract schema"
  let image = get value "image"
  size <- integer (get image "size") 1 (8 * 1024 * 1024) "image size"
  digest <- parseDigestValue (get image "sha256") "image digest"
  slices <- mapM parseSlice (list (get value "slices"))
  ensure (not (null slices)) "contract slices"
  let models = get value "models"
  storage <- parseAddress (get models "storage")
  waitBase <- parseAddress (get models "wait")
  stageBase <- parseAddress (get models "stage")
  pure
    Contract
      { contractImageSize = fromIntegral size
      , contractImageSha = digest
      , contractSlices = slices
      , contractStorage = storage
      , contractWait = waitBase
      , contractStage = stageBase
      }
  where
    parseSlice item = do
      name <- pure (str (get item "name"))
      ensure (not (null name)) "slice name"
      address <- parseAddress (get item "address")
      offset <- parseAddress (get item "offset")
      lengthValue <- integer (get item "length") 1 (2 * 1024 * 1024) "slice length"
      digest <- parseDigestValue (get item "sha256") "slice digest"
      pure
        Slice
          { sliceName = name
          , sliceAddress = address
          , sliceOffset = fromIntegral offset
          , sliceLength = fromIntegral lengthValue
          , sliceSha = digest
          }

parseAddress :: Value -> IO Word32
parseAddress value =
  maybe (failH ("bad address: " ++ str value)) pure (parseWord32 (str value))

parseDigestValue :: Value -> String -> IO Digest
parseDigestValue value label =
  case parseHex (str value) of
    Just bytes | B.length bytes == 32 -> pure (Digest bytes)
    _ -> failH (label ++ ": 32-byte hexadecimal digest required")

parseHex :: String -> Maybe B.ByteString
parseHex text
  | odd (length text) = Nothing
  | otherwise = B.pack <$> go text
  where
    go [] = Just []
    go (high : low : rest) = do
      highValue <- hexDigit high
      lowValue <- hexDigit low
      ((highValue * 16 + lowValue) :) <$> go rest
    go _ = Nothing
    hexDigit character
      | character >= '0' && character <= '9' = Just (fromIntegral (fromEnum character - fromEnum '0'))
      | character >= 'a' && character <= 'f' = Just (fromIntegral (fromEnum character - fromEnum 'a' + 10))
      | otherwise = Nothing

renderDigest :: Digest -> String
renderDigest (Digest bytes) = hexBytes bytes

loadImage :: FilePath -> Int -> Digest -> IO B.ByteString
loadImage path size digest = do
  bytes <- readRegularWith (pure ()) True (8 * 1024 * 1024) path
  ensure (B.length bytes == size) "stock image length"
  ensure (sha256Hex (sha256 bytes) == renderDigest digest) "stock image digest"
  pure bytes

readBytes :: FilePath -> IO B.ByteString
readBytes path = do
  bytes <- readRegularWith (pure ()) True (2 * 1024 * 1024) path
  ensure (not (B.null bytes)) "empty artifact"
  pure bytes

buildCase :: Env -> FilePath -> Subject -> Scenario -> IO FilePath
buildCase env dir subject scenario = do
  createDirectoryIfMissing True dir
  let expected = expectation scenario
      state = stateBytes scenario
      codeBytes =
        case subject of
          Stock -> B.empty
          FaultyGuest -> envFaulty env
          _ -> envOwned env
      storageBytes =
        case subject of
          FaultyProducer -> envStorageFaulty env
          _ -> envStorage env
      entry =
        case subject of
          Stock -> stockFacadeAddress
          _
            | Just _ <- scenarioLatePhaseOne scenario -> ownedBase + 0x200
            | scenarioReentry scenario -> ownedBase + 0x100
            | otherwise -> ownedBase
      pieces = codePieces env subject codeBytes
      codeArtifacts = [(pieceFile name, name, bytes) | (name, _, bytes) <- pieces]
      artifacts =
        codeArtifacts
          ++ [ ("state.bin", "state", state)
             , ("storage.bin", "storage", storageBytes)
             , ("wait.bin", "wait", envWait env)
             , ("stage.bin", "stage", envStage env)
             ]
  mapM_ (\(file, _, bytes) -> B.writeFile (dir </> file) bytes) artifacts
  artifactValues <- mapM (\(file, ident, bytes) -> artifactEntry file ident bytes) artifacts
  let storageBase = contractStorage (envContract env)
      waitBase = contractWait (envContract env)
      stageBase = contractStage (envContract env)
      codeRegions =
        [ artifactRegion name address (B.length bytes) "rx" name 0 (B.length bytes)
        | (name, address, bytes) <- pieces
        ]
      regions =
        codeRegions
          ++ [ artifactRegion "storage" storageBase 0x600 "rx" "storage" 0 (B.length storageBytes)
             , artifactRegion "wait" waitBase 0x100 "rx" "wait" 0 (B.length (envWait env))
             , artifactRegion "stage" stageBase 0x100 "rx" "stage" 0 (B.length (envStage env))
             , artifactRegion "frame" frameBase 0x4000 "rw" "state" 0x10C 0x4000
             , zeroRegion "result" resultBase 0x10 "rw"
             , artifactRegion "status" statusBase 4 "rw" "state" 0 4
             , artifactRegion "slots" slotsBase 8 "rw" "state" 4 8
             , artifactRegion "mailbox" mailboxBase 0x100 "rw" "state" 12 0x100
             , zeroRegion "stack" stackBase 0x1000 "rw"
             , zeroRegion "exit" exitBase 4 "rx"
             ]
      functions = codeFunctions env subject codeBytes entry
      kind = scenarioKind scenario
      initialRegisters =
        [ ("r0", frameBase)
        , ("r1", kindWord kind)
        , ("r2", resultBase)
        , ("r3", 0)
        , ("r4", 0x44)
        , ("r5", 0x55)
        , ("r6", 0x66)
        , ("r7", 0x77)
        , ("r8", 0x88)
        , ("r9", 0x99)
        , ("r10", 0xaa)
        , ("r11", 0xbb)
        , ("r12", 0)
        , ("sp", stackTop)
        , ("lr", exitBase)
        , ("cpsr", 0x10)
        ]
      value =
        obj
          [ ("schema_version", n 1)
          , ("id", s (scenarioId scenario))
          , ("origin", s (if subject == Stock then "stock" else "synthetic"))
          , ("provenance", s "save facade contract, generated from the image-5 decompilation")
          , ("cpu", obj [("architecture", s "arm"), ("model", s "cortex-r5f"), ("mode", s "arm"), ("endian", s "little")])
          , ("artifacts", arr artifactValues)
          , ("entry", s (hexWord32 entry))
          , ("exit", s (hexWord32 exitBase))
          , ("registers", obj (map (\(name, word) -> (name, s (hexWord32 word))) initialRegisters))
          , ("regions", arr regions)
          , ("functions", arr (map functionJson functions))
          , ("stubs", arr (map stubJson (stubsFor scenario)))
          , ("limits", obj [("instructions", n 200000), ("wall_ms", n 5000)])
          , ("expected", expectedJson scenario expected)
          , ("exclusions", arr (map s exclusions))
          ]
      casePath = dir </> "case.json"
  B.writeFile casePath (encodeCanonical value)
  pure casePath

pieceFile :: String -> FilePath
pieceFile name = name ++ ".bin"

codePieces :: Env -> Subject -> B.ByteString -> [(String, Word32, B.ByteString)]
codePieces env subject codeBytes =
  case subject of
    Stock ->
      [ (sliceName slice, sliceAddress slice, bytes)
      | slice <- contractSlices (envContract env)
      , Just bytes <- [lookup (sliceName slice) (envSlices env)]
      ]
    _ -> [("code", ownedBase, codeBytes)]

codeFunctions :: Env -> Subject -> B.ByteString -> Word32 -> [(Word32, Int)]
codeFunctions env subject codeBytes entry =
  case subject of
    Stock ->
      [ (stockFacadeAddress, 0x2dc)
      , (0x00e51858, 0x68)
      , (0x00e4e5dc, 0x68)
      , (0x00e4e5a8, 0x34)
      , (0x00e50e4c, 0x58)
      , (contractStorage (envContract env), 0x88)
      , (contractStorage (envContract env) + 0x88, 0x88)
      , (contractStorage (envContract env) + 0x114, 0x88)
      , (contractStorage (envContract env) + 0x400, 0x200)
      , (contractWait (envContract env), 0x100)
      , (contractStage (envContract env), 0x100)
      ]
    _ ->
      let size = align4 (B.length codeBytes)
          offset = fromIntegral (entry - ownedBase)
       in [ (entry, size - offset)
          , (contractStorage (envContract env), 0x88)
          , (contractStorage (envContract env) + 0x88, 0x88)
          , (contractStorage (envContract env) + 0x114, 0x88)
          , (contractStorage (envContract env) + 0x400, 0x200)
          , (contractWait (envContract env), 0x100)
          , (contractStage (envContract env), 0x100)
          ]

align4 :: Int -> Int
align4 value = ((value + 3) `div` 4) * 4

functionJson :: (Word32, Int) -> Value
functionJson (address, size) = obj [("address", s (hexWord32 address)), ("size", n (fromIntegral size))]

artifactEntry :: FilePath -> String -> B.ByteString -> IO Value
artifactEntry relative ident bytes =
  pure
    ( obj
        [ ("id", s ident)
        , ("path", s relative)
        , ("size", n (fromIntegral (B.length bytes)))
        , ("sha256", s (sha256Hex (sha256 bytes)))
        ]
    )

artifactRegion :: String -> Word32 -> Int -> String -> String -> Int -> Int -> Value
artifactRegion ident address size permissions artifact offset lengthValue =
  regionJson ident address size permissions
    (obj [("artifact", s artifact), ("offset", n (fromIntegral offset)), ("length", n (fromIntegral lengthValue))])

zeroRegion :: String -> Word32 -> Int -> String -> Value
zeroRegion ident address size permissions =
  regionJson ident address size permissions (obj [("zero", b True)])

regionJson :: String -> Word32 -> Int -> String -> Value -> Value
regionJson ident address size permissions initial =
  obj
    [ ("id", s ident)
    , ("address", s (hexWord32 address))
    , ("size", n (fromIntegral size))
    , ("permissions", s permissions)
    , ("initial", initial)
    ]

stubsFor :: Scenario -> [(Word32, [(String, Word32)], [(String, Word32)])]
stubsFor scenario =
  [ (0x0073abec, [], [("r0", presentWord)])
  , (0x0069f8fc, [("r0", 0xc3)], [("r0", selectorDispatch (scenarioSelectors scenario))])
  , (0x0073ac60, [], [("r0", selectorProfile (scenarioSelectors scenario))])
  , (0x00e4e0b4, [("r0", frameBase), ("r1", metadataWord)], [("r0", 0)])
  , (0x011d5850, [("r0", gatePreset gates)], [("r0", 1)])
  , (0x011d5934, [("r0", extentNameAddress), ("r1", descriptor)], [("r0", 0)])
  , (0x011d591c, [("r0", extentNameAddress), ("r1", descriptor)], [("r0", 0)])
  , (0x007101e4, [("r0", 2)], [("r0", 0)])
  , (0x00723cb8, [], [("r0", taskId)])
  , (0x00724558, [("r0", taskId), ("r1", 0x10)], [("r0", 0)])
  ]
  where
    kind = scenarioKind scenario
    gates = scenarioGates scenario
    presentWord = if selectorPresent (scenarioSelectors scenario) then 1 else 0
    metadataWord = if kind == K0 && gateFrameState gates == 1 then 1 else 0
    descriptor = descriptorAddress (kindBank kind)

stubJson :: (Word32, [(String, Word32)], [(String, Word32)]) -> Value
stubJson (address, inputs, outputs) =
  obj
    [ ("address", s (hexWord32 address))
    , ("inputs", obj (map (\(name, word) -> (name, s (hexWord32 word))) inputs))
    , ("outputs", obj (map (\(name, word) -> (name, s (hexWord32 word))) outputs))
    , ("side_effects", s "registers-only")
    ]

eventJson :: Event -> Value
eventJson (Event address inputs outputs) =
  obj
    [ ("address", s (hexWord32 address))
    , ("inputs", obj (map (\(name, word) -> (name, s (hexWord32 word))) inputs))
    , ("outputs", obj (map (\(name, word) -> (name, s (hexWord32 word))) outputs))
    ]

expectedJson :: Scenario -> Expectation -> Value
expectedJson scenario expected =
  obj
    [ ("status", s "returned")
    , ("registers", obj (map (\(name, word) -> (name, s (hexWord32 word))) (expectedRegisters expected)))
    , ("memory", arr (expectedMemory scenario expected))
    , ("events", arr (map eventJson (expectedEvents expected)))
    ]

expectedRegisters :: Expectation -> [(String, Word32)]
expectedRegisters expected =
  [ ("r0", expectedReturn expected)
  , ("r4", 0x44)
  , ("r5", 0x55)
  , ("r6", 0x66)
  , ("r7", 0x77)
  , ("r8", 0x88)
  , ("r9", 0x99)
  , ("r10", 0xaa)
  , ("r11", 0xbb)
  , ("sp", stackTop)
  , ("pc", exitBase)
  ]

expectedMemory :: Scenario -> Expectation -> [Value]
expectedMemory scenario expected =
  [ memoryEntry "status" 0 (wordBytes (expectedStatus expected))
  , memoryEntry "result" 0 (wordBytes (expectedHandback expected))
  , memoryEntry "slots" 0 (B.replicate 8 0)
  ]
    ++ if scenarioReentry scenario
      then
        [ memoryEntry "result" 4 (wordBytes (expectedReturn expected))
        , memoryEntry "result" 8 (wordBytes (expectedReturn expected))
        ]
      else []
    ++ if expectedShortcut expected
      then []
      else
        [ memoryEntry "frame" (frameOffset (0x27d0 + kindBank kind * 0x150 + 0x04)) (wordBytes (expectedMode expected))
        , memoryEntry "frame" (frameOffset (0x27d0 + kindBank kind * 0x150 + 0x148)) (wordBytes 0)
        , memoryEntry "frame" (frameOffset (0x27d0 + kindBank kind * 0x150 + 0x14c)) (wordBytes (fromMaybe 0 (expectedVariant expected)))
        ]
  where
    kind = scenarioKind scenario

frameOffset :: Word32 -> Int
frameOffset offset = fromIntegral offset

frameArtifactOffset :: Word32 -> Int
frameArtifactOffset offset = 0x10C + fromIntegral offset

memoryEntry :: String -> Int -> B.ByteString -> Value
memoryEntry ident offset bytes =
  obj [("region", s ident), ("offset", n (fromIntegral offset)), ("hex", s (hexBytes bytes))]

wordBytes :: Word32 -> B.ByteString
wordBytes word =
  B.pack
    [ fromIntegral (word .&. 0xff)
    , fromIntegral ((word `shiftR` 8) .&. 0xff)
    , fromIntegral ((word `shiftR` 16) .&. 0xff)
    , fromIntegral (word `shiftR` 24)
    ]

hexBytes :: B.ByteString -> String
hexBytes = concatMap byteHex . B.unpack
  where
    byteHex :: Word8 -> String
    byteHex value = let text = showHex value "" in replicate (2 - length text) '0' ++ text

stateBytes :: Scenario -> B.ByteString
stateBytes scenario =
  foldl'
    (\bytes (offset, word) -> putWord32 offset word bytes)
    (B.replicate stateSize 0)
    (mailboxValues ++ frameValues ++ [(0, scenarioInitialStatus scenario)])
  where
    selectors = scenarioSelectors scenario
    gates = scenarioGates scenario
    completion = scenarioCompletion scenario
    wait = scenarioWait scenario
    codes = completionCodes completion
    firstCode = case codes of
      (code : _) -> fromIntegral code
      [] -> 0
    secondCode = case codes of
      (_ : code : _) -> fromIntegral code
      _ -> firstCode
    phaseTwoCount =
      if expectedPhaseTwo (expectation scenario)
        then phaseTwoCallbacks completion
        else 0
    mailboxValues =
      [ (12 + 4 * index, word)
      | (index, word) <-
          zip
            [0 :: Int ..]
            [ if selectorPresent selectors then 1 else 0
            , selectorDispatch selectors
            , selectorProfile selectors
            , fromIntegral (length codes)
            , firstCode
            , secondCode
            , phaseTwoCount
            , waitFlags wait
            , 0
            , 0
            , 0
            , 0
            , phaseTwoFlags wait
            , descriptorAddress (kindBank (scenarioKind scenario))
            , 0
            , 0
            , stackBase
            , stackTop
            ]
      ]
    frameValues =
      [ (frameArtifactOffset 0x275c, gateFrameState gates)
      , (frameArtifactOffset 0x604, gateCommand gates)
      , (frameArtifactOffset 0x5f0, gatePreset gates)
      , (frameArtifactOffset 0x578, gateLayout gates)
      ]
        ++ concat
          [ [ (frameArtifactOffset (0x27d0 + bank * 0x150 + 0x04), 0xaaaaaaaa)
            , (frameArtifactOffset (0x27d0 + bank * 0x150 + 0x148), 0xbbbbbbbb)
            , (frameArtifactOffset (0x27d0 + bank * 0x150 + 0x14c), 0xcccccccc)
            ]
          | bank <- [0, 1, 2] :: [Word32]
          ]

putWord32 :: Int -> Word32 -> B.ByteString -> B.ByteString
putWord32 offset word bytes =
  B.take offset bytes <> wordBytes word <> B.drop (offset + 4) bytes

runNative :: FilePath -> FilePath -> IO (Either String (NativeObservation, Value, [Region]))
runNative runner casePath = do
  let directory = takeDirectory casePath
  loaded <- try (loadCase casePath)
  case loaded of
    Left (exception :: IOException) -> do
      writeFailure directory "case" (show exception) Null
      pure (Left ("case: " ++ show exception))
    Right (_, regions, wire, _) -> do
      executed <- try (runBounded 120000000 (16 * 1024 * 1024) runner [] wire)
      case executed of
        Left (exception :: IOException) -> do
          writeFailure directory "spawn" (show exception) Null
          pure (Left ("runner spawn: " ++ show exception))
        Right (timedOut, exit, out, err) -> do
          B.writeFile (directory </> "runner.stdout") out
          B.writeFile (directory </> "runner.stderr") err
          if timedOut
            then do
              writeFailure directory "timeout" "host timeout" Null
              pure (Left "host timeout")
            else do
              parsed <- try (jsonRead out)
              case parsed of
                Left (exception :: IOException) -> do
                  writeFailure directory "output" (show exception) Null
                  pure (Left ("runner output: " ++ show exception))
                Right value ->
                  case observeNative exit regions value of
                    Left observationError -> do
                      writeFailure directory "observation" (renderObservationError observationError) value
                      pure (Left (renderObservationError observationError))
                    Right observation -> pure (Right (observation, value, regions))

writeFailure :: FilePath -> String -> String -> Value -> IO ()
writeFailure directory kind message detail =
  B.writeFile
    (directory </> "failure.json")
    (encodeCanonical (obj [("kind", s kind), ("message", s message), ("detail", detail)]))

checkCase :: Scenario -> NativeObservation -> Value -> [Region] -> [CaseProblem]
checkCase scenario observation report regions =
  statusProblems ++ registerProblems ++ memoryProblems ++ eventProblems ++ stageProblems ++ effectProblems
  where
    expected = expectation scenario
    statusProblems =
      [ RunnerMismatch (nativeStatus observation)
      | nativeStatus observation /= Returned
      ]
    registerProblems =
      [ RegisterMismatch register wanted actual
      | (register, wanted) <-
          [ (R0, expectedReturn expected)
          , (R4, 0x44), (R5, 0x55), (R6, 0x66), (R7, 0x77)
          , (R8, 0x88), (R9, 0x99), (R10, 0xaa), (R11, 0xbb)
          , (Sp, stackTop), (Pc, exitBase)
          ]
      , let actual = lookupRegister register observation
      , actual /= Just wanted
      ]
        ++ [ CpsrMismatch actual
           | let actual = lookupRegister Cpsr observation
           , fmap (.&. cpsrControlMask) actual /= Just 0x10
           ]
    memoryProblems =
      [ MemoryMismatch (RegionId ident) offset bytes actual
      | (ident, offset, bytes) <- expectedBytes
      , let actual = memoryAt ident offset (B.length bytes)
      , actual /= Just bytes
      ]
    expectedBytes =
      [ ("status", 0, wordBytes (expectedStatus expected))
      , ("result", 0, wordBytes (expectedHandback expected))
      , ("slots", 0, B.replicate 8 0)
      ]
        ++ if scenarioReentry scenario
          then
            [ ("result", 4, wordBytes (expectedReturn expected))
            , ("result", 8, wordBytes (expectedReturn expected))
            ]
          else []
        ++ if expectedShortcut expected
          then []
          else
            [ ("frame", frameOffset (0x27d0 + kindBank kind * 0x150 + 0x04), wordBytes (expectedMode expected))
            , ("frame", frameOffset (0x27d0 + kindBank kind * 0x150 + 0x148), wordBytes 0)
            , ("frame", frameOffset (0x27d0 + kindBank kind * 0x150 + 0x14c), wordBytes (fromMaybe 0 (expectedVariant expected)))
            ]
    kind = scenarioKind scenario
    eventProblems =
      [ EventsMismatch
      | get report "events" /= arr (map eventJson (expectedEvents expected))
      ]
    stageProblems =
      case lookup (RegionId "mailbox") (nativeMemory observation) of
        Just mailbox | 64 <= B.length mailbox ->
          let start = wordAt 36 mailbox
              end = wordAt 40 mailbox
              faulted = wordAt 56 mailbox
              unexpected = wordAt 60 mailbox
           in [ProviderAbiFault | faulted /= 0]
                ++ if expectedShortcut expected
                  then [ShortcutStage | start /= 0 || end /= 0 || unexpected /= 0]
                  else
                    [ StageStartMismatch start | start /= 0x0108 ]
                      ++ [ StageEndMismatch end | end /= 0x010A ]
                      ++ [ UnexpectedStage | unexpected /= 0 ]
        _ -> [MissingMailbox]
    effectProblems = checkEffects scenario expected observation regions
    memoryAt ident offset lengthValue =
      case lookup (RegionId ident) (nativeMemory observation) of
        Just bytes | offset + lengthValue <= B.length bytes ->
          Just (B.take lengthValue (B.drop offset bytes))
        _ -> Nothing

wordAt :: Int -> B.ByteString -> Word32
wordAt offset bytes =
  foldl'
    (\word index -> word + (fromIntegral (B.index bytes (offset + index)) `shiftL` (8 * index)))
    0
    [0 .. 3]

checkEffects :: Scenario -> Expectation -> NativeObservation -> [Region] -> [CaseProblem]
checkEffects scenario expected observation regions =
  case provenanceWrites (nativeProvenance observation) of
    Nothing ->
      [ EffectsTruncated (provenanceCount (nativeProvenance observation))
      ]
    Just writes -> concatMap classify writes
  where
    bounds = [(ident, base, size) | (ident, GuestAddress base, size) <- regionBounds regions]
    spans = expectedSpans scenario expected
    classify write =
      let GuestAddress address = writeAddress write
          WriteBytes size = writeLength write
       in case regionAt bounds address (fromIntegral size) of
            Nothing -> [UndeclaredWrite (writeAddress write)]
            Just (ident, base, _) ->
              if ident `elem` map RegionId ["stack", "mailbox"]
                then []
                else
                  if any
                    (\(region, offset, lengthValue) ->
                      ident == region
                        && address >= base + fromIntegral offset
                        && address + fromIntegral size <= base + fromIntegral offset + fromIntegral lengthValue)
                    spans
                    then []
                    else [ForbiddenWrite ident (writeAddress write)]

expectedSpans :: Scenario -> Expectation -> [(RegionId, Int, Int)]
expectedSpans scenario expected =
  [ (RegionId "status", 0, 4)
  , (RegionId "slots", 0, 8)
  , (RegionId "result", 0, if scenarioReentry scenario then 12 else 4)
  ]
    ++ if expectedShortcut expected
      then []
      else
        [ (RegionId "frame", frameOffset (0x27d0 + kindBank kind * 0x150 + field), 4)
        | field <- [0x04, 0x148, 0x14c]
        ]
  where
    kind = scenarioKind scenario

regionAt :: [(RegionId, Word64, Int)] -> Word64 -> Int -> Maybe (RegionId, Word64, Int)
regionAt bounds address size =
  find (\(_, base, extent) -> address >= base && address + fromIntegral size <= base + fromIntegral extent) bounds
