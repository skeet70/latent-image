-- SPDX-License-Identifier: AGPL-3.0-only
module Main (main) where

import Control.Exception (SomeException, displayException, try)
import Control.Monad (forM_, when)
import Data.Bits ((.&.))
import Data.List (groupBy)
import qualified Data.Map.Strict as M
import FujiAF.ConsoleStore
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)
import Text.Printf (printf)

report :: Bool -> Store -> IO ()
report quiet store = do
  let es = entries store
      messages = filter ((== Message) . enType) es
      keys = filter ((== ThreadId 0x25) . enThread) messages
      initcont = filter ((== ThreadId 0x3b) . enThread) messages
      counts = M.fromListWith (+) [((enType e,enThread e),1 :: Int) | e <- es]
      codes = M.fromListWith (+) [((enThread e,enCode e),1 :: Int) | e <- messages]
  printf "header: capacity %d next-index %d entries-decoded %d (type 0x0f %d, keytimer 0x25 %d, initcont 0x3b %d)\n"
    (capacity store) (nextIndex store) (length es) (length messages) (length keys) (length initcont)
  printf "type/thread census: %s\n" (unwords [printf "(%02x,%02x)x%d" (typeByte kind) thread count | ((kind,thread),count) <- M.toAscList counts])
  when (not quiet) $ do
    forM_ [("keytimer",keys),("initcont",initcont)] $ \(label,group) -> do
      printf "-- %s entries (%d)\n" (label :: String) (length group)
      forM_ group $ \e ->
        printf "  slot %4d code 0x%04x param 0x%04x arg 0x%08x tick %6s raw %s\n"
          (enSlot e) (enCode e) (enParam e) (enArg e) (maybe "unknown" (printf "%d") (enTick e)) (hexRun (enRaw e))
    let others = filter ((/= Message) . enType) es
    printf "-- non-message ticks (%d)\n" (length others)
    forM_ others $ \e -> printf "  slot %4d type %02x tick %6s\n"
      (enSlot e) (typeByte (enType e)) (maybe "unknown" (printf "%d") (enTick e))
  printf "-- initcont codes: %s\n" (unwords [printf "0x%04x@%s" (enCode e) (maybe "unknown" (printf "%d") (enTick e)) | e <- initcont])
  printf "-- all type-0x0f codes by thread: %s\n" (unwords [printf "%02x:%04x x%d" thread code count | ((thread,code),count) <- M.toAscList codes])
  when (not (null keys)) $ do
    printf "-- keytimer same-code runs: %s\n" (unwords [printf "0x%04x:%d" (enCode first) (length run) | run@(first:_) <- groupBy (\a b -> enCode a == enCode b) keys])
    let groups = M.fromListWith (++) [(enCode e .&. 0xff7f,[e]) | e <- keys]
    summaries <- mapM (\(code,group) -> do
      let presses = length [() | e <- group, enCode e .&. 0x80 == 0]
          releases = length group-presses
          ticks = [tick | e <- group, Just tick <- [enTick e]]
      pure (printf "0x%04x press %d release %d unreleased %d ticks %d..%d" code presses releases (presses-releases) (minimum ticks) (maximum ticks) :: String)) (M.toAscList groups)
    printf "-- keytimer per-code summary (release = code | 0x80 in the low byte): %s\n" (unwords summaries)
    let buckets = M.fromListWith (+) [(tick `div` 1000,1 :: Int) | e <- keys, Just (Tick tick) <- [enTick e]]
    printf "-- keytimer entries per 1000 ticks: %s\n" (unwords [show (M.findWithDefault 0 bucket buckets) | bucket <- [0..fst (M.findMax buckets)]])

main :: IO ()
main = do
  result <- try $ do
    args <- getArgs
    let (quiet,rest) = case args of "--quiet":xs -> (True,xs); _ -> (False,args)
    case rest of
      ["--help"] -> putStrLn "Usage: ring-check [--quiet] (store FILE | high-known FILE | windows DIRECTORY)\nX-M1 1.50 console records in captured slot order; no camera I/O."
      _ -> do
        (source,extra) <- either fail pure (parseInput rest)
        when (not (null extra)) (fail "Unexpected ring-check arguments")
        loadStore source >>= report quiet
  case result :: Either SomeException () of
    Left failure -> hPutStrLn stderr (displayException failure) >> exitFailure
    Right () -> pure ()
