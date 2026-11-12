-- | bank-allowlist-tests ALLOWLIST CHECKER ELF_00C20000 ELF_00C21F88 ENGINE_ELF MODAL_FRAGMENT ENGINE_FRAGMENT
--
-- The ELFs are //host/firmware:bank-elf-* and :engine-* outputs for the
-- current bank sources. Fixtures replace function words in the 0x00c40000 ELF.
module Main (main) where

import A32 (Forbidden (..), Reg (..))
import qualified A32
import ArmElf
import BankAllowlist
import BankAllowlistComposition (composeBankAllowlist)
import Control.Monad (forM, forM_, unless, when)
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (find)
import Data.Word (Word32)
import RamPushLayout (menuEngineRegionExtentFor, modalEngineBankVariant)
import System.Directory (getTemporaryDirectory, removeFile)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitFailure)
import System.IO (hClose, openBinaryTempFile)
import System.Process (readProcessWithExitCode)

check :: String -> Bool -> IO ()
check label passed = unless passed $ do
  putStrLn ("FAIL: " ++ label)
  exitFailure

main :: IO ()
main = do
  [allowlistPath, checker, elfLow, elfHigh, engineElf, modalFragmentPath, engineFragmentPath] <- getArgs
  allowlistBytes <- B.readFile allowlistPath
  allowlist <- either (fail . ("allowlist: " ++)) pure (parseAllowlist allowlistBytes)
  clean <- B.readFile elfLow
  forM_ [elfLow, elfHigh] $ \path -> do
    bytes <- B.readFile path
    report <- run allowlist bytes
    check (path ++ " passes the allowlist; violations: " ++ show (map renderFinding (violations report))) (null (violations report))

  cleanElf <- either fail pure (parseElf clean)
  let tagged identity = cleanElf
        { elfSymbols = Symbol "RAM_PUSH_BANK_VARIANT" identity 0 OtherSymbol Absolute : elfSymbols cleanElf }
  check "render ELF identity refuses the standard publication count"
    (case checkBank allowlist (tagged 1) of Left _ -> True; Right _ -> False)
  check "unknown ELF bank identity refuses"
    (case checkBank allowlist (tagged 2) of Left _ -> True; Right _ -> False)
  let symbol name = maybe (error ("no symbol " ++ name)) symbolValue (find ((== name) . symbolName) (elfSymbols cleanElf))
      function name =
        case find (\s -> symbolName s == name && symbolKind s == FunctionSymbol) (elfSymbols cleanElf) of
          Just s -> (symbolValue s, symbolValue s + symbolSize s)
          Nothing -> error ("no function " ++ name)
      wordsIn (start, end) = [(a, w) | a <- [start, start + 4 .. end - 4], Just w <- [wordAt cleanElf a]]
      firstWhere name predicate =
        maybe (error ("no matching word in " ++ name)) fst (find (predicate . snd) (wordsIn (function name)))
      -- The literal word holding rb_last_status's address, which
      -- rb_status_record loads and stores through.
      statusLiteral = firstWhere "rb_status_record" (== symbol "rb_last_status")
      statusStore = firstWhere "rb_status_record" (\w -> w .&. 0x0ff00000 == 0x05800000)
      probeCall = firstWhere "rb_probe_entry" (\w -> w .&. 0xff000000 == 0xeb000000)
      frameSetup = firstWhere "rb_status_last" (== 0xe1a0b00d)
      -- rb_restore_run's `bl rb_assert_run`, which passes a 0 trace that
      -- rb_assert_one tests before storing through it; the clean ELFs above
      -- pass with it.
      zeroCall = maybe (error "no bl rb_assert_run in rb_restore_run") fst (find (\(a, w) -> w == branchLink a (fst (function "rb_assert_run"))) (wordsIn (function "rb_restore_run")))
      zeroTest = firstWhere "rb_assert_one" (\w -> w .&. 0x0ff0ffff == 0x03500000)
      fixture site word = patch cleanElf clean site word
      expect label bytes reasons = do
        report <- run allowlist bytes
        let found = [(findingAddress f, findingReason f) | f <- violations report]
        check (label ++ ": expected " ++ show reasons ++ ", got " ++ show (map renderFinding (violations report))) (found == reasons)

  let functionSymbol name = maybe (error ("no function " ++ name)) id
        (find (\s -> symbolName s == name && symbolKind s == FunctionSymbol) (elfSymbols cleanElf))
      alias name original = (functionSymbol original) {symbolName = name}
      aliasSymbols =
        [ alias "aaa_assert_alias" "rb_assert_one"
        , alias "zzz_assert_alias" "rb_assert_one"
        , alias "aaa_status_alias" "rb_status_record"
        , alias "aaa_probe_alias" "rb_probe_entry"
        ]
      withAliases elf = elf {elfSymbols = aliasSymbols ++ elfSymbols elf}
      checked policy elf = either fail pure (checkBank policy elf)
      counts report = (reportFunctions report, reportInstructions report, reportProvenStores report, reportProvenBranches report)
  baseline <- checked allowlist cleanElf
  aliased <- checked allowlist (withAliases cleanElf)
  check "exact aliases pass without duplicating analyzed bodies or effects"
    (null (violations aliased) && counts aliased == counts baseline)
  check "declarations on nonrepresentative names remain used"
    (map renderDeclaration (reportUnused aliased) == map renderDeclaration (reportUnused baseline))
  reordered <- checked allowlist (cleanElf {elfSymbols = elfSymbols cleanElf ++ reverse aliasSymbols})
  check "alias representative does not depend on symbol table order"
    (reportValue aliased == reportValue reordered)
  let aliasPolicy = allowlist {declarations =
        [d {declarationFunction = if declarationFunction d == "rb_assert_one" then "zzz_assert_alias" else declarationFunction d}
        | d <- declarations allowlist]}
  renamed <- checked aliasPolicy (withAliases cleanElf)
  check "a declaration may name any exact alias"
    (null (violations renamed) && length (reportUnused renamed) == length (reportUnused baseline))
  unknownElf <- either fail pure (parseElf (fixture probeCall 0xe12fff33))
  unknownAlias <- checked allowlist (withAliases unknownElf)
  check "an unknown effect in an aliased body is still refused once"
    ([(findingAddress f, findingFunction f, findingReason f) | f <- violations unknownAlias]
      == [(probeCall, "aaa_probe_alias", Unproven BranchAccess (Argument (Reg 3)))])
  uncheckedZero <- either fail pure (parseElf (fixture zeroTest 0xe1a00000))
  zeroAlias <- checked aliasPolicy (withAliases uncheckedZero)
  check "callee argument obligations survive a nonrepresentative declaration"
    ([(findingAddress f, findingReason f) | f <- violations zeroAlias] == [(zeroCall, UnprovenArgument)])
  forM_
    [ ("same entry, shorter extent", (alias "overlap" "rb_status_record") {symbolSize = symbolSize (functionSymbol "rb_status_record") - 4})
    , ("same entry, longer extent", (alias "overlap" "rb_status_record") {symbolSize = symbolSize (functionSymbol "rb_status_record") + 4})
    , ("interior entry", (alias "overlap" "rb_status_record")
        {symbolValue = symbolValue (functionSymbol "rb_status_record") + 4, symbolSize = symbolSize (functionSymbol "rb_status_record") - 4})
    ] $ \(label, overlappingSymbol) -> do
      refused <- checked allowlist (cleanElf {elfSymbols = overlappingSymbol : aliasSymbols ++ elfSymbols cleanElf})
      check ("partial overlap refuses: " ++ label)
        (any (\f -> case findingReason f of LayoutProblem _ -> True; _ -> False) (violations refused))
  let callerStart = fst (function "rb_assert_one")
      callerSize = snd (function "rb_assert_one") - callerStart
      resultWords = [0xe92d4000, branchLink (callerStart + 4) (fst (function "rb_status_record")), 0xe5801000, 0xe8bd8000]
        ++ replicate (fromIntegral (callerSize `div` 4) - 4) 0xe320f000
      resultBytes = patchBytes cleanElf clean callerStart
        (B.pack [fromIntegral (word `shiftR` shift) | word <- resultWords, shift <- [0,8,16,24 :: Int]])
  resultElf <- either fail pure (parseElf resultBytes)
  undeclaredResult <- checked aliasPolicy (withAliases resultElf)
  check "aliased callee results still require an effect declaration"
    (any (\f -> findingAddress f == callerStart + 8
      && findingReason f == Unproven StoreAccess (Result (BankFunction "aaa_status_alias"))) (violations undeclaredResult))
  forM_ ["rb_status_record", "aaa_status_alias"] $ \calleeName -> do
    let resultPolicy = aliasPolicy {declarations = declarations aliasPolicy ++
          [Declaration "zzz_assert_alias" StoreAccess (Result (BankFunction calleeName)) "synthetic callee result store"]}
    resultReport <- checked resultPolicy (withAliases resultElf)
    check ("callee result provenance resolves alias " ++ calleeName)
      (null (violations resultReport)
        && any (\f -> findingAddress f == callerStart + 8 && findingVerdict f /= Violation) (declared resultReport))
  let missingPolicy = allowlist {declarations = declarations allowlist ++
        [Declaration "not_a_function" StoreAccess (Argument (Reg 0)) "synthetic missing symbol"]}
  missing <- checked missingPolicy (withAliases cleanElf)
  check "aliases do not admit declarations for absent symbols"
    (any ((== UnknownDeclaredFunction) . findingReason) (violations missing))

  let indirectCallee = fst (function "rb_status_record")
      replaceFunction :: String -> [Word32] -> B.ByteString -> B.ByteString
      replaceFunction name instructions bytes =
        let (start, end) = function name
            padded = instructions ++ replicate (fromIntegral ((end - start) `div` 4) - length instructions) 0xe320f000
         in patchBytes cleanElf bytes start (B.pack [fromIntegral (word `shiftR` shift) | word <- padded, shift <- [0,8,16,24 :: Int]])
      indirectPolicy = allowlist {declarations = declarations allowlist ++
        [Declaration "rb_status_record" StoreAccess (Deref (Argument (Reg 0))) "synthetic callee writes through a field of its argument"]}
      framePolicy = indirectPolicy {declarations = declarations indirectPolicy ++
        [Declaration "rb_assert_one" StoreAccess (Cell StackFrame) "synthetic caller supplies a pointer field from its local object"]}
      frameProgram prefix =
        let callSite = callerStart + fromIntegral (length prefix * 4)
            instructions = prefix ++ [branchLink callSite indirectCallee, 0xe28dd008, 0xe8bd8010]
            bytes = replaceFunction "rb_status_record" [0xe5901000, 0xe5812000, 0xe12fff1e]
              (replaceFunction "rb_assert_one" instructions clean)
         in (callSite, bytes)
  forM_ [("exact", [0xe92d4010, 0xe24dd008, 0xe1a0000d]), ("merged", [0xe92d4010, 0xe24dd008, 0xe1a0000d, 0x128d0004])] $ \(label, prefix) -> do
    let (callSite, bytes) = frameProgram prefix
    refused <- run indirectPolicy bytes
    check (label ++ " local pointer field requires an explicit caller declaration")
      (any (\f -> findingAddress f == callSite && findingReason f == Unproven StoreAccess (Cell StackFrame)) (violations refused))
    admitted <- run framePolicy bytes
    check (label ++ " local pointer field uses only its declared frame provenance")
      (any (\f -> findingAddress f == callSite && findingReason f == Unproven StoreAccess (Cell StackFrame)) (declared admitted)
        && not (any ((== callSite) . findingAddress) (violations admitted)))
  let (unknownCall, unknownBytes) = frameProgram [0xe92d4010, 0xe24dd008, 0xe1a00083]
  unknownFrame <- run framePolicy unknownBytes
  check "unknown pointers do not acquire local frame provenance"
    (any (\f -> findingAddress f == unknownCall && findingReason f == UnprovenArgument) (violations unknownFrame))
  -- A stock call may write through any frame address the caller passed it.
  let stockTarget = case stockCalls allowlist of
        (call : _) -> stockCallAddress call
        [] -> error "the allowlist declares no stock call"
      writableBase = symbol "rb_last_status"
      slotWord opcode half = opcode .|. ((half .&. 0xf000) `shiftL` 4) .|. (half .&. 0xfff)
      slotPrefix = [0xe92d4010, 0xe24dd008, slotWord 0xe3004000 writableBase, slotWord 0xe3404000 (writableBase `shiftR` 16), 0xe58d4000]
      withCall instructions =
        let site = callerStart + fromIntegral (length instructions * 4)
         in replaceFunction "rb_assert_one" (instructions ++ [branchLink site stockTarget] ++ [0xe59d2000, 0xe3a03001, 0xe5823000, 0xe28dd008, 0xe8bd8010]) clean
      escapedSlot = withCall (slotPrefix ++ [0xe1a0000d])
      keptSlot = withCall slotPrefix
      rechecked = replaceFunction "rb_assert_one"
        (slotPrefix ++ [0xe1a0000d, branchLink (callerStart + 24) stockTarget,
          0xe59d2000, 0xe3520003, 0x8a000000, 0xe7843102, 0xe28dd008, 0xe8bd8010]) clean
  escapedReport <- run allowlist escapedSlot
  check "a slot whose address a stock call received loses its value"
    (any (\f -> findingAddress f == callerStart + 36) (violations escapedReport))
  keptReport <- run allowlist keptSlot
  check "a slot whose address never escaped keeps its value"
    (not (any (\f -> findingAddress f == callerStart + 32) (violations keptReport)))
  recheckedReport <- run allowlist rechecked
  check "a reloaded slot with a range check bounds its store index"
    (not (any (\f -> findingAddress f == callerStart + 40) (violations recheckedReport)))
  -- A bank veneer that calls stock through a register.
  let veneer = fst (function "rb_status_record")
      veneerBytes = replaceFunction "rb_status_record"
        [0xe52de004, slotWord 0xe300c000 stockTarget, slotWord 0xe340c000 (stockTarget `shiftR` 16), 0xe12fff3c, 0xe49de004, 0xe12fff1e] clean
      throughVeneer callee = replaceFunction "rb_assert_one"
        (slotPrefix ++ [0xe1a0000d, branchLink (callerStart + 24) veneer,
          0xe59d2000, 0xe3a03001, 0xe5823000, 0xe28dd008, 0xe8bd8010]) callee
      savedThroughVeneer = replaceFunction "rb_assert_one"
        (slotPrefix ++ [0xe1a0000d, branchLink (callerStart + 24) veneer, 0xe28dd008, 0xe8bd8010]) veneerBytes
  veneerReport <- run allowlist (throughVeneer veneerBytes)
  check "a slot whose address an owned veneer reaching stock received loses its value"
    (any (\f -> findingAddress f == callerStart + 36) (violations veneerReport))
  check "a veneer that saves and restores lr has no finding of its own"
    (not (any (\f -> findingFunction f == "rb_status_record") (violations veneerReport)))
  plainReport <- run allowlist (throughVeneer (replaceFunction "rb_status_record" [0xe1a00000, 0xe12fff1e] clean))
  check "a bank call that reaches no stock still forgets the caller's escaped local"
    (any (\f -> findingAddress f == callerStart + 36) (violations plainReport))
  savedReport <- run allowlist savedThroughVeneer
  check "a saved return address survives a call that receives a pointer to a local"
    (not (any (\f -> findingAddress f == callerStart + 32) (violations savedReport)))
  -- `add r0, sp, #1` exposes entry sp-15, inside the rb_last_status word at -16.
  unalignedReport <- run allowlist (withCall (slotPrefix ++ [0xe28d0001]))
  check "a frame word holding the lowest exposed byte loses its value"
    (any (\f -> findingAddress f == callerStart + 36) (violations unalignedReport))
  -- `str r4, [sp, #-4]` writes entry sp-12, below the sp the call is made with.
  let belowSpBytes = replaceFunction "rb_assert_one"
        [0xe92d4010, slotWord 0xe3004000 writableBase, slotWord 0xe3404000 (writableBase `shiftR` 16), 0xe50d4004,
          branchLink (callerStart + 16) stockTarget, 0xe51d2004, 0xe3a03001, 0xe5823000, 0xe8bd8010] clean
  belowSpReport <- run allowlist belowSpBytes
  check "a frame word below the call's stack pointer loses its value"
    (any (\f -> findingAddress f == callerStart + 28) (violations belowSpReport))
  -- `sub r0, sp, r1` keeps the frame root, so the whole frame is exposed.
  subReport <- run allowlist (withCall (slotPrefix ++ [0xe04d0001]))
  check "a frame address minus an unknown index exposes the frame"
    (any (\f -> findingAddress f == callerStart + 36) (violations subReport))
  -- A declared store through an unbounded frame index may reach any frame word.
  let frameIndexPolicy = allowlist {declarations = declarations allowlist ++
        [Declaration "rb_assert_one" StoreAccess (Index StackFrame) "synthetic two-word local array"]}
      frameIndexBytes = replaceFunction "rb_assert_one"
        (slotPrefix ++ [0xe78d1100, 0xe59d2000, 0xe3a03001, 0xe5823000, 0xe28dd008, 0xe8bd8010]) clean
  frameIndexReport <- run frameIndexPolicy frameIndexBytes
  check "a declared store through an unbounded frame index clears the frame"
    (any (\f -> findingAddress f == callerStart + 32) (violations frameIndexReport))
  -- The frame word above the escaped local, not only the escaped span, loses
  -- its value: a span rule keeps the word at sp+4 and proves the store.
  let abovePrefix = slotPrefix ++ [0xe58d4004, 0xe1a0000d]
      aboveSite = callerStart + fromIntegral (length abovePrefix * 4)
      aboveBytes = replaceFunction "rb_assert_one"
        (abovePrefix ++ [branchLink aboveSite stockTarget, 0xe59d2004, 0xe3a03001, 0xe5823000, 0xe28dd008, 0xe8bd8010]) clean
  aboveReport <- run allowlist aboveBytes
  check "a frame word above the escaped local loses its value after the call"
    (any (\f -> findingAddress f == aboveSite + 12) (violations aboveReport))
  -- A bank callee that writes through a frame pointer it was handed, without
  -- reaching stock, overwrites the caller's slot all the same.
  let writerCallee = replaceFunction "rb_status_record" [0xe5801000, 0xe12fff1e] clean
      writerPolicy = allowlist {declarations = declarations allowlist ++
        [Declaration "rb_status_record" StoreAccess (Argument (Reg 0)) "synthetic callee writes through its argument"]}
      writerPrefix = slotPrefix ++ [0xe3e01000, 0xe1a0000d]
      writerSite = callerStart + fromIntegral (length writerPrefix * 4)
      writerBytes = replaceFunction "rb_assert_one"
        (writerPrefix ++ [branchLink writerSite indirectCallee, 0xe59d2000, 0xe3a03001, 0xe5823000, 0xe28dd008, 0xe8bd8010]) writerCallee
  writerReport <- run writerPolicy writerBytes
  check "a bank callee that writes the frame it was handed loses the caller's slot"
    (any (\f -> findingAddress f == writerSite + 12) (violations writerReport))
  -- A tail-call veneer (`bx r12`) is a call to stock like `blx r12`.
  let tailVeneer = replaceFunction "rb_status_record"
        [slotWord 0xe300c000 stockTarget, slotWord 0xe340c000 (stockTarget `shiftR` 16), 0xe12fff1c] clean
  tailReport <- run allowlist (throughVeneer tailVeneer)
  check "a bx r12 veneer reaching stock forgets the caller's escaped local"
    (any (\f -> findingAddress f == callerStart + 36) (violations tailReport))
  -- Two bank calls in a chain: the caller's slot is gone after the first call
  -- whatever the second one reaches.
  let chainMiddle = fst (function "rb_status_record")
      chainVeneer = replaceFunction "rb_probe_entry"
        [slotWord 0xe300c000 stockTarget, slotWord 0xe340c000 (stockTarget `shiftR` 16), 0xe12fff1c] clean
      chainCallee = replaceFunction "rb_status_record"
        [0xe92d4002, branchLink (chainMiddle + 4) (fst (function "rb_probe_entry")), 0xe8bd4002, 0xe12fff1e] chainVeneer
  chainReport <- run allowlist (throughVeneer chainCallee)
  check "a two-level bank chain reaching stock forgets the caller's escaped local"
    (any (\f -> findingAddress f == callerStart + 36) (violations chainReport))
  -- A result the analysis does not model takes the frame from a frame operand:
  -- `add r1, sp, #4` puts entry sp-12 in r1 while the slot at entry sp-16 holds
  -- rb_last_status, so exposing the frame forgets that slot at the call.
  let frameCase label prefix = do
        let site = callerStart + fromIntegral (length prefix * 4)
        report <- run allowlist (withCall prefix)
        check (label ++ " exposes the frame")
          (any (\f -> findingAddress f == site + 12) (violations report))
  frameCase "a multiply of a frame address" (slotPrefix ++ [0xe28d1004, 0xe0000291])
  frameCase "an add with carry of a frame address" (slotPrefix ++ [0xe28d1004, 0xe0a10002])
  frameCase "a subtract with carry of a frame address" (slotPrefix ++ [0xe28d1004, 0xe0c10002])
  frameCase "a reverse subtract with carry of a frame address" (slotPrefix ++ [0xe28d1004, 0xe0e10002])
  frameCase "a signed byte extend of a frame address" (slotPrefix ++ [0xe28d1004, 0xe6af0071])
  -- An operand the analysis cannot resolve still carries a frame address into
  -- the result.
  frameCase "a register-shifted frame address" (slotPrefix ++ [0xe3a01020, 0xe1a0017d])
  frameCase "a frame address as a reverse subtract's subtrahend" (slotPrefix ++ [0xe28d1004, 0xe0610002])
  frameCase "a multiply with a frame factor and a scalar accumulator" (slotPrefix ++ [0xe28d1004, 0xe3a02001, 0xe3e03003, 0xe0203291])
  -- A frame index added to an exact pointer: without the operand rule the
  -- result keeps only the pointer's root and the frame is lost.
  let offsetPrefix = [0xe08d2000, 0xe3001000, 0xe3401001, 0xe0810002]
      offsetStore = callerStart + fromIntegral (length offsetPrefix * 4)
      offsetBytes = replaceFunction "rb_assert_one" (offsetPrefix ++ [0xe5803000, 0xe12fff1e]) clean
  offsetReport <- run allowlist offsetBytes
  check "a frame index added to an exact pointer keeps the frame"
    (any (\f -> findingAddress f == offsetStore && findingReason f == Unproven StoreAccess (Index StackFrame)) (violations offsetReport))
  -- A push at entry sp allocates locals for the callees that receive their
  -- address; only the slot the push wrote lr into keeps its value across the
  -- call.
  let pushedLocalBytes = replaceFunction "rb_assert_one"
        [0xe92d401c, slotWord 0xe3004000 writableBase, slotWord 0xe3404000 (writableBase `shiftR` 16), 0xe58d4000,
         0xe1a0000d, branchLink (callerStart + 20) stockTarget, 0xe59d2000, 0xe3a03001, 0xe5823000, 0xe8bd801c] clean
  pushedLocalReport <- run allowlist pushedLocalBytes
  check "a pushed local a call received the address of loses its value"
    (any (\f -> findingAddress f == callerStart + 32 && findingReason f == Unproven StoreAccess (Index (StockAddress 0))) (violations pushedLocalReport))
  -- A pushed local a call received the address of is no longer a known 0, so
  -- the compare cannot prune the path that stores through entry r6.
  let staleLocalBytes = replaceFunction "rb_assert_one"
        [0xe92d401c, 0xe3a01000, 0xe58d1000, 0xe1a0000d, branchLink (callerStart + 16) stockTarget,
         0xe59d1000, 0xe3510000, 0x15863000, 0xe8bd801c] clean
  staleLocalReport <- run allowlist staleLocalBytes
  check "a pushed local a call received the address of no longer prunes a path"
    (any (\f -> findingAddress f == callerStart + 28 && findingReason f == Unproven StoreAccess (Argument (Reg 6))) (violations staleLocalReport))
  -- A pushed register slot is a local like any other once a callee holds its
  -- address, even when the push also saved the return address.
  let pushedRegisterBytes = replaceFunction "rb_assert_one"
        [0xe92d4f80, 0xe28d0004, branchLink (callerStart + 8) stockTarget,
         0xe59d2004, 0xe3a03001, 0xe5823000, 0xe8bd8f80] clean
  pushedRegisterReport <- run allowlist pushedRegisterBytes
  check "a pushed register slot a call received the address of loses its value"
    (any (\f -> findingAddress f == callerStart + 20 && findingReason f == Unproven StoreAccess (Index (StockAddress 0))) (violations pushedRegisterReport))
  -- RRX shifts the carry into bit 31, so an exact pointer does not survive it.
  let rrxBytes = replaceFunction "rb_assert_one"
        [slotWord 0xe3004000 writableBase, slotWord 0xe3404000 (writableBase `shiftR` 16), 0xe1a00064, 0xe3a03001, 0xe5803000, 0xe12fff1e] clean
  rrxReport <- run allowlist rrxBytes
  check "a rotate-right-extend of an exact pointer is not the pointer"
    (any (\f -> findingAddress f == callerStart + 16 && findingReason f == Unproven StoreAccess (Index (StockAddress 0))) (violations rrxReport))
  -- RRX in a register-offset address takes the carry, so the store is not the
  -- base pointer's declared cell.
  let rrxOffsetBytes = replaceFunction "rb_assert_one"
        [slotWord 0xe3004000 writableBase, slotWord 0xe3404000 (writableBase `shiftR` 16), 0xe3a01008, 0xe3a03001,
         0xe7843061, 0xe12fff1e] clean
  rrxOffsetReport <- run allowlist rrxOffsetBytes
  check "a rotate-right-extend in a register-offset address is not the index"
    (any (\f -> findingAddress f == callerStart + 16
      && findingReason f == Unproven StoreAccess (Index (BankObject "rb_last_status"))) (violations rrxOffsetReport))
  -- The entry push's return slot is exempt only where every incoming path
  -- wrote it. Each path sets the flags before the join so the two states meet.
  let joinProgram =
        [ 0xe3500000, 0x1a000004, 0xe92d4010, 0xe3520000, 0xea000006, 0xe320f000, 0xe320f000
        , 0xe24dd008, 0xe58de004, 0xe3530000, 0xe320f000, 0xe320f000, 0xe1a0000d
        , branchLink (callerStart + 52) stockTarget, 0xe59df004
        ]
      joinBytes = replaceFunction "rb_assert_one" joinProgram clean
  joinReport <- run allowlist joinBytes
  check "a return address exempt on one path only does not survive the join"
    (any (\f -> findingAddress f == callerStart + 56 && findingReason f == UnboundedBranch) (violations joinReport))
  -- A call that received a local's address drops the pushed register slots,
  -- but a span store that can still reach one is refused.
  let savedSpanBytes = replaceFunction "rb_assert_one"
        [0xe92d4010, 0xe24dd008, 0xe2004002, 0xe1a0000d, branchLink (callerStart + 16) stockTarget,
         0xe78d3104, 0xe28dd008, 0xe8bd8010] clean
  savedSpanReport <- run allowlist savedSpanBytes
  check "a span store that can reach a pushed register slot is refused"
    (any (\f -> findingAddress f == callerStart + 20 && findingReason f == StackStoreOverSaved) (violations savedSpanReport))
  -- A slot only one incoming path pushed is still a saved slot for a span
  -- store that can reach it after the join.
  let savedJoinBytes = replaceFunction "rb_assert_one"
        [0xe3500000, 0x1a000002, 0xe92d4010, 0xe3520000, 0xea000003, 0xe24dd008, 0xe58d4000, 0xe58de004,
         0xe3530000, 0xe2011001, 0xe24dd008, 0xe78d3181, 0xe28dd008, 0xe8bd8010] clean
  savedJoinReport <- run allowlist savedJoinBytes
  check "a span store that can reach a slot only one path pushed is refused"
    (any (\f -> findingAddress f == callerStart + 44 && findingReason f == StackStoreOverSaved) (violations savedJoinReport))
  -- A register saved after sp has moved is a saved slot like the entry push's.
  let savedAfterMoveBytes = replaceFunction "rb_assert_one"
        [0xe52de004, 0xe52d4004, 0xe24dd008, 0xe2011001, 0xe78d3181, 0xe28dd008, 0xe49d4004, 0xe49df004] clean
  savedAfterMoveReport <- run allowlist savedAfterMoveBytes
  check "a register saved below the entry stack pointer is a saved slot"
    (any (\f -> findingAddress f == callerStart + 16 && findingReason f == StackStoreOverSaved) (violations savedAfterMoveReport))
  -- The difference of two frame addresses is a scalar, not an address: it does
  -- not expose the frame, so the slot below the exposed operands survives.
  let differenceBytes = replaceFunction "rb_assert_one"
        [0xe92d4010, 0xe24dd010, slotWord 0xe3004000 writableBase, slotWord 0xe3404000 (writableBase `shiftR` 16), 0xe58d4000,
         0xe28d1008, 0xe28d200c, 0xe0410002, branchLink (callerStart + 32) stockTarget,
         0xe59d2000, 0xe3a03001, 0xe5823000, 0xe28dd010, 0xe8bd8010] clean
  differenceReport <- run allowlist differenceBytes
  check "a frame-minus-frame difference does not expose the frame"
    (not (any ((== callerStart + 44) . findingAddress) (violations differenceReport)))
  -- The addressing-mode operand rule: a frame index added to an entry pointer
  -- is a frame address, so the store is not the pointer's declared root.
  let pointerPolicy = allowlist {declarations = declarations allowlist ++
        [Declaration "rb_assert_one" StoreAccess (Argument (Reg 0)) "synthetic store through entry r0"]}
      pointerBytes = replaceFunction "rb_assert_one" [0xe08d2001, 0xe7803002, 0xe12fff1e] clean
  pointerReport <- run pointerPolicy pointerBytes
  check "a frame index added to an entry pointer keeps the frame"
    (any (\f -> findingAddress f == callerStart + 4 && findingReason f == Unproven StoreAccess (Index StackFrame)) (violations pointerReport))
  -- Each operation that keeps or loses a root after a frame address.
  frameCase "a bitwise operation on a frame address" (slotPrefix ++ [0xe00d0001])
  frameCase "a shift of a frame address" (slotPrefix ++ [0xe1a0008d])
  frameCase "MOVT of a frame address" (slotPrefix ++ [0xe28d1004, 0xe3401000])
  frameCase "a zero-extend of a frame address" (slotPrefix ++ [0xe28d1004, 0xe6ef0071])
  -- A frame pointer widened at a loop head keeps the frame; the store through
  -- it is an undeclared frame-index store, not an unbounded one.
  let loopPrefix = slotPrefix ++ [0xe28d4004]
      loopStore = callerStart + fromIntegral (length loopPrefix * 4) + 4
      loopBytes = replaceFunction "rb_assert_one"
        (loopPrefix ++ [0xe2444004, 0xe5843000, 0xe1540000, 0x1afffffb, 0xe12fff1e]) clean
  loopReport <- run allowlist loopBytes
  check "a frame pointer widened at a loop head keeps the frame"
    (any (\f -> findingAddress f == loopStore && findingReason f == Unproven StoreAccess (Index StackFrame)) (violations loopReport))
  -- A return address in a slot the prologue's push did not write is not a
  -- saved slot: the call forgets it and the return through it is unbounded.
  let savedSlotBytes = replaceFunction "rb_assert_one"
        [0xe92d4010, 0xe24dd008, 0xe58de000, 0xe1a0000d, branchLink (callerStart + 16) stockTarget, 0xe49df008] clean
  savedSlotReport <- run allowlist savedSlotBytes
  check "a return address in a non-prologue slot does not survive a call"
    (any (\f -> findingAddress f == callerStart + 20 && findingReason f == UnboundedBranch) (violations savedSlotReport))
  -- `clz pc, r1` is an UNPREDICTABLE program-counter destination and is refused.
  let pcPrefix = slotPrefix ++ [0xe28d1004, 0xe16fff11]
      pcSite = callerStart + fromIntegral ((length slotPrefix + 1) * 4)
  pcReport <- run allowlist (withCall pcPrefix)
  check "a program-counter destination of a compute form is refused"
    (any (\f -> findingAddress f == pcSite && findingReason f == ForbiddenInstruction Unadmitted) (violations pcReport))
  -- Every decoded form that writes the program counter is refused.
  let pcDestinations =
        [ ("MUL", 0xe00f0291), ("MLA", 0xe02f3291)
        , ("UMULL RdHi", 0xe08f0291), ("UMULL RdLo", 0xe082f291)
        , ("UMLAL RdHi", 0xe0af0291), ("UMLAL RdLo", 0xe0a4f291)
        , ("SMULL RdHi", 0xe0cf0291), ("SMULL RdLo", 0xe0c2f291)
        , ("SMLAL RdHi", 0xe0ef0291), ("SMLAL RdLo", 0xe0e4f291)
        , ("UMAAL RdHi", 0xe04f0291), ("UMAAL RdLo", 0xe042f291)
        , ("MLS", 0xe06f3291), ("CLZ", 0xe16fff11)
        , ("UXTB", 0xe6eff071), ("UXTH", 0xe6fff071)
        , ("SXTB", 0xe6aff071), ("SXTH", 0xe6bff071)
        , ("UXTAB", 0xe6e1f072), ("UXTAH", 0xe6f1f072)
        , ("SXTAB", 0xe6a1f072), ("SXTAH", 0xe6b1f072)
        , ("REV", 0xe6bfff31), ("REV16", 0xe6bfffb1), ("RBIT", 0xe6ffff31)
        , ("SBFX", 0xe7a3f051), ("UBFX", 0xe7e3f051)
        , ("BFI", 0xe7c3f011), ("BFC", 0xe7c3f01f)
        , ("LDRD", 0xe1c1e0d0), ("STRD", 0xe1c1e0f0)
        ]
      admitted = [label | (label, word) <- pcDestinations, A32.decode frameSetup word /= A32.Rejected Unadmitted]
  check ("the decoder refuses every program-counter destination; admitted: " ++ show admitted) (null admitted)
  let unpredictablePc =
        [ ("LDM writeback", 0xe93f0fff)
        , ("pre-indexed LDR", 0xe5bf0004)
        , ("pre-indexed LDRH", 0xe1ff00b2)
        , ("pre-indexed LDRD", 0xe1ef00d8)
        , ("register-shifted MOV Rd", 0xe1a0f110)
        , ("register-shifted ADD Rn", 0xe08f0211)
        , ("register-shifted MOV Rm", 0xe1a0011f)
        , ("register-shifted MOV Rs", 0xe1a00f11)
        ]
      admittedUnpredictable = [label | (label, word) <- unpredictablePc, A32.decode frameSetup word /= A32.Rejected Unadmitted]
  check ("the decoder refuses every unpredictable program-counter form; admitted: " ++ show admittedUnpredictable) (null admittedUnpredictable)
  -- An empty block-transfer register list is UNPREDICTABLE.
  let emptyBlockList = [("LDM", 0xe8900000), ("STM", 0xe92d0000)]
      admittedEmptyBlock = [label | (label, word) <- emptyBlockList, A32.decode frameSetup word /= A32.Rejected Unadmitted]
  check ("the decoder refuses an empty block-transfer register list; admitted: " ++ show admittedEmptyBlock) (null admittedEmptyBlock)
  forM_ [elfLow, elfHigh] $ \path -> do
    bytes <- B.readFile path
    image <- either fail pure (parseElf bytes)
    let findSymbol name = maybe (error ("no symbol " ++ name)) id (find ((== name) . symbolName) (elfSymbols image))
        caller = findSymbol "rb_assert_one"
        callee = findSymbol "rb_status_record"
        base = symbolValue (findSymbol "__ram_push_start")
        pointer = base + 0x100
        wide opcode immediate = opcode .|. ((immediate .&. 0xf000) `shiftL` 4) .|. (immediate .&. 0xfff)
        callSite = symbolValue caller + 12
        emit :: Symbol -> [Word32] -> B.ByteString -> B.ByteString
        emit target instructions subject =
          let padded = instructions ++ replicate (fromIntegral (symbolSize target `div` 4) - length instructions) 0xe320f000
           in patchBytes image subject (symbolValue target)
                (B.pack [fromIntegral (word `shiftR` shift) | word <- padded, shift <- [0,8,16,24 :: Int]])
        program = emit callee [0xe5901000, 0xe5812000, 0xe12fff1e]
          (emit caller [0xe92d4010, wide 0xe3000000 pointer, wide 0xe3400000 (pointer `shiftR` 16),
            branchLink callSite (symbolValue callee), 0xe8bd8010] bytes)
        relativePolicy offset = indirectPolicy {declarations = declarations indirectPolicy ++
          [Declaration "rb_assert_one" StoreAccess (Cell (AllocationCell (AllocationOffset offset))) "synthetic allocation-relative pointer field"]}
    relocated <- run (relativePolicy 0x100) program
    check (path ++ " preserves an exact allocation-relative cell after relocation")
      (any (\f -> findingAddress f == callSite && findingReason f == Unproven StoreAccess (Cell (AllocationCell (AllocationOffset 0x100)))) (declared relocated)
        && not (any ((== callSite) . findingAddress) (violations relocated)))
    wrongOffset <- run (relativePolicy 0x104) program
    check (path ++ " refuses a different allocation-relative cell")
      (any ((== callSite) . findingAddress) (violations wrongOffset))


  -- A call to a stock address the allowlist does not name.
  expect "undeclared stock call" (fixture probeCall (branchLink probeCall 0x00700000))
    [(probeCall, UndeclaredStockTarget 0x00700000)]
  -- An indirect call through an argument register no declaration covers.
  expect "undeclared indirect call" (fixture probeCall 0xe12fff33)
    [(probeCall, Unproven BranchAccess (Argument (Reg 3)))]
  -- Stores through the literal, retargeted outside the bank's writable data.
  expect "store to undeclared stock RAM" (fixture statusLiteral 0x01235678)
    [(statusStore, StoreOutside 0x01235678 0x0123567b StockMemory)]
  expect "store to device registers" (fixture statusLiteral 0xfff40000)
    [(statusStore, StoreOutside 0xfff40000 0xfff40003 DeviceSpace)]
  expect "store into bank code" (fixture statusLiteral (symbol "rb_status_record"))
    [(statusStore, StoreOutside (symbol "rb_status_record") (symbol "rb_status_record" + 3) BankCode)]
  expect "store into the installer mailbox" (fixture statusLiteral (symbol "ram_push_mailbox"))
    [(statusStore, StoreOutside (symbol "ram_push_mailbox") (symbol "ram_push_mailbox" + 3) InstallerArea)]
  expect "store into Classic Chrome code" (fixture statusLiteral (symbol "cc_ram_convert"))
    [(statusStore, StoreOutside (symbol "cc_ram_convert") (symbol "cc_ram_convert" + 3) BankCode)]
  let bundleStart = symbol "RAM_PUSH_BANK_END"
  forM_
    [ ("relocated D3 hook", bundleStart + 0x1d8)
    , ("relocated bundle start", bundleStart)
    , ("relocated bundle tail", symbol "__ram_push_installer_start" - 4)
    ] $ \(label, address) ->
      expect ("store into " ++ label) (fixture statusLiteral address)
        [(statusStore, StoreOutside address (address + 3) ExecutableBundle)]
  expect "store into installer code" (fixture statusLiteral (symbol "ram_push_install"))
    [(statusStore, StoreOutside (symbol "ram_push_install") (symbol "ram_push_install" + 3) InstallerArea)]
  expect "store into generated bank name data" (fixture statusLiteral (bundleStart - 0x400)) []
  expect "store into the last generated bank word" (fixture statusLiteral (bundleStart - 4)) []
  -- Forbidden instruction classes in place of `mov r11, sp`.
  expect "cp15 cache maintenance" (fixture frameSetup 0xee070f15)
    [(frameSetup, ForbiddenInstruction (Coprocessor 15))]
  expect "supervisor call" (fixture frameSetup 0xef000000)
    [(frameSetup, ForbiddenInstruction SupervisorCall)]
  expect "interrupt mask change" (fixture frameSetup 0xf10c0080)
    [(frameSetup, ForbiddenInstruction InterruptMask)]
  expect "MOVW with PC as destination" (fixture frameSetup 0xe300f000)
    [(frameSetup, ForbiddenInstruction Unadmitted)]
  expect "MOVT with PC as destination" (fixture frameSetup 0xe340f000)
    [(frameSetup, ForbiddenInstruction Unadmitted)]
  -- A writeback transfer based on the program counter and a register-shifted
  -- data-processing form with the program counter in a register field are
  -- UNPREDICTABLE.
  expect "LDM writeback based on the program counter" (fixture frameSetup 0xe93f0fff)
    [(frameSetup, ForbiddenInstruction Unadmitted)]
  expect "pre-indexed load based on the program counter" (fixture frameSetup 0xe5bf0004)
    [(frameSetup, ForbiddenInstruction Unadmitted)]
  expect "register-shifted data processing with the program counter" (fixture frameSetup 0xe1a0f110)
    [(frameSetup, ForbiddenInstruction Unadmitted)]
  -- BLX with the program counter is UNPREDICTABLE.
  expect "BLX with the program counter" (fixture frameSetup 0xe12fff3f)
    [(frameSetup, ForbiddenInstruction Unadmitted)]
  -- Signed halfword multiplies in place of `mov r11, sp`.
  expect "SMLABB" (fixture frameSetup 0xe1030281) []
  expect "SMLALBB" (fixture frameSetup 0xe1432281)
    [(frameSetup, ForbiddenInstruction Unadmitted)]
  expect "SMLABB with a PC operand" (fixture frameSetup 0xe103028f)
    [(frameSetup, ForbiddenInstruction Unadmitted)]
  expect "SMULWB" (fixture frameSetup 0xe12302a1) []
  expect "SMULBB with nonzero bits 15:12" (fixture frameSetup 0xe1631281)
    [(frameSetup, ForbiddenInstruction Unadmitted)]
  expect "SMLABB with PC as accumulator" (fixture frameSetup 0xe103f281)
    [(frameSetup, ForbiddenInstruction Unadmitted)]
  -- Flag-setting multiplies write NZCV the compare-pruned paths rely on.
  expect "MULS" (fixture frameSetup 0xe0110392)
    [(frameSetup, ForbiddenInstruction Unadmitted)]
  expect "UMULLS" (fixture frameSetup 0xe0932190)
    [(frameSetup, ForbiddenInstruction Unadmitted)]
  expect "read CPSR" (fixture frameSetup 0xe10fc000) []
  expect "write application flags" (fixture frameSetup 0xe128f00c) []
  forM_ [0xe12cf00c,0xe121f00c,0xe168f00c,0xe328f001,0xe128f00f,0xe10ff000,0xe14fc000,0xe124f00c] $ \word ->
    check ("privileged or unsupported status encoding refuses: " ++ show word)
      (A32.decode frameSetup word == A32.Rejected SystemRegister)
  let statusFunction = fst (function "rb_assert_one")
      statusBranch = statusFunction + 12
      forbiddenTarget = 0x00700000
      conditionalBranch = 0x1a000000 .|. (((forbiddenTarget - statusBranch - 8) `shiftR` 2) .&. 0x00ffffff)
      functionProgram words_ = patchBytes cleanElf clean statusFunction
        (B.pack [fromIntegral (word `shiftR` shift) | word <- words_, shift <- [0,8,16,24 :: Int]])
      statusFinding report = any (\finding -> findingAddress finding == statusBranch
        && findingReason finding == UndeclaredStockTarget forbiddenTarget) (violations report)
  invalidated <- run allowlist (functionProgram [0xe3a00000,0xe3500000,0xe128f001,conditionalBranch,0xe12fff1e])
  check "restoring application flags invalidates the preceding comparison" (statusFinding invalidated)
  overwritten <- run allowlist (functionProgram [0xe3a00000,0xe10f0000,0xe3500000,conditionalBranch,0xe12fff1e])
  check "reading status replaces the destination value" (statusFinding overwritten)
  preserved <- run allowlist (functionProgram [0xe3a00000,0xe3500000,0xe10f1000,conditionalBranch,0xe12fff1e])
  check "reading status preserves comparison flags" (not (statusFinding preserved))
  skipped <- run allowlist (functionProgram [0xe3a00000,0xe3500000,0x1128f001,conditionalBranch,0xe12fff1e])
  check "a skipped status write preserves comparison flags" (not (statusFinding skipped))
  let stateRoot = 0x01467588
      actionRoot = 0x00cfe2a8
      indexedPolicy = allowlist
        {declarations = declarations allowlist ++
          [Declaration "rb_assert_one" StoreAccess (Index (StockAddress stateRoot)) "synthetic indexed state store"
          ,Declaration "rb_assert_one" BranchAccess (Deref (Deref (Index (StockAddress actionRoot)))) "synthetic captured action slot"]}
      wide opcode half = opcode .|. ((half .&. 0xf000) `shiftL` 4) .|. (half .&. 0xfff)
      completeProgram rootInstruction middle root tailInstruction =
        functionProgram ([wide 0xe3002000 root,wide 0xe3402000 (root `shiftR` 16),rootInstruction] ++ middle
          ++ [tailInstruction]
          ++ replicate (fromIntegral ((snd (function "rb_assert_one") - statusFunction) `div` 4) - 7) 0xe320f000)
      storeProgram rootInstruction root = completeProgram rootInstruction [0xe1a01080,0xe0811002,0xe1c130b0] root 0xe12fff1e
      callProgram rootInstruction root = completeProgram rootInstruction [0xe7922100,0xe1a01723,0xe7910002] root 0xe12fff10
      rootRefusal finding = case findingReason finding of
        UnboundedStore -> True
        UnboundedBranch -> True
        Unproven _ _ -> True
        UnprovenArgument -> True
        _ -> False
      verifyRoot label policy bytes expected = do
        report <- run policy bytes
        let refused = violations report
        check (label ++ ": " ++ show (map renderFinding refused))
          (if expected then null refused else not (null refused) && all rootRefusal refused)
  verifyRoot "commuted state root retains its exact declared index" indexedPolicy (storeProgram 0xe320f000 stateRoot) True
  verifyRoot "commuted state root still requires a declaration" allowlist (storeProgram 0xe320f000 stateRoot) False
  verifyRoot "commuted state root rejects a different root" indexedPolicy (storeProgram 0xe320f000 (stateRoot + 4)) False
  verifyRoot "two unknown store operands do not invent a root" indexedPolicy (storeProgram 0xe1a02083 stateRoot) False
  verifyRoot "large scalar index retains the captured table root" indexedPolicy (callProgram 0xe320f000 actionRoot) True
  verifyRoot "commuted action root still requires a declaration" allowlist (callProgram 0xe320f000 actionRoot) False
  verifyRoot "commuted action root rejects a different root" indexedPolicy (callProgram 0xe320f000 (actionRoot + 4)) False
  verifyRoot "two unknown call operands do not invent a root" indexedPolicy (callProgram 0xe1a02083 actionRoot) False
  -- rb_assert_one's `cmp rN, #0` replaced by `mov r0, r0`: the 0 trace
  -- reaches its store.
  expect "untested 0 argument" (fixture zeroTest 0xe1a00000)
    [(zeroCall, UnprovenArgument)]
  -- rb_assert_one calls rb_status_record, then the undeclared stock address,
  -- with rb_status_record's first two words replaced by a hint and `second`.
  let callee = fst (function "rb_status_record")
      afterCallee = statusFunction + 4
      callerWords = [branchLink statusFunction callee, branchLink afterCallee forbiddenTarget, 0xe12fff1e]
      trapProgram second = patch cleanElf (patch cleanElf (functionProgram callerWords) callee 0xe320f000) (callee + 4) second
      fallsThrough report = any (\finding -> findingAddress finding == afterCallee
        && findingReason finding == UndeclaredStockTarget forbiddenTarget) (violations report)
  trapped <- run allowlist (trapProgram 0xeafffffe)
  check "a call to hints then `b .` has no fall-through" (not (fallsThrough trapped))
  conditionalSpin <- run allowlist (trapProgram 0x1afffffe)
  check "a call to hints then a conditional `bne .` falls through" (fallsThrough conditionalSpin)
  forwardBranch <- run allowlist (trapProgram 0xea000000)
  check "a call to hints then a branch to another word falls through" (fallsThrough forwardBranch)

  -- The menu engine variant: its writable statics are a section of their own,
  -- inside the engine region and outside its code.
  engineBytes <- B.readFile engineElf
  engineImage <- either fail pure (parseElf engineBytes)
  engineLayout <- either fail pure (readLayout engineImage)
  let EngineExtents codeStart codeEnd dataStart' dataEnd = maybe (error "the engine ELF has no engine region") id (engine engineLayout)
      engineCodeExtent = (codeStart, codeEnd)
      engineDataExtent = (dataStart', dataEnd)
      regionEnd = codeStart + menuEngineRegionExtentFor modalEngineBankVariant
      replaceSection name change elf =
        elf {elfSections = [if sectionName s == name then change s else s | s <- elfSections elf]}
      moveSymbol name value elf =
        elf {elfSymbols = [if symbolName s == name then s {symbolValue = value} else s | s <- elfSymbols elf]}
      layoutRules elf = case readLayout elf of
        Left _ -> []
        Right layout -> [findingReason f | f <- layoutFindings elf layout, LayoutProblem _ <- [findingReason f]]
  check "the engine data follows the engine code inside the engine region"
    ( fst engineDataExtent >= snd engineCodeExtent
        && snd engineDataExtent <= regionEnd
        && regionOf engineLayout (fst engineDataExtent) == EngineData
        && writable (regionOf engineLayout (fst engineDataExtent))
        && not (writable (regionOf engineLayout (fst engineCodeExtent)))
    )
  check "the linked engine layout has no layout findings"
    (null (layoutFindings engineImage engineLayout))
  check "an executable engine data section is refused"
    (LayoutProblem (SectionExtent ".menu_engine_data")
      `elem` layoutRules (replaceSection ".menu_engine_data" (\s -> s {sectionFlags = sectionFlags s .|. 0x4}) engineImage))
  check "engine data overlapping the engine code is refused"
    (LayoutProblem EngineRegion
      `elem` layoutRules (moveSymbol "__rb_menu_engine_data_start" (fst engineCodeExtent) engineImage))
  check "engine data past the engine extent is refused"
    (LayoutProblem EngineRegion `elem` layoutRules (moveSymbol "__rb_menu_engine_data_end" (regionEnd + 4) engineImage))

  composed <- forM
    [ (allowlistPath, "menu/recipe_bank_allowlist.json")
    , (modalFragmentPath, "menu/recipe_bank_modal_allowlist_fragment.json")
    , (engineFragmentPath, "menu/recipe_bank_menu_engine_allowlist_fragment.json")
    ] $ \(path, name) -> do
      contents <- B.readFile path
      pure (name, contents)
  engineAllowlist <- either fail pure
    (composeBankAllowlist modalEngineBankVariant composed >>= parseAllowlist)
  let engineFunction name = case [s | s <- elfSymbols engineImage, symbolName s == name, symbolSize s > 0] of
        (s : _) -> (symbolValue s, symbolValue s + symbolSize s)
        _ -> error ("no sized symbol " ++ name)
      engineFirst predicate = case [a | (a, w) <- [(a, w) | a <- [fst (engineFunction "rb_menu_engine_apply"), fst (engineFunction "rb_menu_engine_apply") + 4 .. snd (engineFunction "rb_menu_engine_apply") - 4], Just w <- [wordAt engineImage a]], predicate w] of
        (a : _) -> a
        _ -> error "no matching engine word"
      engineReport bytes = either fail pure (parseElf bytes >>= checkBank engineAllowlist)
      busyLiteral = engineFirst (\w -> regionOf engineLayout w == EngineData)
      busyStore = engineFirst (\w -> w .&. 0x0ff00000 == 0x05c00000)
  intoData <- engineReport (patch engineImage engineBytes busyLiteral (fst engineDataExtent))
  check "a store into the engine data region passes"
    (not (any ((== busyStore) . findingAddress) (violations intoData)))
  intoCode <- engineReport (patch engineImage engineBytes busyLiteral (fst engineCodeExtent))
  check "a store into the engine code is still refused"
    (any (\f -> findingAddress f == busyStore
      && findingReason f == StoreOutside (fst engineCodeExtent) (fst engineCodeExtent) BankCode) (violations intoCode))
  intoEnd <- engineReport (patch engineImage engineBytes busyLiteral (snd engineDataExtent))
  check "a store at the engine data end is refused"
    (any (\f -> findingAddress f == busyStore
      && findingReason f == StoreOutside (snd engineDataExtent) (snd engineDataExtent) AllocationTail) (violations intoEnd))
  intoLast <- engineReport (patch engineImage engineBytes busyLiteral (snd engineDataExtent - 1))
  check "a store in the last engine data byte passes"
    (not (any ((== busyStore) . findingAddress) (violations intoLast)))
  when (snd engineCodeExtent < fst engineDataExtent) $ do
    intoGap <- engineReport (patch engineImage engineBytes busyLiteral (snd engineCodeExtent))
    check "a store in the gap after the engine code is refused"
      (any (\f -> findingAddress f == busyStore
        && findingReason f == StoreOutside (snd engineCodeExtent) (snd engineCodeExtent) AllocationTail) (violations intoGap))

  -- The command-line checker's exit status: 0 clean, 1 with a violation.
  temporary <- getTemporaryDirectory
  (cleanCode, _, _) <- readProcessWithExitCode checker [allowlistPath, elfLow] ""
  check "checker exits 0 on the clean bank" (cleanCode == ExitSuccess)
  (path, handle) <- openBinaryTempFile temporary "bank-fixture.elf"
  B.hPut handle (fixture probeCall (branchLink probeCall 0x00700000))
  hClose handle
  (fixtureCode, output, _) <- readProcessWithExitCode checker [allowlistPath, path] ""
  removeFile path
  check ("checker exits 1 on an undeclared stock call; output: " ++ output) (fixtureCode == ExitFailure 1)
  putStrLn "bank allowlist: confinement and instruction checks passed"

run :: Allowlist -> B.ByteString -> IO Report
run allowlist bytes = either fail pure (parseElf bytes >>= checkBank allowlist)

-- | `bl target` placed at `site`.
branchLink :: Word32 -> Word32 -> Word32
branchLink site target = 0xeb000000 .|. (((target - site - 8) `shiftR` 2) .&. 0x00ffffff)

-- | The ELF with the word at a loaded address replaced.
patch :: Elf -> B.ByteString -> Word32 -> Word32 -> B.ByteString
patch elf bytes address word = patchBytes elf bytes address
  (B.pack [fromIntegral (word `shiftR` (8 * i)) | i <- [0 .. 3 :: Int]])

patchBytes :: Elf -> B.ByteString -> Word32 -> B.ByteString -> B.ByteString
patchBytes elf bytes address fresh =
  case find holds (elfSections elf) of
    Just section ->
      let offset = fromIntegral (sectionOffset section + address - sectionAddress section)
       in B.take offset bytes <> fresh <> B.drop (offset + B.length fresh) bytes
    Nothing -> error ("no section holds " ++ show address)
 where
  holds section =
    sectionAllocated section && sectionAddress section <= address
      && toInteger address + toInteger (B.length fresh) <= toInteger (sectionAddress section) + toInteger (sectionSize section)
