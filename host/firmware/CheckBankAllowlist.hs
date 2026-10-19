-- | check-bank-allowlist ALLOWLIST.json KERNEL.elf [--report REPORT.json]
-- Prints every declared and violating site of the bank ELF; exits 1 when any
-- violation remains and 2 when an input cannot be read.
module Main (main) where

import BankAllowlist
import qualified Data.ByteString as B
import qualified FujiAF.Host as H
import FujiAF.JSON (encodeCanonical)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitWith)
import System.IO (hPutStrLn, stderr)

main :: IO ()
main = do
  arguments <- getArgs
  (allowlistPath, elfPath, reportPath) <- case arguments of
    [a, e] -> pure (a, e, Nothing)
    [a, e, "--report", r] -> pure (a, e, Just r)
    _ -> usage
  allowlistBytes <- H.readRegularWith (pure ()) False (1024 * 1024) allowlistPath
  elfBytes <- H.readRegularWith (pure ()) False (4 * 1024 * 1024) elfPath
  report <- either (\problem -> hPutStrLn stderr problem >> exitWith (ExitFailure 2)) pure (checkBankFile allowlistBytes elfBytes)
  mapM_ (putStrLn . ("declared  " ++) . renderFinding) (declared report)
  mapM_ (putStrLn . ("unused    " ++) . renderDeclaration) (reportUnused report)
  mapM_ (putStrLn . ("VIOLATION " ++) . renderFinding) (violations report)
  putStrLn
    ( show (length (violations report)) ++ " violations, " ++ show (length (declared report)) ++ " declared sites, "
        ++ show (reportProvenStores report) ++ " proven stores, " ++ show (reportProvenBranches report) ++ " proven branches, "
        ++ show (reportInstructions report) ++ " instructions in " ++ show (reportFunctions report) ++ " functions"
    )
  mapM_ (\path -> B.writeFile path (encodeCanonical (reportValue report) <> B.singleton 10)) reportPath
  exitWith (if null (violations report) then ExitSuccess else ExitFailure 1)
 where
  usage = hPutStrLn stderr "usage: check-bank-allowlist ALLOWLIST.json KERNEL.elf [--report REPORT.json]" >> exitWith (ExitFailure 2)
