{-# LANGUAGE OverloadedStrings #-}
-- | Semaphore-specific observation checks. The call view is supplied by
-- 'Emulation.SchedulerPairCheck'; this module owns the semaphore record layout and the
-- independent expected-state rules for the seven native operations.
module Emulation.SchedulerPairSemaphore
  ( SemaphoreCall (..), checkSemaphoreCall, SemaphoreLayout (..), semaphoreLayout, semaphoreWindows ) where

import Control.Monad (foldM, forM_, unless, when)
import Data.Bits ((.&.), (.|.), complement, shiftL, shiftR)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.SchedulerPairCatalog (Address (..), SemaphoreService (..))
import Emulation.SchedulerPairFamily
  ( FamilyBoundary (..), FamilyBoundaryKind (..), FamilyCompletion (..), FamilyRefusal (..), FamilyTrace (..) )
import Emulation.SchedulerPairModel (Memory)
import Emulation.SchedulerPairPlan (Role (..))
import Replacement.RtosOwnFirst.Regions (Domain (..))

class SemaphoreCall c where
  semaphoreDomain :: c -> Domain
  semaphoreCore :: c -> Int
  semaphoreArguments :: c -> [Word32]
  semaphoreBefore :: c -> Memory
  semaphoreAfter :: c -> Memory
  semaphoreR0 :: c -> Word32
  semaphoreWrites :: c -> [(Address, Int, Word32)]
  semaphoreTask :: c -> Word32
  semaphoreService :: c -> Maybe SemaphoreService
  semaphoreTrace :: c -> Maybe FamilyTrace

expect :: FamilyRefusal -> Bool -> Either FamilyRefusal ()
expect refusal valid = unless valid (Left refusal)

wordAt :: Word32 -> Memory -> Either FamilyRefusal Word32
wordAt address memory = maybe (Left FamilyEffect) Right (M.lookup (Address address) memory)

byteAt :: Word32 -> Memory -> Either FamilyRefusal Word32
byteAt address memory = do
  word <- wordAt (address .&. complement 3) memory
  pure ((word `shiftR` (8 * fromIntegral (address .&. 3))) .&. 0xff)

argAt :: [Word32] -> Int -> Either FamilyRefusal Word32
argAt arguments index = case drop index arguments of
  value : _ -> Right value
  [] -> Left FamilyReturn

-- | Replays only the writes whose word the bounded snapshot captured; the rest are outside
-- the observation's declared state.
replayWrite :: Memory -> (Address, Int, Word32) -> Either FamilyRefusal Memory
replayWrite memory (Address address, size, value)
  | not (M.member (Address (address .&. complement 3)) memory) = Right memory
  | otherwise = do
      expect FamilyEffect (size `elem` [1, 2, 4] && address `mod` fromIntegral size == 0)
      old <- wordAt address memory
      let shift = fromIntegral (address .&. 3) * 8
          mask = if size == 4 then maxBound else (1 `shiftL` (size * 8)) - 1
      expect FamilyEffect (value .&. complement mask == 0)
      pure (M.insert (Address address) ((old .&. complement (mask `shiftL` shift)) .|. (value `shiftL` shift)) memory)

statusInvalid, statusAlreadyExists, statusNotCreated, statusOverflow, statusReleased, statusTimeout :: Word32
statusInvalid = 0xffffffee
statusAlreadyExists = 0xffffffd7
statusNotCreated = 0xffffffd6
statusOverflow = 0xffffffe4
statusReleased = 0xffffffcf
statusTimeout = 0xffffffce

data SemaphoreLayout = SemaphoreLayout
  { layoutConfigs :: Word32
  , layoutControls :: Word32
  , layoutWaits :: Word32
  , layoutTimers :: Word32
  }

semaphoreLayout :: Domain -> SemaphoreLayout
semaphoreLayout DomainA = SemaphoreLayout 0x4005dfd8 0x4005af0c 0x40054efc 0x4005549c
semaphoreLayout DomainB = SemaphoreLayout 0x400edf98 0x400eaecc 0x400e4ebc 0x400e545c

semaphoreWindows :: Domain -> [(Word32, Word32)]
semaphoreWindows DomainA =
  [ (0x47604, 0x47844), (0x47844, 0x47a54), (0x47a54, 0x47c94)
  , (0x47c94, 0x47dd0), (0x47dd0, 0x48044), (0x4d2a8, 0x4d338), (0x4d338, 0x4d39c) ]
semaphoreWindows DomainB =
  [ (0xd75c4, 0xd7804), (0xd7804, 0xd7a14), (0xd7a14, 0xd7c54)
  , (0xd7c54, 0xd7d90), (0xd7d90, 0xd8004), (0xdd26c, 0xdd2fc), (0xdd2fc, 0xdd360) ]

inSemaphoreWindow :: Domain -> Word32 -> Bool
inSemaphoreWindow domain pc = any (\(lo, hi) -> pc >= lo && pc < hi) (semaphoreWindows domain)

configAddress, controlAddress, waitAddress :: Domain -> Word32 -> Word32
configAddress domain slot = layoutConfigs (semaphoreLayout domain) + (slot - 1) * 0x10
controlAddress domain slot = layoutControls (semaphoreLayout domain) + (slot - 1) * 0x10
waitAddress domain slot = layoutWaits (semaphoreLayout domain) + (slot - 1) * 0x0c

isSemaphoreTimer :: Domain -> Word32 -> Bool
isSemaphoreTimer _ value =
  any (\domain -> let layout = semaphoreLayout domain in
         (value >= layoutTimers layout && value < layoutTimers layout + 120 * 0x0c && (value - layoutTimers layout) `mod` 0x0c == 0))
      [DomainA, DomainB]

timeoutCallback :: Domain -> Word32
timeoutCallback DomainA = 0x44318
timeoutCallback DomainB = 0xd42d8

tickEntry :: Domain -> Word32
tickEntry DomainA = 0x44d8c
tickEntry DomainB = 0xd4d4c

resolve :: Domain -> Word32 -> Either FamilyRefusal (Domain, Word32)
resolve entry idWord = do
  let slot = idWord .&. 0xffff
  domain <- case idWord `shiftR` 16 of
    0 -> Right entry
    1 -> Right DomainA
    2 -> Right DomainB
    _ -> Left FamilyReturn
  expect FamilyReturn (slot >= 1 && slot <= 250)
  pure (domain, slot)

queueTasks :: Memory -> Word32 -> Either FamilyRefusal [Word32]
queueTasks memory control = go (0 :: Int) =<< wordAt control memory
  where
    go depth task
      | depth > 256 = Left FamilyDestructor
      | task == control = Right []
      | otherwise = do
          rest <- wordAt task memory >>= go (depth + 1)
          pure (task : rest)

waiterWait :: Memory -> Word32 -> Either FamilyRefusal Word32
waiterWait memory task = wordAt (task + 0x14) memory

checkSemaphoreCall :: SemaphoreCall c => Role -> c -> [c] -> Either FamilyRefusal ()
checkSemaphoreCall role call others = do
  trace <- maybe (Left FamilyContinuation) Right (semaphoreTrace call)
  expect FamilyContinuation (familyTraceCompletion trace == FamilyReturnedCompletion)
  replayed <- foldM replayWrite (semaphoreBefore call) (semaphoreWrites call)
  let beforeKeys = M.keys (semaphoreBefore call)
  expect FamilyEffect (all (\address -> M.lookup address replayed == M.lookup address (semaphoreAfter call)) beforeKeys)
  service <- maybe (Left FamilyReturn) Right (semaphoreService call)
  idWord <- argAt (semaphoreArguments call) 0
  let domain = semaphoreDomain call
  case service of
    SemaphoreCreate -> checkCreate role call domain idWord
    SemaphoreDelete -> checkDelete role call domain idWord
    SemaphoreSignal -> checkSignal call domain idWord False
    SemaphoreInterruptSignal -> checkSignal call domain idWord True
    SemaphoreWait -> checkWait call domain idWord False trace
    SemaphorePoll -> checkPoll call domain idWord
    SemaphoreTimedWait -> checkWait call domain idWord True trace
  checkContinuation call trace
  checkTimer call others trace service

checkCreate :: SemaphoreCall c => Role -> c -> Domain -> Word32 -> Either FamilyRefusal ()
checkCreate role call domain idWord = do
  let slot = idWord .&. 0xffff
  if slot < 1 || slot > 250
    then case role of
      Owned -> expect FamilyReturn (semaphoreR0 call == statusInvalid)
      _ -> pure ()
    else do
      request <- argAt (semaphoreArguments call) 1
      exists <- wordAt (configAddress domain slot + 0x0c) (semaphoreBefore call)
      if exists /= 0
        then expect FamilyReturn (semaphoreR0 call == statusAlreadyExists)
        else do
          expect FamilyReturn (semaphoreR0 call == 0)
          attributes <- wordAt request (semaphoreBefore call)
          initial <- wordAt (request + 4) (semaphoreBefore call)
          maximumCount <- wordAt (request + 8) (semaphoreBefore call)
          forM_ [ (configAddress domain slot, attributes)
                , (configAddress domain slot + 4, initial)
                , (configAddress domain slot + 8, maximumCount)
                , (configAddress domain slot + 0x0c, 1)
                , (controlAddress domain slot + 0x0c, initial)
                ] $ \(address, value) -> wordAt address (semaphoreAfter call) >>= expect FamilyEffect . (== value)

checkDelete :: SemaphoreCall c => Role -> c -> Domain -> Word32 -> Either FamilyRefusal ()
checkDelete role call domain idWord = do
  let slot = idWord .&. 0xffff
  if slot < 1 || slot > 250
    then case role of
      Owned -> expect FamilyReturn (semaphoreR0 call == statusInvalid)
      _ -> pure ()
    else do
      expect FamilyReturn (semaphoreR0 call == 0)
      forM_ [ (configAddress domain slot, 0)
            , (configAddress domain slot + 4, 1)
            , (configAddress domain slot + 8, 1)
            , (configAddress domain slot + 0x0c, 0)
            , (controlAddress domain slot + 0x0c, 1)
            ] $ \(address, value) -> wordAt address (semaphoreAfter call) >>= expect FamilyEffect . (== value)
      when (role == Owned) $ do
        expect FamilyEffect (wordAt (controlAddress domain slot) (semaphoreAfter call) == Right (controlAddress domain slot))
        expect FamilyEffect (wordAt (controlAddress domain slot + 4) (semaphoreAfter call) == Right (controlAddress domain slot))
        tasks <- queueTasks (semaphoreBefore call) (controlAddress domain slot)
        forM_ tasks $ \task -> do
          wait <- waiterWait (semaphoreBefore call) task
          wordAt wait (semaphoreAfter call) >>= expect FamilyLifetime . (== statusReleased)
          byteAt (task + 0x0c) (semaphoreAfter call) >>= expect FamilyLifetime . (\state -> state .&. 1 /= 0)

checkSignal :: SemaphoreCall c => c -> Domain -> Word32 -> Bool -> Either FamilyRefusal ()
checkSignal call domain idWord _handler = do
  (object, slot) <- resolve domain idWord
  let control = controlAddress object slot
  first <- wordAt control (semaphoreBefore call)
  if first == control
    then do
      count <- wordAt (control + 0x0c) (semaphoreBefore call)
      maximumCount <- wordAt (configAddress object slot + 8) (semaphoreBefore call)
      if count < maximumCount
        then do
          expect FamilyReturn (semaphoreR0 call == 0)
          wordAt (control + 0x0c) (semaphoreAfter call) >>= expect FamilyEffect . (== count + 1)
        else expect FamilyReturn (semaphoreR0 call == statusOverflow)
    else do
      expect FamilyReturn (semaphoreR0 call == 0)
      wait <- waiterWait (semaphoreBefore call) first
      wordAt wait (semaphoreAfter call) >>= expect FamilyLifetime . (== 0)
      byteAt (first + 0x0c) (semaphoreAfter call) >>= expect FamilyLifetime . (\state -> state .&. 1 /= 0)
      next <- wordAt first (semaphoreBefore call)
      wordAt control (semaphoreAfter call) >>= expect FamilyEffect . (== next)

checkPoll :: SemaphoreCall c => c -> Domain -> Word32 -> Either FamilyRefusal ()
checkPoll call domain idWord = do
  (object, slot) <- resolve domain idWord
  exists <- wordAt (configAddress object slot + 0x0c) (semaphoreBefore call)
  expect FamilyReturn (exists /= 0 || semaphoreR0 call == statusNotCreated)
  let control = controlAddress object slot
  count <- wordAt (control + 0x0c) (semaphoreBefore call)
  if semaphoreR0 call == 0
    then wordAt (control + 0x0c) (semaphoreAfter call) >>= expect FamilyEffect . (== count - 1)
    else expect FamilyReturn (semaphoreR0 call == statusTimeout)

taskSlotOf :: Domain -> Word32 -> Maybe Word32
taskSlotOf domain task =
  let base = case domain of DomainA -> 0x4005ff0c; DomainB -> 0x400efecc
      offset = task - base
   in if task >= base && offset `mod` 0x38 == 0 && offset `div` 0x38 < 120 then Just (offset `div` 0x38 + 1) else Nothing

checkWait :: SemaphoreCall c => c -> Domain -> Word32 -> Bool -> FamilyTrace -> Either FamilyRefusal ()
checkWait call domain idWord timed trace = do
  (object, slot) <- resolve domain idWord
  exists <- wordAt (configAddress object slot + 0x0c) (semaphoreBefore call)
  expect FamilyReturn (exists /= 0 || semaphoreR0 call == statusNotCreated)
  let control = controlAddress object slot
      task = semaphoreTask call
  taskSlot <- maybe (Left FamilyAssociation) Right (taskSlotOf domain task)
  let wait = waitAddress domain taskSlot
      saved = any ((== FamilySaveKind) . familyBoundaryKind) (familyTraceHistory trace)
  if not saved
    then do
      expect FamilyReturn (semaphoreR0 call == 0)
      count <- wordAt (control + 0x0c) (semaphoreBefore call)
      wordAt (control + 0x0c) (semaphoreAfter call) >>= expect FamilyEffect . (== count - 1)
    else do
      expect FamilyLifetime (semaphoreR0 call `elem` [0, statusReleased, statusTimeout])
      wordAt wait (semaphoreAfter call) >>= expect FamilyLifetime . (== semaphoreR0 call)
      byteAt (task + 0x0c) (semaphoreAfter call) >>= expect FamilyLifetime . (\state -> state .&. 1 /= 0)
      when (semaphoreR0 call == statusTimeout) $ expect FamilyTimer timed
      when (semaphoreR0 call == 0) $ do
        count <- wordAt (control + 0x0c) (semaphoreBefore call)
        wordAt (control + 0x0c) (semaphoreAfter call) >>= expect FamilyEffect . (== count)

checkContinuation :: SemaphoreCall c => c -> FamilyTrace -> Either FamilyRefusal ()
checkContinuation call trace = do
  let history = familyTraceHistory trace
      task = semaphoreTask call
      core = semaphoreCore call
      domain = semaphoreDomain call
  forM_ (filter (\boundary -> familyBoundaryKind boundary `elem` [FamilySaveKind, FamilyResumeKind]) history) $ \boundary ->
    expect FamilyAssociation (familyBoundaryCore boundary == core && familyBoundaryTask boundary == task)
  forM_ (filter (\boundary -> familyBoundaryKind boundary == FamilySaveKind) history) $ \boundary ->
    case familyBoundaryRegisters boundary of
      registers | length registers == 16 -> expect FamilyContinuation (inSemaphoreWindow domain (registers !! 14))
      _ -> Left FamilyContinuation
  forM_ (filter (\boundary -> familyBoundaryKind boundary == FamilyResumeKind) history) $ \boundary ->
    case familyBoundaryRegisters boundary of
      registers | length registers == 16 -> expect FamilyContinuation (inSemaphoreWindow domain (registers !! 15))
      _ -> Left FamilyContinuation

checkTimer :: SemaphoreCall c => c -> [c] -> FamilyTrace -> SemaphoreService -> Either FamilyRefusal ()
checkTimer call others trace service = do
  let history = familyTraceHistory trace
      domain = semaphoreDomain call
      task = semaphoreTask call
      inserts = [value | boundary <- history, familyBoundaryKind boundary == FamilyTimerInsertKind, Just value <- [familyBoundaryValue boundary]]
      siblingInserts = [value | other <- others, boundary <- maybe [] familyTraceHistory (semaphoreTrace other), familyBoundaryKind boundary == FamilyTimerInsertKind, Just value <- [familyBoundaryValue boundary]]
      removes = [value | boundary <- history, familyBoundaryKind boundary == FamilyTimerRemoveKind, Just value <- [familyBoundaryValue boundary]]
      tickPops = [boundary | boundary <- history, boundaryAddressOf boundary == tickEntry domain]
  forM_ inserts $ \value -> do
    expect FamilyTimer (isSemaphoreTimer domain value)
    wordAt (value + 4) (semaphoreAfter call) >>= expect FamilyTimer . (== timeoutCallback domain)
    wordAt (value + 8) (semaphoreAfter call) >>= expect FamilyAssociation . (== task)
  forM_ removes $ \value -> expect FamilyTimer (value `elem` inserts || value `elem` siblingInserts)
  case service of
    SemaphoreTimedWait -> do
      expect FamilyTimer (semaphoreR0 call /= statusTimeout || not (null inserts))
      when (semaphoreR0 call == statusTimeout) $ expect FamilyTimer (not (null tickPops))
    SemaphoreSignal -> forM_ removes $ \value -> expect FamilyTimer (value `elem` inserts || value `elem` siblingInserts)
    SemaphoreDelete -> forM_ inserts $ \value -> expect FamilyTimer (value `elem` removes)
    _ -> expect FamilyTimer (null inserts && null removes)

boundaryAddressOf :: FamilyBoundary -> Word32
boundaryAddressOf boundary = case familyBoundaryAddress boundary of Address address -> address
