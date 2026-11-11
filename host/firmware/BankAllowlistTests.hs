-- | bank-allowlist-tests ALLOWLIST CHECKER ELF_00C20000 ELF_00C21F88
--
-- The ELFs are //host/firmware:bank-elf-* outputs for the current bank sources.
-- Fixtures replace function words in the 0x00c40000 ELF.
module Main (main) where

import A32 (Forbidden (..), Reg (..))
import qualified A32
import ArmElf
import BankAllowlist
import Control.Monad (forM_, unless)
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (find)
import Data.Word (Word32)
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
  [allowlistPath, checker, elfLow, elfHigh] <- getArgs
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
        (any ((== LayoutProblem) . findingReason) (violations refused))
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
