{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import qualified Data.ByteString as B
import qualified Data.Set as Set
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.Joins
import Replacement.StartupArtifact.Types
import System.Exit (exitFailure)

main :: IO ()
main = do
  let timer = timerPiece [arenaWrite, WriteRange (CorePhysicalTarget Core0) 0x5476c 0x5477c]
      submission = submissionPiece [WriteRange PhysicalTarget 0xa10200 0xa5a200]
      storage = storagePiece [WriteRange PhysicalTarget 0xa10200 0xa5a200]
      evidence pieces = JoinEvidence (error "unused") (error "unused") (error "unused") pieces
      check name pieces = and [holds (evidence pieces) | HostCheck checkName holds <- joinChecks reservation, checkName == name]
  unless (all (\name -> check name [timer, submission, storage]) names) refuse
  unless (not (check "task-writers-exclude-reserved-arena" [timer, submission, storagePiece [WriteRange PhysicalTarget 0xa5a200 0xa5a204]])) refuse
  unless (not (check "task-writers-exclude-reserved-arena" [timer, submission, storagePiece [WriteRange PhysicalTarget 0x40a5a200 0x40a5b200]])) refuse
  unless (not (check "timer-writes-stay-outside-allocatable-pool" [timerPiece [arenaWrite, WriteRange PhysicalTarget 0xa10200 0xa10204], submission, storage])) refuse
  unless (not (check "timer-declares-its-arena" [timerPiece [WriteRange PhysicalTarget 0xa5a200 0xa5aa00], submission, storage])) refuse
  unless (not (check "timer-arenas-fit-reserved-span" [(timerPiece [arenaWrite]) { pieceFacts = TimerFacts (LinkedBytes B.empty B.empty) (LinkedBytes B.empty B.empty) (PoolSpan 0xa5a200 0xa5aa00) (PoolSpan 0xa5aa00 0xa5aa00) }, submission, storage])) refuse
  unless (not (check "task-pool-allocatable-end" [timer, submission { pieceFacts = TaskSubmissionFacts (LinkedBytes B.empty B.empty) badPool }, storage])) refuse
  where
    refuse = putStrLn "timer arena join regression" >> exitFailure
    names = ["task-pool-allocatable-end", "timer-arenas-fit-reserved-span", "task-writers-exclude-reserved-arena"
            , "timer-writes-stay-outside-allocatable-pool", "timer-declares-its-arena"]
    reservation = case [join | join <- registeredJoins, joinName (joinKind join) == "timer-task-arena-reservation"] of
      [join] -> join
      _ -> error "timer arena reservation join is not registered once"
    arenaWrite = WriteRange PhysicalTarget 0xa5a200 0xa5b200
    records = either (error . show) id (label TaskSubmission "records")
    cursor = either (error . show) id (label TaskSubmission "cursor")
    goodPool = PoolAllocation (PoolSpan 0xa10200 0xa5a200) 0xa5a200 (PoolSpan 0xa5a200 0xa5b200) records cursor [] [] [] (Citation "test")
    badPool = PoolAllocation (PoolSpan 0xa10200 0xa5b200) 0xa5b200 (PoolSpan 0xa5a200 0xa5b200) records cursor [] [] [] (Citation "test")
    timerPiece writes = piece Timer (TimerFacts (LinkedBytes B.empty B.empty) (LinkedBytes B.empty B.empty) (PoolSpan 0xa5a200 0xa5aa00) (PoolSpan 0xa5aa00 0xa5b200)) writes
    submissionPiece writes = piece TaskSubmission (TaskSubmissionFacts (LinkedBytes B.empty B.empty) goodPool) writes
    storagePiece writes = piece TaskStorage (TaskStorageFacts (LinkedBytes B.empty B.empty)) writes
    piece kind facts writes = Piece kind Set.empty [] [] [] [] (Catalog [] [] (EnumeratedWrites writes (Premised (Premise "test"))) []) facts
