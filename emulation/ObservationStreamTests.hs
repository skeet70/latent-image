{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Host counterexample for the two source meanings: a frozen interval end and the stable snapshot
-- total at collection. Leading, rendezvous and trailing ranges must keep their exact rows even when
-- later execution raises the page total; a total that changes while paging must refuse.
module Main (main) where

import Control.Exception (SomeException, try)
import Control.Monad (unless)
import Data.Aeson (object, (.=))
import qualified Data.ByteString as B
import Data.Either (isLeft)
import Data.IORef (newIORef, readIORef, writeIORef)
import Emulation.ObservationStream
  ( TraceHitRef (..), TraceHitSnapshot (..), TraceHitSource (..), foldTraceHitSources, rowNext, withRowReader )
import Emulation.Sha256 (sha256)
import EmulatorUsb (TraceHit (..), TraceHitCursor (..), TraceHitLimit (..), TraceHitPage (..))
import System.Directory (createDirectoryIfMissing, getTemporaryDirectory, removeDirectoryRecursive)
import System.Exit (exitFailure)
import System.FilePath ((</>))

row :: Int -> TraceHit
row instructions = TraceHit
  { traceSite = "timeline", traceCore = 0, traceInstructions = fromIntegral instructions
  , traceR0 = fromIntegral instructions, traceR1 = 0, traceR2 = 0, traceR3 = 0
  , traceLr = Nothing, traceSp = Nothing, traceMemory = [], traceProvenanceSequence = Nothing }

rows :: [TraceHit]
rows = map row [0 .. 23]

pageAt :: Int -> TraceHitCursor -> TraceHitLimit -> TraceHitPage
pageAt snapshot (TraceHitCursor from) (TraceHitLimit limit) =
  TraceHitPage snapshot (TraceHitCursor from) [hit | (index, hit) <- zip [0 :: Int ..] rows, index >= from, index < min (from + limit) snapshot]

source :: Int -> Int -> Int -> TraceHitSource
source start end snapshot = TraceHitSource (TraceHitCursor start) (TraceHitCursor end) (TraceHitSnapshot snapshot) (\cursor limit -> pure (pageAt snapshot cursor limit))

instructionsOf :: [TraceHit] -> [Integer]
instructionsOf = map traceInstructions

main :: IO ()
main = do
  leading <- foldTraceHitSources [source 0 3 8] (\values hit -> pure (values ++ [hit])) []
  exitRows <- foldTraceHitSources [source 3 5 5] (\values hit -> pure (values ++ [hit])) []
  trailing <- foldTraceHitSources [source 5 8 8] (\values hit -> pure (values ++ [hit])) []
  let covered = leading ++ trailing
  unless (instructionsOf covered == [0, 1, 2, 5, 6, 7]) (putStrLn ("FAIL: leading/trailing order " ++ show (instructionsOf covered)) >> exitFailure)
  unless (instructionsOf exitRows == [3, 4]) (putStrLn ("FAIL: rendezvous rows " ++ show (instructionsOf exitRows)) >> exitFailure)
  let changing snapshot (TraceHitCursor from) (TraceHitLimit limit) =
        TraceHitPage (if from == 0 then snapshot else snapshot + 1) (TraceHitCursor from)
          [hit | (index, hit) <- zip [0 :: Int ..] rows, index >= from, index < min (from + limit) snapshot]
  changed <- try (foldTraceHitSources [TraceHitSource (TraceHitCursor 0) (TraceHitCursor 24) (TraceHitSnapshot 24) (\cursor limit -> pure (changing 24 cursor limit))] (\values hit -> pure (values ++ [hit])) [])
  unless (isLeft (changed :: Either SomeException [TraceHit])) (putStrLn "FAIL: a changed page total was accepted" >> exitFailure)
  stickySidecar
  putStrLn "observation-stream interval/snapshot counterexample passed"

stickySidecar :: IO ()
stickySidecar = do
  root <- getTemporaryDirectory
  let directory = root </> "observation-stream-sticky"
  createDirectoryIfMissing True directory
  let bytes = "{\"site\":\"timeline\"}\n{\"site\":\"timeline\"}\n"
      reference count = object ["trace_hits" .= TraceHitRef "rows.jsonl" (sha256 bytes) count]
      wrongDigest = object ["trace_hits" .= TraceHitRef "rows.jsonl" (sha256 "different sidecar") 2]
  B.writeFile (directory </> "rows.jsonl") bytes
  countSeen <- newIORef (Right (), Right (), Right (), Right ())
  countResult <- withRowReader directory (reference 3) $ \reader -> do
    first <- rowNext reader
    second <- rowNext reader
    third <- rowNext reader
    fourth <- rowNext reader
    writeIORef countSeen (fmap (const ()) first, fmap (const ()) second, fmap (const ()) third, fmap (const ()) fourth)
    pure (Right ())
  (firstSeen, secondSeen, thirdSeen, fourthSeen) <- readIORef countSeen
  case (countResult, firstSeen, secondSeen, thirdSeen, fourthSeen) of
    (Left _, Right (), Right (), Left _, Left _) -> pure ()
    _ -> putStrLn "FAIL: repeated EOF did not keep the count failure sticky" >> exitFailure
  digestSeen <- newIORef (Right (), Right ())
  digestResult <- withRowReader directory wrongDigest $ \reader -> do
    _ <- rowNext reader
    _ <- rowNext reader
    first <- rowNext reader
    second <- rowNext reader
    writeIORef digestSeen (fmap (const ()) first, fmap (const ()) second)
    pure (Right ())
  (digestFirst, digestSecond) <- readIORef digestSeen
  case (digestResult, digestFirst, digestSecond) of
    (Left _, Left _, Left _) -> pure ()
    _ -> putStrLn "FAIL: repeated finish did not keep the digest failure sticky" >> exitFailure
  removeDirectoryRecursive directory
