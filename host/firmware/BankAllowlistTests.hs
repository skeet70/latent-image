-- | bank-allowlist-tests ALLOWLIST CHECKER ELF_00C20000 ELF_00C21F88
--
-- The ELFs are //host/firmware:bank-elf-*, the relink script's output for the
-- current bank sources. Each synthetic fixture is the 0x00c40000 ELF with one
-- word replaced at a site located by symbol, so it differs from a clean build
-- in exactly the violation it names.
module Main (main) where

import A32 (Forbidden (..), Reg (..))
import qualified A32
import ArmElf
import BankAllowlist
import Control.Monad (forM_, unless)
import Data.Bits (shiftR, (.&.), (.|.))
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
      completeProgram rootInstruction middle root tailInstruction =
        functionProgram ([0xe59f2010,rootInstruction] ++ middle ++ [0xea000000,root]
          ++ replicate (fromIntegral ((snd (function "rb_assert_one") - statusFunction) `div` 4) - 8) 0xe320f000
          ++ [tailInstruction])
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
