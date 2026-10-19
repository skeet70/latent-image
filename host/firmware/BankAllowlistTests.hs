-- | bank-allowlist-tests ALLOWLIST CHECKER ELF_00C20000 ELF_00C21F88
--
-- The ELFs are //host/firmware:bank-elf-*, the relink script's output for the
-- current bank sources. Each synthetic fixture is the 0x00c20000 ELF with one
-- word replaced at a site located by symbol, so it differs from a clean build
-- in exactly the violation it names.
module Main (main) where

import A32 (Forbidden (..), Reg (..))
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
  -- Forbidden instruction classes in place of `mov r11, sp`.
  expect "cp15 cache maintenance" (fixture frameSetup 0xee070f15)
    [(frameSetup, ForbiddenInstruction (Coprocessor 15))]
  expect "supervisor call" (fixture frameSetup 0xef000000)
    [(frameSetup, ForbiddenInstruction SupervisorCall)]
  expect "interrupt mask change" (fixture frameSetup 0xf10c0080)
    [(frameSetup, ForbiddenInstruction InterruptMask)]

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
  putStrLn "bank allowlist: 13 checks passed"

run :: Allowlist -> B.ByteString -> IO Report
run allowlist bytes = either fail pure (parseElf bytes >>= checkBank allowlist)

-- | `bl target` placed at `site`.
branchLink :: Word32 -> Word32 -> Word32
branchLink site target = 0xeb000000 .|. (((target - site - 8) `shiftR` 2) .&. 0x00ffffff)

-- | The ELF with the word at a loaded address replaced.
patch :: Elf -> B.ByteString -> Word32 -> Word32 -> B.ByteString
patch elf bytes address word =
  case find holds (elfSections elf) of
    Just section ->
      let offset = fromIntegral (sectionOffset section + address - sectionAddress section)
       in B.take offset bytes <> B.pack [fromIntegral (word `shiftR` (8 * i)) | i <- [0 .. 3 :: Int]] <> B.drop (offset + 4) bytes
    Nothing -> error ("no section holds " ++ show address)
 where
  holds section =
    sectionAllocated section && sectionAddress section <= address
      && toInteger address + 4 <= toInteger (sectionAddress section) + toInteger (sectionSize section)
