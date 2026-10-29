{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
-- | Original-versus-owned modal-75 editor-cancel differential cases.
module Main where

import Control.Monad (foldM, forM, forM_, unless, when)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import Data.Bits (shiftR, (.&.))
import Data.List (find)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe)
import Data.String (fromString)
import Data.Word (Word32)
import Emulation.NativeCase (Region (..), loadCase, runCase)
import Emulation.NativeObservation
  ( GuestAddress (..)
  , NativeObservation (..)
  , Register (..)
  , RegionId (..)
  , RunnerStatus (..)
  , hexWord32
  , lookupRegister
  , observeNative
  , parseWord32
  , regionIdOf
  , registerLabel
  , renderObservationError
  , reportRunnerExit
  , runnerStatusLabel
  )
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (get)
import FujiAF.JSON (encodeCanonical)
import Numeric (showHex)
import System.Directory (createDirectoryIfMissing)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))

stockEntry, exitAddr, queryAddr, stageAddr, transitionAddr :: GuestAddress
stockEntry = GuestAddress 0x00c77794
exitAddr = GuestAddress 0x00c77800
queryAddr = GuestAddress 0x0063a0f8
stageAddr = GuestAddress 0x00d3d858
transitionAddr = GuestAddress 0x00c12aac

stockBytesCount :: Int
stockBytesCount = 0x68

stateBase, sessionBase, dataBase, logRegionBase, stackBase, guardBase, faultyBase :: GuestAddress
stateBase = GuestAddress 0x0146761c
sessionBase = GuestAddress 0x00954d04
dataBase = GuestAddress 0x00956000
logRegionBase = GuestAddress 0x00957000
stackBase = GuestAddress 0x00959000
guardBase = GuestAddress 0x00958000
faultyBase = GuestAddress 0x00958400

stackSize, guardSize :: Int
stackSize = 0x1000
guardSize = 12

queryFunction, stageFunction, transitionFunction, guardFunction, faultyAbiFunction, faultyStateFunction :: [Word32]
queryFunction = [0xe59fc00c, 0xe58c0000, 0xe59f0008, 0xe5900000, 0xe12fff1e, guestWord logRegionBase, guestWord dataBase]
stageFunction = [0xe59fc008, 0xe58c0004, 0xe58c1008, 0xe12fff1e, guestWord logRegionBase]
transitionFunction = [0xe59fc010, 0xe59c200c, 0xe2822001, 0xe58c200c, 0xe59c0010, 0xe12fff1e, guestWord logRegionBase]
guardFunction = [0xe59fc000, 0xe12fff1c, guestWord stockEntry]
faultyAbiFunction = [0xe3a04000, 0xe12fff1e]
faultyStateFunction = [0xe3a0001c, 0xe59fc004, 0xe1cc00b0, 0xe12fff1e, guestWord stateBase + 2]

guestWord :: GuestAddress -> Word32
guestWord = fromIntegral . guestAddress

hexAddress :: GuestAddress -> String
hexAddress = hexWord32 . guestWord

hexB :: B.ByteString -> String
hexB = concatMap (\w -> let h = showHex w "" in replicate (2 - length h) '0' ++ h) . B.unpack

word32le :: Word32 -> B.ByteString
word32le w = B.pack
  [ fromIntegral w
  , fromIntegral (w `shiftR` 8)
  , fromIntegral (w `shiftR` 16)
  , fromIntegral (w `shiftR` 24)
  ]

assemble :: [Word32] -> B.ByteString
assemble = B.concat . map word32le

data Symbol = Symbol Word32 Word32

parseSymbols :: FilePath -> IO (M.Map String Symbol)
parseSymbols path = do
  rows <- zip [1 :: Int ..] . lines <$> readFile path
  foldM add M.empty rows
  where
    add symbols (lineNumber, line) = case words line of
      [a, z, _, name] -> insert symbols lineNumber name a z
      [a, _, name] -> insert symbols lineNumber name a "0"
      _ -> pure symbols
    insert symbols lineNumber name a z =
      case (parseWord32 a, parseWord32 z) of
        (Just address, Just size)
          | M.member name symbols -> error ("duplicate symbol " ++ name ++ " at line " ++ show lineNumber)
          | otherwise -> pure (M.insert name (Symbol address size) symbols)
        _ -> error ("invalid symbol line " ++ show lineNumber)

symbolAddress :: M.Map String Symbol -> String -> GuestAddress
symbolAddress symbols name = case lookupSymbol symbols name of Symbol address _ -> GuestAddress (fromIntegral address)

functionSymbol :: M.Map String Symbol -> String -> (GuestAddress, Word32)
functionSymbol symbols name = case lookupSymbol symbols name of
  Symbol address size
    | size == 0 -> error ("zero-size function symbol " ++ name)
    | otherwise -> (GuestAddress (fromIntegral address), size)

lookupSymbol :: M.Map String Symbol -> String -> Symbol
lookupSymbol symbols name = fromMaybe (error ("kernel symbol missing: " ++ name)) (M.lookup name symbols)

data Permission = ReadOnly | ReadWrite | ReadExecute

permissionLabel :: Permission -> String
permissionLabel ReadOnly = "r"
permissionLabel ReadWrite = "rw"
permissionLabel ReadExecute = "rx"

data CaseRegion = CaseRegion
  { caseRegion :: Region
  , casePermission :: Permission
  }

mkRegion :: RegionId -> GuestAddress -> Int -> Permission -> B.ByteString -> CaseRegion
mkRegion ident base size permission bytes =
  CaseRegion
    ( Region
        { description = object
            [ "id" .= regionName ident
            , "address" .= hexAddress base
            , "size" .= size
            , "permissions" .= permissionLabel permission
            ]
        , baseAddress = fromIntegral (guestAddress base)
        , regionSize = fromIntegral size
        , regionBytes = bytes
        }
    )
    permission

data CaseSpec = CaseSpec
  { caseId :: String
  , caseProvenance :: String
  , caseEntry :: GuestAddress
  , caseExit :: GuestAddress
  , caseRegions :: [CaseRegion]
  , caseFunctions :: [(GuestAddress, Word32)]
  , caseInitial :: [(Register, Word32)]
  , caseExpectedStatus :: RunnerStatus
  , caseExpectedRegisters :: [(Register, Word32)]
  , caseExpectedMemory :: [(RegionId, B.ByteString)]
  }

exclusion :: String
exclusion = "single modal-75 action; the retained query, stage-open and transition services are modeled at their stock entries; no scheduler, interrupt, frame or device path"

regionJson :: CaseRegion -> Value
regionJson (CaseRegion region permission) =
  object
    [ "id" .= regionName (regionIdOf region)
    , "address" .= hexAddress (GuestAddress (fromIntegral (baseAddress region)))
    , "size" .= regionSize region
    , "permissions" .= permissionLabel permission
    , "initial" .=
        ( if B.all (== 0) (regionBytes region)
            then object ["zero" .= True]
            else object
              [ "artifact" .= regionName (regionIdOf region)
              , "offset" .= (0 :: Int)
              , "length" .= B.length (regionBytes region)
              ]
        )
    ]

artifactJson :: CaseRegion -> Value
artifactJson (CaseRegion region _) =
  object
    [ "id" .= regionName (regionIdOf region)
    , "path" .= (regionName (regionIdOf region) ++ ".bin")
    , "size" .= B.length (regionBytes region)
    , "sha256" .= sha256Hex (sha256 (regionBytes region))
    ]

caseValue :: CaseSpec -> Value
caseValue spec =
  object
    [ "schema_version" .= (1 :: Int)
    , "id" .= caseId spec
    , "origin" .= ("synthetic" :: String)
    , "provenance" .= caseProvenance spec
    , "cpu" .= object ["architecture" .= ("arm" :: String), "model" .= ("cortex-r5f" :: String), "mode" .= ("arm" :: String), "endian" .= ("little" :: String)]
    , "artifacts" .= [artifactJson r | r <- caseRegions spec, not (B.all (== 0) (regionBytes (caseRegion r)))]
    , "entry" .= hexAddress (caseEntry spec)
    , "exit" .= hexAddress (caseExit spec)
    , "registers" .= object [fromString (registerLabel r) .= hexWord32 value | (r, value) <- caseInitial spec]
    , "regions" .= map regionJson (caseRegions spec)
    , "functions" .=
        [ object ["address" .= hexAddress address, "size" .= size]
        | (address, size) <- caseFunctions spec
        ]
    , "stubs" .= ([] :: [Value])
    , "limits" .= object ["instructions" .= (4000000 :: Int), "wall_ms" .= (4000 :: Int)]
    , "expected" .=
        object
          [ "status" .= runnerStatusLabel (caseExpectedStatus spec)
          , "registers" .= object [fromString (registerLabel r) .= hexWord32 value | (r, value) <- caseExpectedRegisters spec]
          , "memory" .=
              [ object ["region" .= regionName ident, "offset" .= (0 :: Int), "hex" .= hexB bytes]
              | (ident, bytes) <- caseExpectedMemory spec
              ]
          , "events" .= ([] :: [Value])
          ]
    , "exclusions" .= [exclusion]
    ]

writeCase :: FilePath -> CaseSpec -> IO FilePath
writeCase directory spec = do
  createDirectoryIfMissing True directory
  forM_ (caseRegions spec) $ \(CaseRegion region _) ->
    when (not (B.all (== 0) (regionBytes region))) $
      B.writeFile (directory </> (regionName (regionIdOf region) ++ ".bin")) (regionBytes region)
  let path = directory </> "case.json"
  B.writeFile path (encodeCanonical (caseValue spec))
  pure path

loadObserved :: FilePath -> FilePath -> IO NativeObservation
loadObserved runner path = do
  (_, regions, _, _) <- loadCase path
  (_, report) <- runCase runner path
  exit <- either (error . renderObservationError) pure (reportRunnerExit report)
  either (error . renderObservationError) pure (observeNative exit regions (get report "runner"))

registerValue :: NativeObservation -> Register -> Word32
registerValue observation register =
  fromMaybe (error ("missing register " ++ registerLabel register)) (lookupRegister register observation)

regionBytesOf :: NativeObservation -> RegionId -> B.ByteString
regionBytesOf observation ident =
  fromMaybe (error ("missing memory region " ++ regionName ident)) (lookup ident (nativeMemory observation))

cpsrControl :: Word32 -> Word32
cpsrControl word = word .&. 0x3ff

preservedRegisters :: [Register]
preservedRegisters = [R0, R4, R5, R6, R7, R8, R9, R10, R11, Sp, Pc]

memoryRegions :: [RegionId]
memoryRegions = [RegionId "state", RegionId "session", RegionId "log"]

readWordAt :: B.ByteString -> Int -> Word32
readWordAt bytes offset =
  fromIntegral (B.index bytes offset)
    + fromIntegral (B.index bytes (offset + 1)) * 0x100
    + fromIntegral (B.index bytes (offset + 2)) * 0x10000
    + fromIntegral (B.index bytes (offset + 3)) * 0x1000000

readHalfwordAt :: B.ByteString -> Int -> Word32
readHalfwordAt bytes offset =
  fromIntegral (B.index bytes offset) + fromIntegral (B.index bytes (offset + 1)) * 0x100

semanticProblems :: Word32 -> Word32 -> Word32 -> NativeObservation -> [String]
semanticProblems queryResult sessionValue transitionResult observation =
  [ "query argument " ++ hexWord32 queryArgument ++ ", not " ++ hexWord32 0x53
  | queryArgument /= 0x53
  ]
    ++ [ "return " ++ hexWord32 actualReturn ++ ", not " ++ hexWord32 expectedReturn
       | actualReturn /= expectedReturn
       ]
    ++ [ "stage kind " ++ hexWord32 stageKind ++ ", not " ++ hexWord32 expectedStageKind
       | stageKind /= expectedStageKind
       ]
    ++ [ "stage id " ++ hexWord32 stageId ++ ", not " ++ hexWord32 expectedStageId
       | stageId /= expectedStageId
       ]
    ++ [ "transition count " ++ show transitionCount ++ ", not " ++ show expectedTransitionCount
       | transitionCount /= expectedTransitionCount
       ]
    ++ [ "transition word " ++ hexWord32 transitionWord ++ ", not " ++ hexWord32 transitionResult
       | transitionWord /= transitionResult
       ]
    ++ [ "state " ++ hexWord32 stateWord ++ ", not " ++ hexWord32 expectedState
       | stateWord /= expectedState
       ]
  where
    denied = queryResult /= 0 && sessionValue /= 1
    expectedReturn = if denied then 0 else transitionResult
    expectedStageKind = if denied then 0 else 0x0f
    expectedStageId = if denied then 0 else 0x1a
    expectedTransitionCount = if denied then 0 else 1
    expectedState = if denied then 0 else if transitionResult `elem` [0xffffffff, 2] then 0x1d else 0
    logBytes = regionBytesOf observation (RegionId "log")
    stateBytes = regionBytesOf observation (RegionId "state")
    queryArgument = readWordAt logBytes 0
    stageKind = readWordAt logBytes 4
    stageId = readWordAt logBytes 8
    transitionCount = readWordAt logBytes 12
    transitionWord = readWordAt logBytes 16
    stateWord = readHalfwordAt stateBytes 2
    actualReturn = registerValue observation R0

compareObservation :: NativeObservation -> NativeObservation -> [String]
compareObservation oracle candidate =
  statusMismatch ++ registerMismatches ++ controlMismatches ++ memoryMismatches
  where
    statusMismatch = ["status is " ++ runnerStatusLabel (nativeStatus candidate) ++ ", not returned" | nativeStatus candidate /= Returned]
    registerMismatches =
      [ "register " ++ registerLabel register ++ ": " ++ hexWord32 actual ++ ", not " ++ hexWord32 expected
      | register <- preservedRegisters
      , let expected = registerValue oracle register
      , let actual = registerValue candidate register
      , actual /= expected
      ]
    controlMismatches =
      [ "oracle control state " ++ hexWord32 oracleControl ++ " differs from entry"
      | oracleControl /= 0x10
      ]
        ++ [ "control state " ++ hexWord32 candidateControl ++ ", not " ++ hexWord32 oracleControl
           | candidateControl /= oracleControl
           ]
      where
        oracleControl = cpsrControl (registerValue oracle Cpsr)
        candidateControl = cpsrControl (registerValue candidate Cpsr)
    memoryMismatches =
      [ "memory " ++ regionName ident ++ " differs"
      | ident <- memoryRegions
      , regionBytesOf oracle ident /= regionBytesOf candidate ident
      ]

initialRegisters :: Word32 -> [(Register, Word32)]
initialRegisters stack =
  [ (R0, 0), (R1, 0), (R2, 0), (R3, 0)
  , (R4, sentinel 4), (R5, sentinel 5), (R6, sentinel 6), (R7, sentinel 7)
  , (R8, sentinel 8), (R9, sentinel 9), (R10, sentinel 10), (R11, sentinel 11)
  , (R12, 0), (Sp, stack), (Lr, guestWord exitAddr), (Cpsr, 0x10)
  ]
  where
    sentinel n = 0xa5a50000 + n

scenarioRegions :: Word32 -> Word32 -> Word32 -> Permission -> [CaseRegion]
scenarioRegions queryResult sessionValue transitionResult sessionPermission =
  [ mkRegion (RegionId "state") stateBase 8 ReadWrite (B.replicate 8 0)
  , mkRegion (RegionId "session") sessionBase 8 sessionPermission (B.replicate 4 0 <> word32le sessionValue)
  , mkRegion (RegionId "data") dataBase 4 ReadOnly (word32le queryResult)
  , mkRegion (RegionId "log") logRegionBase 20 ReadWrite (B.replicate 16 0 <> word32le transitionResult)
  , mkRegion (RegionId "query-service") queryAddr 32 ReadExecute (assemble queryFunction)
  , mkRegion (RegionId "stage-service") stageAddr 32 ReadExecute (assemble stageFunction)
  , mkRegion (RegionId "transition-service") transitionAddr 32 ReadExecute (assemble transitionFunction)
  , mkRegion (RegionId "exit") exitAddr 4 ReadExecute (word32le 0xe12fff1e)
  , mkRegion (RegionId "stack") stackBase stackSize ReadWrite (B.replicate stackSize 0)
  ]

data Context = Context
  { contextRunner :: FilePath
  , contextOutput :: FilePath
  , contextSymbols :: M.Map String Symbol
  , contextKernelText :: B.ByteString
  , contextStockBytes :: B.ByteString
  }

scenario :: Context -> String -> Word32 -> Word32 -> Word32 -> Bool -> (CaseSpec, CaseSpec)
scenario context identifier queryResult sessionValue transitionResult sessionReadOnly = (stock, owned)
  where
    provenanceText = "differential case " ++ identifier ++ ": X-M1 1.50 FUN_00c77794 executes as the oracle; retained services are modeled at 0x0063a0f8, 0x00d3d858 and 0x00c12aac"
    baseRegions = scenarioRegions queryResult sessionValue transitionResult (if sessionReadOnly then ReadOnly else ReadWrite)
    serviceFunctions = [(queryAddr, 28), (stageAddr, 20), (transitionAddr, 28)]
    stock = CaseSpec
      { caseId = identifier ++ "-stock"
      , caseProvenance = provenanceText
      , caseEntry = stockEntry
      , caseExit = exitAddr
      , caseRegions = baseRegions ++ [mkRegion (RegionId "stock") stockEntry stockBytesCount ReadExecute (contextStockBytes context)]
      , caseFunctions = (stockEntry, fromIntegral stockBytesCount) : serviceFunctions
      , caseInitial = initialRegisters 0x00959ff0
      , caseExpectedStatus = Returned
      , caseExpectedRegisters = []
      , caseExpectedMemory = []
      }
    owned = CaseSpec
      { caseId = identifier ++ "-owned"
      , caseProvenance = provenanceText
      , caseEntry = ownedEntry
      , caseExit = exitAddr
      , caseRegions = baseRegions
          ++ [ mkRegion (RegionId "stock") stockEntry 4 ReadOnly (B.take 4 (contextStockBytes context))
             , mkRegion (RegionId "ktext") kernelBase (B.length (contextKernelText context)) ReadExecute (contextKernelText context)
             ]
      , caseFunctions = ownedFunctions
      , caseInitial = initialRegisters 0x00959ff0
      , caseExpectedStatus = Returned
      , caseExpectedRegisters = []
      , caseExpectedMemory = []
      }
    ownedEntry = symbolAddress (contextSymbols context) "rb_ui_editor_cancel"
    kernelBase = symbolAddress (contextSymbols context) "__rb_image_start"
    ownedFunctions =
      [ functionSymbol (contextSymbols context) "rb_ui_editor_cancel"
      , functionSymbol (contextSymbols context) "rb_call_modal_query"
      , functionSymbol (contextSymbols context) "rb_open_editor_cancel_stage"
      , functionSymbol (contextSymbols context) "rb_call_stage_open"
      , functionSymbol (contextSymbols context) "rb_call_transition"
      ] ++ serviceFunctions

withExpected :: CaseSpec -> [(Register, Word32)] -> [(RegionId, B.ByteString)] -> CaseSpec
withExpected spec registers memory = spec { caseExpectedRegisters = registers, caseExpectedMemory = memory }

data Outcome = Outcome
  { outcomeId :: String
  , outcomeStockStatus :: RunnerStatus
  , outcomeOwnedStatus :: RunnerStatus
  , outcomeOracle :: NativeObservation
  , outcomeOwned :: NativeObservation
  , outcomeMismatches :: [String]
  }

runScenario :: Context -> String -> Word32 -> Word32 -> Word32 -> Bool -> IO Outcome
runScenario context identifier queryResult sessionValue transitionResult sessionReadOnly = do
  let (stockSpec, ownedSpec) = scenario context identifier queryResult sessionValue transitionResult sessionReadOnly
      probeSpec = stockSpec { caseId = identifier ++ "-probe", caseExpectedRegisters = [(R0, 0)], caseExpectedMemory = [] }
  probePath <- writeCase (contextOutput context </> caseId probeSpec) probeSpec
  probe <- loadObserved (contextRunner context) probePath
  when (nativeStatus probe /= Returned) $
    error (identifier ++ ": oracle did not return: " ++ runnerStatusLabel (nativeStatus probe))
  unless (null (semanticProblems queryResult sessionValue transitionResult probe)) $
    error (identifier ++ ": original fixture semantics: " ++ show (semanticProblems queryResult sessionValue transitionResult probe))
  let oracleRegisters = [(register, registerValue probe register) | register <- preservedRegisters]
      oracleMemory = [(ident, regionBytesOf probe ident) | ident <- memoryRegions]
      stockVerified = withExpected stockSpec oracleRegisters oracleMemory
      ownedVerified = withExpected ownedSpec oracleRegisters oracleMemory
  stockPath <- writeCase (contextOutput context </> caseId stockVerified) stockVerified
  stock <- loadObserved (contextRunner context) stockPath
  unless (null (compareObservation probe stock)) $
    error (identifier ++ ": oracle is not stable: " ++ show (compareObservation probe stock))
  ownedPath <- writeCase (contextOutput context </> caseId ownedVerified) ownedVerified
  owned <- loadObserved (contextRunner context) ownedPath
  pure Outcome
    { outcomeId = identifier
    , outcomeStockStatus = nativeStatus stock
    , outcomeOwnedStatus = nativeStatus owned
    , outcomeOracle = probe
    , outcomeOwned = owned
    , outcomeMismatches = compareObservation probe owned
    }

faultyCase :: String -> String -> [Word32] -> NativeObservation -> CaseSpec
faultyCase identifier provenance words' oracle = CaseSpec
  { caseId = identifier
  , caseProvenance = provenance
  , caseEntry = faultyBase
  , caseExit = exitAddr
  , caseRegions =
      scenarioRegions 0 0 0xffffffff ReadWrite
        ++ [mkRegion (RegionId "faulty") faultyBase (B.length bytes) ReadExecute bytes]
  , caseFunctions = [(faultyBase, fromIntegral (B.length bytes))]
  , caseInitial = initialRegisters 0x00959ff0
  , caseExpectedStatus = Returned
  , caseExpectedRegisters = [(register, registerValue oracle register) | register <- preservedRegisters]
  , caseExpectedMemory = [(ident, regionBytesOf oracle ident) | ident <- memoryRegions]
  }
  where
    bytes = assemble words'

guardCase :: Context -> CaseSpec
guardCase context = CaseSpec
  { caseId = "om-guard-fetch"
  , caseProvenance = "negative control: a modelled transfer to 0x00c77794 must fetch-guard when that address is not executable"
  , caseEntry = guardBase
  , caseExit = exitAddr
  , caseRegions =
      [ mkRegion (RegionId "guard") guardBase guardSize ReadExecute (assemble guardFunction)
      , mkRegion (RegionId "stock") stockEntry 4 ReadOnly (B.take 4 (contextStockBytes context))
      , mkRegion (RegionId "exit") exitAddr 4 ReadExecute (word32le 0xe12fff1e)
      , mkRegion (RegionId "stack") stackBase stackSize ReadWrite (B.replicate stackSize 0)
      ]
  , caseFunctions = [(guardBase, fromIntegral guardSize)]
  , caseInitial = initialRegisters 0x00959ff0
  , caseExpectedStatus = Returned
  , caseExpectedRegisters = [(R0, 0)]
  , caseExpectedMemory = []
  }

main :: IO ()
main = do
  arguments <- getArgs
  runner <- required arguments "--runner"
  build <- required arguments "--build"
  output <- required arguments "--out"
  id5Path <- required arguments "--id5"
  id5BaseArgument <- required arguments "--id5-base"
  id5Base <- pure (parseAddress "--id5-base" id5BaseArgument)
  symbols <- parseSymbols (build </> "kernel.syms")
  kernelText <- B.readFile (build </> "kernel-text.bin")
  unless (not (B.null kernelText)) (error "kernel-text.bin is empty")
  id5 <- B.readFile id5Path
  unless (id5Base <= guestWord stockEntry) (error "--id5-base lies above the stock entry")
  let offset = fromIntegral (guestWord stockEntry - id5Base) :: Int
  unless (offset + stockBytesCount <= B.length id5) (error "stock bytes are truncated")
  let stockBytes = B.take stockBytesCount (B.drop offset id5)
  unless (B.length stockBytes == stockBytesCount) (error "stock bytes are truncated")
  let context = Context runner output symbols kernelText stockBytes
      scenarios =
        [ ("om-q0-s0-tminus1", 0, 0, 0xffffffff, False)
        , ("om-q0-s1-tminus1", 0, 1, 0xffffffff, False)
        , ("om-q0-s2-t5d", 0, 2, 0x5d, False)
        , ("om-q1-s0-tminus1", 1, 0, 0xffffffff, False)
        , ("om-q1-s1-tminus1", 1, 1, 0xffffffff, False)
        , ("om-q1-s2-tminus1", 1, 2, 0xffffffff, False)
        , ("om-q2-s0-t5d", 2, 0, 0x5d, False)
        , ("om-q2-s1-t2", 2, 1, 2, False)
        , ("om-q0-s0-t2", 0, 0, 2, False)
        , ("om-q0-s0-t5d", 0, 0, 0x5d, True)
        ]
  outcomes <- forM scenarios $ \(identifier, queryResult, sessionValue, transitionResult, sessionReadOnly) ->
    runScenario context identifier queryResult sessionValue transitionResult sessionReadOnly
  let stateOutcome = fromMaybe (error "state scenario missing") (find ((== "om-q0-s0-tminus1") . outcomeId) outcomes)
      oracle = outcomeOracle stateOutcome
      faultyAbiPath = output </> "om-faulty-abi"
      faultyStatePath = output </> "om-faulty-state"
  faultyAbiSpec <- pure (faultyCase "om-faulty-abi" "faulty guest: mov r4,#0 then return" faultyAbiFunction oracle)
  faultyAbiCasePath <- writeCase faultyAbiPath faultyAbiSpec
  faultyAbi <- loadObserved runner faultyAbiCasePath
  let faultyAbiCaught = not (null (compareObservation oracle faultyAbi))
  faultyStateSpec <- pure (faultyCase "om-faulty-state" "faulty guest: write state 0x1c then return" faultyStateFunction oracle)
  faultyStateCasePath <- writeCase faultyStatePath faultyStateSpec
  faultyState <- loadObserved runner faultyStateCasePath
  let faultyStateCaught = not (null (compareObservation oracle faultyState))
  guardPath <- writeCase (output </> "om-guard-fetch") (guardCase context)
  guard <- loadObserved runner guardPath
  let guardOk = nativeStatus guard == FetchGuard
      controls :: [(String, Bool)]
      controls =
        [ ("faulty-abi", faultyAbiCaught)
        , ("faulty-state", faultyStateCaught)
        , ("forbidden-fetch", guardOk)
        ]
      failed = [outcome | outcome <- outcomes, not (null (outcomeMismatches outcome))]
      controlsOk = all snd controls
  B.writeFile (output </> "owned-menu-results.json") (encodeCanonical (object
    [ "cases" .= [object
        [ "id" .= outcomeId outcome
        , "stock_status" .= runnerStatusLabel (outcomeStockStatus outcome)
        , "owned_status" .= runnerStatusLabel (outcomeOwnedStatus outcome)
        , "mismatches" .= outcomeMismatches outcome
        ] | outcome <- outcomes]
    , "controls" .= [object ["id" .= name, "ok" .= ok] | (name, ok) <- controls]
    ]))
  putStrLn ("ran " ++ show (length outcomes) ++ " differential cases: "
    ++ show (length outcomes - length failed) ++ " matched; controls "
    ++ (if controlsOk then "refused as required" else "FAILED"))
  when (not (null failed) || not controlsOk) exitFailure

parseAddress :: String -> String -> Word32
parseAddress label text = fromMaybe (error ("invalid " ++ label ++ ": " ++ text)) (parseWord32 text)

required :: [String] -> String -> IO String
required arguments name =
  case dropWhile (/= name) arguments of
    (_ : value : _) -> pure value
    _ -> error ("missing " ++ name)
