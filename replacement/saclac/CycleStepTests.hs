{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
-- | Refusal-path tests for the per-cycle stock step: the pairing refuses a cycle whose
-- dumps are missing, out of order or superseded, and the state comparison reports only a
-- declared-span byte or a byte the step itself changed.
module Main (main) where

import Control.Monad (unless)
import qualified Data.ByteString as B
import Data.List (isInfixOf)
import Data.Word (Word8)
import Replacement.Saclac.CycleStep
import System.Exit (exitFailure)

check :: String -> Bool -> IO Bool
check name ok = do
  putStrLn ((if ok then "ok   " else "FAIL ") ++ name)
  pure ok

dump :: String -> Int -> Integer -> Dump
dump label index instructions = Dump label index instructions ("dumps/" ++ label ++ ".bin") 6291456 (replicate 64 '0') []

queueIn :: Integer -> Int -> Int -> TimedEvent
queueIn instructions operation sender = TimedEvent instructions (EventQueueIn operation sender [1, 2, 3, 4])

peer :: Integer -> Int -> Int -> TimedEvent
peer instructions queue operation = TimedEvent instructions (EventPeerRequest queue operation [9, 9])

layout :: [Span]
layout = [Span 0x00900000 0x00b00000, Span 0x01600000 0x01a00000]

emptyState :: State
emptyState = State [Segment (spanLow span') (B.replicate (fromIntegral (spanBytes span')) 0) | span' <- layout]

withByte :: State -> Integer -> Word8 -> State
withByte state address byte = either (const state) id (overlayState state address (B.singleton byte))

refusalsOf :: [Dump] -> [TimedEvent] -> [String]
refusalsOf dumps events = map refusalReason (snd (pairCycles dumps events))

main :: IO ()
main = do
  results <- sequence
    [ do
        let dumps = [dump "wake" 0 1001, dump "frame" 0 1003, dump "window-completion" 0 1101]
            events = [queueIn 1000 0x3b4b 47, peer 1050 44 0x3c0c, peer 1060 45 0x3d00, queueIn 1100 0x3b8d 44]
            (cycles, refusals) = pairCycles dumps events
        _ <- check "qualified cycle pairs wake, frame and completion" (length cycles == 1 && null refusals)
        _ <- check "qualified cycle keeps the step's two posts" (case cycles of [paired] -> length (cyclePosts paired) == 2; _ -> False)
        _ <- check "qualified cycle ends at the completion dump" (case cycles of [paired] -> cycleBoundary paired == 1101; _ -> False)
        check "qualified cycle brackets the frame" (case cycles of [paired] -> eventInstructions (cycleWakeEvent paired) < dumpInstructions (cycleFrame paired); _ -> False)
    , do
        let dumps = [dump "wake" 0 1001, dump "frame" 0 1003]
        check "missing completion refuses the wake" (any ("without a window-completion" `isInfixOf`) (refusalsOf dumps [queueIn 1000 0x3b4b 47]))
    , do
        let dumps = [dump "wake" 0 1001, dump "window-completion" 0 1101]
        check "missing frame refuses the wake" (any ("without a frame dump" `isInfixOf`) (refusalsOf dumps [queueIn 1000 0x3b4b 47, queueIn 1100 0x3b8d 44]))
    , do
        let dumps = [dump "frame" 0 999, dump "wake" 0 1001]
        check "frame before a wake is refused" (any ("without a preceding wake" `isInfixOf`) (refusalsOf dumps [queueIn 1000 0x3b4b 47]))
    , do
        let dumps = [dump "wake" 0 1001, dump "wake" 1 2001, dump "frame" 0 2003, dump "window-completion" 0 2101]
            events = [queueIn 1000 0x3b4b 47, queueIn 2000 0x3b4b 47, peer 2050 44 0x3c0c, peer 2060 45 0x3d00, queueIn 2100 0x3b8d 44]
            (cycles, refusals) = pairCycles dumps events
        check "a superseded wake is refused and the later cycle kept" (length cycles == 1 && any ("superseded" `isInfixOf`) (map refusalReason refusals))
    , do
        let dumps = [dump "wake" 0 1001, dump "frame" 0 1101, dump "window-completion" 0 1101]
            events = [queueIn 1000 0x3b4b 47, queueIn 1100 0x3b8d 44]
        check "dump order is refused" (any ("dump order" `isInfixOf`) (refusalsOf dumps events))
    , do
        let dumps = [dump "wake" 0 1001, dump "frame" 0 1003, dump "window-completion" 0 1101]
        check "a wake without its queue-in event is refused" (any ("no 0x3b4b queue-in" `isInfixOf`) (refusalsOf dumps [peer 1050 44 0x3c0c, queueIn 1100 0x3b8d 44]))
    , do
        let dumps = [dump "wake" 0 1001, dump "frame" 0 1003, dump "window-completion" 0 1101]
        check "an overlapping wake is refused" (any ("later 0x3b4b wake" `isInfixOf`) (refusalsOf dumps [queueIn 1000 0x3b4b 47, queueIn 1090 0x3b4b 47, queueIn 1100 0x3b8d 44]))
    , do
        let bytes = B.pack [fromIntegral (index `mod` 251) | index <- [0 .. 0x600000 - 1 :: Int]]
            split = splitState layout bytes
        _ <- check "a dump splits into its declared ranges in order" (fmap (\state -> stateSlice state 0x01600000 1) split == Right (Just (B.pack [fromIntegral (0x200000 `mod` 251 :: Int)])) && fmap (\state -> stateSlice state 0x00900001 1) split == Right (Just (B.pack [1])))
        _ <- check "a dump shorter than its ranges is refused" (case splitState layout (B.take 16 bytes) of Left _ -> True; _ -> False)
        check "an address between the ranges is not held" (fmap (`stateHolds` 0x00b00000) split == Right False)
    , check "overlapping and adjacent windows merge, disjoint ones stay apart"
        (mergeWindows [Window 0x3000 0x1000, Window 0x1000 0x1800, Window 0x2000 0x1000, Window 0x8000 0x100]
          == [Window 0x1000 0x3000, Window 0x8000 0x100])
    , do
        let initial = emptyState
            predicted = withByte (withByte initial 0x0169cf30 7) 0x00920000 1
            observed = withByte (withByte initial 0x0169cf30 8) 0x00920000 2
            spans = [Span 0x0169cf24 0x0169d370]
            differences = compareState spans (netChanged initial predicted) predicted observed
            outside = withByte initial 0x00920000 2
        _ <- check "a declared-span byte differs" (any (\difference -> differenceAddress difference == 0x0169cf30 && differenceDeclared difference) differences)
        _ <- check "a step-changed byte outside the spans differs" (any (\difference -> differenceAddress difference == 0x00920000 && not (differenceDeclared difference)) differences)
        _ <- check "equal compared bytes are not differences" (null (compareState spans (netChanged initial predicted) predicted predicted))
        check "a foreign byte outside the spans and the step's writes is not a difference" (not (any (\difference -> differenceAddress difference == 0x00920000) (compareState spans [] initial outside)))
    ]
  unless (and results) exitFailure
