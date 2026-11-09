{-# LANGUAGE OverloadedStrings #-}

-- | The grid, the comparison and the controls against the owned bodies: each mutant differs from the
-- owned body on at least one case, and the owned body and the equivalents match it on every case.
module Main (main) where

import Control.Exception (IOException, try)
import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value (Null, Object, String), eitherDecodeStrict', encode, object, (.=))
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (complement, (.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.ByteString.Char8 as C
import Data.Char (isAsciiLower, isDigit)
import Data.Either (isLeft, isRight)
import Data.List (nub, sort)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.NativeCase (runnerDigest)
import Emulation.NativeObservation
  ( GuestAddress (..)
  , InstructionCount (..)
  , EngineError (..)
  , NativeObservation (..)
  , Provenance (..)
  , ProvenanceTotal (..)
  , Register (..)
  , RegionId (..)
  , RunnerStatus (..)
  , Write (..)
  , WriteBytes (..)
  , registerLabels
  )
import Emulation.OwnedCase (CaseId (..))
import Emulation.Sha256 (parseSha256Hex)
import FujiAF.Host (get, list)
import Numeric (showHex)
import qualified FujiAF.Host as Host
import Replacement.RtosOwnFirst.Artifact (LinkedDomain (..))
import Replacement.RtosOwnFirst.Assembly
import Replacement.RtosOwnFirst.Command
import Replacement.RtosOwnFirst.Controls
import Replacement.RtosOwnFirst.Generate
import Replacement.RtosOwnFirst.Generated
import Replacement.RtosOwnFirst.Oracle
import Replacement.RtosOwnFirst.Regions hiding (GuestAddress (..))
import Replacement.RtosOwnFirst.Replay
import Replacement.RtosOwnFirst.Shrink
import Replacement.RtosOwnFirst.Verdict
import System.Directory (copyFile, createDirectoryIfMissing, doesDirectoryExist, doesFileExist, listDirectory, removeDirectoryRecursive, removeFile)
import System.Environment (getArgs, getEnv)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath (takeExtension, (</>))
import System.IO (BufferMode (..), hSetBuffering, stdout)

check :: String -> Bool -> IO Bool
check label result = do
  putStrLn ((if result then "ok   " else "FAIL ") ++ label)
  pure result

expectedCases :: Slot -> Int
expectedCases slot = 64 + (if slot == S164 then 9 else 3) * 24 * case slot of
  S60 -> 32
  S128 -> 40
  S12c -> 40
  S16c -> 40
  S154 -> 25
  S14c -> 1
  S164 -> 1
  S1618 -> 6
  _ -> 5

privileged :: Word32 -> Bool
privileged cpsr = cpsr .&. complement 0xf80f00df == 0 && cpsr .&. 0x1f `elem` [0x13, 0x1f]

-- | An observation with every register 0 except the overrides and the given caller-frame and stack bytes.
observation :: [(Register, Word32)] -> B.ByteString -> [(Word32, Int)] -> NativeObservation
observation overrides callerFrame writes =
  NativeObservation
    { nativeStatus = Returned
    , nativeRegisters = M.fromList [(register, M.findWithDefault 0 register (M.fromList overrides)) | register <- registerLabels]
    , nativeMemory =
        [ (RegionId "context", B.replicate 4 0)
        , (RegionId "words", B.replicate 0x18 0)
        , (RegionId "tasks-low", B.replicate 112 0)
        , (RegionId "tasks-59", B.replicate 56 0)
        , (RegionId "tasks-119", B.replicate 56 0)
        , (RegionId "buffer", B.replicate 16 0)
        , (RegionId "caller-frame", callerFrame)
        , (RegionId "call-trace", B.replicate 64 0)
        ]
    , nativeProvenance = Complete (ProvenanceTotal (fromIntegral (length writes))) [Write (GuestAddress (fromIntegral address)) (WriteBytes (fromIntegral width)) | (address, width) <- writes]
    , nativeInstructions = InstructionCount 1
    , nativeEngineError = EngineError 0
    , nativeKernel = Nothing
    }

runFrom :: [(Register, Word32)] -> B.ByteString -> [WriteEvent] -> Either String Run
runFrom overrides callerFrame writes = runOf DomainA (observation overrides callerFrame [(eventAddress e, eventWidth e) | e <- writes]) writes

firstGeneratedRow :: Row
firstGeneratedRow = case generateRows (GenerationSeed 1) (generationCount 1) of
  row : _ -> row
  [] -> error "generateRows returned no row"

generationCount :: Int -> GenerationCount
generationCount count = either error id (mkGenerationCount count)

shrinkBudget :: Int -> ShrinkBudget
shrinkBudget budget = either error id (mkShrinkBudget budget)

aggregateAdmission :: DomainTable -> Bool
aggregateAdmission table =
  let entries = [(slot, either error id (slotEntry table slot)) | slot <- slots]
      symbol slot = "fixed_" ++ slotTag slot
      entryRows = [object ["group" .= ("own-first" :: String), "name" .= slotTag slot, "address" .= hexText address, "symbol" .= symbol slot, "veneer" .= symbol slot] | (slot, address) <- entries]
      domainValue :: Word32 -> Int -> [Value] -> Value
      domainValue base size rows = object
        [ "loader_base" .= hexText base, "loader_size" .= size, "entries" .= rows
        , "carriers" .= [object ["group" .= ("own-first" :: String), "start" .= hexText base, "end" .= hexText (base + fromIntegral size)]]
        , "externals" .= ([] :: [Value]), "demand" .= ([] :: [Value])
        ]
      carriers = BL.toStrict (encode (object
        [ "fill_word" .= ("0xe7f000f0" :: String)
        , "domains" .= object ["a" .= domainValue 0x40000 (domainLoaderSize DomainA) entryRows, "b" .= domainValue 0xd0000 (domainLoaderSize DomainB) []]
        ]))
      placements = BL.toStrict (encode (object ["placements" .=
        [object ["address" .= ("0x40000" :: String), "bytes" .= (4 :: Int), "section" .= (".text.packed" :: String)]
        ,object ["address" .= ("0x40004" :: String), "bytes" .= (4 :: Int), "section" .= (".rodata" :: String)]]]))
      top = maximum (map ((+ 4) . snd) entries)
      binary = B.replicate (fromIntegral (top - 0x40000)) 0x5a
      stock = B.replicate (domainLoaderSize DomainA) 0x33
      nm address size name = showHex address "" ++ " " ++ showHex size "" ++ " T " ++ name ++ "\n"
      symbols = concat [nm address (4 :: Word32) (symbol slot) | (slot, address) <- entries] ++ nm (0x40000 :: Word32) (4 :: Word32) "packed"
      admit rows plans = aggregateDomain 0xe7f000f0 DomainA table stock binary (C.pack rows) plans carriers
      accepted = do
        owned <- admit symbols placements
        body <- ownedBody owned S60
        pure (bodyEntry body == either error id (slotEntry table S60)
          && case bodyCode body of
            [(0x40000, image)] -> B.take 8 image == B.replicate 8 0x5a && B.take 4 (B.drop 8 image) == B.replicate 4 0x33
            _ -> False)
   in accepted == Right True
        && isLeft (admit (symbols ++ nm (0x40008 :: Word32) (4 :: Word32) "unplaced") placements)
        && isLeft (admit (symbols ++ nm (0x40008 :: Word32) (4 :: Word32) "fixed_s60") placements)
        && isLeft (admit symbols (BL.toStrict (encode (object ["placements" .=
          [object ["address" .= ("0x900000" :: String), "bytes" .= (4 :: Int), "section" .= (".text.outside" :: String)]]]))))

pureChecks :: DomainTable -> [IO Bool]
pureChecks tableA =
  let frame = B.replicate 16 0
      baseRegisters = [(R0, 1), (Sp, stackPointer), (Cpsr, 0x600000d3), (Lr, 0x958f00)]
      stackWrite = WriteEvent (stackPointer - 4) 4 0x1234
      frameWrite = WriteEvent stackPointer 4 0x1234
      bufferWrite = WriteEvent bufferPointer 4 7
      same a b = case (a, b) of
        (Right x, Right y) -> null (compareRuns x y)
        _ -> False
      differs a b = case (a, b) of
        (Right x, Right y) -> map differenceKindOf (compareRuns x y)
        _ -> ["unparsed"]
      differenceKindOf d = case d of
        StatusDiffers {} -> "status"
        RegisterDiffers register _ _ -> show register
        ControlDiffers {} -> "control"
        MemoryDiffers (RegionId name) -> name
        WritesDiffer -> "writes"
        ReferenceTruncated -> "truncated"
        SubjectTruncated -> "truncated"
      reference = runFrom baseRegisters frame [bufferWrite]
      validId text = case text of
        first : rest -> isAsciiLower first && length rest <= 63 && all (\c -> isAsciiLower c || isDigit c || c == '-') rest
        [] -> False
      loader = B.pack [fromIntegral (i `div` 4) | i <- [0 .. domainLoaderSize DomainA - 1 :: Int]]
      codeStarts slot = either (const []) (map fst . bodyCode) (stockBody DomainA tableA loader slot)
   in [ check "xorshift is the 13/17/5 generator" (xorshift 1 == 0x42021)
      , check "each slot's grid has the designed size" (and [length (caseGrid slot) == expectedCases slot | slot <- slots])
      , check "every entry CPSR is SVC or SYS with flags, Q, GE, I and F only" (all (privileged . entryCpsr . rowEntry) (concatMap caseGrid slots))
      , check "entry states cover both modes, I, F and the flag patterns" (length entryStates == 24 && length (nub (map entryCpsr entryStates)) == 24)
      , check "+0x16c rows reach +0x148 = 1, +0x128 failing on I and a null task" $
          let rows = caseGrid S16c
           in any ((/= 0) . rowContext) rows && any (\row -> rowContext row == 0 && entryIrqMasked (rowEntry row)) rows && any ((== NullTask) . rowTask) rows
      , check "+0x168 rows hold each r0 value" (all (`elem` map rowR0 (caseGrid S168)) argumentValues)
      , check "every slot crosses core-one identity and comparison identity on the IRQ stack" (all (\slot -> and [any (\row -> rowStack row == CoreOneIrqStack && rowIdentity row == Just identity && rowEntry row == entry) (caseGrid slot) | identity <- [0x8003, 0x4003], entry <- entryStates]) slots)
      , check "save/restore callers cover both sequences on both stacks" (and [any (\row -> rowCalls row == calls && rowStack row == stack && rowEntry row == entry) (caseGrid S164) | calls <- [SaveRestoreSave, NestedSaveRestore], stack <- [OrdinaryStack, CoreOneIrqStack], entry <- entryStates])
      , check "+0x1618 rows hold every CP14 value" (all (\v -> Just v `elem` map rowIdentity (caseGrid S1618)) identityValues)
      , check "+0x60 rows sweep every task and T+0xe byte" (all (`elem` [(rowTask row, rowFlagByte row) | row <- caseGrid S60]) [(t, b) | t <- taskChoices, b <- flagBytes])
      , check "case ids are runner ids" (all validId ["mutant-s1618-b-s1618-" ++ show n | n <- [1000 :: Int .. 1100]])
      , check "an identical run has no difference" (same reference reference)
      , check "flags and lr are not compared" (same reference (runFrom [(R0, 1), (Sp, stackPointer), (Cpsr, 0x000000d3), (Lr, 0)] frame [bufferWrite]))
      , check "mode, I, F, T and A are compared" (and [differs reference (runFrom [(R0, 1), (Sp, stackPointer), (Cpsr, cpsr)] frame [bufferWrite]) == ["control"] | cpsr <- [0x600000df, 0x60000053, 0x60000093, 0x600000f3, 0x600001d3]])
      , check "r0, r4-r11 and sp are compared; r1-r3 and r12 are not" $
          differs reference (runFrom (baseRegisters ++ [(R1, 9), (R12, 9)]) frame [bufferWrite]) == []
            && and [differs reference (runFrom (baseRegisters ++ [(register, 9)]) frame [bufferWrite]) == [show register] | register <- [R0, R4, R5, R6, R7, R8, R9, R10, R11, Sp]]
      , check "stack writes below entry sp are not compared" (same reference (runFrom baseRegisters frame [stackWrite, bufferWrite, stackWrite]))
      , check "caller-frame writes and bytes are compared" (differs reference (runFrom baseRegisters (B.replicate 16 1) [frameWrite, bufferWrite]) == ["caller-frame", "writes"])
      , check "write order is compared" (differs (runFrom baseRegisters frame [bufferWrite, frameWrite]) (runFrom baseRegisters frame [frameWrite, bufferWrite]) == ["writes"])
      , check "a truncated subject differs" (case reference of Right run -> compareRuns run run {runWrites = Nothing} == [SubjectTruncated]; Left _ -> False)
      , check "stock bodies map the helper for +0x1618 and the callees for +0x16c" (codeStarts S1618 == [0x412bc, 0x427b8] && codeStarts S16c == [0x4118c, 0x4cbd8, 0x4c620])
      , check "stock code is the loader's bytes at the extent" (either (const False) (\body -> bodyCode body == [(0x47540, B.take 0x40 (B.drop 0x7540 loader))]) (stockBody DomainA tableA loader S60))
      , check "A32 encodings" $
          assembleAt 0x41198 [branchLink 0x4cbd8, movw r3 0x4760, ldrb r0 r3 0xe, on CondNe (mvnI r0 24), tstI r3 0x80, msrControl r1, mrcIdentity r1, mul r2 r2 r3, movShift r3 r3 Asr 3, push [r11, lr], pop [r11, pc], bxLr]
            == Right [0xeb002e8e, 0xe3043760, 0xe5d3000e, 0x13e00018, 0xe3130080, 0xe121f001, 0xee121e10, 0xe0020392, 0xe1a031c3, 0xe92d4800, 0xe8bd8800, 0xe12fff1e]
      , check "labels resolve forward and backward" (assembleAt 0x1000 [mark (Label "top"), on CondEq (branch (Label "end")), branch (Label "top"), mark (Label "end")] == Right [0x0a000000, 0xeafffffd])
      , check "one mutant per slot and two equivalents" (sort (map controlSlot mutants) == slots && length equivalents == 2 && all ((== MustFail) . controlExpectation) mutants && all ((== MustPass) . controlExpectation) equivalents)
      , check "generated rows are counted and valid" $
          let seed = GenerationSeed 0x2545f4914f6cdd1d
              rows = generateRows seed (generationCount 32)
           in length rows == 32 && all validRow rows
      , check "the seed changes the generated corpus" (generateRows (GenerationSeed 1) (generationCount 8) /= generateRows (GenerationSeed 2) (generationCount 8))
      , check "a generated task word is a record start of the 120-record table" $
          let row = firstGeneratedRow
           in validRow row
                && all (\offset -> validRow row {rowTask = TaskAt offset} == (offset `elem` [0, 119 * recordBytes])) [0, 4, 8, 119 * recordBytes, 120 * recordBytes, -recordBytes]
      , check "generation, budget and control inputs are bounded" $
          all isLeft [parseGenerationCount "0", parseGenerationCount "257"]
            && all isLeft [parseGenerationSeed "zz", parseGenerationSeed "0x"]
            && all isLeft [parseShrinkBudget "0", parseShrinkBudget "257"]
            && all isRight [parseGenerationCount "1", parseGenerationCount "256"]
            && all isRight [parseGenerationSeed "0x2545f4914f6cdd1d", parseGenerationSeed "17"]
            && isRight (parseShrinkBudget "32")
      , check "count and budget constructors refuse invalid values" $
          isLeft (mkGenerationCount 0) && isLeft (mkGenerationCount 257) && isLeft (mkShrinkBudget 0) && isLeft (mkShrinkBudget 257)
            && isRight (mkGenerationCount 1) && isRight (mkShrinkBudget 1)
      , check "the command line rejects incomplete or misplaced generation flags" $
          all isLeft
            [ parseCommand ["--generate"]
            , parseCommand ["--generate", "4"]
            , parseCommand ["--generate", "4", "--seed"]
            , parseCommand ["--seed", "1"]
            , parseCommand ["--shrink-budget", "4"]
            , parseCommand ["--control", "mutant-s16c"]
            , parseCommand ["--replay"]
            , parseCommand ["--generate", "4", "--seed", "1", "--replay", "summary.json"]
            , parseCommand ["--generate", "4", "--seed", "1", "--timing-only"]
            , parseCommand ["--generate", "0", "--seed", "1"]
            , parseCommand ["--generate", "4", "--seed", "1", "--shrink-budget", "0"]
            , parseCommand ["--generate", "4", "--seed", "1", "--control", "mutant-s128"]
            , parseCommand ["--replay", "summary.json", "--seed", "1"]
            ]
            && isRight (parseCommand [])
            && isRight (parseCommand ["--generate", "4", "--seed", "1"])
            && isRight (parseCommand ["--replay", "summary.json"])
      , check "aggregate input requires both images, symbols and placements" $
          let flags = ["--aggregate-a-bin", "--aggregate-a-symbols", "--placements-a", "--aggregate-b-bin", "--aggregate-b-symbols", "--placements-b"]
              arguments = concatMap (\flag -> [flag, "input"]) flags
           in isRight (parseCommand arguments)
                && all (\missing -> isLeft (parseCommand (concatMap (\flag -> [flag, "input"]) (filter (/= missing) flags)))) flags
                && isRight (parseCommand (["--owned-a-bin", "standalone-a", "--owned-a-symbols", "standalone-a.symbols", "--owned-b-bin", "standalone-b", "--owned-b-symbols", "standalone-b.symbols"] ++ arguments))
      , check "aggregate fixed ABI entries coexist with packed code and rodata without admitting unplaced functions" (aggregateAdmission tableA)
      , check "recorded boundaries parse into typed kinds" $
          parseDifferenceKind "RegisterKind R0" == Right (RegisterKind R0)
            && parseDifferenceKind "MemoryKind (RegionId \"buffer\")" == Right (MemoryKind (RegionId "buffer"))
            && parseDifferenceKind "StatusKind" == Right StatusKind
            && isLeft (parseDifferenceKind "RegisterKind nope")
            && isLeft (parseDifferenceKind "")
      , check "replay refuses a non-generation or empty source" $
          let source :: String -> Value
              source mode = object ["schema" .= (1 :: Int), "mode" .= mode, "slot" .= ("0x16c" :: String), "domains" .= ([] :: [Value])]
           in isLeft (readRecordedCases (source "replay")) && isLeft (readRecordedCases (source "generate"))
      , check "the command line rejects unknown options and bare common flags" $
          all isLeft
            [ parseCommand ["--generat4", "4"]
            , parseCommand ["--runner"]
            , parseCommand ["--work"]
            , parseCommand ["--owned-a-bin"]
            , parseCommand ["--timing-only", "--unknown", "x"]
            ]
      , check "replay requires both reduction records and keyed roles" $
          let reduction :: String -> Value
              reduction role = object ["role" .= role, "original" .= Null, "reduced" .= Null]
              domain :: Value
              domain = object ["domain" .= ("a" :: String), "candidate_reduction" .= reduction "owned"]
              summary :: Value
              summary = object ["schema" .= (1 :: Int), "mode" .= ("generate" :: String), "slot" .= ("0x16c" :: String), "control" .= ("mutant-s16c" :: String), "domains" .= [domain]]
              mismatchDomain :: Value
              mismatchDomain = object ["domain" .= ("a" :: String), "candidate_reduction" .= reduction "mutant-s16c", "control_reduction" .= Null]
              mismatch :: Value
              mismatch = object ["schema" .= (1 :: Int), "mode" .= ("generate" :: String), "slot" .= ("0x16c" :: String), "control" .= ("mutant-s16c" :: String), "domains" .= [mismatchDomain]]
           in isLeft (readRecordedCases summary) && isLeft (readRecordedCases mismatch)
      , do
          let row = firstGeneratedRow
          reduction <- shrinkRow (shrinkBudget 32) (const (pure False)) row
          check "reduction stops at a fixed point when no candidate reproduces" (reductionStatus reduction == FixedPoint && reductionReduced reduction == row)
      , do
          let row = firstGeneratedRow
          reduction <- shrinkRow (shrinkBudget 1) (const (pure True)) row
          check "reduction reports an exhausted budget" (reductionStatus reduction == BudgetExhausted)
      , do
          let row = firstGeneratedRow
          reduction <- shrinkRow (shrinkBudget 32) (\candidate -> pure (taskSlotOf candidate == 0)) row
          check "reduction accepts only valid smaller inputs" (reductionStatus reduction == FixedPoint && taskSlotOf (reductionReduced reduction) == 0 && validRow (reductionReduced reduction))
      ]

-- | Every grid row with the controls alone, or 'sampleKeys' with the reference repeated as a control too.
data Scope = FullGrid | Sample deriving (Eq)

scopeRows :: Scope -> Slot -> [CaseKey]
scopeRows scope slot = case scope of
  FullGrid -> map CaseKey [0 .. length (caseGrid slot) - 1]
  Sample -> sampleKeys slot

-- | Runs a slot's rows against the owned body and each of its controls, with the owned runs as the
-- reference; returns the failures and the slot's report.
nativeChecks :: Scope -> FilePath -> FilePath -> Word32 -> Domain -> DomainTable -> OwnedDomain -> Slot -> IO ([String], Maybe SlotReport)
nativeChecks scope runner temporary fill domain table owned slot = do
  let name = domainLabel domain ++ " " ++ slotTag slot
      run = runOnce scope runner temporary domain slot
      report label control outcome = do
        putStrLn
          ( "     " ++ name ++ " " ++ label ++ ": " ++ show (outcomeCases outcome - outcomeMatches outcome) ++ " of " ++ show (outcomeCases outcome)
              ++ " cases differ, " ++ show (outcomeDifferingReturned outcome) ++ " of them returned"
          )
        pure (Right control)
  case (,) <$> ownedBody owned slot <*> slotEntry table slot of
    Left problem -> pure ([name ++ ": " ++ problem], Nothing)
    Right (body, entry) -> do
      reference <- run "owned" body
      let baseline = [name ++ ": an owned run did not return" | not (allReturned reference)]
      repeated <- if scope == Sample
        then do
          outcome <- outcomeOf reference <$> run "owned-repeat" body
          (: []) <$> report "owned-repeat" (ControlResult "owned-repeat" MustPass "the reference body against itself" outcome) outcome
        else pure []
      controls <- forM [control | control <- mutants ++ equivalents, controlSlot control == slot] $ \control ->
        case controlCode domain table control >>= \code -> replaceEntry fill code body of
          Left problem -> pure (Left (name ++ " " ++ controlName control ++ ": " ++ problem))
          Right subject -> do
            outcome <- outcomeOf reference <$> run (controlName control) subject
            report (controlName control) (ControlResult (controlName control) (controlExpectation control) (controlNote control) outcome) outcome
      let results = [result | Right result <- repeated ++ controls]
          failures = baseline ++ [problem | Left problem <- controls] ++ [name ++ " " ++ resultControl result ++ " does not hold" | result <- results, not (holds result)]
      pure (failures, Just (SlotReport domain slot entry reference (outcomeOf reference reference) results))

runOnce :: Scope -> FilePath -> FilePath -> Domain -> Slot -> String -> Body -> IO (M.Map CaseKey Run)
runOnce scope runner temporary domain slot role body = do
  let directory = temporary </> role ++ "-" ++ domainLabel domain ++ "-" ++ slotTag slot
  runs <- runRows runner directory role domain slot (scopeRows scope slot) body
  removeDirectoryRecursive directory
  pure runs

-- | A +0x14c body of fill words stops on every case: as the stock body it fails the run, and as a mutant it does not hold.
stoppingChecks :: Scope -> FilePath -> FilePath -> Word32 -> DomainTable -> OwnedDomain -> IO [Bool]
stoppingChecks scope runner temporary fill table owned =
  case (,,) <$> ownedBody owned S14c <*> (ownedBody owned S14c >>= replaceEntry fill []) <*> slotEntry table S14c of
    Left problem -> (: []) <$> check ("stopping body: " ++ problem) False
    Right (body, stopping, entry) -> do
      ownedRuns <- runOnce scope runner temporary DomainA S14c "owned-reference" body
      stoppingRuns <- runOnce scope runner temporary DomainA S14c "stopping" stopping
      let asMutant = ControlResult "stopping" MustFail "fill words only" (outcomeOf ownedRuns stoppingRuns)
          asStock = SlotReport DomainA S14c entry stoppingRuns (outcomeOf stoppingRuns ownedRuns) []
      sequence
        [ check "a fill-word body stops on every case" (not (M.null stoppingRuns) && all ((/= Returned) . runStatus) (M.elems stoppingRuns))
        , check "a stock body that stops exits 1" (verdictExit [asStock] == ExitFailure 1)
        , check "a mutant that never returns does not hold" (not (holds asMutant) && outcomeDifferingStopped (resultOutcome asMutant) == outcomeCases (resultOutcome asMutant))
        ]

-- | The generated corpus and its first reduced difference against the existing +0x16c control,
-- with the owned body as the reference the native tests can build without a loader file.
generatedChecks :: FilePath -> FilePath -> Word32 -> Domain -> DomainTable -> OwnedDomain -> IO [Bool]
generatedChecks runner temporary fill domain table owned =
  case (ownedBody owned S16c, [control | control <- mutants, controlSlot control == S16c]) of
    (Right body, control : _) -> case controlCode domain table control >>= \code -> replaceEntry fill code body of
      Left problem -> (: []) <$> check ("generated corpus: " ++ problem) False
      Right controlBody -> do
        let input =
              GeneratedInput
                { generatedRunner = runner
                , generatedWork = temporary </> "generated-" ++ domainLabel domain
                , generatedDomainId = domain
                , generatedReference = (OwnedRole, body)
                , generatedSubjects = [(ControlRole control, controlBody)]
                , generatedSeed = GenerationSeed 0x2545f4914f6cdd1d
                , generatedCount = generationCount 12
                , generatedBudget = shrinkBudget 32
                }
        result <- runGeneratedDomain input
        case generatedResults result of
          [subject] -> case subjectReduction subject of
            Just report -> do
              let originalKinds = map differenceKind (shrinkOriginalDifferences report)
                  replayKinds = map differenceKind (shrinkReplayDifferences report)
                  recordedOriginal = RecordedCase (ControlRole control) domain RecordedOriginal (shrinkOriginalLabel report) originalKinds (map differenceText (shrinkOriginalDifferences report)) (shrinkOriginalBatches report)
                  recordedReduced = RecordedCase (ControlRole control) domain RecordedReduced (shrinkReducedLabel report) replayKinds (map differenceText (shrinkReducedDifferences report)) (shrinkReducedBatches report)
                  replayWork = temporary </> "replay-" ++ domainLabel domain
                  originalReport = replayReferenceBatch (shrinkOriginalBatches report) </> caseIdText (shrinkOriginalLabel report) ++ ".native-result.json"
                  reducedReport = replayReferenceBatch (shrinkReducedBatches report) </> caseIdText (shrinkReducedLabel report) ++ ".native-result.json"
              originalBytes <- B.readFile originalReport
              reducedBytes <- B.readFile reducedReport
              let fixtureName = "unrelated-fixture-" ++ domainLabel domain ++ ".txt"
                  fixtureSource = replayReferenceBatch (shrinkOriginalBatches report)
              B.writeFile (fixtureSource </> fixtureName) "unrelated fixture"
              originalReplay <- replayCase runner replayWork recordedOriginal
              reducedReplay <- replayCase runner replayWork recordedReduced
              preservedOriginal <- B.readFile originalReport
              preservedReduced <- B.readFile reducedReport
              counts <- freshBatchCounts replayWork
              fixturePreserved <- doesFileExist (fixtureSource </> fixtureName)
              fixtureCopied <- containsFileNamed replayWork fixtureName
              wrongBoundary <- replayCase runner (temporary </> "replay-wrong-" ++ domainLabel domain) recordedReduced {recordedBoundary = [StatusKind]}
              runnerHash <- runnerDigest runner >>= either fail pure . parseSha256Hex
              let incompleteSource = temporary </> "replay-incomplete-source-" ++ domainLabel domain
              copyDirectory (replaySubjectBatch (shrinkReducedBatches report)) incompleteSource
              preparedIncomplete <- preflightCase runnerHash recordedReduced {recordedBatches = ReplayBatches (replayReferenceBatch (shrinkReducedBatches report)) incompleteSource}
              incompleteEntries <- listDirectory incompleteSource
              case [entry | entry <- incompleteEntries, takeExtension entry == ".bin"] of
                artifact : _ -> removeFile (incompleteSource </> artifact)
                [] -> fail ("no artifact to remove in " ++ incompleteSource)
              incompleteResult <- executeCase runner (temporary </> "replay-incomplete-work-" ++ domainLabel domain) preparedIncomplete
              let changedSource = temporary </> "replay-changed-source-" ++ domainLabel domain
              copyDirectory (replaySubjectBatch (shrinkReducedBatches report)) changedSource
              preparedChanged <- preflightCase runnerHash recordedReduced {recordedBatches = ReplayBatches (replayReferenceBatch (shrinkReducedBatches report)) changedSource}
              changeInitialR0 changedSource
              changedAfterPreflight <- executeCase runner (temporary </> "replay-changed-work-" ++ domainLabel domain) preparedChanged
              renamed <-
                try
                  (loadBatchRuns runner (replayReferenceBatch (shrinkReducedBatches report)) (const (CaseId "changed-binding")) domain [(shrinkOriginalKey report, shrinkReducedRow report)])
                  :: IO (Either IOException (M.Map CaseKey Run))
              corrupted <- corruptBatch (replaySubjectBatch (shrinkReducedBatches report)) (temporary </> "corrupt-" ++ domainLabel domain)
              changedCase <-
                try
                  (replayCase runner replayWork recordedReduced {recordedBatches = ReplayBatches (replayReferenceBatch (shrinkReducedBatches report)) corrupted})
                  :: IO (Either IOException ReplayCaseResult)
              changedRunner <- try (replayCase "/bin/true" replayWork recordedReduced) :: IO (Either IOException ReplayCaseResult)
              missing <- try (replayCase runner replayWork recordedReduced {recordedCaseId = CaseId "missing-case"}) :: IO (Either IOException ReplayCaseResult)
              sequence
                [ check "generated corpus: the reference returns" (generatedReferenceReturned result)
                , check "generated corpus: the +0x16c control differs on a returned case" (outcomeDifferingReturned (subjectOutcome subject) > 0)
                , check "generated corpus: the reduction preserves the compared boundary" (not (null originalKinds) && replayKinds == originalKinds)
                , check "generated corpus: the reduction reaches a smaller valid task record" (validRow (shrinkReducedRow report) && taskSlotOf (shrinkReducedRow report) < taskSlotOf (shrinkOriginalRow report))
                , check "generated corpus: the in-run replay reproduces the reduced case" (shrinkReplayDifferences report == shrinkReducedDifferences report)
                , check "replay: the original case reproduces its recorded difference" (replayReproduces originalReplay)
                , check "replay: the reduced case reproduces its recorded difference" (replayReproduces reducedReplay)
                , check "replay: the preserved reports are unchanged" (preservedOriginal == originalBytes && preservedReduced == reducedBytes)
                , check "replay: fresh batches hold only the selected case" (not (null counts) && all (== 1) counts)
                , check "replay: unrelated source files are not copied" (fixturePreserved && not fixtureCopied)
                , check "replay: a wrong typed boundary does not reproduce" (not (replayReproduces wrongBoundary))
                , check "replay: an execution failure is incomplete and not reproduced" (case replayStatus incompleteResult of ReplayIncomplete _ -> True; _ -> False)
                , check "replay: an input changed after preflight is incomplete" (case replayStatus changedAfterPreflight of ReplayIncomplete _ -> True; _ -> False)
                , check "replay: a changed case byte is rejected before execution" (case changedCase of Left _ -> True; Right _ -> False)
                , check "replay: a changed runner is rejected before execution" (case changedRunner of Left _ -> True; Right _ -> False)
                , check "replay: a missing recorded case is rejected" (case missing of Left _ -> True; Right _ -> False)
                , check "replay: a renamed case label is rejected" (case renamed of Left _ -> True; Right _ -> False)
                ]
            Nothing -> (: []) <$> check "generated corpus: the control difference was reduced" False
          _ -> (: []) <$> check "generated corpus: one subject result" False
    (Right _, []) -> (: []) <$> check "generated corpus: no +0x16c control" False
    (Left problem, _) -> (: []) <$> check ("generated corpus: " ++ problem) False

changeInitialR0 :: FilePath -> IO ()
changeInitialR0 directory = do
  batch <- B.readFile (directory </> "batch.json") >>= either fail pure . eitherDecodeStrict'
  entry <- case list (get batch "cases") of
    [single] -> pure single
    _ -> fail "expected one reduced case"
  let path = directory </> Host.str (get entry "path")
  value <- B.readFile path >>= either fail pure . eitherDecodeStrict'
  case value of
    Object fields -> case KM.lookup "registers" fields of
      Just (Object registers) ->
        BL.writeFile path (encode (Object (KM.insert "registers" (Object (KM.insert "r0" (String "0x12345678") registers)) fields)))
      _ -> fail "case has no register object"
    _ -> fail "case is not an object"

copyDirectory :: FilePath -> FilePath -> IO ()
copyDirectory source destination = do
  createDirectoryIfMissing True destination
  entries <- listDirectory source
  forM_ entries $ \entry -> copyFile (source </> entry) (destination </> entry)

corruptBatch :: FilePath -> FilePath -> IO FilePath
corruptBatch source destination = do
  copyDirectory source destination
  entries <- listDirectory destination
  case [entry | entry <- entries, takeExtension entry == ".bin"] of
    artifact : _ -> B.appendFile (destination </> artifact) (B.singleton 0)
    [] -> fail ("no artifact to corrupt in " ++ source)
  pure destination

freshBatchCounts :: FilePath -> IO [Int]
freshBatchCounts root = do
  cases <- listDirectory root
  fmap concat . forM cases $ \caseDirectory -> do
    let batchPath = root </> caseDirectory </> "reference" </> "batch-0" </> "batch.json"
    exists <- doesFileExist batchPath
    if not exists
      then pure []
      else do
        bytes <- B.readFile batchPath
        pure $ case eitherDecodeStrict' bytes of
          Right value -> [length (list (get value "cases"))]
          Left _ -> [-1]

containsFileNamed :: FilePath -> String -> IO Bool
containsFileNamed root name = do
  entries <- listDirectory root
  fmap or . forM entries $ \entry -> do
    let path = root </> entry
    directory <- doesDirectoryExist path
    if directory then containsFileNamed path name else pure (entry == name)

main :: IO ()
main = do
  hSetBuffering stdout LineBuffering
  arguments <- getArgs
  temporary <- getEnv "TEST_TMPDIR"
  case arguments of
    [scopeName, runner, regionsPath, binA, symbolsA, binB, symbolsB] -> do
      scope <- case scopeName of
        "full" -> pure FullGrid
        "sample" -> pure Sample
        _ -> fail ("unknown scope " ++ scopeName)
      table <- either fail pure . parseRegionTable =<< B.readFile regionsPath
      linked <- mapM (\(bin, symbols) -> LinkedDomain <$> B.readFile bin <*> B.readFile symbols) [(binA, symbolsA), (binB, symbolsB)]
      pureResults <- sequence (pureChecks (domainTable table DomainA))
      sampleResult <- check "each slot's sample is at most 60 structured rows" (and [not (null keys) && length keys <= 60 && all (\(CaseKey index) -> index < length (caseGrid slot) - 64) keys | slot <- slots, let keys = sampleKeys slot])
      owned <- forM (zip domains linked) $ \(domain, linkedDomain) ->
        either (fail . ((domainLabel domain ++ ": ") ++) . show) pure (ownedDomain (tableFill table) domain (domainTable table domain) linkedDomain)
      stopping <- case owned of
        ownedA : _ -> stoppingChecks scope runner temporary (tableFill table) (domainTable table DomainA) ownedA
        [] -> pure [False]
      checked <- fmap concat . forM (zip domains owned) $ \(domain, ownedRows) ->
        mapM (nativeChecks scope runner temporary (tableFill table) domain (domainTable table domain) ownedRows) slots
      generated <- fmap concat . forM (zip domains owned) $ \(domain, ownedRows) ->
        generatedChecks runner temporary (tableFill table) domain (domainTable table domain) ownedRows
      let failures = concatMap fst checked
          reports = [report | (_, Just report) <- checked]
      nativeResult <- check "owned bodies return and the controls hold in both loaders" (null failures && length reports == 2 * length slots)
      mapM_ (putStrLn . ("     " ++)) failures
      verdictResult <- check "the verdict over every slot report exits 0" (verdictExit reports == ExitSuccess)
      unless (and (nativeResult : verdictResult : sampleResult : pureResults ++ stopping ++ generated)) exitFailure
    _ -> fail "usage: own-first-oracle-tests full|sample RUNNER REGIONS BIN_A SYMBOLS_A BIN_B SYMBOLS_B"
