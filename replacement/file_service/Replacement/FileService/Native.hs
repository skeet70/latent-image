{-# LANGUAGE OverloadedStrings #-}

-- | Stock-against-owned native cases for the file service: one A32 driver calls stock entries
-- in order over scripted stock services, and the stock image bytes, the owned ELF and each
-- deliberately wrong owned ELF run the same cases.
module Replacement.FileService.Native
  ( Extent (..)
  , extentContains
  , Code (..)
  , DataRegion (..)
  , Call (..)
  , Service (..)
  , MemoryRow (..)
  , Scenario (..)
  , Stop (..)
  , Role (..)
  , Observation (..)
  , runRole
  , compareRoles
  , controlDetections
  , word32le
  , words32le
  , hexWord
  ) where

import qualified ArmElf as E
import Control.Exception (evaluate)
import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value (..), encode, object, (.=))
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.IORef (modifyIORef', newIORef, readIORef)
import Data.List (nub, partition)
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32)
import qualified Emulation.NativeCase as Native
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (showHex)
import qualified Replacement.FixedImage.Layout as Layout
import System.Directory (createDirectoryIfMissing, doesFileExist)
import System.FilePath ((</>))

-- | A retired stock extent @[low, high)@ of image 4.
data Extent = Extent {extentLow :: Word32, extentHigh :: Word32}
  deriving (Eq, Show)

extentContains :: Extent -> Word32 -> Bool
extentContains (Extent low high) address = low <= address && address < high

-- | The retired extents a suite maps and the stock function starts and sizes inside them,
-- and the retained stock code both roles run, with its function starts and sizes.
data Code = Code
  { codeImageBase :: Word32
  , codeExtents :: [(String, Extent)]
  , codeStockFunctions :: [(Word32, Word32)]
  , codeRetained :: [(String, Extent)]
  , codeRetainedFunctions :: [(Word32, Word32)]
  }

-- | Stock data a case maps, with its initial bytes and its exact final bytes.
data DataRegion = DataRegion
  { dataName :: String
  , dataAddress :: Word32
  , dataInitial :: B.ByteString
  , dataFinal :: B.ByteString
  }

-- | One driver call: the stock entry, r0..r3, the words at sp, sp+4, ..., and whether r0 is a result.
data Call = Call
  { callEntry :: Word32
  , callArguments :: [Word32]
  , callStack :: [Word32]
  , callReturns :: Bool
  }

data MemoryRow
  = AtAddress Word32 Int B.ByteString
  | AtRegister String Int B.ByteString
  | AtIndirect String Int B.ByteString

-- | One scripted stock service call, in the order the case makes them.
data Service = Service
  { serviceAddress :: Word32
  , serviceInputs :: [(String, Word32)]
  , serviceOutputs :: [(String, Word32)]
  , serviceReads :: [MemoryRow]
  , serviceWrites :: [MemoryRow]
  }

-- | Where a kernel case ends, and the registers execution must hold there besides pc.
data Stop = Stop
  { stopAddress :: Word32
  , stopRegisters :: [(String, Word32)]
  }

-- | A scenario with a stop is a kernel case that ends when execution reaches it.
data Scenario = Scenario
  { scenarioName :: String
  , scenarioEntry :: Word32
  , scenarioData :: [DataRegion]
  , scenarioCalls :: [Call]
  , scenarioServices :: [Service]
  , scenarioResults :: [Word32]
  , scenarioStop :: Maybe Stop
  }

data Role = Stock | Owned String E.Elf

roleLabel :: Role -> String
roleLabel Stock = "stock"
roleLabel (Owned label _) = label

-- | The role part of a case id; each role runs in its own batch, so every control shares one.
roleTag :: Role -> String
roleTag Stock = "stock"
roleTag (Owned "owned" _) = "owned"
roleTag (Owned _ _) = "control"

data Observation = Observation
  { observationCase :: String
  , observationPassed :: Bool
  , observationRunnerStatus :: String
  , observationMismatches :: [String]
  , observationInstructions :: Integer
  , observationMemory :: [(String, String)]
  , observationEvents :: Value
  , observationRegisters :: Value
  }

hexWord :: Word32 -> String
hexWord value = "0x" ++ replicate (8 - length digits) '0' ++ digits
  where
    digits = showHex value ""

word32le :: Word32 -> B.ByteString
word32le value = B.pack [fromIntegral (value `shiftR` shift) | shift <- [0, 8, 16, 24]]

words32le :: [Word32] -> B.ByteString
words32le = B.concat . map word32le

registers :: [(String, Word32)] -> Value
registers pairs = object [Key.fromString name .= hexWord value | (name, value) <- pairs]

-- Driver ------------------------------------------------------------------

driverBase, resultsBase, stackBase, stackTop, exitAddress, trapWord :: Word32
driverBase = 0x300000
resultsBase = 0x301000
stackBase = 0x302000
stackTop = 0x304000
exitAddress = 0x305000
trapWord = 0xe7f000f0

movw, movt :: Word32 -> Word32 -> Word32
movw register value = 0xe3000000 .|. ((value .&. 0xf000) `shiftL` 4) .|. (register `shiftL` 12) .|. (value .&. 0xfff)
movt register value =
  0xe3400000 .|. (((value `shiftR` 16) .&. 0xf000) `shiftL` 4) .|. (register `shiftL` 12) .|. ((value `shiftR` 16) .&. 0xfff)

branchLink :: Word32 -> Word32 -> Word32
branchLink pc target = 0xeb000000 .|. (((target - pc - 8) `shiftR` 2) .&. 0xffffff)

driverWords :: [Call] -> [Word32]
driverWords calls = 0xe92d4010 : go (driverBase + 4) 0 calls ++ [0xe8bd8010]
  where
    go _ _ [] = []
    go pc slot (Call entry arguments stack returns : rest) =
      let frame = 8 * ((fromIntegral (length stack) + 1) `div` 2) :: Word32
          reserve = if null stack then [] else [0xe24dd000 .|. frame]
          spill = concat [[movw 12 value, movt 12 value, 0xe58dc000 .|. (4 * index)] | (index, value) <- zip [0 ..] stack]
          loads = concat [[movw register value, movt register value] | (register, value) <- zip [0 ..] arguments]
          before = reserve ++ spill ++ loads
          callAt = pc + 4 * fromIntegral (length before)
          release = if null stack then [] else [0xe28dd000 .|. frame]
          store = if returns then [movw 12 (resultsBase .&. 0xffff), movt 12 resultsBase, 0xe58c0000 .|. (4 * slot)] else []
          block = before ++ [branchLink callAt entry] ++ release ++ store
       in block ++ go (pc + 4 * fromIntegral (length block)) (if returns then slot + 1 else slot) rest

-- Case documents ------------------------------------------------------------

data Region = Region String Word32 String B.ByteString

codeBytes :: B.ByteString -> Code -> Role -> Extent -> B.ByteString
codeBytes image code role extent@(Extent low high) = case role of
  Stock -> B.take size (B.drop (fromIntegral (low - codeImageBase code)) image)
  Owned _ elf ->
    B.pack
      [ byte
      | address <- [low .. high - 1]
      , let owned = [E.byteAt elf address | section <- E.elfSections elf, E.sectionAllocated section, inside section address]
            trap = B.index (word32le trapWord) (fromIntegral (address .&. 3))
            byte = case owned of
              (Just value : _) -> value
              _ -> trap
      ]
  where
    size = fromIntegral (high - low)
    inside section address =
      E.sectionSize section > 0
        && E.sectionAddress section <= address
        && address < E.sectionAddress section + E.sectionSize section
        && extentContains extent (E.sectionAddress section)

-- | The owned link's fixed-bank sections as one region from the lowest section to the end of the highest.
fixedBody :: E.Elf -> Maybe (Word32, B.ByteString)
fixedBody elf = case sections of
  [] -> Nothing
  _ ->
    let low = minimum (map E.sectionAddress sections)
        high = maximum [E.sectionAddress section + E.sectionSize section | section <- sections]
     in Just (low, B.pack [maybe 0 id (E.byteAt elf address) | address <- [low .. high - 1]])
  where
    sections =
      [ section
      | section <- E.elfSections elf
      , E.sectionAllocated section
      , E.sectionSize section > 0
      , extentContains ownedFixed (E.sectionAddress section)
      ]

ownedFixed :: Extent
ownedFixed = Extent (Layout.imageBase + Layout.headerBytes) Layout.imageLimit

-- | Stock functions, or the owned functions in the retired extents and those direct A32 branches reach from them.
functionsOf :: Code -> Role -> [(Word32, Word32)]
functionsOf code Stock = codeStockFunctions code
functionsOf code (Owned _ elf) = M.toList (reach entries seeds)
  where
    functions = M.fromList
      [ (E.symbolValue symbol, E.symbolSize symbol)
      | symbol <- E.elfSymbols elf
      , E.symbolKind symbol == E.FunctionSymbol
      , E.symbolSize symbol > 0
      , any (\extent -> extentContains extent (E.symbolValue symbol)) (ownedFixed : map snd (codeExtents code))
      ]
    entries = M.filterWithKey (\low _ -> any (\(_, extent) -> extentContains extent low) (codeExtents code)) functions
    seeds = concat [branchTargets low (high - low) | (_, Extent low high) <- codeExtents code]
    containing target = case M.lookupLE target functions of
      Just (low, size) | target < low + size -> Just (low, size)
      _ -> Nothing
    reach found [] = found
    reach found (target : rest) = case containing target of
      Just (low, size) | not (M.member low found) -> reach (M.insert low size found) (branchTargets low size ++ rest)
      _ -> reach found rest
    branchTargets low size =
      [ fromIntegral (toInteger address + 8 + 4 * signed)
      | address <- [low, low + 4 .. low + size - 4]
      , Just instruction <- [wordAt address]
      , instruction `shiftR` 28 /= 0xf
      , instruction .&. 0x0e000000 == 0x0a000000
      , let offset = toInteger (instruction .&. 0xffffff)
            signed = if offset >= 0x800000 then offset - 0x1000000 else offset
      ]
    wordAt :: Word32 -> Maybe Word32
    wordAt address = do
      bytes <- mapM (E.byteAt elf) [address .. address + 3]
      pure (foldr (\byte value -> value `shiftL` 8 .|. fromIntegral byte) 0 bytes)

rowValue :: MemoryRow -> Value
rowValue row = case row of
  AtAddress address offset bytes -> object ["address" .= hexWord address, "offset" .= offset, "hex" .= Native.hexBytes bytes]
  AtRegister register offset bytes -> object ["register" .= register, "offset" .= offset, "hex" .= Native.hexBytes bytes]
  AtIndirect register offset bytes -> object ["indirect_register" .= register, "offset" .= offset, "hex" .= Native.hexBytes bytes]

preserved :: [(String, Word32)]
preserved = [("r" ++ show index, 0x10101010 * fromIntegral index) | index <- [4 :: Int .. 11]]

-- | One scenario's case; @body@ is the owned role's 'fixedBody'.
caseDocument :: Code -> B.ByteString -> Role -> Maybe (Word32, B.ByteString) -> Scenario -> (Value, [(String, B.ByteString)])
caseDocument code image role body scenario = (document, artifacts)
  where
    driver = words32le (driverWords (scenarioCalls scenario))
    regions =
      [Region name low "rx" (codeBytes image code role extent) | (name, extent@(Extent low _)) <- codeExtents code]
        ++ [Region name low "rx" (codeBytes image code Stock extent) | (name, extent@(Extent low _)) <- codeRetained code]
        ++ [Region "owned-fixed" low "rx" bytes | Just (low, bytes) <- [body]]
        ++ [Region "stop" (stopAddress stop) "rx" (word32le trapWord) | Just stop <- [scenarioStop scenario]]
        ++ [Region (dataName region) (dataAddress region) "rw" (dataInitial region) | region <- scenarioData scenario]
        ++ [ Region "driver" driverBase "rx" driver
           , Region "results" resultsBase "rw" (B.replicate 0x100 0)
           , Region "stack" stackBase "rw" (B.replicate (fromIntegral (stackTop - stackBase)) 0xa5)
           , Region "exit" exitAddress "rx" (word32le 0xe12fff1e)
           ]
    -- The owned bank body is its own artifact, so a batch reads its bytes once across cases.
    (bodyRegions, caseRegions) = partition (\(Region name _ _ _) -> name == "owned-fixed") regions
    placed = concatMap place (filter (not . null) [caseRegions, bodyRegions])
    place group =
      let blob = B.concat [bytes | Region _ _ _ bytes <- group]
       in [(region, sha256Hex (sha256 blob), offset, blob) | (region, offset) <- zip group (scanl (+) 0 [B.length bytes | Region _ _ _ bytes <- group])]
    blobs = nub [(name, blob) | (_, name, _, blob) <- placed]
    artifactId name = "a-" ++ take 40 name
    artifacts = [(name ++ ".bin", blob) | (name, blob) <- blobs]
    stackFrame = object ["region" .= ("stack" :: String), "offset" .= (0 :: Int), "length" .= (fromIntegral (stackTop - stackBase) :: Int)]
    services = scenarioServices scenario
    stubs =
      [ object
          [ "address" .= hexWord address
          , "calls" .=
              [ object
                  [ "inputs" .= registers (serviceInputs service)
                  , "outputs" .= registers (serviceOutputs service)
                  , "reads" .= map rowValue (serviceReads service)
                  , "writes" .= map rowValue (serviceWrites service)
                  ]
              | service <- services
              , serviceAddress service == address
              ]
          ]
      | address <- nub (map serviceAddress services)
      ]
    events =
      [ object
          [ "address" .= hexWord (serviceAddress service)
          , "inputs" .= registers (serviceInputs service)
          , "outputs" .= registers (serviceOutputs service)
          ]
      | service <- services
      ]
    initial = [("r" ++ show index, 0x0e0e0e00 + fromIntegral index) | index <- [0 :: Int .. 3]] ++ preserved
      ++ [("r12", 0x0c0c0c0c), ("sp", stackTop), ("lr", exitAddress), ("cpsr", maybe 0x10 (const 0xd3) (scenarioStop scenario))]
    expectedMemory =
      [object ["region" .= dataName region, "offset" .= (0 :: Int), "hex" .= Native.hexBytes (dataFinal region)] | region <- scenarioData scenario]
        ++ [ object ["region" .= ("results" :: String), "offset" .= (0 :: Int), "hex" .= Native.hexBytes (words32le (scenarioResults scenario))]
           | not (null (scenarioResults scenario))
           ]
    document =
      object $
        [ "schema_version" .= (1 :: Int)
        , "id" .= (scenarioName scenario ++ "-" ++ roleTag role)
        , "origin" .= (case role of Stock -> "stock"; Owned _ _ -> "synthetic" :: String)
        , "provenance" .= ("X-M1 1.50 image4 file-service extents at x70view 55436fc85c0560dd252b0410af6f2427fe198cd3 byte-read boundaries; " ++ roleLabel role ++ " role" :: String)
        , "cpu" .= object ["architecture" .= ("arm" :: String), "model" .= ("cortex-r5f" :: String), "mode" .= ("arm" :: String), "endian" .= ("little" :: String)]
        , "artifacts" .= [object ["id" .= artifactId name, "path" .= (name ++ ".bin"), "size" .= B.length blob, "sha256" .= name] | (name, blob) <- blobs]
        , "entry" .= hexWord driverBase
        , "exit" .= hexWord exitAddress
        , "registers" .= registers initial
        , "regions" .=
            [ object
                [ "id" .= name
                , "address" .= hexWord address
                , "size" .= B.length bytes
                , "permissions" .= permissions
                , "initial" .= object ["artifact" .= artifactId blobName, "offset" .= offset, "length" .= B.length bytes]
                ]
            | (Region name address permissions bytes, blobName, offset, _) <- placed
            ]
        , "functions" .=
            [ object ["address" .= hexWord address, "size" .= size]
            | (address, size) <- functionsOf code role ++ codeRetainedFunctions code ++ [(driverBase, fromIntegral (B.length driver))]
            ]
        , "stubs" .= stubs
        , "limits" .= object ["instructions" .= (200000 :: Int), "wall_ms" .= (2000 :: Int)]
        ]
          ++ [ "ignored_memory" .= [object ["region" .= ("stack" :: String), "offset" .= (0 :: Int), "length" .= (fromIntegral (stackTop - stackBase) :: Int), "reason" .= ("private-frame" :: String)]]
             | Nothing <- [scenarioStop scenario]
             ]
          ++ [ "expected" .=
                 object
                   ( [ "status" .= (maybe "returned" (const "stop_address") (scenarioStop scenario) :: String)
                     , "registers" .= registers (maybe (preserved ++ [("sp", stackTop), ("pc", exitAddress)]) (\stop -> stopRegisters stop ++ [("pc", stopAddress stop)]) (scenarioStop scenario))
                     , "memory" .= expectedMemory
                     , "events" .= events
                     ]
                       ++ ["ignored_memory" .= [stackFrame] | Just _ <- [scenarioStop scenario]]
                   )
             , "exclusions" .= ["Host Cortex-R5F native profile. Stock services are scripted stubs at their entries; no kernel, scheduling, interrupt or device qualification." :: String]
             ]
          ++ ["kernel" .= object ["stop" .= hexWord (stopAddress stop)] | Just stop <- [scenarioStop scenario]]

-- Execution ---------------------------------------------------------------

field :: String -> Value -> Value
field name (Object fields) = maybe Null id (KM.lookup (Key.fromString name) fields)
field _ _ = Null

text :: Value -> String
text (String value) = T.unpack value
text _ = ""

-- | The report's verdict, registers, events and the memory rows at @compared@ addresses, fully
-- evaluated so the report itself is not retained.
observe :: [String] -> Native.CaseReport -> Observation
observe compared report = forced `seq` observation
  where
    value = Native.reportValue report
    runner = field "runner" value
    observation = Observation
      { observationCase = Native.caseIdText (Native.reportCase report)
      , observationPassed = text (field "status" value) == "passed"
      , observationRunnerStatus = text (field "status" runner)
      , observationMismatches = case field "mismatches" value of
          Array items -> map text (V.toList items)
          _ -> []
      , observationInstructions = case field "instructions" runner of
          Number count -> truncate count
          String count | [(parsed, "")] <- reads (T.unpack count) -> parsed
          _ -> 0
      , observationMemory = case field "memory" runner of
          Array items -> [(address, text (field "bytes" item)) | item <- V.toList items, let address = text (field "address" item), address `elem` compared]
          _ -> []
      , observationEvents = field "events" runner
      , observationRegisters = field "registers" runner
      }
    strings = concat ([observationCase observation, observationRunnerStatus observation] ++ observationMismatches observation
      ++ concat [[address, bytes] | (address, bytes) <- observationMemory observation])
    forced = foldr seq () strings `seq` observationPassed observation `seq` observationInstructions observation
      `seq` observationEvents observation `seq` observationRegisters observation `seq` ()

-- | Writes each scenario's case for one role into its own directory and runs them as one batch.
runRole :: FilePath -> FilePath -> Code -> B.ByteString -> Role -> [Scenario] -> IO [Observation]
runRole runner directory code image role scenarios = do
  let batchDirectory = directory </> roleLabel role
      compared = nub (hexWord resultsBase : [hexWord (dataAddress region) | scenario <- scenarios, region <- scenarioData scenario])
  createDirectoryIfMissing True batchDirectory
  let body = case role of
        Owned _ elf -> fixedBody elf
        Stock -> Nothing
  entries <- forM scenarios $ \scenario -> do
    let (document, artifacts) = caseDocument code image role body scenario
        ident = scenarioName scenario ++ "-" ++ roleTag role
    forM_ artifacts $ \(name, bytes) -> do
      written <- doesFileExist (batchDirectory </> name)
      unless written (B.writeFile (batchDirectory </> name) bytes)
    BL.writeFile (batchDirectory </> (ident ++ ".case.json")) (encode document)
    pure (object ["id" .= ident, "path" .= (ident ++ ".case.json")])
  let batchPath = batchDirectory </> "batch.json"
  BL.writeFile batchPath (encode (object ["schema_version" .= (1 :: Int), "cases" .= entries]))
  collected <- newIORef []
  summary <- Native.runBatch runner batchPath $ \report -> do
    BL.writeFile (batchDirectory </> (Native.caseIdText (Native.reportCase report) ++ ".report.json")) (encode (Native.reportValue report))
    observation <- evaluate (observe compared report)
    modifyIORef' collected (observation :)
  observations <- reverse <$> readIORef collected
  unless (Native.summaryStatus summary == Native.BatchComplete && length observations == length scenarios) $
    fail ("native batch did not complete for " ++ roleLabel role ++ ": " ++ show (Native.summaryReason summary))
  pure observations

-- | The differences between the stock and owned observations of one scenario: either role
-- failing its independent expectation, and any difference in the data regions, the
-- results, the ordered service events, the preserved registers on return, or r0..r3 and sp
-- at a stop.
compareRoles :: Scenario -> Observation -> Observation -> [String]
compareRoles scenario stock owned =
  [ role ++ " " ++ observationRunnerStatus observation ++ ": " ++ unwords (observationMismatches observation)
  | (role, observation) <- [("stock", stock), ("owned", owned)]
  , not (observationPassed observation)
  ]
    ++ ["memory " ++ name | (name, address) <- compared, memoryAt address stock /= memoryAt address owned]
    ++ ["service events" | observationEvents stock /= observationEvents owned]
    ++ ["registers" | map (registerOf stock) kept /= map (registerOf owned) kept]
  where
    compared = [(dataName region, dataAddress region) | region <- scenarioData scenario] ++ [("results", resultsBase)]
    memoryAt address observation = lookup (hexWord address) (observationMemory observation)
    kept = maybe (map fst preserved ++ ["sp"]) (const ["r0", "r1", "r2", "r3", "sp"]) (scenarioStop scenario)
    registerOf observation name = field name (observationRegisters observation)

-- | The cases a deliberately wrong owned ELF fails after reaching the case exit: a
-- returned or stopped status with a wrong observation, or @stub_contract@ at the exit
-- address, the runner's status for a scripted call the case never made. Other statuses are
-- listed apart.
controlDetections :: [Observation] -> ([String], [(String, String)])
controlDetections observations =
  ( [observationCase observation | observation <- observations, atExit observation, not (observationPassed observation)]
  , [ (observationCase observation, observationRunnerStatus observation)
    | observation <- observations
    , not (atExit observation)
    ]
  )
  where
    atExit observation =
      observationRunnerStatus observation `elem` ["returned", "stop_address"]
        || ( observationRunnerStatus observation == "stub_contract"
               && field "pc" (observationRegisters observation) == String (T.pack (hexWord exitAddress))
           )
