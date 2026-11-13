{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Host counterexample for the two source meanings: a frozen interval end and the stable snapshot
-- total at collection. Leading, rendezvous and trailing ranges must keep their exact rows even when
-- later execution raises the page total; a total that changes while paging must refuse.
module Main (main) where

import Control.Exception (SomeException, try)
import Control.Monad (unless)
import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Either (isLeft)
import Data.IORef (newIORef, readIORef, writeIORef)
import Emulation.ObservationStream
  ( ObservationRef (..), TraceHitRef (..), TraceHitSnapshot (..), TraceHitSource (..)
  , VerifiedObservation (..)
  , countObservationDrawers, decodeObservationRef, foldTraceHitSources, freezeTraceHitSource
  , loadReceiptObservations, loadVerifiedObservation, resolveReference
  , rowNext, withRowReader )
import Emulation.Sha256 (sha256, sha256Hex)
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
  frozenInterval
  stickySidecar
  verifiedDocument
  receiptBinding
  putStrLn "observation-stream interval/snapshot and verified-document counterexamples passed"

-- | The pre-exit interval is frozen at the count its range ends at, the segment's steps then record
-- more hits, and the segment's observation folds the interval after them. The frozen interval keeps
-- its rows; the same interval read after the steps refuses.
frozenInterval :: IO ()
frozenInterval = do
  total <- newIORef (8 :: Int)
  let interval = TraceHitSource (TraceHitCursor 0) (TraceHitCursor 3) (TraceHitSnapshot 8) $ \cursor limit -> do
        current <- readIORef total
        pure (pageAt current cursor limit)
  frozen <- freezeTraceHitSource interval
  writeIORef total 11
  replayed <- foldTraceHitSources [frozen] (\values hit -> pure (values ++ [hit])) []
  unless (instructionsOf replayed == [0, 1, 2])
    (putStrLn ("FAIL: a frozen interval lost its rows, got " ++ show (instructionsOf replayed)) >> exitFailure)
  late <- try (foldTraceHitSources [interval] (\values hit -> pure (values ++ [hit])) [])
  unless (isLeft (late :: Either SomeException [TraceHit]))
    (putStrLn "FAIL: an interval was read after its total moved" >> exitFailure)

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

verifiedDocument :: IO ()
verifiedDocument = do
  root <- getTemporaryDirectory
  let directory = root </> "observation-stream-verified"
      document = "{\"phase\":\"replay\",\"trace_hits\":[]}\n"
      size = B.length document
      path = directory </> "obs.json"
      hits = TraceHitRef "obs.json" (sha256 document) 0
      reference phase = ObservationRef phase "obs.json" (sha256 document) (fromIntegral size) hits
      element = object
        [ "phase" .= ("replay" :: String), "path" .= ("obs.json" :: FilePath)
        , "sha256" .= sha256Hex (sha256 document), "bytes" .= (fromIntegral size :: Integer)
        , "trace_hits" .= hits ]
      refused label ref = do
        result <- loadVerifiedObservation directory ref
        unless (isLeft result) (putStrLn ("FAIL: " ++ label ++ " was accepted") >> exitFailure)
  createDirectoryIfMissing True directory
  B.writeFile path document
  case decodeObservationRef element of
    Left problem -> putStrLn ("FAIL: receipt element did not decode: " ++ problem) >> exitFailure
    Right decoded -> unless (decoded == reference "replay") (putStrLn "FAIL: decoded reference differs" >> exitFailure)
  loaded <- loadVerifiedObservation directory (reference "replay")
  case loaded of
    Left problem -> putStrLn ("FAIL: verified document was refused: " ++ problem) >> exitFailure
    Right verified -> unless
      (verifiedObservationPath verified == path
        && verifiedObservationValue verified == object ["phase" .= ("replay" :: String), "trace_hits" .= ([] :: [Int])])
      (putStrLn "FAIL: verified document differs" >> exitFailure)
  refused "a byte-count drift" (ObservationRef "replay" "obs.json" (sha256 document) (fromIntegral size + 1) hits)
  refused "a digest drift" (ObservationRef "replay" "obs.json" (sha256 "other") (fromIntegral size) hits)
  refused "a phase drift" (ObservationRef "menu" "obs.json" (sha256 document) (fromIntegral size) hits)
  refused "a nested path" (ObservationRef "replay" "sub/obs.json" (sha256 document) (fromIntegral size) hits)
  refused "an absolute path" (ObservationRef "replay" "/obs.json" (sha256 document) (fromIntegral size) hits)
  refused "a missing document" (ObservationRef "replay" "absent.json" (sha256 document) (fromIntegral size) hits)
  unless (resolveReference directory "obs.json" == Right path) (putStrLn "FAIL: plain reference path did not resolve" >> exitFailure)
  unless (isLeft (resolveReference directory "sub/obs.json" :: Either String FilePath)) (putStrLn "FAIL: nested reference path resolved" >> exitFailure)
  removeDirectoryRecursive directory

receiptBinding :: IO ()
receiptBinding = do
  root <- getTemporaryDirectory
  let directory = root </> "observation-stream-receipt"
  createDirectoryIfMissing True directory
  let sidecar = "{\"site\":\"render-tree-entry\"}\n{\"site\":\"timeline\"}\n{\"site\":\"render-picker-entry\"}\n"
      hitRef = object ["schema" .= (1 :: Int), "path" .= ("rows.jsonl" :: FilePath), "sha256" .= sha256Hex (sha256 sidecar), "count" .= (3 :: Int)]
      document = BL.toStrict (encode (object ["phase" .= ("replay" :: String), "trace_hits" .= hitRef]))
      reference = object
        [ "phase" .= ("replay" :: String), "path" .= ("obs.json" :: FilePath)
        , "sha256" .= sha256Hex (sha256 document), "bytes" .= (fromIntegral (B.length document) :: Integer)
        , "trace_hits" .= hitRef ]
      receipt = object ["observations" .= [reference]]
  B.writeFile (directory </> "rows.jsonl") sidecar
  B.writeFile (directory </> "obs.json") document
  loaded <- loadReceiptObservations directory receipt
  case loaded of
    Left problem -> putStrLn ("FAIL: streamed receipt observation refused: " ++ problem) >> exitFailure
    Right [verified] -> do
      count <- countObservationDrawers verified
      unless (count == Right 2) (putStrLn ("FAIL: streamed drawer count " ++ show count) >> exitFailure)
    Right _ -> putStrLn "FAIL: receipt observation count differs" >> exitFailure
  B.appendFile (directory </> "rows.jsonl") "{\"site\":\"render-tree-entry\"}\n"
  tampered <- loadReceiptObservations directory receipt
  unless (isLeft tampered) (putStrLn "FAIL: changed sidecar was accepted" >> exitFailure)
  B.writeFile (directory </> "rows.jsonl") sidecar
  B.appendFile (directory </> "obs.json") "\n"
  tamperedDocument <- loadReceiptObservations directory receipt
  unless (isLeft tamperedDocument) (putStrLn "FAIL: changed observation document was accepted" >> exitFailure)
  removeDirectoryRecursive directory
