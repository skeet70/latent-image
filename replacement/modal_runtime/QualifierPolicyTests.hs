{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Concurrent (newEmptyMVar, putMVar, takeMVar)
import Control.Exception (finally)
import Control.Monad (unless)
import Data.Aeson (Value, object, (.=))
import Data.IORef (atomicModifyIORef', newIORef, readIORef)
import Replacement.ModalRuntime.QualifierPolicy (executionIdentity, installedCheckpointMatches)
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
  mapM_ parallelAdmission [1, 2, 3]

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
