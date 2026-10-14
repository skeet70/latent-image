-- SPDX-License-Identifier: AGPL-3.0-only
module Main (main) where

import Control.Exception (SomeException, displayException, try)
import Control.Monad (forM_)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import FujiAF.ConsoleStore
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)
import Text.Printf (printf)

report :: [ThreadId] -> Store -> IO ()
report selected store = do
  let es = entries store
      groups = M.fromListWith (++) [(enThread e,[e]) | e <- es]
      interesting = [e | e <- es, null selected || enThread e `elem` selected]
      summary (thread,group) =
        let ticks = [tick | e <- group, Just tick <- [enTick e]]
            bounds = if null ticks then "tunknown" else printf "t%d..%d" (minimum ticks) (maximum ticks)
        in printf "%02x:%d(x%d,%s)" thread (length group) (length group) bounds
  printf "next-index %d entries %d\n" (nextIndex store) (length es)
  printf "-- per-thread summary: %s\n" (unwords (map summary (M.toAscList groups)))
  forM_ interesting $ \e ->
    printf "  slot %4d type %02x thr %02x arg 0x%08x code 0x%04x param 0x%04x tick %6s raw %s%s\n"
      (enSlot e) (typeByte (enType e)) (enThread e) (enArg e) (enCode e) (enParam e)
      (maybe "unknown" (printf "%d") (enTick e)) (hexRun (B.take 10 (enRaw e)))
      (if enType e == Marker then " str " ++ ascii (B.take 8 (B.drop 8 (enRaw e))) else "")

main :: IO ()
main = do
  result <- try $ do
    args <- getArgs
    case args of
      ["--help"] -> putStrLn "Usage: trace-census (store FILE | high-known FILE | windows DIRECTORY) [THREAD_HEX ...]\nX-M1 1.50 console records in captured slot order; no camera I/O."
      _ -> do
        (source,rest) <- either fail pure (parseInput args)
        selected <- either fail pure (mapM parseThread rest)
        loadStore source >>= report selected
  case result :: Either SomeException () of
    Left failure -> hPutStrLn stderr (displayException failure) >> exitFailure
    Right () -> pure ()
