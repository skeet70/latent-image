{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | The saclac engine comparison: stock's dispatcher and machine 0's router against the
-- owned image at the same addresses, over seeded scenarios and recorded action stubs.
module Replacement.Saclac.Suite
  ( runSuite
  , CaseResult (..)
  , ControlResult (..)
  , sourceIdentity
  , boundEntries
  , imageDigest
  , caseTableValue
  ) where

import qualified ArmElf as E
import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits
import qualified Data.ByteString as B
import Data.List (sortOn)
import Data.Maybe (fromMaybe, listToMaybe)
import Data.Word (Word32)
import Emulation.Judge
  ( ReportControl (..)
  , StockEvidence (..)
  , evidenceAt
  , evidenceProblems
  )
import Emulation.Mutation
  ( Extent (..)
  , Site (..)
  , Variant (..)
  , VariantOutcome (..)
  , eligibleSites
  , extentsFor
  , mutatedWord
  , patchElf
  , planMutations
  , renderMutationError
  , variantControl
  )
import Emulation.NativeObservation
  ( GuestAddress (..)
  , NativeObservation (..)
  , RegionId (..)
  , Register (..)
  , RunnerStatus (..)
  , hexWord32
  )
import Emulation.OwnedCase
import Emulation.ScenarioGeneration
import Emulation.Sha256 (sha256, sha256Hex)
import qualified Replacement.Saclac.Artifact as Artifact
import System.Directory (createDirectoryIfMissing, removeDirectoryRecursive)
import System.FilePath ((</>))

sourceIdentity :: String
sourceIdentity =
  "saclac byte-read contract, split-proposal.md section 5.4 increment A; canonical image5 base 0x00bf1a00 SHA256 "
    ++ imageDigest
    ++ "; dispatcher 0x00efca6c, router 0x00efcb88, tables 0x00f12f70 and 0x00f133ec, arrays 0x0169d468 and 0x0169d4b4"

imageDigest :: String
imageDigest = "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"

imageBase :: Word32
imageBase = 0xbf1a00

-- | The two extents, read from the one declaration in Replacement.Saclac.Artifact.
dispatchExtent, routerExtent :: Artifact.Extent
dispatchExtent = case Artifact.extents of
  (extent : _) -> extent
  [] -> error "Replacement.Saclac.Artifact declares no dispatch extent"
routerExtent = case Artifact.extents of
  (_ : extent : _) -> extent
  _ -> error "Replacement.Saclac.Artifact declares no router extent"

dispatchEntry, dispatchSize, routerEntry, routerSize :: Word32
dispatchEntry = Artifact.extentLow dispatchExtent
dispatchSize = fromIntegral (Artifact.extentBytes dispatchExtent)
routerEntry = Artifact.extentLow routerExtent
routerSize = fromIntegral (Artifact.extentBytes routerExtent)

routerRegionEntry, routerRegionSize :: Word32
routerRegionEntry = 0xefd2fc
routerRegionSize = 0xf01b14 - 0xefd2fc

tablesBase, tablesSize :: Word32
tablesBase = 0xf10000
tablesSize = 0x5000

cellTables, actionTables, secondTables, stateCounts :: Word32
cellTables = 0xf12f70
actionTables = 0xf133ec
secondTables = 0xf12074
stateCounts = 0xf1158c

rowsBase, statesBase, ramBase :: Word32
rowsBase = 0x169d468
statesBase = 0x169d4b4
ramBase = 0x169d400

traceEntry, traceSize :: Word32
traceEntry = 0xf46c38
traceSize = 0x60

stubBase, stubStride :: Word32
stubBase = 0x210000
stubStride = 0x80

driverBase, ringBase, ringSize, resultsBase, stackBase, exitAddress :: Word32
driverBase = 0x300000
ringBase = 0x204000
ringSize = 0x2000
resultsBase = 0x207000
stackBase = 0x400000
exitAddress = 0x208000

stackTop :: Word32
stackTop = stackBase + 0x1000

-- | Machine m's row count. The product with its state count sums to the 5,794 cells the
-- cell tables hold.
rowCounts :: [Word32]
rowCounts =
  [ 48, 20, 9, 15, 5, 4, 8, 7, 28, 12, 6, 21, 51, 20, 6, 4, 7, 28, 8, 7, 3, 4, 13, 9, 8, 2, 52
  , 9, 14, 6, 6, 6, 8, 4, 9, 6, 5
  ]

-- Scenarios --------------------------------------------------------------

data Event
  = Dispatch Word32 Word32 Word32 Word32 Word32
  | Router Word32 Word32 Word32
  deriving (Eq, Show)

data Nested
  = NestedRouter Word32 Word32
  | NestedRouterAt Word32 Word32 Word32
  | NestedState Word32 Word32
  deriving (Eq, Show)

data Scenario = Scenario
  { scenarioName :: String
  , scenarioEvents :: [Event]
  , scenarioInitialStates :: [(Word32, Word32)]
  , scenarioAllActions :: [Word32]
  , scenarioNested :: Maybe (Word32, Word32, Nested)
  , scenarioZeroCellTable :: Maybe Word32
  } deriving (Eq, Show)

fixedScenarios :: [Scenario]
fixedScenarios =
  [ Scenario "reenter-own" [Dispatch 0 0 1 0x11 0x22] [] [0] (Just (0, 3, NestedState 0 7)) Nothing
  , Scenario "reenter-other" [Dispatch 0 23 4 0x11 0x22] [(1, 4)] [0, 1] (Just (0, 22, NestedRouterAt 0xefd2fc 164 0x44)) Nothing
  , Scenario "no-cell-table" [Dispatch 5 0 0 0x11 0x22] [] [] Nothing (Just 5)
  , Scenario "router-event" [Router 0 0 0x22] [] [] Nothing Nothing
  ]

generatorIdentity :: GeneratorVersion
generatorIdentity = GeneratorVersion "saclac-engine-1"

generatorLimits :: SequenceBounds
generatorLimits = either (error . show) id (mkSequenceBounds 1 6)

scenarioSeed :: ScenarioSeed
scenarioSeed = ScenarioSeed 20261112

seededScenarios :: B.ByteString -> [Scenario]
seededScenarios image =
  case generateScenarios (generator image) scenarioSeed 32 of
    Left problem -> error (show problem)
    Right generated ->
      concatMap flatten (Emulation.ScenarioGeneration.generatedScenarios generated)
  where
    flatten scenario =
      [ scenario
          { scenarioName = scenarioName scenario ++ "-e" ++ show index
          , scenarioEvents = [event]
          }
      | (index, event) <- zip [0 :: Int ..] (scenarioEvents scenario)
      ]

generator :: B.ByteString -> Generator Scenario Event
generator image = Generator
  { generatorVersion = generatorIdentity
  , generatorBounds = generatorLimits
  , generatorEvents = scenarioEvents
  , generatorScenario = build
  }
  where
    build _ index bounds rng0 =
      let drawCount = 1 + fromIntegral (fst (drawWord64 rng0) `mod` fromIntegral (maximumEvents bounds)) :: Int
          (events, _) = foldl step ([], snd (drawWord64 rng0)) [1 .. drawCount]
          step (acc, rng) _ = let (event, rng') = drawEvent rng in (acc ++ [event], rng')
          drawEvent rng =
            let (entry, r1) = drawElement [DispatchEntryTag, DispatchEntryTag, DispatchEntryTag, RouterEntryTag] rng
                (machineWord, r2) = drawWord64 r1
                machine = fromIntegral (machineWord `mod` 37) :: Word32
                (rowWord, r3) = drawWord64 r2
                (stateWord, r4) = drawWord64 r3
                (a1Word, r5) = drawWord64 r4
                (a2Word, r6) = drawWord64 r5
                a1 = fromIntegral a1Word
                a2 = fromIntegral a2Word
                row = fromIntegral rowWord
                state = fromIntegral stateWord
                stateBound = stateCountOf image machine
             in ( case entry of
                    DispatchEntryTag ->
                      Dispatch machine (row `mod` rowCountOf machine) (state `mod` stateBound) a1 a2
                    RouterEntryTag ->
                      Router (row `mod` 61) (state `mod` stateCountOf image 0) a2
                , r6
                )
       in Scenario
            { scenarioName = "generated-" ++ pad index
            , scenarioEvents = events
            , scenarioInitialStates = []
            , scenarioAllActions = []
            , scenarioNested = Nothing
            , scenarioZeroCellTable = Nothing
            }
    pad index = let text = show index in replicate (2 - length text) '0' ++ text

data EntryTag = DispatchEntryTag | RouterEntryTag
  deriving (Eq, Show)

-- Image accessors --------------------------------------------------------

slice :: B.ByteString -> Word32 -> Word32 -> B.ByteString
slice image address size =
  B.take (fromIntegral size) (B.drop (fromIntegral (address - imageBase)) image)

byteAt :: B.ByteString -> Word32 -> Word32
byteAt image address = fromIntegral (B.index image (fromIntegral (address - imageBase)))

wordAt :: B.ByteString -> Word32 -> Word32
wordAt image address = readWordAt image (fromIntegral (address - imageBase))

halfAt :: B.ByteString -> Word32 -> Word32
halfAt image address =
  fromIntegral (B.index image offset) .|. (fromIntegral (B.index image (offset + 1)) `shiftL` 8)
  where
    offset = fromIntegral (address - imageBase)

cellBase, actionBase, secondBase :: B.ByteString -> Word32 -> Word32
cellBase image machine = wordAt image (cellTables + 4 * machine)
actionBase image machine = wordAt image (actionTables + 4 * machine)
secondBase image machine = wordAt image (secondTables + 4 * machine)

stateCountOf :: B.ByteString -> Word32 -> Word32
stateCountOf image machine = halfAt image (stateCounts + 2 * machine)

rowCountOf :: Word32 -> Word32
rowCountOf machine = fromMaybe 1 (listToMaybe (drop (fromIntegral machine) rowCounts))

maxActionIndex :: B.ByteString -> Word32 -> Word32
maxActionIndex image machine =
  maximum
    ( 0
        : [ byteAt image (cellBase image machine + 2 * index)
          | index <- [0 .. stateCountOf image machine * rowCountOf machine - 1]
          ]
    )

cellOf :: B.ByteString -> Word32 -> Word32 -> Word32 -> (Word32, Int)
cellOf image machine row state =
  ( byteAt image (base + 2 * index)
  , let raw = byteAt image (base + 2 * index + 1)
     in if raw >= 128 then fromIntegral raw - 256 else fromIntegral raw
  )
  where
    index = (state + row * stateCountOf image machine) .&. 0xffff
    base = cellBase image machine

-- Stubs and the driver ---------------------------------------------------

movwR, movtR :: Word32 -> Word32 -> Word32
movwR register immediate =
  0xe3000000 .|. ((immediate .&. 0xf000) `shiftL` 4) .|. (register `shiftL` 12) .|. (immediate .&. 0xfff)
movtR register immediate =
  0xe3400000
    .|. (((immediate `shiftR` 16) .&. 0xf000) `shiftL` 4)
    .|. (register `shiftL` 12)
    .|. ((immediate `shiftR` 16) .&. 0xfff)

loadImmediate :: Word32 -> Word32 -> [Word32]
loadImmediate register value = [movwR register value, movtR register value]

ldrR, strR :: Word32 -> Word32 -> Word32 -> Word32
ldrR register base offset = 0xe5900000 .|. (base `shiftL` 16) .|. (register `shiftL` 12) .|. (offset .&. 0xfff)
strR register base offset = 0xe5800000 .|. (base `shiftL` 16) .|. (register `shiftL` 12) .|. (offset .&. 0xfff)

strhR :: Word32 -> Word32 -> Word32 -> Word32
strhR register base offset =
  0xe1c000b0 .|. (base `shiftL` 16) .|. (register `shiftL` 12) .|. ((offset .&. 0xf0) `shiftL` 4) .|. (offset .&. 0xf)

addImm, subImm :: Word32 -> Word32 -> Word32 -> Word32
addImm register base immediate = 0xe2800000 .|. (base `shiftL` 16) .|. (register `shiftL` 12) .|. (immediate .&. 0xfff)
subImm register base immediate = 0xe2400000 .|. (base `shiftL` 16) .|. (register `shiftL` 12) .|. (immediate .&. 0xfff)

movImm :: Word32 -> Word32 -> Word32
movImm register immediate = 0xe3a00000 .|. (register `shiftL` 12) .|. (immediate .&. 0xff)

blAt :: Word32 -> Word32 -> Word32
blAt pc target = 0xeb000000 .|. (((target - pc - 8) `shiftR` 2) .&. 0xffffff)

bxLr :: Word32
bxLr = 0xe12fff1e

-- | Eight words per record: tag, machine, index, r0, r1, r2, r3, spare.
ringPrefix :: [Word32]
ringPrefix =
  loadImmediate 12 ringBase
    ++ [ ldrR 3 12 0
       , addImm 3 3 1
       , strR 3 12 0
       , subImm 3 3 1
       , 0xe1a03283 -- mov r3, r3, lsl #5
       , 0xe08cc003 -- add r12, r12, r3
       , addImm 12 12 4
       ]

traceStub :: [Word32]
traceStub =
  ringPrefix
    ++ [ movImm 3 1
       , strR 3 12 0
       , strR 0 12 4
       , strR 1 12 8
       , strR 2 12 12
       , movImm 3 0
       , strR 3 12 16
       , strR 3 12 20
       , strR 3 12 24
       , strR 3 12 28
       , movImm 0 0
       , bxLr
       ]

actionStub :: Word32 -> Word32 -> Word32 -> Maybe Nested -> [Word32]
actionStub at machine index nested = prefix ++ body
  where
    prefix =
      [ 0xe92d000c ] -- push {r2, r3}
        ++ ringPrefix
        ++ [ movImm 3 2
           , strR 3 12 0
           , movwR 3 machine
           , strR 3 12 4
           , movwR 3 index
           , strR 3 12 8
           , strR 0 12 12
           , strR 1 12 16
           , 0xe8bd000c -- pop {r2, r3}
           , strR 2 12 20
           , strR 3 12 24
           ]
    after = at + 4 * fromIntegral (length prefix + 4)
    body = case nested of
      Nothing -> loadImmediate 0 (0x1000 + (machine .&. 0xff) * 0x100 + (index .&. 0xff)) ++ [bxLr]
      Just (NestedRouter event a2) ->
        loadImmediate 0 event ++ loadImmediate 1 a2 ++ [blAt after routerEntry, bxLr]
      Just (NestedRouterAt target event a2) ->
        loadImmediate 0 event ++ loadImmediate 1 a2 ++ [blAt after target, bxLr]
      Just (NestedState target value) ->
        loadImmediate 3 value
          ++ loadImmediate 12 statesBase
          ++ [addImm 12 12 (2 * target), strhR 3 12 0]
          ++ loadImmediate 0 (0x1000 + (machine .&. 0xff) * 0x100 + (index .&. 0xff))
          ++ [bxLr]

eventMachine :: Event -> Word32
eventMachine (Dispatch machine _ _ _ _) = machine
eventMachine (Router _ _ _) = 0

eventState :: Event -> Word32
eventState (Dispatch _ _ state _ _) = state
eventState (Router _ state _) = state

driverWords :: [(Word32, Word32)] -> [Event] -> [Word32]
driverWords initialStates events = preamble ++ concat blocks ++ [0xe8bd8010]
  where
    preamble =
      0xe92d4010
        : concat
          [ loadImmediate 3 state
              ++ loadImmediate 12 statesBase
              ++ [addImm 12 12 (2 * machine), strhR 3 12 0]
          | (machine, state) <- initialStates
          ]
    blocks = snd (foldl place (driverBase + 4 * fromIntegral (length preamble), []) (zip [0 :: Int ..] events))
    place (address, acc) (index, event) =
      let block = eventBlock address index event
       in (address + 4 * fromIntegral (length block), acc ++ [block])
    eventBlock address index event =
      let stageState =
            loadImmediate 3 (eventState event)
              ++ loadImmediate 12 statesBase
              ++ [addImm 12 12 (2 * eventMachine event), strhR 3 12 0]
          (pre, call) = case event of
            Dispatch machine row _ a1 a2 ->
              ( loadImmediate 3 row
                  ++ loadImmediate 12 rowsBase
                  ++ [addImm 12 12 (2 * machine), strhR 3 12 0]
                  ++ stageState
                  ++ loadImmediate 0 machine
                  ++ loadImmediate 1 a1
                  ++ loadImmediate 2 a2
              , dispatchEntry
              )
            Router code _ a2 ->
              (stageState ++ loadImmediate 0 code ++ loadImmediate 1 a2, routerEntry)
       in pre
            ++ [blAt (address + 4 * fromIntegral (length pre)) call]
            ++ loadImmediate 12 resultsBase
            ++ [strR 0 12 (4 * fromIntegral index)]

-- Cases ------------------------------------------------------------------

data Role = Stock | Owned
  deriving (Eq, Show)

roleLabel :: Role -> String
roleLabel Stock = "stock"
roleLabel Owned = "owned"

data Patch = Patch
  { patchWord :: Word32
  , patchMachine :: Word32
  , patchIndex :: Word32
  , patchNested :: Maybe Nested
  , patchStub :: Word32
  } deriving (Eq, Show)

data Plan = Plan
  { planPatches :: [Patch]
  , planZeroCell :: Maybe Word32
  }

planFor :: B.ByteString -> Scenario -> Plan
planFor image scenario = Plan (assign (nestedOverride candidates)) (scenarioZeroCellTable scenario)
  where
    candidates
      | null (scenarioAllActions scenario) =
          cellPatches
            ++ [ (actionBase image 0 + 4 * index, 0, index, Nothing)
               | any isRouter (scenarioEvents scenario)
               , index <- [0 .. maxActionIndex image 0]
               , wordAt image (actionBase image 0 + 4 * index) /= 0
               ]
      | otherwise =
          [ (actionBase image machine + 4 * index, machine, index, Nothing)
          | machine <- scenarioAllActions scenario
          , index <- [0 .. maxActionIndex image machine]
          , wordAt image (actionBase image machine + 4 * index) /= 0
          ]
    cellPatches =
      concat
        [ (if action == 0 then [] else [(actionBase image machine + 4 * action, machine, action, Nothing)])
            ++ [ (secondBase image machine + 4 * fromIntegral next, machine, 0x100 + fromIntegral next, Nothing)
               | next > 0
               , let address = secondBase image machine + 4 * fromIntegral next
               , wordAt image address /= 0
               ]
        | Dispatch machine row state _ _ <- scenarioEvents scenario
        , let (action, next) = cellOf image machine row state
        ]
    isRouter (Router _ _ _) = True
    isRouter _ = False
    nestedOverride current = case scenarioNested scenario of
      Nothing -> current
      Just (machine, index, nested) ->
        [ if m == machine && i == index then (address, machine, index, Just nested) else (address, m, i, Nothing)
        | (address, m, i, _) <- current
        ]
    assign entries =
      [ Patch address machine index nested (stubBase + stubStride * fromIntegral k)
      | (k, (address, machine, index, nested)) <- zip [0 :: Int ..] (dedupe entries)
      ]
    dedupe = go []
      where
        go seen [] = reverse seen
        go seen (entry@(address, _, _, _) : rest)
          | any (\(other, _, _, _) -> other == address) seen = go seen rest
          | otherwise = go (entry : seen) rest

planStubWords :: Plan -> [Word32]
planStubWords plan =
  concat
    [ take (fromIntegral (stubStride `div` 4)) (actionStub (patchStub patch) (patchMachine patch) (patchIndex patch) (patchNested patch) ++ repeat 0)
    | patch <- planPatches plan
    ]

planFunctions :: Plan -> [(Word32, Word32)]
planFunctions plan = [(patchStub patch, stubStride) | patch <- planPatches plan]

patchedTables :: B.ByteString -> Plan -> B.ByteString
patchedTables image plan = foldl replace (slice image tablesBase tablesSize) patches
  where
    patches =
      [(fromIntegral (patchWord patch - tablesBase), patchStub patch) | patch <- planPatches plan]
        ++ [(fromIntegral (cellTables + 4 * machine - tablesBase), 0) | machine <- maybe [] (: []) (planZeroCell plan)]
    replace bytes (offset, value) = B.take offset bytes <> word32le value <> B.drop (offset + 4) bytes

ownedOverlay :: E.Elf -> [(Word32, B.ByteString)]
ownedOverlay elf =
  [ (E.sectionAddress section, sectionBytes section)
  | section <- E.elfSections elf
  , E.sectionAllocated section
  , E.sectionSize section > 0
  , E.sectionAddress section >= dispatchEntry
  , E.sectionAddress section < routerEntry + routerSize
  ]
  where
    sectionBytes section =
      B.pack
        [ fromMaybe (error "owned ELF hole") (E.byteAt elf address)
        | address <- [E.sectionAddress section .. E.sectionAddress section + E.sectionSize section - 1]
        ]

applyOverlay :: Word32 -> B.ByteString -> [(Word32, B.ByteString)] -> B.ByteString
applyOverlay base bytes overlay = foldl apply bytes overlay
  where
    apply current (address, content) =
      B.take offset current <> content <> B.drop (offset + B.length content) current
      where
        offset = fromIntegral (address - base)

engineBytes :: B.ByteString -> Role -> E.Elf -> B.ByteString
engineBytes image Stock _ = slice image dispatchEntry (dispatchSize + routerSize)
engineBytes image Owned elf =
  applyOverlay dispatchEntry (slice image dispatchEntry (dispatchSize + routerSize)) (ownedOverlay elf)

region :: String -> Word32 -> Permission -> B.ByteString -> CaseRegion
region name address permission bytes =
  mkRegion (RegionId name) (GuestAddress (fromIntegral address)) (B.length bytes) permission bytes

makeCase :: B.ByteString -> E.Elf -> Role -> Scenario -> CaseSpec
makeCase image elf role scenario = CaseSpec
  { caseId = scenarioName scenario ++ "-" ++ roleLabel role
  , caseProvenance = sourceIdentity
  , caseExclusion =
      "Host Cortex-R5F native profile. Stock's tables and state arrays are mapped at their addresses; stock action words and the trace entry are recording stubs, so the comparison observes which action the engine enters with which arguments and what it stores, not the action bodies. No kernel, scheduling, interrupt, timer or device qualification."
  , caseEntry = GuestAddress (fromIntegral driverBase)
  , caseExit = GuestAddress (fromIntegral exitAddress)
  , caseRegions =
      [ region "engine" dispatchEntry ReadExecute (engineBytes image role elf)
      , region "routers" routerRegionEntry ReadExecute (slice image routerRegionEntry routerRegionSize)
      , region "tables" tablesBase ReadOnly (patchedTables image plan)
      , region "trace" traceEntry ReadExecute (B.take (fromIntegral traceSize) (assemble traceStub <> B.replicate 256 0))
      , region "stubs" stubBase ReadExecute (let bytes = assemble stubWords in if B.null bytes then word32le 0 else bytes)
      , region "driver" driverBase ReadExecute (assemble driver)
      , region "ram" ramBase ReadWrite (B.replicate 0x200 0)
      , region "ring" ringBase ReadWrite (B.replicate (fromIntegral ringSize) 0)
      , region "results" resultsBase ReadWrite (B.replicate 0x100 0)
      , region "stack" stackBase ReadWrite (B.replicate 0x1000 0)
      , region "exit" exitAddress ReadExecute (word32le bxLr)
      ]
  , caseFunctions = [(GuestAddress (fromIntegral address), size) | (address, size) <- functions]
  , caseInitial =
      [ (R0, 0)
      , (R1, 0)
      , (R2, 0)
      , (R3, 0)
      , (R4, 0)
      , (R5, 0)
      , (R6, 0)
      , (R7, 0)
      , (R8, 0)
      , (R9, 0)
      , (R10, 0)
      , (R11, 0)
      , (R12, 0)
      , (Sp, stackTop)
      , (Lr, exitAddress)
      , (Cpsr, 0x10)
      ]
  , caseExpectedStatus = Returned
  , caseExpectedRegisters = [(Sp, stackTop), (R11, 0)]
  , caseExpectedMemory = []
  , caseIgnoredMemory = []
  , caseCp14Identity = Nothing
  , caseKernel = Nothing
  }
  where
    plan = planFor image scenario
    stubWords = planStubWords plan
    driver = driverWords (scenarioInitialStates scenario) (scenarioEvents scenario)
    functions = case role of
      Stock ->
        [(dispatchEntry, dispatchSize), (routerEntry, routerSize), (routerRegionEntry, routerRegionSize), (traceEntry, traceSize)]
          ++ [(driverBase, fromIntegral (4 * length driver))]
          ++ planFunctions plan
      Owned ->
        sortOn fst
          ( [ (E.symbolValue symbol, E.symbolSize symbol)
            | symbol <- E.elfSymbols elf
            , E.symbolKind symbol == E.FunctionSymbol
            , E.symbolSize symbol > 0
            , E.symbolValue symbol >= dispatchEntry
            , E.symbolValue symbol < routerEntry + routerSize
            ]
              ++ [(routerRegionEntry, routerRegionSize), (traceEntry, traceSize)]
              ++ [(driverBase, fromIntegral (4 * length driver))]
              ++ planFunctions plan
          )

-- Comparison -------------------------------------------------------------

ringRecords :: NativeObservation -> [(Word32, Word32, Word32, Word32, Word32, Word32, Word32, Word32)]
ringRecords observation = case lookup (RegionId "ring") (nativeMemory observation) of
  Nothing -> []
  Just bytes ->
    let count = min 200 (fromIntegral (readWordAt bytes 0) :: Int)
        one index =
          let at = 4 + index * 32
           in ( readWordAt bytes at
              , readWordAt bytes (at + 4)
              , readWordAt bytes (at + 8)
              , readWordAt bytes (at + 12)
              , readWordAt bytes (at + 16)
              , readWordAt bytes (at + 20)
              , readWordAt bytes (at + 24)
              , readWordAt bytes (at + 28)
              )
     in [one index | index <- [0 .. count - 1]]

resultsWords :: Scenario -> NativeObservation -> [Word32]
resultsWords scenario observation = case lookup (RegionId "results") (nativeMemory observation) of
  Nothing -> []
  Just bytes -> [readWordAt bytes (4 * index) | index <- [0 .. length (scenarioEvents scenario) - 1]]

ramBytes :: NativeObservation -> B.ByteString
ramBytes observation = fromMaybe B.empty (lookup (RegionId "ram") (nativeMemory observation))

-- | The 37 machine state halfwords, read where the dispatcher reads them.
stateWords :: NativeObservation -> [Word32]
stateWords observation = case lookup (RegionId "ram") (nativeMemory observation) of
  Nothing -> []
  Just bytes ->
    [ readWordAt bytes (fromIntegral (statesBase - ramBase) + 2 * machine) .&. 0xffff
    | machine <- [0 .. 36]
    ]

compareScenario :: Scenario -> NativeObservation -> NativeObservation -> [String]
compareScenario scenario stock owned =
  [ role ++ " status " ++ show (nativeStatus observation) ++ " expected returned"
  | (role, observation) <- [("stock", stock), ("owned", owned)]
  , nativeStatus observation /= Returned
  ]
    ++ ["return words" | resultsWords scenario stock /= resultsWords scenario owned]
    ++ ["recorded calls" | ringRecords stock /= ringRecords owned]
    ++ ["row and state arrays" | ramBytes stock /= ramBytes owned]

-- | A comparison that does not react to a changed projection is reported as a difference.
controlsFor :: Scenario -> NativeObservation -> NativeObservation -> [String]
controlsFor scenario stock owned =
  [ "return-word control" | null (compareScenario scenario (mutateRegion (RegionId "results") 0 stock) owned) ]
    ++ ["recorded-call control" | null (compareScenario scenario (replaceRegion (RegionId "ring") (word32le 1 <> B.replicate 28 0xa5) stock) owned) ]
    ++ ["array control" | null (compareScenario scenario (mutateRegion (RegionId "ram") 0 stock) owned) ]

mutateRegion :: RegionId -> Int -> NativeObservation -> NativeObservation
mutateRegion target offset observation = observation
  { nativeMemory =
      [ (key, if key == target then B.take offset bytes <> word32le (readWordAt bytes offset + 1) <> B.drop (offset + 4) bytes else bytes)
      | (key, bytes) <- nativeMemory observation
      ]
  }

replaceRegion :: RegionId -> B.ByteString -> NativeObservation -> NativeObservation
replaceRegion target replacement observation = observation
  { nativeMemory = [(key, if key == target then replacement else bytes) | (key, bytes) <- nativeMemory observation] }

-- Results ----------------------------------------------------------------

data CaseResult = CaseResult
  { caseResultName :: String
  , caseResultEntry :: Word32
  , caseResultExtent :: Word32
  , caseResultEvidence :: StockEvidence
  , caseResultDifferences :: [String]
  , caseResultStates :: [(String, [Word32])]
  } deriving (Eq, Show)

data ControlResult = ControlResult
  { controlName :: String
  , controlReported :: Bool
  , controlExpected :: String
  , controlDifferences :: [String]
  } deriving (Eq, Show)

entryOf :: Scenario -> (Word32, Word32)
entryOf scenario = case reverse (scenarioEvents scenario) of
  (Dispatch _ _ _ _ _ : _) -> (dispatchEntry, dispatchSize)
  (Router _ _ _ : _) -> (routerEntry, routerSize)
  [] -> (dispatchEntry, dispatchSize)

boundEntries :: [(Word32, Word32, String)]
boundEntries =
  [ (dispatchEntry, dispatchSize, "xm1_saclac_dispatch_entry")
  , (routerEntry, routerSize, "xm1_saclac_router_entry")
  ]

caseTableValue :: Value
caseTableValue = object
  [ "schema" .= (1 :: Int)
  , "suite" .= ("saclac-engine" :: String)
  , "source" .= ("Replacement.Saclac.Suite fixedScenarios" :: String)
  , "entries" .=
      [ object
          [ "case" .= scenarioName scenario
          , "entry" .= hexWord32 entry
          , "extent" .= size
          , "stock" .= ("image" :: String)
          ]
      | scenario <- fixedScenarios
      , let (entry, size) = entryOf scenario
      ]
  ]

-- Running ----------------------------------------------------------------

checkInputs :: FilePath -> FilePath -> IO (B.ByteString, E.Elf)
checkInputs imagePath elfPath = do
  image <- B.readFile imagePath
  unless (sha256Hex (sha256 image) == imageDigest) (fail "image5 identity differs")
  elf <- either fail pure . E.parseElf =<< B.readFile elfPath
  forM_ [(dispatchEntry, "xm1_saclac_dispatch_entry"), (routerEntry, "xm1_saclac_router_entry")] $ \(address, name) ->
    case [symbol | symbol <- E.elfSymbols elf, E.symbolName symbol == name, E.symbolKind symbol == E.FunctionSymbol] of
      [symbol] | E.symbolValue symbol == address -> pure ()
      _ -> fail ("owned entry is missing or moved: " ++ name)
  forM_ (filter (\section -> E.sectionAllocated section && E.sectionSize section > 0) (E.elfSections elf)) $ \section ->
    unless
      (E.sectionAddress section >= dispatchEntry && E.sectionAddress section + E.sectionSize section <= routerEntry + routerSize)
      (fail "owned allocated section escapes the two stock extents")
  pure (image, elf)

runRole :: FilePath -> FilePath -> B.ByteString -> E.Elf -> Role -> [Scenario] -> IO [NativeObservation]
runRole runner directory image elf role scenarios = do
  let batchDirectory = directory </> roleLabel role
  createDirectoryIfMissing True batchDirectory
  batch <- writeBatch batchDirectory (map (makeCase image elf role) scenarios)
  map snd <$> loadObservedBatch runner batch

stockEvidenceAt :: FilePath -> Scenario -> IO StockEvidence
stockEvidenceAt directory scenario = do
  let (entry, size) = entryOf scenario
  evidenceAt (directory </> "stock" </> (scenarioName scenario ++ "-stock.native-result.json")) entry size

runSuite :: FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ([CaseResult], [ControlResult])
runSuite runner directory imagePath elfPath wrongPath = do
  (image, elf) <- checkInputs imagePath elfPath
  (_, wrongElf) <- checkInputs imagePath wrongPath
  let scenarios = filter (not . pointerConflict image) (fixedScenarios ++ seededScenarios image)
  createDirectoryIfMissing True directory
  stocks <- runRole runner directory image elf Stock scenarios
  owneds <- runRole runner directory image elf Owned scenarios
  evidence <- mapM (stockEvidenceAt directory) scenarios
  let results =
        [ CaseResult
            { caseResultName = scenarioName scenario
            , caseResultEntry = fst (entryOf scenario)
            , caseResultExtent = snd (entryOf scenario)
            , caseResultEvidence = found
            , caseResultDifferences =
                compareScenario scenario stock owned
                  ++ controlsFor scenario stock owned
                  ++ evidenceProblems found
            , caseResultStates =
                [ (roleLabel role, stateWords observation)
                | (role, observation) <- [(Stock, stock), (Owned, owned)]
                ]
            }
        | (scenario, stock, owned, found) <- zip4 scenarios stocks owneds evidence
        ]
  forM_ results $ \result -> length (caseResultDifferences result) `seq` pure ()
  outcomes <- runVariants runner directory image elf scenarios stocks
  wrong <- wrongVariantControl runner directory image elf wrongElf
  pure (results, wrong : map outcomeControl outcomes)
  where
    outcomeControl outcome =
      let control = variantControl outcome
       in ControlResult
            { controlName = reportControlName control
            , controlReported = reportControlDetected control
            , controlExpected = reportControlExpected control
            , controlDifferences = reportControlDifferences control
            }

-- | The deliberately wrong owned image enters each action with the table base in r3.
wrongVariantControl :: FilePath -> FilePath -> B.ByteString -> E.Elf -> E.Elf -> IO ControlResult
wrongVariantControl runner directory image elf wrongElf = do
  stocks <- runRole runner (directory </> "wrong-stock") image elf Stock fixedScenarios
  wrongs <- runRole runner (directory </> "wrong-r3") image wrongElf Owned fixedScenarios
  let differing =
        [ (scenarioName scenario, differences)
        | (scenario, stock, wrong) <- zip3 fixedScenarios stocks wrongs
        , let differences = compareScenario scenario stock wrong
        , not (null differences)
        ]
  pure ControlResult
    { controlName = "wrong-r3-variant"
    , controlReported = any (\(_, differences) -> "recorded calls" `elem` differences) differing
    , controlExpected = "recorded calls"
    , controlDifferences = [scenario ++ ": " ++ difference | (scenario, differences) <- differing, difference <- differences]
    }

runVariants :: FilePath -> FilePath -> B.ByteString -> E.Elf -> [Scenario] -> [NativeObservation] -> IO [VariantOutcome]
runVariants runner directory image elf scenarios stocks = do
  let entries = ["xm1_saclac_dispatch_entry", "xm1_saclac_router_entry"]
  extents <- either fail pure (extentsFor elf entries ["xm1_saclac"])
  sites <- either fail pure (eligibleSites elf extents)
  let located = [(extent, site) | site <- sites, Just extent <- [extentAt extents (siteAddress site)]]
      requested = min 8 (length located)
  if requested == 0
    then fail "the owned crate has no eligible mutation site"
    else pure ()
  variants <- either (fail . renderMutationError) pure (planMutations scenarioSeed requested located)
  let requiredAddresses = [0xefcbd8, 0xefcc10, 0xefcc44, 0xefcc54, 0xefcc88]
      drawnAddresses = map variantAddress variants
      missingSites = [address | address <- requiredAddresses, address `notElem` map siteAddress sites]
      transitionVariants =
        [ Variant scenarioSeed (length variants + offset) (extentName extent) (siteAddress site)
            (siteWord site) (mutatedWord (siteKind site) (siteWord site)) (siteKind site)
        | (offset, (extent, site)) <-
            zip [0 :: Int ..]
              [ (extent, site)
              | (extent, site) <- located
              , siteAddress site `elem` requiredAddresses
              , siteAddress site `notElem` drawnAddresses
              ]
        ]
  unless (null missingSites) (fail ("the transition sites moved: " ++ show missingSites))
  forM (variants ++ transitionVariants) $ \variant -> do
    patched <- either fail pure (patchElf elf variant)
    variantElf <- either fail pure (E.parseElf patched)
    let label = "mutant-" ++ show (variantIndex variant)
    observed <- runRole runner (directory </> label) image variantElf Owned scenarios
    let differing =
          [ (scenario, differences)
          | (scenario, stock, owned) <- zip3 scenarios stocks observed
          , let differences = compareScenario scenario stock owned
          , not (null differences)
          ]
    removeDirectoryRecursive (directory </> label)
    pure VariantOutcome
      { outcomeVariant = variant
      , outcomeDetected = not (null differing)
      , outcomeCases = [scenarioName scenario | (scenario, _) <- differing]
      , outcomeDifferences = case differing of
          ((scenario, differences) : _) -> map ((scenarioName scenario ++ ": ") ++) differences
          [] -> []
      }

extentAt :: [Extent] -> Word32 -> Maybe Extent
extentAt extents address = listToMaybe
  [ extent
  | extent <- extents
  , extentStart extent <= address
  , address < extentStart extent + extentSize extent
  ]

-- | The four per-machine pointer arrays and the state-count array. A second-word
-- patch inside them would change a word the dispatcher reads as a pointer, so a
-- case that would patch one is not staged: stock would enter a data address.
pointerArrays :: [(Word32, Word32)]
pointerArrays =
  [ (cellTables, 37 * 4)
  , (actionTables, 37 * 4)
  , (secondTables, 37 * 4)
  , (stateCounts, 37 * 2)
  ]

pointerConflict :: B.ByteString -> Scenario -> Bool
pointerConflict image scenario =
  any inside [patchWord patch | patch <- planPatches (planFor image scenario)]
  where
    inside address = any (\(base, size) -> address >= base && address < base + size) pointerArrays

zip4 :: [a] -> [b] -> [c] -> [d] -> [(a, b, c, d)]
zip4 (a : as) (b : bs) (c : cs) (d : ds) = (a, b, c, d) : zip4 as bs cs ds
zip4 _ _ _ _ = []
