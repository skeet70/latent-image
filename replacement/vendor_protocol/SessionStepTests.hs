{-# LANGUAGE GHC2021 #-}
module Main where

import Control.Monad (unless)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import SessionStep (runBoundedGroup)
import System.Exit (ExitCode (..), die)

-- A child that reports more than a pipe holds must be drained while it runs, and a child that
-- never exits must be stopped at the deadline rather than held by the caller.
main :: IO ()
main = do
  (timed, code, out, _) <- runBoundedGroup 5000000 (1024 * 1024) "/bin/sh"
    ["-c", "head -c 300000 /dev/zero"] B.empty
  unless (not timed && code == ExitSuccess && B.length out == 300000) $
    die ("a large report is not drained: " ++ show (timed, code, B.length out))

  (capped, capCode, capOut, _) <- runBoundedGroup 5000000 (64 * 1024) "/bin/sh"
    ["-c", "head -c 300000 /dev/zero"] B.empty
  unless (capped && B.length capOut == 64 * 1024) $
    die ("the output cap does not end the child: " ++ show (capped, capCode, B.length capOut))

  (bounded, _, _, _) <- runBoundedGroup 1000000 (1024 * 1024) "/bin/sh"
    ["-c", "trap '' TERM INT; sleep 30"] B.empty
  unless bounded (die "a child that never exits is not stopped at the deadline")

  (_, status, _, _) <- runBoundedGroup 5000000 (1024 * 1024) "/bin/sh" ["-c", "exit 3"] B.empty
  unless (status == ExitFailure 3) (die ("the exit status is not propagated: " ++ show status))

  (_, _, echoed, _) <- runBoundedGroup 5000000 (1024 * 1024) "/bin/sh"
    ["-c", "cat > /dev/null; printf ok"] (BC.replicate (1024 * 1024) 'x')
  unless (echoed == BC.pack "ok") (die ("input is not delivered: " ++ show echoed))

  putStrLn "The bounded step drains a large report, stops a hung child at its deadline, propagates the exit status and delivers input."
