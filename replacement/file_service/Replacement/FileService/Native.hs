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
import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value (..), encode, object, (.=))
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.IORef (modifyIORef', newIORef, readIORef)
import Data.List (nub)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32)
import qualified Emulation.NativeCase as Native
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (showHex)
import qualified Replacement.FixedImage.Layout as Layout
import System.Directory (createDirectoryIfMissing)
import System.FilePath ((</>))

-- | A retired stock extent @[low, high)@ of image 4.
data Extent = Extent {extentLow :: Word32, extentHigh :: Word32}
  deriving (Eq, Show)

extentContains :: Extent -> Word32 -> Bool
extentContains (Extent low high) address = low <= address && address < high

-- | The retired extents a suite maps and the stock function starts and sizes inside them.
data Code = Code
  { codeImageBase :: Word32
  , codeExtents :: [(String, Extent)]
  , codeStockFunctions :: [(Word32, Word32)]
  }

-- | Stock data a case maps, with its initial bytes and its exact final bytes.
data DataRegion = DataRegion
  { dataName :: String
  , dataAddress :: Word32
  , dataInitial :: B.ByteString
  , dataFinal :: B.ByteString
  }

-- | One driver call: the stock entry, r0..r3, and whether r0 is a result.
data Call = Call
  { callEntry :: Word32
  , callArguments :: [Word32]
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

data Scenario = Scenario
  { scenarioName :: String
  , scenarioEntry :: Word32
  , scenarioData :: [DataRegion]
  , scenarioCalls :: [Call]
  , scenarioServices :: [Service]
  , scenarioResults :: [Word32]
  }

data Role = Stock | Owned String E.Elf

roleLabel :: Role -> String
roleLabel Stock = "stock"
roleLabel (Owned label _) = label

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
    go pc slot (Call entry arguments returns : rest) =
      let loads = concat [[movw register value, movt register value] | (register, value) <- zip [0 ..] arguments]
          callAt = pc + 4 * fromIntegral (length loads)
          store = if returns then [movw 12 (resultsBase .&. 0xffff), movt 12 resultsBase, 0xe58c0000 .|. (4 * slot)] else []
          block = loads ++ [branchLink callAt entry] ++ store
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

functionsOf :: Code -> Role -> [(Word32, Word32)]
functionsOf code Stock = codeStockFunctions code
functionsOf code (Owned _ elf) = nub
  [ (E.symbolValue symbol, E.symbolSize symbol)
  | symbol <- E.elfSymbols elf
  , E.symbolKind symbol == E.FunctionSymbol
  , E.symbolSize symbol > 0
  , any (\extent -> extentContains extent (E.symbolValue symbol)) (ownedFixed : map snd (codeExtents code))
  ]

rowValue :: MemoryRow -> Value
rowValue row = case row of
  AtAddress address offset bytes -> object ["address" .= hexWord address, "offset" .= offset, "hex" .= Native.hexBytes bytes]
  AtRegister register offset bytes -> object ["register" .= register, "offset" .= offset, "hex" .= Native.hexBytes bytes]
  AtIndirect register offset bytes -> object ["indirect_register" .= register, "offset" .= offset, "hex" .= Native.hexBytes bytes]

preserved :: [(String, Word32)]
preserved = [("r" ++ show index, 0x10101010 * fromIntegral index) | index <- [4 :: Int .. 11]]

caseDocument :: Code -> B.ByteString -> Role -> Scenario -> (Value, [(String, B.ByteString)])
caseDocument code image role scenario = (document, artifacts)
  where
    driver = words32le (driverWords (scenarioCalls scenario))
    regions =
      [Region name low "rx" (codeBytes image code role extent) | (name, extent@(Extent low _)) <- codeExtents code]
        ++ [Region "owned-fixed" low "rx" bytes | Owned _ elf <- [role], Just (low, bytes) <- [fixedBody elf]]
        ++ [Region (dataName region) (dataAddress region) "rw" (dataInitial region) | region <- scenarioData scenario]
        ++ [ Region "driver" driverBase "rx" driver
           , Region "results" resultsBase "rw" (B.replicate 0x100 0)
           , Region "stack" stackBase "rw" (B.replicate (fromIntegral (stackTop - stackBase)) 0xa5)
           , Region "exit" exitAddress "rx" (word32le 0xe12fff1e)
           ]
    artifactName bytes = sha256Hex (sha256 bytes)
    artifactId bytes = "a-" ++ take 40 (artifactName bytes)
    artifacts = nub [(artifactName bytes ++ ".bin", bytes) | Region _ _ _ bytes <- regions]
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
      ++ [("r12", 0x0c0c0c0c), ("sp", stackTop), ("lr", exitAddress), ("cpsr", 0x10)]
    expectedMemory =
      [object ["region" .= dataName region, "offset" .= (0 :: Int), "hex" .= Native.hexBytes (dataFinal region)] | region <- scenarioData scenario]
        ++ [ object ["region" .= ("results" :: String), "offset" .= (0 :: Int), "hex" .= Native.hexBytes (words32le (scenarioResults scenario))]
           | not (null (scenarioResults scenario))
           ]
    document =
      object
        [ "schema_version" .= (1 :: Int)
        , "id" .= (scenarioName scenario ++ "-" ++ roleLabel role)
        , "origin" .= (case role of Stock -> "stock"; Owned _ _ -> "synthetic" :: String)
        , "provenance" .= ("X-M1 1.50 image4 file-service extents at x70view 55436fc85c0560dd252b0410af6f2427fe198cd3 byte-read boundaries; " ++ roleLabel role ++ " role" :: String)
        , "cpu" .= object ["architecture" .= ("arm" :: String), "model" .= ("cortex-r5f" :: String), "mode" .= ("arm" :: String), "endian" .= ("little" :: String)]
        , "artifacts" .= [object ["id" .= artifactId bytes, "path" .= name, "size" .= B.length bytes, "sha256" .= sha256Hex (sha256 bytes)] | (name, bytes) <- artifacts]
        , "entry" .= hexWord driverBase
        , "exit" .= hexWord exitAddress
        , "registers" .= registers initial
        , "regions" .=
            [ object
                [ "id" .= name
                , "address" .= hexWord address
                , "size" .= B.length bytes
                , "permissions" .= permissions
                , "initial" .= object ["artifact" .= artifactId bytes, "offset" .= (0 :: Int), "length" .= B.length bytes]
                ]
            | Region name address permissions bytes <- regions
            ]
        , "functions" .=
            [ object ["address" .= hexWord address, "size" .= size]
            | (address, size) <- functionsOf code role ++ [(driverBase, fromIntegral (B.length driver))]
            ]
        , "stubs" .= stubs
        , "limits" .= object ["instructions" .= (200000 :: Int), "wall_ms" .= (2000 :: Int)]
        , "ignored_memory" .= [object ["region" .= ("stack" :: String), "offset" .= (0 :: Int), "length" .= (fromIntegral (stackTop - stackBase) :: Int), "reason" .= ("private-frame" :: String)]]
        , "expected" .=
            object
              [ "status" .= ("returned" :: String)
              , "registers" .= registers (preserved ++ [("sp", stackTop), ("pc", exitAddress)])
              , "memory" .= expectedMemory
              , "events" .= events
              ]
        , "exclusions" .= ["Host Cortex-R5F native profile. Stock services are scripted stubs at their entries; no kernel, scheduling, interrupt or device qualification." :: String]
        ]

-- Execution ---------------------------------------------------------------

field :: String -> Value -> Value
field name (Object fields) = maybe Null id (KM.lookup (Key.fromString name) fields)
field _ _ = Null

text :: Value -> String
text (String value) = T.unpack value
text _ = ""

observe :: Native.CaseReport -> Observation
observe report = Observation
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
      Array items -> [(text (field "address" item), text (field "bytes" item)) | item <- V.toList items]
      _ -> []
  , observationEvents = field "events" runner
  , observationRegisters = field "registers" runner
  }
  where
    value = Native.reportValue report
    runner = field "runner" value

-- | Writes each scenario's case for one role into its own directory and runs them as one batch.
runRole :: FilePath -> FilePath -> Code -> B.ByteString -> Role -> [Scenario] -> IO [Observation]
runRole runner directory code image role scenarios = do
  let batchDirectory = directory </> roleLabel role
  createDirectoryIfMissing True batchDirectory
  entries <- forM scenarios $ \scenario -> do
    let (document, artifacts) = caseDocument code image role scenario
        ident = scenarioName scenario ++ "-" ++ roleLabel role
    forM_ artifacts $ \(name, bytes) -> B.writeFile (batchDirectory </> name) bytes
    BL.writeFile (batchDirectory </> (ident ++ ".case.json")) (encode document)
    pure (object ["id" .= ident, "path" .= (ident ++ ".case.json")])
  let batchPath = batchDirectory </> "batch.json"
  BL.writeFile batchPath (encode (object ["schema_version" .= (1 :: Int), "cases" .= entries]))
  collected <- newIORef []
  summary <- Native.runBatch runner batchPath $ \report -> do
    BL.writeFile (batchDirectory </> (Native.caseIdText (Native.reportCase report) ++ ".report.json")) (encode (Native.reportValue report))
    modifyIORef' collected (observe report :)
  observations <- reverse <$> readIORef collected
  unless (Native.summaryStatus summary == Native.BatchComplete && length observations == length scenarios) $
    fail ("native batch did not complete for " ++ roleLabel role ++ ": " ++ show (Native.summaryReason summary))
  pure observations

-- | The differences between the stock and owned observations of one scenario: either role
-- failing its independent expectation, and any difference in the data regions, the
-- results, the ordered service events or the preserved registers.
compareRoles :: Scenario -> Observation -> Observation -> [String]
compareRoles scenario stock owned =
  [ role ++ " " ++ observationRunnerStatus observation ++ ": " ++ unwords (observationMismatches observation)
  | (role, observation) <- [("stock", stock), ("owned", owned)]
  , not (observationPassed observation)
  ]
    ++ ["memory " ++ name | (name, address) <- compared, memoryAt address stock /= memoryAt address owned]
    ++ ["service events" | observationEvents stock /= observationEvents owned]
    ++ ["preserved registers" | map (registerOf stock) kept /= map (registerOf owned) kept]
  where
    compared = [(dataName region, dataAddress region) | region <- scenarioData scenario] ++ [("results", resultsBase)]
    memoryAt address observation = lookup (hexWord address) (observationMemory observation)
    kept = map fst preserved ++ ["sp"]
    registerOf observation name = field name (observationRegisters observation)

-- | The cases a deliberately wrong owned ELF fails after reaching the case exit: a
-- returned status with a wrong observation, or @stub_contract@ at the exit address, the
-- runner's status for a scripted call the case never made. Other statuses are listed apart.
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
      observationRunnerStatus observation == "returned"
        || ( observationRunnerStatus observation == "stub_contract"
               && field "pc" (observationRegisters observation) == String (T.pack (hexWord exitAddress))
           )
