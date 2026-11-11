{-# LANGUAGE GHC2021 #-}
module Main (main) where

import Control.Monad (forM, unless)
import qualified Data.ByteString as B
import Data.Word
import Replacement.PowerLifecycle.Area2Oracle
import qualified Replacement.RecordStore.Model as R
import System.Environment (getArgs)
import System.Exit (ExitCode(..), die)
import System.FilePath (isAbsolute)
import System.IO (hClose)
import System.Process

word :: Word32 -> B.ByteString
word value = B.pack [fromIntegral (value `div` (256 ^ index)) | index <- [0..3 :: Int]]
encodeInput :: Input -> B.ByteString
encodeInput input = B.concat
  [ B.pack [80,76,50,1,major,minor], word failure, B.singleton (if backupFails input then 1 else 0), current, saved, live
  , R.bankBytes (R.geometry R.Kind2) R.Bank0 (initialMedium input)
  , R.bankBytes (R.geometry R.Kind2) R.Bank1 (initialMedium input)
  , B.concat (map encodeUpdate (updates input))
  ]
  where
    Version major minor = version input
    Record current = working input
    Record saved = snapshot input
    Backing live = backing input
    failure = case fault input of R.NoFault -> maxBound; R.FailRequest (R.PersistenceStep step) -> fromIntegral step; _ -> error "host executor fault not supported"
    encodeUpdate operation = case operation of
      LiveByte offset value -> B.singleton 0 <> word (fromIntegral offset) <> word (fromIntegral value)
      Sequence value -> B.singleton 1 <> word (fromIntegral value) <> word 0
      Save selector -> B.singleton 2 <> word (if selector == Snapshot then 0 else 1) <> word 0
encodeObservation :: Observation -> B.ByteString
encodeObservation observation = B.concat
  [ word (fromIntegral (length (returnedStatuses observation))), B.concat (map word (returnedStatuses observation))
  , B.pack [if observedArea2Open observation then 1 else 0, if observedArea3Open observation then 1 else 0]
  , current, saved, live
  , R.bankBytes (R.geometry R.Kind2) R.Bank0 (observedMedium observation)
  , R.bankBytes (R.geometry R.Kind2) R.Bank1 (observedMedium observation)
  , observedBackup observation
  ]
  where
    Record current = observedWorking observation
    Record saved = observedSnapshot observation
    Backing live = observedBacking observation

execute :: FilePath -> B.ByteString -> IO (ExitCode, B.ByteString)
execute executable input = do
  (Just incoming, Just outgoing, _, processHandle) <- createProcess (proc executable []) { std_in = CreatePipe, std_out = CreatePipe }
  B.hPut incoming input
  hClose incoming
  output <- B.hGetContents outgoing
  status <- waitForProcess processHandle
  pure (status, output)

main :: IO ()
main = do
  args <- getArgs
  executable <- case args of [path] | isAbsolute path -> pure path; _ -> die "usage: power-lifecycle-oracle /absolute/path/to/host-executor"
  unless (not (null scenarios)) (die "empty power lifecycle scenario selection")
  failures <- forM scenarios $ \(name,input) -> do
    (status,actual) <- execute executable (encodeInput input)
    let expected = encodeObservation (observe input)
        passed = status == ExitSuccess && actual == expected
    putStrLn (name ++ if passed then ": agrees" else ": behavioral difference")
    pure (not passed)
  refusals <- forM [B.empty, B.pack [0,0,0,0], B.pack [80,76,50,1]] $ \input -> do
    (status,_) <- execute executable input
    pure (status /= ExitSuccess)
  unless (not (or failures) && and refusals) (die "power lifecycle oracle or refusal control failed")
