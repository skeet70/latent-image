{-# LANGUAGE OverloadedStrings, ScopedTypeVariables #-}
-- Independent H0 probes: JSON buffers and synthetic subprocesses only.
module Main where
import qualified FujiAF.Host as H
import qualified AF.Simulation as AF
import FujiAF.JSON
import Control.Concurrent (threadDelay)
import Control.Exception
import Control.Monad
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Either (isLeft)
import GHC.Clock (getMonotonicTimeNSec)
import System.Directory
import System.Exit
import System.Posix.Signals
import System.Posix.Types (CPid)
import System.Timeout

check :: Bool -> String -> IO ()
check = H.ensure
jsonProbes :: IO ()
jsonProbes = do
 forM_ ["{\"a\":1,\"\\u0061\":2}","{\"x\":[{\"a\":0,\"a\":0}]}","+1","01","1.",".1","1e","NaN","Infinity","true false",B.pack [34,255,34],"\"\\uD800\""] $ \raw ->
  check (isLeft (decodeStrict raw)) "invalid/duplicate/UTF8/surrogate JSON accepted"
 forM_ ["1.0","1e0","1E+0","-0.0"] $ \raw -> check (isLeft (decodeStrictIntegers raw)) "noninteger token spelling accepted"
 forM_ ["0","-0","18446744073709551615"] $ \raw -> check (not (isLeft (decodeStrictIntegers raw))) "exact integer token rejected"
 forM_ ["1e3","1e+3","1e-3","-1.0","0.125"] $ \raw -> check (not (isLeft (decodeStrict raw))) "valid bounded general JSON number rejected"
 let canonical=H.obj [("z",H.s "\x1f642\x7f\n"),("a",H.n 18446744073709551615)]
 check (encodeCanonical canonical=="{\"a\":18446744073709551615,\"z\":\"\\ud83d\\ude42\\u007f\\n\"}") "canonical ASCII/exact large integer changed"
 check (isLeft (decodeStrict (B.replicate 66 91<>"0"<>B.replicate 66 93))) "nesting cap bypass"
 -- A short exponent token must be rejected or encoded within a finite cap.
 -- Earlier migrated code expanded this 9-byte input to 1,000,001 bytes.
 forM_ ["1e1000000","1e-1000000"] $ \raw -> do
  result <- timeout 1000000 (try (case decodeStrict raw of Left _->pure 0; Right v->evaluate (B.length (encodeCanonical v))) :: IO (Either SomeException Int))
  check (case result of Just (Left _)->True; Just (Right size)->size<=8192; Nothing->False) "number encoding lacks finite expansion guard"
 putStrLn "F03 independent JSON lexical/duplicate/UTF8/surrogate/integer/ASCII/depth/expansion probes passed"

killGroup :: CPid -> IO ()
killGroup pid=signalProcessGroup sigKILL pid `catch` \(_::IOException)->pure ()
notRunning :: CPid -> IO Bool
notRunning pid=do
 result <- try (B.readFile ("/proc/"++show pid++"/stat"))
 pure $ case result of
  Left (_::IOException)->True
  Right raw->case C.words (C.drop 1 (C.dropWhile (/=')') raw)) of state:_->state=="Z" || state=="X"; _->False
processProbes :: IO ()
processProbes=do
 _ <- H.runBounded 1000000 64 "/bin/true" [] B.empty
 before <- length <$> listDirectory "/proc/self/fd"
 forM_ [1::Int ..3] $ \_ -> do
  (stopped,status,out,err)<-H.runBounded 300000 1024 "/bin/sh" ["-c","echo $$; sleep 10 <&0 >&- 2>&- & echo $!; exit 0"] (B.replicate (1024*1024) 65)
  case mapM (\word->case reads word of [(p,"")]->Just p; _->Nothing) (words (C.unpack out)) of
   Just [group,child] -> flip finally (killGroup group) $ do
    check (not stopped && status==ExitSuccess && B.null err) "success subprocess outcome changed"
    threadDelay 30000
    gone <- notRunning child
    check gone "successful child left running descendant holding stdin"
   _ -> H.failH "synthetic PID framing failed"
 after <- length <$> listDirectory "/proc/self/fd"
 check (after<=before+1) "successful child leaked blocked writer/pipe descriptors"
 start <- getMonotonicTimeNSec
 (late,_,retained,_) <- H.runBounded 150000 1024 "/bin/sh" ["-c","printf retained; sleep 10 & wait"] B.empty
 end <- getMonotonicTimeNSec
 check (late && retained=="retained" && end-start<2000000000) "retained-pipe timeout/output/reap bound failed"
 (overflow,_,prefix,_) <- H.runBounded 500000 257 "/bin/sh" ["-c","while :; do printf 0123456789; done"] B.empty
 check (overflow && B.length prefix==257) "output cap or retained prefix failed"
 putStrLn "F03 independent successful-group/writer-FD/retained-pipe/deadline/output-cap probes passed"
main :: IO ()
main = jsonProbes >> processProbes >> bridgeProbes

bridgeProbes :: IO ()
bridgeProbes=H.withTemporary "fuji-af-f03-bridge-" $ \root -> do
 let helper=root++"/synthetic-bridge.sh"
 writeFile helper "#!/bin/sh\nsleep 10 <&0 >&- 2>&- &\nprintf '%s\\n' \"$!\" > \"$0.pid\"\nexit 0\n"
 permissions<-getPermissions helper
 setPermissions helper (setOwnerExecutable True permissions)
 bridge<-AF.openBridge helper
 flip finally (AF.abortBridge bridge) $ do
  -- The finite wait covers scheduling the synthetic helper, not hardware.
  ready <- timeout 1000000 (let await=do exists<-doesFileExist(helper++".pid"); if exists then pure () else threadDelay 1000>>await in await)
  check (ready==Just ()) "synthetic bridge PID record missing"
  raw<-readFile(helper++".pid")
  child<-case reads raw of [(pid,rest)] | all (`elem` (" \n"::String)) rest->pure pid; _->H.failH "synthetic bridge PID invalid"
  AF.closeBridge bridge
  threadDelay 30000
  gone<-notRunning child
  check gone "successful AF bridge left running descendant holding stdin"
 putStrLn "F03 independent successful AF bridge group cleanup probe passed"
