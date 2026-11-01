{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
-- | Original-versus-owned modal-75 editor-cancel differential cases.
module Main where

import Control.Monad (forM, unless, when)
import qualified Data.ByteString as B
import Data.Bits ((.|.))
import Data.List (find)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe)
import Data.Word (Word32)
import Emulation.NativeObservation
  ( GuestAddress (..)
  , NativeObservation (..)
  , Register (..)
  , RegionId (..)
  , RunnerStatus (..)
  , Write (..)
  , WriteBytes (..)
  , hexWord32
  , registerLabel
  , provenanceWrites
  , runnerStatusLabel
  )
import Emulation.OwnedCase
import Emulation.Sha256 (parseSha256Hex, sha256, sha256Hex)
import Replacement.ModalRuntime.KernelBinding (KernelDigests (..), KernelSymbol (..), BoundKernel (..), bindKernel)
import qualified StockInventory as SI
import FujiAF.JSON (encodeCanonical)
import Data.Aeson (object, (.=))
import System.Directory (createDirectoryIfMissing, doesPathExist)
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
faultyAbiFunction = [0xe3a04000]
faultyStateFunction = [0xe3a0201c, 0xe307c61e, 0xe340c146, 0xe1cc20b0]

requiredFunctions :: [String]
requiredFunctions =
  [ "rb_ui_editor_cancel"
  , "rb_call_modal_query"
  , "rb_open_editor_cancel_stage"
  , "rb_call_stage_open"
  , "rb_call_transition"
  ]

exclusion :: String
exclusion = "single modal-75 action; the retained query, stage-open and transition services are modeled at their stock entries; no scheduler, interrupt, frame or device path"

preservedRegisters :: [Register]
preservedRegisters = [R0, R4, R5, R6, R7, R8, R9, R10, R11, Sp, Pc]

memoryRegions :: [RegionId]
memoryRegions = [RegionId "state", RegionId "session", RegionId "log"]

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
  statusMismatch ++ registerMismatches ++ controlMismatches ++ memoryMismatches ++ effectMismatches
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
        ++ ["caller-owned stack differs" | callerStack oracle /= callerStack candidate]
    effectMismatches =
      [ "ordered non-stack writes differ"
      | case (nonStackWrites oracle, nonStackWrites candidate) of
          (Just left, Just right) -> left /= right
          _ -> True
      ]

nonStackWrites :: NativeObservation -> Maybe [(GuestAddress, Int)]
nonStackWrites observation =
  fmap
    (map (\write -> (writeAddress write, fromIntegral (writeBytes (writeLength write))))
      . filter (\write ->
          let address = guestAddress (writeAddress write)
              end = address + fromIntegral (writeBytes (writeLength write))
           in not (guestAddress stackBase <= address && end <= guestAddress stackBase + fromIntegral stackSize - 16)))
    (provenanceWrites (nativeProvenance observation))

callerStack :: NativeObservation -> B.ByteString
callerStack observation = B.drop (stackSize - 16) (regionBytesOf observation (RegionId "stack"))

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
      , caseExclusion = exclusion
      , caseEntry = stockEntry
      , caseExit = exitAddr
      , caseRegions = baseRegions ++ [mkRegion (RegionId "stock") stockEntry stockBytesCount ReadExecute (contextStockBytes context)]
      , caseFunctions = (stockEntry, fromIntegral stockBytesCount) : serviceFunctions
      , caseInitial = initialRegisters 0x00959ff0
      , caseExpectedStatus = Returned
      , caseExpectedRegisters = [(Sp, 0x00959ff0)]
      , caseExpectedMemory = []
      }
    owned = CaseSpec
      { caseId = identifier ++ "-owned"
      , caseProvenance = provenanceText
      , caseExclusion = exclusion
      , caseEntry = ownedEntry
      , caseExit = exitAddr
      , caseRegions = baseRegions
          ++ [ mkRegion (RegionId "stock") stockEntry 4 ReadOnly (B.take 4 (contextStockBytes context))
             , mkRegion (RegionId "ktext") kernelBase (B.length (contextKernelText context)) ReadExecute (contextKernelText context)
             ]
      , caseFunctions = ownedFunctions
      , caseInitial = initialRegisters 0x00959ff0
      , caseExpectedStatus = Returned
      , caseExpectedRegisters = [(Sp, 0x00959ff0)]
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
  stockPath <- writeCase (contextOutput context </> caseId stockSpec) stockSpec
  stock <- loadObserved (contextRunner context) stockPath
  when (nativeStatus stock /= Returned) $
    error (identifier ++ ": oracle did not return: " ++ runnerStatusLabel (nativeStatus stock))
  unless (null (semanticProblems queryResult sessionValue transitionResult stock)) $
    error (identifier ++ ": original fixture semantics: " ++ show (semanticProblems queryResult sessionValue transitionResult stock))
  let oracleRegisters = [(register, registerValue stock register) | register <- preservedRegisters]
      oracleMemory = [(ident, regionBytesOf stock ident) | ident <- memoryRegions]
      ownedVerified = withExpected ownedSpec oracleRegisters oracleMemory
  ownedPath <- writeCase (contextOutput context </> caseId ownedVerified) ownedVerified
  owned <- loadObserved (contextRunner context) ownedPath
  pure Outcome
    { outcomeId = identifier
    , outcomeStockStatus = nativeStatus stock
    , outcomeOwnedStatus = nativeStatus owned
    , outcomeOracle = stock
    , outcomeOwned = owned
    , outcomeMismatches = compareObservation stock owned
    }

faultyCase :: Context -> String -> String -> [Word32] -> NativeObservation -> CaseSpec
faultyCase context identifier provenance mutation oracle =
  owned
    { caseId = identifier
    , caseProvenance = provenance
    , caseEntry = faultyBase
    , caseRegions = caseRegions owned ++ [mkRegion (RegionId "faulty") faultyBase (B.length bytes) ReadExecute bytes]
    , caseFunctions = (faultyBase, fromIntegral (B.length bytes)) : caseFunctions owned
    , caseExpectedRegisters = [(register, registerValue oracle register) | register <- preservedRegisters]
    , caseExpectedMemory = [(ident, regionBytesOf oracle ident) | ident <- memoryRegions]
    }
  where
    (_, owned) = scenario context identifier 0 0 0xffffffff False
    entry = guestWord (caseEntry owned)
    bytes =
      assemble
        ([0xe92d4008, 0xe59fc000 .|. fromIntegral (4 * (1 + length mutation)), 0xe12fff3c]
          ++ mutation ++ [0xe8bd8008, entry])

guardCase :: Context -> CaseSpec
guardCase context = CaseSpec
  { caseId = "om-guard-fetch"
  , caseProvenance = "negative control: a modelled transfer to 0x00c77794 must fetch-guard when that address is not executable"
  , caseExclusion = exclusion
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
  exists <- doesPathExist output
  when exists $ error "output path already exists"
  id5Path <- required arguments "--id5"
  id5BaseArgument <- required arguments "--id5-base"
  id5Base <- pure (parseAddress "--id5-base" id5BaseArgument)
  elfDigest <- required arguments "--kernel-elf-sha256" >>= either error pure . parseSha256Hex
  textDigest <- required arguments "--kernel-text-sha256" >>= either error pure . parseSha256Hex
  symsDigest <- required arguments "--kernel-syms-sha256" >>= either error pure . parseSha256Hex
  backend <- readBounded runner (64 * 1024 * 1024)
  unless (sha256Hex (sha256 backend) == "b51544c62053d1bf31816d2c3e4b439e3847296b2eef1f99c8f17b5d75da2860") $
    error "runner differs from E-NATIVE-APSR-MSR-150"
  kernelBytes <- readBounded (build </> "kernel.elf") (2 * 1024 * 1024)
  suppliedText <- readBounded (build </> "kernel-text.bin") 0xa300
  symsBytes <- readBounded (build </> "kernel.syms") (1024 * 1024)
  let expected = KernelDigests elfDigest textDigest symsDigest
  bound <- either error pure (bindKernel expected requiredFunctions kernelBytes suppliedText symsBytes)
  let symbols = M.map (\(KernelSymbol address size) -> Symbol address size) (boundSymbols bound)
      kernelText = boundText bound
  id5 <- B.readFile id5Path
  unless (id5Base == 0x00bf1a00 && B.length id5 == 5185120 && sha256Hex (sha256 id5) == "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a") $
    error "id5 differs from the pinned X-M1 1.50 image"
  derived <- either error pure $
    SI.deriveFunction (SI.GuestAddress id5Base) id5
      (SI.FunctionWindow (SI.GuestAddress (guestWord stockEntry)) (SI.GuestAddress 0x00c777fc)) []
  let declaration (address, size) = SI.FunctionDeclaration (SI.GuestAddress address) (SI.FunctionSize size)
  either error pure (SI.checkDeclarations [declaration (guestWord stockEntry, stockBytesCount)] [declaration derived])
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
  createDirectoryIfMissing True output
  let artifact path expectedDigest bytes = object ["path" .= path, "expected_sha256" .= (expectedDigest :: String), "sha256" .= sha256Hex (sha256 bytes)]
      expectedArtifact path digest bytes = object ["path" .= path, "expected_sha256" .= sha256Hex digest, "sha256" .= sha256Hex (sha256 bytes)]
  B.writeFile (output </> "inputs.json") $ encodeCanonical $ object
    [ "runner" .= artifact runner "b51544c62053d1bf31816d2c3e4b439e3847296b2eef1f99c8f17b5d75da2860" backend
    , "id5" .= artifact id5Path "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a" id5
    , "kernel" .= expectedArtifact (build </> "kernel.elf") elfDigest kernelBytes
    , "kernel_text" .= expectedArtifact (build </> "kernel-text.bin") textDigest suppliedText
    , "kernel_symbols" .= expectedArtifact (build </> "kernel.syms") symsDigest symsBytes
    , "elf_text_sha256" .= sha256Hex (sha256 kernelText)
    , "elf_text_address" .= hexWord32 (boundTextAddress bound)
    , "global_admission" .= False
    ]
  outcomes <- forM scenarios $ \(identifier, queryResult, sessionValue, transitionResult, sessionReadOnly) ->
    runScenario context identifier queryResult sessionValue transitionResult sessionReadOnly
  let stateOutcome = fromMaybe (error "state scenario missing") (find ((== "om-q0-s0-tminus1") . outcomeId) outcomes)
      oracle = outcomeOracle stateOutcome
      faultyAbiPath = output </> "om-faulty-abi"
      faultyStatePath = output </> "om-faulty-state"
  faultyAbiSpec <- pure (faultyCase context "om-faulty-abi" "negative control: execute owned cancellation, then change r4" faultyAbiFunction oracle)
  faultyAbiCasePath <- writeCase faultyAbiPath faultyAbiSpec
  faultyAbi <- loadObserved runner faultyAbiCasePath
  let faultyAbiCaught =
        nativeStatus faultyAbi == Returned
          && registerValue faultyAbi R4 == 0
          && null (compareObservation oracle faultyAbi {nativeRegisters = M.insert R4 (registerValue oracle R4) (nativeRegisters faultyAbi)})
  faultyStateSpec <- pure (faultyCase context "om-faulty-state" "negative control: execute owned cancellation, then change state to 0x1c" faultyStateFunction oracle)
  faultyStateCasePath <- writeCase faultyStatePath faultyStateSpec
  faultyState <- loadObserved runner faultyStateCasePath
  let stateMemory = regionBytesOf oracle (RegionId "state")
      faultyStateCaught =
        nativeStatus faultyState == Returned
          && all (\register -> registerValue faultyState register == registerValue oracle register) preservedRegisters
          && cpsrControl (registerValue faultyState Cpsr) == cpsrControl (registerValue oracle Cpsr)
          && regionBytesOf faultyState (RegionId "state") == B.take 2 stateMemory <> B.pack [0x1c, 0] <> B.drop 4 stateMemory
          && all (\ident -> regionBytesOf faultyState ident == regionBytesOf oracle ident) [RegionId "session", RegionId "log"]
          && nonStackWrites faultyState == fmap (++ [(GuestAddress (guestAddress stateBase + 2), 2)]) (nonStackWrites oracle)
          && callerStack faultyState == callerStack oracle
  guardPath <- writeCase (output </> "om-guard-fetch") (guardCase context)
  guard <- loadObserved runner guardPath
  faultyStackPath <- writeCase (output </> "om-faulty-caller-stack")
    (faultyCase context "om-faulty-caller-stack" "negative control: execute owned cancellation, then write at incoming SP" [0xe58d0008] oracle)
  faultyStack <- loadObserved runner faultyStackPath
  let guardOk = nativeStatus guard == FetchGuard && registerValue guard Pc == guestWord stockEntry
      faultyStackCaught =
        nativeStatus faultyStack == Returned
          && all (\register -> registerValue faultyStack register == registerValue oracle register) preservedRegisters
          && cpsrControl (registerValue faultyStack Cpsr) == cpsrControl (registerValue oracle Cpsr)
          && all (\ident -> regionBytesOf faultyStack ident == regionBytesOf oracle ident) memoryRegions
          && callerStack faultyStack == word32le (registerValue oracle R0) <> B.drop 4 (callerStack oracle)
          && callerStack faultyStack /= callerStack oracle
          && nonStackWrites faultyStack == fmap (++ [(GuestAddress (guestAddress stackBase + fromIntegral stackSize - 16), 4)]) (nonStackWrites oracle)
      controls :: [(String, Bool)]
      controls =
        [ ("faulty-abi", faultyAbiCaught)
        , ("faulty-state", faultyStateCaught)
        , ("faulty-caller-stack", faultyStackCaught)
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

readBounded :: FilePath -> Int -> IO B.ByteString
readBounded path limit = do
  bytes <- B.readFile path
  unless (not (B.null bytes) && B.length bytes <= limit) (error (path ++ ": size outside (0," ++ show limit ++ "]"))
  pure bytes
