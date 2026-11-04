{-# LANGUAGE OverloadedStrings #-}

-- | The grid, the comparison and the controls against the owned bodies: each mutant differs from the
-- owned body on at least one case, and the owned body and the equivalents match it on every case.
module Main (main) where

import Control.Monad (forM, unless)
import Data.Bits (complement, (.&.))
import qualified Data.ByteString as B
import Data.Char (isAsciiLower, isDigit)
import Data.List (nub, sort)
import qualified Data.Map.Strict as M
import Data.Maybe (isJust)
import Data.Word (Word32)
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
import Replacement.RtosOwnFirst.Artifact (LinkedDomain (..))
import Replacement.RtosOwnFirst.Assembly
import Replacement.RtosOwnFirst.Controls
import Replacement.RtosOwnFirst.Oracle
import Replacement.RtosOwnFirst.Regions hiding (GuestAddress (..))
import System.Directory (removeDirectoryRecursive)
import System.Environment (getArgs, getEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.IO (BufferMode (..), hSetBuffering, stdout)

check :: String -> Bool -> IO Bool
check label result = do
  putStrLn ((if result then "ok   " else "FAIL ") ++ label)
  pure result

expectedCases :: Slot -> Int
expectedCases slot = 64 + 24 * case slot of
  S60 -> 32
  S128 -> 40
  S12c -> 40
  S16c -> 40
  S154 -> 25
  S14c -> 1
  S164 -> 1
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
        ]
    , nativeProvenance = Complete (ProvenanceTotal (fromIntegral (length writes))) [Write (GuestAddress (fromIntegral address)) (WriteBytes (fromIntegral width)) | (address, width) <- writes]
    , nativeInstructions = InstructionCount 1
    , nativeEngineError = EngineError 0
    }

runFrom :: [(Register, Word32)] -> B.ByteString -> [WriteEvent] -> Either String Run
runFrom overrides callerFrame writes = runOf DomainA (observation overrides callerFrame [(eventAddress e, eventWidth e) | e <- writes]) writes

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
      , check "the grid is deterministic" (map caseGrid slots == map caseGrid slots)
      , check "every entry CPSR is SVC or SYS with flags, Q, GE, I and F only" (all (privileged . entryCpsr . rowEntry) (concatMap caseGrid slots))
      , check "entry states cover both modes, I, F and the flag patterns" (length entryStates == 24 && length (nub (map entryCpsr entryStates)) == 24)
      , check "+0x16c rows reach +0x148 = 1, +0x128 failing on I and a null task" $
          let rows = caseGrid S16c
           in any ((/= 0) . rowContext) rows && any (\row -> rowContext row == 0 && entryIrqMasked (rowEntry row)) rows && any ((== NullTask) . rowTask) rows
      , check "+0x168 rows hold each r0 value" (all (`elem` map rowR0 (caseGrid S168)) argumentValues)
      , check "+0x1618 rows hold each CP14 value and others hold none" (all (\v -> Just v `elem` map rowIdentity (caseGrid S1618)) identityValues && all (not . isJust . rowIdentity) (caseGrid S60))
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
      ]

-- | Runs a slot's grid against the owned body and each of its controls; returns the failures.
nativeChecks :: FilePath -> FilePath -> Word32 -> Domain -> DomainTable -> OwnedDomain -> Slot -> IO [String]
nativeChecks runner temporary fill domain table owned slot = do
  let name = domainLabel domain ++ " " ++ slotTag slot
      directory role = temporary </> role ++ "-" ++ domainLabel domain ++ "-" ++ slotTag slot
      run role body = do
        runs <- runSlot runner (directory role) role domain slot body
        removeDirectoryRecursive (directory role)
        pure runs
  case ownedBody owned slot of
    Left problem -> pure [name ++ ": " ++ problem]
    Right body -> do
      reference <- run "owned" body
      let baseline = [name ++ ": an owned run did not return" | not (all ((== Returned) . runStatus) (M.elems reference))]
      controls <- forM [control | control <- mutants ++ equivalents, controlSlot control == slot] $ \control ->
        case controlCode domain table control >>= \code -> replaceEntry fill code body of
          Left problem -> pure [name ++ " " ++ controlName control ++ ": " ++ problem]
          Right subject -> do
            outcome <- outcomeOf reference <$> run (controlName control) subject
            let differing = outcomeCases outcome - outcomeMatches outcome
            putStrLn ("     " ++ name ++ " " ++ controlName control ++ ": " ++ show differing ++ " of " ++ show (outcomeCases outcome) ++ " cases differ")
            pure $ case controlExpectation control of
              MustFail -> [name ++ " " ++ controlName control ++ ": no case differs" | differing == 0]
              MustPass -> [name ++ " " ++ controlName control ++ ": " ++ show differing ++ " cases differ, first " ++ show (outcomeFirst outcome) | differing /= 0]
      pure (baseline ++ concat controls)

main :: IO ()
main = do
  hSetBuffering stdout LineBuffering
  arguments <- getArgs
  temporary <- getEnv "TEST_TMPDIR"
  case arguments of
    [runner, regionsPath, binA, symbolsA, binB, symbolsB] -> do
      table <- either fail pure . parseRegionTable =<< B.readFile regionsPath
      linked <- mapM (\(bin, symbols) -> LinkedDomain <$> B.readFile bin <*> B.readFile symbols) [(binA, symbolsA), (binB, symbolsB)]
      pureResults <- sequence (pureChecks (domainTable table DomainA))
      failures <- fmap concat . forM (zip domains linked) $ \(domain, linkedDomain) ->
        case ownedDomain (tableFill table) domain (domainTable table domain) linkedDomain of
          Left problem -> pure [domainLabel domain ++ ": " ++ show problem]
          Right owned -> concat <$> mapM (nativeChecks runner temporary (tableFill table) domain (domainTable table domain) owned) slots
      nativeResult <- check "owned bodies return and the controls hold in both loaders" (null failures)
      mapM_ (putStrLn . ("     " ++)) failures
      unless (and (nativeResult : pureResults)) exitFailure
    _ -> fail "usage: own-first-oracle-tests RUNNER REGIONS BIN_A SYMBOLS_A BIN_B SYMBOLS_B"
