{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Concurrent (newEmptyMVar, putMVar, takeMVar)
import Control.Exception (finally)
import Control.Monad (forM_, unless)
import Data.Aeson (Value, object, (.=))
import Data.IORef (atomicModifyIORef', newIORef, readIORef)
import Emulation.Sha256 (Sha256, parseSha256Hex, sha256Hex)
import Replacement.ModalRuntime.QualifierPolicy
  ( MatrixPin (..), OwnedArchive (..), OwnedBase (..), Qualification (..), Role (..), executionIdentity, installedCheckpointMatches
  , ownedBaseInputs, parseInvocation, qualificationOptions, roleFirmware )
import Emulation.ParallelCases (mapBounded)

main :: IO ()
main = do
  let identity role firmware archive = executionIdentity $ object
        [ "role" .= (role :: String), "tools" .= object ["viewer" .= ("viewer" :: String)]
        , "trajectory" .= object ["sha256" .= ("route" :: String)]
        , "inputs" .= object ["firmware" .= (firmware :: String), "owned_archive" .= (archive :: String)] ]
      stock = identity "stock" "firmware" "old"
      owned = identity "owned" "firmware" "old"
  expect "owned input preserves stock observation" (stock == identity "stock" "firmware" "new")
  expect "owned input invalidates owned observation" (owned /= identity "owned" "firmware" "new")
  expect "stock input invalidates stock observation" (stock /= identity "stock" "new" "old")
  expect "stock input invalidates owned observation" (owned /= identity "owned" "new" "old")
  expect "owned archive change reuses all 67 stock role identities"
    (length [() | route <- [1..67 :: Int], executionIdentity (roleInput route "old") == executionIdentity (roleInput route "new")] == 67)
  let checkpoint = object ["snapshot_sha256" .= ("snapshot" :: String), "installation_sha256" .= ("installation" :: String)]
      restored snapshot installation = object
        [ "install_checkpoint_restored" .= True
        , "install_checkpoint" .= object ["snapshot_sha256" .= (snapshot :: String), "record_sha256" .= (installation :: String)] ]
      named = object ["installed_checkpoint" .= checkpoint]
  expect "named checkpoint requires matching restore" (installedCheckpointMatches named (restored "snapshot" "installation"))
  expect "named checkpoint refuses wrong snapshot" (not (installedCheckpointMatches named (restored "wrong" "installation")))
  expect "named checkpoint refuses wrong installation" (not (installedCheckpointMatches named (restored "snapshot" "wrong")))
  expect "named checkpoint refuses producer observation" (not (installedCheckpointMatches named (object ["install_checkpoint_saved" .= True])))
  ownedBaseOption
  mapM_ parallelAdmission [1, 2, 3]

ownedBaseOption :: IO ()
ownedBaseOption = do
  let stockDat = ("firmware.DAT", digest 'a')
      staged = StagedBase "time.DAT" (digest 'b')
      control = ControlArchive "control.a" (digest 'c')
  expect "no option keeps the default root, the shared base, the pinned archive and the engine225 matrix"
    (parseInvocation "default" ["execute"] == Right (Qualification "default" SharedBase PinnedArchive Engine225Matrix, ["execute"]))
  expect "a staged base keeps the engine225 matrix"
    (parseInvocation "default" ["--owned-base", "time.DAT", sha256Hex (digest 'b'), "execute"]
      == Right (Qualification "default" staged PinnedArchive Engine225Matrix, ["execute"]))
  expect "options before the command select the root, the staged base, the control archive and the matrix"
    (parseInvocation "default" ["--root", "fresh", "--owned-base", "time.DAT", sha256Hex (digest 'b')
      , "--control-archive", "control.a", sha256Hex (digest 'c'), "--matrix", "seek-moved", "execute"]
      == Right (Qualification "fresh" staged control SeekMovedMatrix, ["execute"]))
  forM_ [ Qualification "default" SharedBase PinnedArchive Engine225Matrix, Qualification "fresh" staged PinnedArchive Engine225Matrix
        , Qualification "fresh" SharedBase control SeekMovedMatrix, Qualification "fresh" SharedBase PinnedArchive SeekMovedMatrix ] $ \qualification ->
    expect "child processes receive the same qualification"
      (parseInvocation "other" (qualificationOptions qualification ++ ["role-slotted", "oracle-open", "owned"])
        == Right (qualification, ["role-slotted", "oracle-open", "owned"]))
  expect "a malformed base digest is refused" (isLeft (parseInvocation "default" ["--owned-base", "time.DAT", "abc", "execute"]))
  expect "a base without a digest is refused" (isLeft (parseInvocation "default" ["--owned-base", "time.DAT"]))
  expect "a control archive without a digest is refused" (isLeft (parseInvocation "default" ["--control-archive", "control.a"]))
  expect "an unknown matrix is refused" (isLeft (parseInvocation "default" ["--matrix", "other", "execute"]))
  expect "the stock role boots the stock DAT under a staged base" (roleFirmware staged stockDat Stock == stockDat)
  expect "the owned role boots the stock DAT by default" (roleFirmware SharedBase stockDat Owned == stockDat)
  expect "the owned role boots the staged base" (roleFirmware staged stockDat Owned == ("time.DAT", digest 'b'))
  let identity role base = executionIdentity $ object
        [ "role" .= (role :: String), "tools" .= object ["viewer" .= ("viewer" :: String)]
        , "trajectory" .= object ["sha256" .= ("route" :: String)]
        , "inputs" .= object (["firmware" .= sha256Hex (digest 'a')] ++ ownedBaseInputs base) ]
  expect "a staged base invalidates the owned observation" (identity "owned" SharedBase /= identity "owned" staged)
  expect "a different staged base invalidates the owned observation"
    (identity "owned" staged /= identity "owned" (StagedBase "time.DAT" (digest 'c')))
  expect "a staged base preserves the stock observation" (identity "stock" SharedBase == identity "stock" staged)

digest :: Char -> Sha256
digest character = either error id (parseSha256Hex (replicate 64 character))

isLeft :: Either a b -> Bool
isLeft (Left _) = True
isLeft _ = False

roleInput :: Int -> String -> Value
roleInput route archive = object
  [ "role" .= ("stock" :: String), "trajectory" .= object ["sha256" .= show route]
  , "inputs" .= object ["firmware" .= ("firmware" :: String), "owned_archive" .= archive] ]

expect :: String -> Bool -> IO ()
expect label accepted = unless accepted (fail label)

parallelAdmission :: Int -> IO ()
parallelAdmission slots = do
  active <- newIORef (0 :: Int, 0 :: Int)
  barrier <- newEmptyMVar
  let enter item = do
        count <- atomicModifyIORef' active $ \(running, peak) ->
          ((running + 1, max peak (running + 1)), running + 1)
        (do
          if count == slots then mapM_ (const (putMVar barrier ())) [1 .. slots] else pure ()
          takeMVar barrier
          pure (item :: Int)) `finally`
            atomicModifyIORef' active (\(running, peak) -> ((running - 1, peak), ()))
  results <- mapBounded slots enter [1 .. slots]
  (running, peak) <- readIORef active
  expect "parallel actions preserve their results" (results == [1 .. slots])
  expect "parallel admission respects its ceiling and uses every slot" (running == 0 && peak == slots)
