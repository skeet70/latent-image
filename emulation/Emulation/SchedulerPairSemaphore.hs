{-# LANGUAGE OverloadedStrings #-}
-- | Semaphore-specific observation checks. The call view is supplied by
-- 'Emulation.SchedulerPairCheck'; this module owns the semaphore record layout and the
-- independent expected-state rules for the seven native operations.
module Emulation.SchedulerPairSemaphore
  ( SemaphoreCall (..), checkSemaphoreCall, checkPendingSemaphore, SemaphoreLayout (..), semaphoreLayout, semaphoreWindows ) where

import Control.Monad (foldM, forM_, unless, when)
import Data.Bits ((.&.), (.|.), complement, shiftL, shiftR)
import qualified Data.Map.Strict as M
import Data.Word (Word32, Word64)
import Emulation.SchedulerPairCatalog (Address (..), Semantic (..), SemaphoreService (..))
import Emulation.SchedulerPairFamily
  ( FamilyBoundary (..), FamilyBoundaryKind (..), FamilyCompletion (..), FamilyRefusal (..), FamilyTrace (..), Pending (..)
  , TimerKind (..), TimerWitness (..), ReleaseWitness (..) )
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
  semaphoreCpsr :: c -> Word32
  semaphoreWrites :: c -> [(Address, Int, Word32)]
  semaphoreTask :: c -> Word32
  semaphoreEpoch :: c -> Word64
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
  | not (M.member (Address aligned) memory) = Right memory
  | otherwise = do
      expect FamilyEffect (size `elem` [1, 2, 4] && address `mod` fromIntegral size == 0)
      old <- wordAt aligned memory
      let shift = fromIntegral (address .&. 3) * 8
          mask = if size == 4 then maxBound else (1 `shiftL` (size * 8)) - 1
      expect FamilyEffect (value .&. complement mask == 0)
      pure (M.insert (Address aligned) ((old .&. complement (mask `shiftL` shift)) .|. (value `shiftL` shift)) memory)
  where aligned = address .&. complement 3

statusInvalid, statusAlreadyExists, statusNotCreated, statusOverflow, statusReleased, statusTimeout, statusContext :: Word32
statusInvalid = 0xffffffee
statusAlreadyExists = 0xffffffd7
statusNotCreated = 0xffffffd6
statusOverflow = 0xffffffd5
statusReleased = 0xffffffcf
statusTimeout = 0xffffffce
statusContext = 0xffffffe7

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

handlerContext :: Domain -> Word32
handlerContext DomainA = 0x400545a4
handlerContext DomainB = 0x400e4564

dispatchEnabled :: Domain -> Word32
dispatchEnabled DomainA = 0x40054764
dispatchEnabled DomainB = 0x400e4724

-- | The status a refusal path must return before any operation effect.
data Admission = Refused Word32 | Admitted (Domain, Word32)

admission :: SemaphoreCall c => Role -> c -> SemaphoreService -> Domain -> Word32 -> Either FamilyRefusal Admission
admission role call service domain idWord
  | role == Stock && invalid = Left FamilyReturn
  | service `elem` [SemaphoreCreate, SemaphoreDelete] = do
      if invalid then pure (Refused statusInvalid) else do
        exists <- wordAt (configAddress domain slot + 0x0c) (semaphoreBefore call)
        pure (if service == SemaphoreCreate && exists /= 0 then Refused statusAlreadyExists else Admitted (domain, slot))
  | otherwise = do
      handler <- wordAt (handlerContext domain) (semaphoreBefore call)
      dispatch <- if service `elem` [SemaphoreWait, SemaphoreTimedWait]
        then wordAt (dispatchEnabled domain) (semaphoreBefore call) else Right 1
      let contextRefusal
            | (handler /= 0) /= (service == SemaphoreInterruptSignal) = Just statusContext
            | semaphoreCpsr call .&. 0x80 /= 0 = Just statusContext
            | dispatch == 0 = Just statusContext
            | otherwise = Nothing
      case contextRefusal of
        Just status -> pure (Refused status)
        Nothing -> if invalid then pure (Refused statusInvalid) else case object of
          Nothing -> pure (Refused statusInvalid)
          Just (objectDomain, objectSlot) -> do
            exists <- wordAt (configAddress objectDomain objectSlot + 0x0c) (semaphoreBefore call)
            pure (if exists == 0 then Refused statusNotCreated else Admitted (objectDomain, objectSlot))
  where
    slot = idWord .&. 0xffff
    classBits = (idWord `shiftR` 16) .&. 0xffff
    invalid = slot < 1 || slot > 250 || (service `notElem` [SemaphoreCreate, SemaphoreDelete] && classBits > 2)
    object = case classBits of
      0 -> Just (domain, slot)
      1 -> Just (DomainA, slot)
      2 -> Just (DomainB, slot)
      _ -> Nothing

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

checkSemaphoreCall :: SemaphoreCall c => c -> [TimerWitness] -> Either FamilyRefusal ()
checkSemaphoreCall = checkSemaphoreCallFor Owned

checkSemaphoreCallFor :: SemaphoreCall c => Role -> c -> [TimerWitness] -> Either FamilyRefusal ()
checkSemaphoreCallFor role call witnesses = do
  trace <- maybe (Left FamilyContinuation) Right (semaphoreTrace call)
  expect FamilyContinuation (familyTraceCompletion trace == FamilyReturnedCompletion)
  replayed <- foldM replayWrite (semaphoreBefore call) (semaphoreWrites call)
  let beforeKeys = M.keys (semaphoreBefore call)
  expect FamilyEffect (all (\address -> M.lookup address replayed == M.lookup address (semaphoreAfter call)) beforeKeys)
  service <- maybe (Left FamilyReturn) Right (semaphoreService call)
  idWord <- argAt (semaphoreArguments call) 0
  let domain = semaphoreDomain call
  admitted <- admission role call service domain idWord
  case admitted of
    Refused expected -> do
      expect FamilyReturn (semaphoreR0 call == expected)
      let history = familyTraceHistory trace
          before = semaphoreBefore call
          after = semaphoreAfter call
      expect FamilyEffect (null (semaphoreWrites call))
      expect FamilyEffect (M.keys after == M.keys before
        && all (\address -> M.lookup address after == M.lookup address before) (M.keys before))
      expect FamilyTimer (null [boundary | boundary <- history, familyBoundaryKind boundary `elem` [FamilyTimerInsertKind, FamilyTimerRemoveKind, FamilyTimerConsumeKind]])
      expect FamilyContinuation (null [boundary | boundary <- history, familyBoundaryKind boundary == FamilySaveKind])
    Admitted (object, slot) -> case service of
      SemaphoreCreate -> checkCreate call object slot
      SemaphoreDelete -> checkDelete role call object slot
      SemaphoreSignal -> checkSignal call object slot
      SemaphoreInterruptSignal -> checkSignal call object slot
      SemaphoreWait -> checkWait call domain object slot False trace
      SemaphorePoll -> checkPoll call object slot
      SemaphoreTimedWait -> checkWait call domain object slot True trace
  checkContinuation call trace
  checkTimer call witnesses trace service

checkCreate :: SemaphoreCall c => c -> Domain -> Word32 -> Either FamilyRefusal ()
checkCreate call domain slot = do
  request <- argAt (semaphoreArguments call) 1
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
checkDelete role call domain slot = do
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
      expected <- releaseOutcome (semaphoreBefore call) task
      byteAt (task + 0x0c) (semaphoreAfter call) >>= expect FamilyLifetime . (== expected)

checkSignal :: SemaphoreCall c => c -> Domain -> Word32 -> Either FamilyRefusal ()
checkSignal call domain slot = do
  let control = controlAddress domain slot
  first <- wordAt control (semaphoreBefore call)
  if first == control
    then do
      count <- wordAt (control + 0x0c) (semaphoreBefore call)
      maximumCount <- wordAt (configAddress domain slot + 8) (semaphoreBefore call)
      if count < maximumCount
        then do
          expect FamilyReturn (semaphoreR0 call == 0)
          wordAt (control + 0x0c) (semaphoreAfter call) >>= expect FamilyEffect . (== count + 1)
        else expect FamilyReturn (semaphoreR0 call == statusOverflow)
    else do
      expect FamilyReturn (semaphoreR0 call == 0)
      wait <- waiterWait (semaphoreBefore call) first
      wordAt wait (semaphoreAfter call) >>= expect FamilyLifetime . (== 0)
      expected <- releaseOutcome (semaphoreBefore call) first
      byteAt (first + 0x0c) (semaphoreAfter call) >>= expect FamilyLifetime . (== expected)
      next <- wordAt first (semaphoreBefore call)
      wordAt control (semaphoreAfter call) >>= expect FamilyEffect . (== next)

releaseOutcome :: Memory -> Word32 -> Either FamilyRefusal Word32
releaseOutcome memory task = do
  state <- byteAt (task + 0x0c) memory
  flags <- byteAt (task + 0x0e) memory
  terminating <- wordAt (task + 0x24) memory
  pure $
    if terminating /= 0 then (if flags .&. 0x01 == 0 then 0 else 1)
    else if state .&. 0x04 /= 0 then 4 else 1

checkPoll :: SemaphoreCall c => c -> Domain -> Word32 -> Either FamilyRefusal ()
checkPoll call domain slot = do
  let control = controlAddress domain slot
  count <- wordAt (control + 0x0c) (semaphoreBefore call)
  if count > 0
    then do
      expect FamilyReturn (semaphoreR0 call == 0)
      wordAt (control + 0x0c) (semaphoreAfter call) >>= expect FamilyEffect . (== count - 1)
    else do
      expect FamilyReturn (semaphoreR0 call == statusTimeout)
      wordAt (control + 0x0c) (semaphoreAfter call) >>= expect FamilyEffect . (== count)

taskSlotOf :: Domain -> Word32 -> Maybe Word32
taskSlotOf domain task =
  let base = case domain of DomainA -> 0x4005ff0c; DomainB -> 0x400efecc
      offset = task - base
   in if task >= base && offset `mod` 0x38 == 0 && offset `div` 0x38 < 120 then Just (offset `div` 0x38 + 1) else Nothing

checkWait :: SemaphoreCall c => c -> Domain -> Domain -> Word32 -> Bool -> FamilyTrace -> Either FamilyRefusal ()
checkWait call entryDomain object slot timed trace = do
  let control = controlAddress object slot
      task = semaphoreTask call
      saved = any ((== FamilySaveKind) . familyBoundaryKind) (familyTraceHistory trace)
  count <- wordAt (control + 0x0c) (semaphoreBefore call)
  timeout <- if timed then argAt (semaphoreArguments call) 1 else Right 0xffffffff
  if timed && timeout == 0
    then do
      expect FamilyLifetime (not saved)
      if count == 0
        then do
          expect FamilyReturn (semaphoreR0 call == statusTimeout)
          wordAt (control + 0x0c) (semaphoreAfter call) >>= expect FamilyEffect . (== count)
        else do
          expect FamilyReturn (semaphoreR0 call == 0)
          wordAt (control + 0x0c) (semaphoreAfter call) >>= expect FamilyEffect . (== count - 1)
    else if not saved
      then do
        expect FamilyReturn (count > 0)
        expect FamilyReturn (semaphoreR0 call == 0)
        wordAt (control + 0x0c) (semaphoreAfter call) >>= expect FamilyEffect . (== count - 1)
      else do
        taskSlot <- maybe (Left FamilyAssociation) Right (taskSlotOf entryDomain task)
        let wait = waitAddress entryDomain taskSlot
        expect FamilyLifetime (semaphoreR0 call `elem` [0, statusReleased, statusTimeout])
        wordAt wait (semaphoreAfter call) >>= expect FamilyLifetime . (== semaphoreR0 call)
        byteAt (task + 0x0c) (semaphoreAfter call) >>= expect FamilyLifetime . (\state -> state .&. 1 /= 0)
        when (semaphoreR0 call == 0) $ do
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
  case (familyTraceBefore trace, familyTraceAfter trace) of
    (beforeRegisters, afterRegisters) | length beforeRegisters == 16 && length afterRegisters == 16 -> do
      expect FamilyContinuation (all (\register -> beforeRegisters !! register == afterRegisters !! register) [4..11])
      expect FamilyContinuation (familyTraceBeforeCpsr trace .&. 0xc0 == familyTraceAfterCpsr trace .&. 0xc0)
    _ -> Left FamilyContinuation

checkTimer :: SemaphoreCall c => c -> [TimerWitness] -> FamilyTrace -> SemaphoreService -> Either FamilyRefusal ()
checkTimer call witnesses trace service = do
  let history = familyTraceHistory trace
      domain = semaphoreDomain call
      task = semaphoreTask call
      epoch = semaphoreEpoch call
      returnAt = maximum (0 : map familyBoundaryInstructions history)
      ownInserts = [witness | witness <- witnesses, timerEpoch witness == epoch, timerKind witness == TimerInserted]
      ownEvents = [witness | witness <- witnesses, timerEpoch witness == epoch, timerKind witness `elem` [TimerRemoved, TimerConsumed]]
      earlier witness = [insert | insert <- witnesses, timerKind insert == TimerInserted
        , timerEntry insert == timerEntry witness
        , timerArmedAt insert <= timerArmedAt witness]
      armStart = case ownInserts of
        (witness:_) -> timerArmedAt witness
        [] -> case [timerArmedAt witness | witness <- witnesses, timerKind witness == TimerInserted
                    , timerTask witness == task, timerArmedAt witness <= returnAt] of
          [] -> 0
          values -> minimum values
      within witness = timerArmedAt witness >= armStart && timerArmedAt witness <= returnAt
      inserts = ownInserts ++ [witness | witness <- witnesses, timerKind witness == TimerInserted
        , timerTask witness == task, within witness]
      retire insert = [witness | witness <- witnesses, timerKind witness `elem` [TimerRemoved, TimerConsumed]
        , timerEntry witness == timerEntry insert, timerArmedAt witness >= timerArmedAt insert]
      expiry = [witness | witness <- witnesses, timerKind witness == TimerConsumed
        , timerTask witness == task, not (null (earlier witness))]
      saved = any ((== FamilySaveKind) . familyBoundaryKind) history
  forM_ ownInserts $ \witness -> do
    expect FamilyTimer (isSemaphoreTimer domain (timerEntry witness))
    wordAt (timerEntry witness + 4) (semaphoreAfter call) >>= expect FamilyTimer . (== timeoutCallback domain)
    wordAt (timerEntry witness + 8) (semaphoreAfter call) >>= expect FamilyAssociation . (== task)
  forM_ ownEvents $ \witness -> expect FamilyTimer (not (null (earlier witness)))
  case service of
    SemaphoreTimedWait -> do
      timeout <- argAt (semaphoreArguments call) 1
      let positive = signedWord timeout > 0
      when (positive && saved) $ do
        expect FamilyTimer (not (null inserts))
        forM_ inserts $ \insert -> expect FamilyTimer (not (null (retire insert)))
      let expired = semaphoreR0 call == statusTimeout && timeout /= 0
      expect FamilyTimer (timeout == 0 || not expired || not (null ownInserts))
      when expired $ expect FamilyTimer (not (null expiry))
    SemaphoreSignal -> pure ()
    SemaphoreInterruptSignal -> pure ()
    SemaphoreDelete -> pure ()
    _ -> expect FamilyTimer (null ownInserts && null ownEvents)

signedWord :: Word32 -> Int
signedWord value = if value .&. 0x80000000 /= 0 then fromIntegral value - 0x100000000 else fromIntegral value

-- | A semaphore wait still blocked at route end. Only the untimed wait and the negative
-- timed wait are forever; a finite timeout still armed fails closed.
checkPendingSemaphore :: [ReleaseWitness] -> Pending -> Either FamilyRefusal ()
checkPendingSemaphore releases pending = case (pendingSemantic pending, pendingTerminal pending) of
  (SemaphoreSemantic SemaphoreWait, Just terminal) -> foreverWait terminal False
  (SemaphoreSemantic SemaphoreTimedWait, Just terminal) -> foreverWait terminal True
  _ -> Left FamilyLifetime
  where
    domain = pendingDomain pending
    args = pendingArguments pending
    foreverWait terminal timed = do
      idWord <- argAt args 0
      (object, slot) <- resolve domain idWord
      let control = controlAddress object slot
          task = pendingTask pending
          objectDomainId = case object of DomainA -> 1; DomainB -> 2
      when timed $ do
        timeout <- argAt args 1
        expect FamilyLifetime (signedWord timeout < 0)
      wait <- wordAt (task + 0x14) terminal
      expect FamilyLifetime (wait /= 0)
      result <- wordAt wait terminal
      expect FamilyLifetime (result == 0)
      queue <- wordAt (wait + 4) terminal
      expect FamilyLifetime (queue == control)
      owner <- wordAt (wait + 8) terminal
      expect FamilyLifetime (owner == objectDomainId)
      state <- wordAt (task + 0x0c) terminal
      expect FamilyLifetime (state .&. 0xff == 0x32)
      members <- queueWalk terminal control
      expect FamilyLifetime (task `elem` members)
      let lockWord = if object == DomainA then 0x400545c4 else 0x400e4584
          schedulerLockAddress = if domain == DomainA then 0x400545c0 else 0x400e4580
      schedulerLock <- wordAt schedulerLockAddress terminal
      objectLock <- wordAt lockWord terminal
      saveCpsr <- maybe (Left FamilyLifetime) Right (pendingSaveCpsr pending)
      saveAt <- maybe (Left FamilyLifetime) Right (pendingSaveAt pending)
      expect FamilyLifetime (saveCpsr .&. 0x80 /= 0)
      expect FamilyLifetime (saveCpsr .&. 0x160 == pendingCpsr pending .&. 0x160)
      expect FamilyLifetime (pendingCpsr pending .&. 0x80 == 0)
      let unlocked lock = [witness | witness <- releases, releaseTask witness == task
            , releaseCore witness == pendingCore pending, releaseAt witness < saveAt
            , (Address lock,0) `elem` releaseWrites witness]
          acquired lock = [witness | witness <- releases, releaseTask witness /= task
            , releaseFirst witness >= saveAt, (Address lock,1) `elem` releaseWrites witness]
          lockOk lock value = not (null (unlocked lock)) && (value == 0 || (value == 1 && not (null (acquired lock))))
      expect FamilyLifetime (lockOk lockWord objectLock && lockOk schedulerLockAddress schedulerLock)
      savedSp <- wordAt (task + 0x18) terminal
      savedPc <- wordAt (task + 0x1c) terminal
      definition <- wordAt (task + 8) terminal
      stackSize <- wordAt (definition + 0x10) terminal
      stackBase <- wordAt (definition + 0x14) terminal
      let alias x = x .&. complement 0x40000000
          base = alias stackBase
          limit = base + stackSize
          saved = alias savedSp
          entrySp = alias (pendingSp pending)
          resumeEntry = if domain == DomainA then 0x43668 else 0xd3668
      expect FamilyLifetime (savedSp .&. 3 == 0 && pendingSp pending .&. 3 == 0)
      expect FamilyLifetime (stackSize >= 36 && stackSize <= 0x10000 && base /= 0)
      expect FamilyLifetime (saved >= base && saved + 36 <= limit)
      expect FamilyLifetime (entrySp >= base && entrySp < limit)
      expect FamilyLifetime (alias savedPc .&. complement 1 == alias resumeEntry .&. complement 1)
      continuation <- wordAt (savedSp + 32) terminal
      expect FamilyContinuation (inSemaphoreWindow domain continuation)

-- | Walks the terminal queue from its self-linked head and requires reciprocal links and a
-- closed cycle. A self-linked caller outside the queue is not a member.
queueWalk :: Memory -> Word32 -> Either FamilyRefusal [Word32]
queueWalk memory control = do
  first <- wordAt control memory
  expect FamilyLifetime (first == control || first /= 0)
  go (0 :: Int) first
  where
    go depth node
      | depth > 256 = Left FamilyDestructor
      | node == control = Right []
      | node == 0 = Left FamilyLifetime
      | otherwise = do
          next <- wordAt node memory
          prev <- wordAt (node + 4) memory
          expect FamilyLifetime (next /= 0 && prev /= 0)
          when (next == control) $ wordAt (control + 4) memory >>= expect FamilyLifetime . (== node)
          when (prev == control) $ wordAt control memory >>= expect FamilyLifetime . (== node)
          when (next /= control) $ wordAt (next + 4) memory >>= expect FamilyLifetime . (== node)
          when (prev /= control) $ wordAt prev memory >>= expect FamilyLifetime . (== node)
          rest <- go (depth + 1) next
          pure (node : rest)
