{-# LANGUAGE GHC2021 #-}
-- | Original-versus-owned modal-75 editor-cancel differential cases.
module Main where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Bits (shiftR, xor)
import Data.List (find)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, mapMaybe)
import Data.String (fromString)
import Data.Word (Word32)
import Emulation.NativeCase (runCase)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (get, list, str)
import FujiAF.JSON (encodeCanonical)
import Numeric (readHex, showHex)
import System.Directory (createDirectoryIfMissing)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))

stockEntry, exitAddr, queryAddr, stageAddr, transitionAddr :: Word32
stockEntry = 0x00c77794
exitAddr = 0x00c77800
queryAddr = 0x0063a0f8
stageAddr = 0x00d3d858
transitionAddr = 0x00c12aac

stockBytesCount :: Int
stockBytesCount = 0x68

stateBase, sessionBase, dataBase, logBase, stackBase, guardBase :: Word32
stateBase = 0x0146761c
sessionBase = 0x00954d04
dataBase = 0x00956000
logBase = 0x00957000
stackBase = 0x00959000
guardBase = 0x00958000

stackSize, guardSize :: Int
stackSize = 0x1000
guardSize = 12

queryFunction, stageFunction, transitionFunction, guardFunction :: [Word32]
queryFunction = [0xe59fc008, 0xe58c0000, 0xe59f0004, 0xe12fff1e, logBase, dataBase]
stageFunction = [0xe59fc008, 0xe58c0004, 0xe58c1008, 0xe12fff1e, logBase]
transitionFunction = [0xe59fc010, 0xe59c200c, 0xe2822001, 0xe58c200c, 0xe59c0010, 0xe12fff1e, logBase]
guardFunction = [0xe59fc000, 0xe12fff1c, stockEntry]

hex :: Word32 -> String
hex x = "0x" ++ replicate (8 - length s) '0' ++ s where s = showHex x ""

hexI :: String -> Word32
hexI s = case readHex (if take 2 s == "0x" then drop 2 s else s) of
  [(value, "")] -> fromIntegral value
  _ -> error ("invalid hexadecimal: " ++ s)

hexB :: B.ByteString -> String
hexB = concatMap (\w -> let h = showHex w "" in replicate (2 - length h) '0' ++ h) . B.unpack

unhex :: String -> B.ByteString
unhex = B.pack . go
  where
    go (a : b : rest) = fromIntegral (digit a * 16 + digit b) : go rest
    go _ = []
    digit c = fromEnum c - (if c >= 'a' then fromEnum 'a' - 10 else fromEnum '0')

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
parseSymbols path = M.fromList . mapMaybe row . lines <$> readFile path
  where
    row line = case words line of
      [a, z, _, name] -> Just (name, Symbol (hexI a) (hexI z))
      [a, _, name] -> Just (name, Symbol (hexI a) 0)
      _ -> Nothing

symbol :: M.Map String Symbol -> String -> Symbol
symbol symbols name = fromMaybe (error ("kernel symbol missing: " ++ name)) (M.lookup name symbols)

symbolAddress :: M.Map String Symbol -> String -> Word32
symbolAddress symbols name = case symbol symbols name of Symbol address _ -> address

symbolSize :: M.Map String Symbol -> String -> Word32
symbolSize symbols name = case symbol symbols name of Symbol _ size -> size

data RegionSpec = RegionSpec
  { regionId :: String
  , regionBase :: Word32
  , regionSize :: Int
  , regionPermission :: String
  , regionBytes :: B.ByteString
  }

data CaseSpec = CaseSpec
  { caseId :: String
  , caseProvenance :: String
  , caseEntry :: Word32
  , caseExit :: Word32
  , caseRegions :: [RegionSpec]
  , caseFunctions :: [(Word32, Word32)]
  , caseInitial :: [(String, Word32)]
  , caseExpectedStatus :: String
  , caseExpectedRegisters :: [(String, Word32)]
  , caseExpectedMemory :: [(String, Int, B.ByteString)]
  }

exclusion :: String
exclusion = "single modal-75 action; the retained query, stage-open and transition services are modeled at their stock entries; no scheduler, interrupt, frame or device path"

regionValue :: RegionSpec -> Value
regionValue region =
  object
    [ "id" .= regionId region
    , "address" .= hex (regionBase region)
    , "size" .= regionSize region
    , "permissions" .= regionPermission region
    , "initial" .=
        ( if B.all (== 0) (regionBytes region)
            then object ["zero" .= True]
            else object ["artifact" .= regionId region, "offset" .= (0 :: Int), "length" .= B.length (regionBytes region)]
        )
    ]

artifactValue :: RegionSpec -> Value
artifactValue region =
  object
    [ "id" .= regionId region
    , "path" .= (regionId region ++ ".bin")
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
    , "artifacts" .= [artifactValue r | r <- caseRegions spec, not (B.all (== 0) (regionBytes r))]
    , "entry" .= hex (caseEntry spec)
    , "exit" .= hex (caseExit spec)
    , "registers" .= object [fromString name .= hex value | (name, value) <- caseInitial spec]
    , "regions" .= map regionValue (caseRegions spec)
    , "functions" .= [object ["address" .= hex address, "size" .= size] | (address, size) <- caseFunctions spec]
    , "stubs" .= ([] :: [Value])
    , "limits" .= object ["instructions" .= (4000000 :: Int), "wall_ms" .= (4000 :: Int)]
    , "expected" .=
        object
          [ "status" .= caseExpectedStatus spec
          , "registers" .= object [fromString name .= hex value | (name, value) <- caseExpectedRegisters spec]
          , "memory" .= [object ["region" .= name, "offset" .= offset, "hex" .= hexB bytes] | (name, offset, bytes) <- caseExpectedMemory spec]
          , "events" .= ([] :: [Value])
          ]
    , "exclusions" .= [exclusion]
    ]

writeCase :: FilePath -> CaseSpec -> IO FilePath
writeCase directory spec = do
  createDirectoryIfMissing True directory
  forM_ (caseRegions spec) $ \region ->
    when (not (B.all (== 0) (regionBytes region))) $
      B.writeFile (directory </> (regionId region ++ ".bin")) (regionBytes region)
  let path = directory </> "case.json"
  BL.writeFile path (encodeCanonical (caseValue spec))
  pure path

runnerObject :: Value -> Value
runnerObject report = get report "runner"

runnerStatus :: Value -> String
runnerStatus report = str (get (runnerObject report) "status")

registerWord :: Value -> String -> Word32
registerWord report name = hexI (str (get (get (runnerObject report) "registers") name))

regionWordBytes :: Value -> Word32 -> B.ByteString
regionWordBytes report base =
  case [entry | entry <- list (get (runnerObject report) "memory"), hexI (str (get entry "address")) == base] of
    [entry] -> unhex (str (get entry "bytes"))
    _ -> error ("runner memory missing region " ++ hex base)

abiRegisters :: [String]
abiRegisters = ["r0", "r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11", "sp", "pc"]

memoryRegions :: [String]
memoryRegions = ["state", "session", "log"]

memoryBase :: String -> Word32
memoryBase "state" = stateBase
memoryBase "session" = sessionBase
memoryBase "log" = logBase
memoryBase name = error ("unknown memory region " ++ name)

observation :: Value -> ([(String, Word32)], [(String, B.ByteString)])
observation report =
  ( [(name, registerWord report name) | name <- abiRegisters]
  , [(name, regionWordBytes report (memoryBase name)) | name <- memoryRegions]
  )

compareObservation :: [(String, Word32)] -> [(String, B.ByteString)] -> Value -> [String]
compareObservation expectedRegisters expectedMemory report =
  statusMismatch ++ registerMismatches ++ memoryMismatches
  where
    statusMismatch = ["status is " ++ runnerStatus report ++ ", not returned" | runnerStatus report /= "returned"]
    actualRegisters = [(name, registerWord report name) | name <- abiRegisters]
    registerMismatches =
      [ "register " ++ name ++ ": " ++ hex actual ++ ", not " ++ hex expected
      | (name, expected) <- expectedRegisters
      , Just actual <- [lookup name actualRegisters]
      , actual /= expected
      ]
    memoryMismatches =
      [ "memory " ++ name ++ " differs"
      | (name, expected) <- expectedMemory
      , regionWordBytes report (memoryBase name) /= expected
      ]

initialRegisters :: Word32 -> [(String, Word32)]
initialRegisters stack =
  [("lr", exitAddr), ("sp", stack), ("cpsr", 0x10)]
    ++ [("r" ++ show n, 0) | n <- [0 :: Int .. 12]]

scenarioRegions :: Word32 -> Word32 -> Word32 -> String -> [RegionSpec]
scenarioRegions queryResult sessionValue transitionResult sessionPermission =
  [ RegionSpec "state" stateBase 8 "rw" (B.replicate 8 0)
  , RegionSpec "session" sessionBase 8 sessionPermission (B.replicate 4 0 <> word32le sessionValue)
  , RegionSpec "data" dataBase 4 "r" (word32le queryResult)
  , RegionSpec "log" logBase 20 "rw" (B.replicate 16 0 <> word32le transitionResult)
  , RegionSpec "query-service" queryAddr 32 "rx" (assemble queryFunction)
  , RegionSpec "stage-service" stageAddr 32 "rx" (assemble stageFunction)
  , RegionSpec "transition-service" transitionAddr 32 "rx" (assemble transitionFunction)
  , RegionSpec "exit" exitAddr 4 "rx" (word32le 0xe12fff1e)
  , RegionSpec "stack" stackBase stackSize "rw" (B.replicate stackSize 0)
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
    baseRegions = scenarioRegions queryResult sessionValue transitionResult (if sessionReadOnly then "r" else "rw")
    serviceFunctions = [(queryAddr, 24), (stageAddr, 20), (transitionAddr, 28)]
    stock = CaseSpec
      { caseId = identifier ++ "-stock"
      , caseProvenance = provenanceText
      , caseEntry = stockEntry
      , caseExit = exitAddr
      , caseRegions = baseRegions ++ [RegionSpec "stock" stockEntry stockBytesCount "rx" (contextStockBytes context)]
      , caseFunctions = (stockEntry, fromIntegral stockBytesCount) : serviceFunctions
      , caseInitial = initialRegisters 0x00959ff0
      , caseExpectedStatus = "returned"
      , caseExpectedRegisters = []
      , caseExpectedMemory = []
      }
    owned = CaseSpec
      { caseId = identifier ++ "-owned"
      , caseProvenance = provenanceText
      , caseEntry = ownedEntry
      , caseExit = exitAddr
      , caseRegions = baseRegions
          ++ [ RegionSpec "stock" stockEntry 4 "r" (B.take 4 (contextStockBytes context))
             , RegionSpec "ktext" kernelBase (B.length (contextKernelText context)) "rx" (contextKernelText context)
             ]
      , caseFunctions = ownedFunctions
      , caseInitial = initialRegisters 0x00959ff0
      , caseExpectedStatus = "returned"
      , caseExpectedRegisters = []
      , caseExpectedMemory = []
      }
    ownedEntry = symbolAddress (contextSymbols context) "rb_ui_editor_cancel"
    kernelBase = symbolAddress (contextSymbols context) "__rb_image_start"
    ownedFunctions =
      [ (ownedEntry, symbolSize (contextSymbols context) "rb_ui_editor_cancel")
      , (symbolAddress (contextSymbols context) "rb_call_modal_query", symbolSize (contextSymbols context) "rb_call_modal_query")
      , (symbolAddress (contextSymbols context) "rb_open_editor_cancel_stage", symbolSize (contextSymbols context) "rb_open_editor_cancel_stage")
      , (symbolAddress (contextSymbols context) "rb_call_stage_open", symbolSize (contextSymbols context) "rb_call_stage_open")
      , (symbolAddress (contextSymbols context) "rb_call_transition", symbolSize (contextSymbols context) "rb_call_transition")
      ] ++ serviceFunctions

withExpected :: CaseSpec -> [(String, Word32)] -> [(String, B.ByteString)] -> CaseSpec
withExpected spec registers memory =
  spec { caseExpectedRegisters = registers, caseExpectedMemory = [(name, 0, bytes) | (name, bytes) <- memory] }

data Outcome = Outcome
  { outcomeId :: String
  , outcomeStockStatus :: String
  , outcomeOwnedStatus :: String
  , outcomeRegisters :: [(String, Word32)]
  , outcomeMemory :: [(String, B.ByteString)]
  , outcomeOwnedReport :: Value
  , outcomeMismatches :: [String]
  }

runScenario :: Context -> String -> Word32 -> Word32 -> Word32 -> Bool -> IO Outcome
runScenario context identifier queryResult sessionValue transitionResult sessionReadOnly = do
  let (stockSpec, ownedSpec) = scenario context identifier queryResult sessionValue transitionResult sessionReadOnly
      probeSpec = stockSpec { caseId = identifier ++ "-probe", caseExpectedRegisters = [("r0", 0)], caseExpectedMemory = [] }
  probeDirectory <- pure (contextOutput context </> caseId probeSpec)
  probePath <- writeCase probeDirectory probeSpec
  probeReport <- snd <$> runCase (contextRunner context) probePath
  when (runnerObject probeReport == Null) $
    error (identifier ++ ": oracle case was invalid: " ++ show probeReport)
  when (runnerStatus probeReport /= "returned") $
    error (identifier ++ ": oracle did not return: " ++ runnerStatus probeReport)
  let (oracleRegisters, oracleMemory) = observation probeReport
      stockVerified = withExpected stockSpec oracleRegisters oracleMemory
      ownedVerified = withExpected ownedSpec oracleRegisters oracleMemory
  stockPath <- writeCase (contextOutput context </> caseId stockVerified) stockVerified
  stockReport <- snd <$> runCase (contextRunner context) stockPath
  when (runnerObject stockReport == Null) $
    error (identifier ++ ": stock case was invalid: " ++ show stockReport)
  let stockMismatches = compareObservation oracleRegisters oracleMemory stockReport
  unless (null stockMismatches) $
    error (identifier ++ ": oracle is not stable: " ++ show stockMismatches)
  ownedPath <- writeCase (contextOutput context </> caseId ownedVerified) ownedVerified
  ownedReport <- snd <$> runCase (contextRunner context) ownedPath
  when (runnerObject ownedReport == Null) $
    error (identifier ++ ": owned case was invalid: " ++ show ownedReport)
  let mismatches = compareObservation oracleRegisters oracleMemory ownedReport
  pure Outcome
    { outcomeId = identifier
    , outcomeStockStatus = runnerStatus stockReport
    , outcomeOwnedStatus = runnerStatus ownedReport
    , outcomeRegisters = oracleRegisters
    , outcomeMemory = oracleMemory
    , outcomeOwnedReport = ownedReport
    , outcomeMismatches = mismatches
    }

guardCase :: Context -> CaseSpec
guardCase context = CaseSpec
  { caseId = "om-guard-fetch"
  , caseProvenance = "negative control: a modelled transfer to 0x00c77794 must fetch-guard when that address is not executable"
  , caseEntry = guardBase
  , caseExit = exitAddr
  , caseRegions =
      [ RegionSpec "guard" guardBase guardSize "rx" (assemble guardFunction)
      , RegionSpec "stock" stockEntry 4 "r" (B.take 4 (contextStockBytes context))
      , RegionSpec "exit" exitAddr 4 "rx" (word32le 0xe12fff1e)
      , RegionSpec "stack" stackBase stackSize "rw" (B.replicate stackSize 0)
      ]
  , caseFunctions = [(guardBase, fromIntegral guardSize)]
  , caseInitial = initialRegisters 0x00959ff0
  , caseExpectedStatus = "returned"
  , caseExpectedRegisters = [("r0", 0)]
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
  let id5Base = hexI id5BaseArgument
  symbols <- parseSymbols (build </> "kernel.syms")
  kernelText <- B.readFile (build </> "kernel-text.bin")
  id5 <- B.readFile id5Path
  let stockBytes = B.take stockBytesCount (B.drop (fromIntegral (stockEntry - id5Base)) id5)
      context = Context runner output symbols kernelText stockBytes
      scenarios =
        [ ("om-q0-s0-tminus1", 0, 0, 0xffffffff, False)
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
      (stateRegisters, stateMemory) = (outcomeRegisters stateOutcome, outcomeMemory stateOutcome)
      actualOwned = outcomeOwnedReport stateOutcome
      wrongReturn = compareObservation (change "r0" (+ 1) stateRegisters) stateMemory actualOwned
      wrongState = compareObservation stateRegisters (changeMemory "state" flipStateByte stateMemory) actualOwned
  guardPath <- writeCase (output </> "om-guard-fetch") (guardCase context)
  guardReport <- snd <$> runCase runner guardPath
  when (runnerObject guardReport == Null) $
    error ("guard case was invalid: " ++ show guardReport)
  let guardOk = runnerStatus guardReport == "fetch_guard"
      controls =
        [ ("wrong-return", not (null wrongReturn))
        , ("wrong-state", not (null wrongState))
        , ("forbidden-fetch", guardOk)
        ]
  let failed = [outcome | outcome <- outcomes, not (null (outcomeMismatches outcome))]
      controlsOk = all snd controls
  BL.writeFile (output </> "owned-menu-results.json") (encodeCanonical (object
    [ "cases" .= [object
        [ "id" .= outcomeId outcome
        , "stock_status" .= outcomeStockStatus outcome
        , "owned_status" .= outcomeOwnedStatus outcome
        , "mismatches" .= outcomeMismatches outcome
        ] | outcome <- outcomes]
    , "controls" .= [object ["id" .= name, "ok" .= ok] | (name, ok) <- controls]
    ]))
  putStrLn ("ran " ++ show (length outcomes) ++ " differential cases: "
    ++ show (length outcomes - length failed) ++ " matched; controls "
    ++ (if controlsOk then "refused as required" else "FAILED"))
  when (not (null failed) || not controlsOk) exitFailure

change :: String -> (Word32 -> Word32) -> [(String, Word32)] -> [(String, Word32)]
change name f = map (\(key, value) -> (key, if key == name then f value else value))

changeMemory :: String -> (B.ByteString -> B.ByteString) -> [(String, B.ByteString)] -> [(String, B.ByteString)]
changeMemory name f = map (\(key, value) -> (key, if key == name then f value else value))

flipStateByte :: B.ByteString -> B.ByteString
flipStateByte bytes =
  B.take 2 bytes <> B.singleton (B.index bytes 2 `xor` 1) <> B.drop 3 bytes

required :: [String] -> String -> IO String
required arguments name =
  case dropWhile (/= name) arguments of
    (_ : value : _) -> pure value
    _ -> error ("missing " ++ name)
