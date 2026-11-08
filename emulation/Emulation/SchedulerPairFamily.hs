{-# LANGUAGE OverloadedStrings #-}
-- | Family-specific observation checks. The call view is supplied by
-- 'Emulation.SchedulerPairCheck'; this module owns the flag/mailbox record
-- layout and the independent expected-state rules.
module Emulation.SchedulerPairFamily
  ( FamilyCall (..), FamilyTrace (..), FamilyBoundary (..), FamilyBoundaryKind (..), FamilyCompletion (..)
  , TimerKind (..), TimerWitness (..), Pending (..), ReleaseWitness (..), FamilyRefusal (..), checkFamilyCall, checkPendingFamily ) where

import Control.Monad (foldM, forM, forM_, unless, when)
import Data.Bits ((.&.), (.|.), complement, shiftL, shiftR)
import qualified Data.Map.Strict as M
import Data.Word (Word32, Word64)
import Emulation.SchedulerPairCatalog (Address (..), FamilyService (..), Semantic (..))
import Emulation.SchedulerPairModel (Memory)
import Replacement.RtosOwnFirst.Regions (Domain (..))

data FamilyCompletion = FamilyReturnedCompletion | FamilyDiscardedCompletion | FamilyDispatchedCompletion deriving (Eq,Show)
data FamilyBoundaryKind = FamilySaveKind | FamilyResumeKind | FamilyReturnKind | FamilyTimerInsertKind | FamilyTimerRemoveKind | FamilyTimerConsumeKind | FamilyOtherKind deriving (Eq,Show)
data FamilyBoundary = FamilyBoundary
  { familyBoundaryKind :: FamilyBoundaryKind
  , familyBoundaryAddress :: Address
  , familyBoundaryInstructions :: Word64
  , familyBoundaryCore :: Int
  , familyBoundaryRegisters :: [Word32]
  , familyBoundaryValue :: Maybe Word32
  , familyBoundaryTask :: Word32
  } deriving (Eq,Show)
data FamilyTrace = FamilyTrace
  { familyTraceCompletion :: FamilyCompletion
  , familyTraceBefore :: [Word32]
  , familyTraceAfter :: [Word32]
  , familyTraceBeforeCpsr :: Word32
  , familyTraceAfterCpsr :: Word32
  , familyTraceHistory :: [FamilyBoundary]
  } deriving (Eq,Show)

class FamilyCall c where
  familyDomain :: c -> Domain
  familyCore :: c -> Int
  familyArguments :: c -> [Word32]
  familyBefore :: c -> Memory
  familyAfter :: c -> Memory
  familyR0 :: c -> Word32
  familyWrites :: c -> [(Address,Int,Word32)]
  familyTask :: c -> Word32
  familyEpoch :: c -> Word64
  familyService :: c -> Maybe FamilyService
  familyTrace :: c -> Maybe FamilyTrace

data FamilyRefusal = FamilyReturn | FamilyEffect | FamilyLifetime | FamilyTimer | FamilyDestructor | FamilyContinuation | FamilyAssociation deriving (Eq,Show)

data TimerKind = TimerInserted | TimerRemoved | TimerConsumed deriving (Eq,Show)
data TimerWitness = TimerWitness
  { timerEntry :: Word32
  , timerTask :: Word32
  , timerEpoch :: Word64
  , timerArmedAt :: Word64
  , timerKind :: TimerKind
  } deriving (Eq,Show)

expect :: FamilyRefusal -> Bool -> Either FamilyRefusal ()
expect refusal valid = unless valid (Left refusal)

wordAt :: Word32 -> Memory -> Either FamilyRefusal Word32
wordAt address memory = maybe (Left FamilyEffect) Right (M.lookup (Address address) memory)

argAt :: [Word32] -> Int -> Either FamilyRefusal Word32
argAt arguments index = case drop index arguments of
  value:_ -> Right value
  [] -> Left FamilyReturn

statusInvalid :: Word32
statusInvalid = 0xffffffee
statusParam :: Word32
statusParam = 0xffffffef
statusAlreadyExists :: Word32
statusAlreadyExists = 0xffffffd7
statusNotCreated :: Word32
statusNotCreated = 0xffffffd6
statusOverflow :: Word32
statusOverflow = 0xffffffe4
statusReleased :: Word32
statusReleased = 0xffffffcf
statusTimeout :: Word32
statusTimeout = 0xffffffce

signedWord :: Word32 -> Int
signedWord value = if value .&. 0x80000000/=0 then fromIntegral value-0x100000000 else fromIntegral value

data FamilyLayout = FamilyLayout
  { layoutMailboxConfig :: Word32
  , layoutFlagConfig :: Word32
  , layoutFlagControl :: Word32
  , layoutMailboxControl :: Word32
  , layoutFlagTimer :: Word32
  , layoutMailboxTimer :: Word32
  }

layout :: Domain -> FamilyLayout
layout DomainA = FamilyLayout 0x4005d16c 0x4005d178 0x400624b4 0x40062c34 0x40055a3c 0x40057fbc
layout DomainB = FamilyLayout 0x400ed12c 0x400ed138 0x400f2474 0x400f2bf4 0x400e59fc 0x400e7f7c

familyWindows :: Domain -> [(Word32,Word32)]
familyWindows DomainA =
  [ (0x44568,0x44634),(0x44744,0x44800),(0x44800,0x448e4),(0x448e4,0x449c0),(0x480cc,0x4813c)
  , (0x4d39c,0x4d434),(0x4d434,0x4d49c),(0x4813c,0x4839c),(0x4839c,0x485c0),(0x485c0,0x486f8)
  , (0x486f8,0x489ac),(0x489ac,0x48b30),(0x48b30,0x48e38),(0x4d5bc,0x4d654),(0x4d654,0x4d6c4)
  , (0x4aa44,0x4ad48),(0x4ad48,0x4af9c),(0x4af9c,0x4b0e4),(0x4b0e4,0x4b38c)
  ]
familyWindows DomainB =
  [ (0xd4528,0xd45f4),(0xd4704,0xd47c0),(0xd47c0,0xd48a4),(0xd48a4,0xd4980),(0xd808c,0xd80fc)
  , (0xdd360,0xdd3f8),(0xdd3f8,0xdd460),(0xd80fc,0xd835c),(0xd835c,0xd8580),(0xd8580,0xd86b8)
  , (0xd86b8,0xd896c),(0xd896c,0xd8af0),(0xd8af0,0xd8df8),(0xdd580,0xdd618),(0xdd618,0xdd688)
  , (0xdaa04,0xdad08),(0xdad08,0xdaf5c),(0xdaf5c,0xdb0a4),(0xdb0a4,0xdb34c)
  ]

inFamilyWindow :: Domain -> Word32 -> Bool
inFamilyWindow domain pc = any (\(lo,hi) -> pc>=lo && pc<hi) (familyWindows domain)

-- | A family call still open at route end, with its entry and terminal words.
data Pending = Pending
  { pendingSemantic :: Semantic
  , pendingDomain :: Domain
  , pendingArguments :: [Word32]
  , pendingBefore :: Memory
  , pendingTerminal :: Maybe Memory
  , pendingTask :: Word32
  , pendingSp :: Word32
  , pendingCpsr :: Word32
  , pendingEntry :: Word32
  , pendingEntered :: Word64
  , pendingSaveCpsr :: Maybe Word32
  } deriving (Eq,Show)

-- | A completed release call's task and the addresses it wrote.
data ReleaseWitness = ReleaseWitness { releaseTask :: Word32, releaseAddresses :: [Address] } deriving (Eq,Show)

-- | Only a forever timed flag wait is modeled; every other pending family call fails closed.
checkPendingFamily :: [ReleaseWitness] -> Pending -> Either FamilyRefusal ()
checkPendingFamily releases pending = case (pendingSemantic pending, pendingTerminal pending) of
  (FamilySemantic FlagTimedWait, Just terminal) -> foreverTimedWait terminal
  _ -> Left FamilyLifetime
  where
    domain = pendingDomain pending
    args = pendingArguments pending
    before = pendingBefore pending
    foreverTimedWait terminal = do
      idWord <- argAt args 0
      out <- argAt args 3
      (object, slot) <- resolveFlag domain idWord
      let family = layout object
          control = flagControl family slot
          task = pendingTask pending
      timeout <- wordAt (pendingSp pending) before
      expect FamilyLifetime (signedWord timeout == -1)
      patternBefore <- wordAt (control+0x0c) before
      want <- argAt args 1
      mode <- argAt args 2
      expect FamilyReturn (mode .&. 0xfffffffe == 0)
      expect FamilyLifetime (not (matches mode want patternBefore))
      wait <- wordAt (task+0x14) terminal
      expect FamilyLifetime (wait /= 0)
      queueWord <- wordAt (wait+4) terminal
      expect FamilyLifetime (queueWord == control)
      timer <- wordAt wait terminal
      expect FamilyTimer (timer == 0)
      state <- wordAt (task+0x0c) terminal
      expect FamilyLifetime (state .&. 0xff == 0x32)
      headWord <- wordAt control terminal
      prevWord <- wordAt (control+4) terminal
      nextWord <- wordAt task terminal
      taskPrev <- wordAt (task+4) terminal
      expect FamilyLifetime (headWord == task && prevWord == task && nextWord == control && taskPrev == control)
      beforeOut <- wordAt out before
      afterOut <- wordAt out terminal
      expect FamilyEffect (beforeOut == afterOut)
      schedulerLock <- wordAt (if domain == DomainA then 0x400545c0 else 0x400e4580) terminal
      objectLock <- wordAt (if object == DomainA then 0x400545c4 else 0x400e4584) terminal
      expect FamilyLifetime (schedulerLock == 0 && objectLock == 0)
      saveCpsr <- maybe (Left FamilyLifetime) Right (pendingSaveCpsr pending)
      expect FamilyLifetime (saveCpsr .&. 0x80 /= 0)
      expect FamilyLifetime (saveCpsr .&. complement 0x80 == pendingCpsr pending .&. complement 0x80)
      let own = [w | w <- releases, releaseTask w == task, Address objectLock `elem` releaseAddresses w]
          elsewhere = [w | w <- releases, releaseTask w /= task, Address objectLock `elem` releaseAddresses w]
      expect FamilyLifetime (not (null own) && null elsewhere)
      savedSp <- wordAt (task+0x18) terminal
      savedPc <- wordAt (task+0x1c) terminal
      stackBase <- wordAt (task+0x20) terminal
      stackSize <- wordAt (task+0x24) terminal
      let base = stackBase .&. complement 3
          limit = base + stackSize
          saved = savedSp .&. complement 3
          entrySp = pendingSp pending .&. complement 3
          resumeEntry = if domain == DomainA then 0x43668 else 0xd3668
      expect FamilyLifetime (stackSize >= 36 && stackSize <= 0x10000 && base /= 0)
      expect FamilyLifetime (saved >= base && saved + 36 <= limit)
      expect FamilyLifetime (entrySp >= base && entrySp < limit)
      expect FamilyLifetime (savedPc .&. complement 1 == resumeEntry .&. complement 1)
      continuation <- wordAt (saved+32) terminal
      expect FamilyContinuation (inFamilyWindow domain continuation)

flagConfig :: FamilyLayout -> Word32 -> Word32
flagConfig family slot = layoutFlagConfig family + (slot-1)*12
flagControl :: FamilyLayout -> Word32 -> Word32
flagControl family slot = layoutFlagControl family + (slot-1)*16
mailboxConfig :: FamilyLayout -> Word32 -> Word32
mailboxConfig family slot = layoutMailboxConfig family + (slot-1)*12
mailboxControl :: FamilyLayout -> Word32 -> Word32
mailboxControl family slot = layoutMailboxControl family + (slot-1)*20

data TimerPath = TimerPath
  { timerArm :: [Word32]
  , timerWake :: [Word32]
  , timerExpire :: [Word32]
  , timerInsertRequired :: Bool
  , timerRemoveAll :: Bool
  , timerExpireAll :: Bool
  , timerForbid :: Bool
  } deriving (Eq,Show)

noTimer :: TimerPath
noTimer = TimerPath [] [] [] False False False False
forbidTimer :: TimerPath
forbidTimer = TimerPath [] [] [] False False False True

data FamilyExpected = FamilyExpected
  { expectedR0 :: Word32
  , expectedWords :: [(Word32,Word32)]
  , expectedOutput :: Maybe (Word32,Word32)
  , expectedTimer :: TimerPath
  } deriving (Eq,Show)

errorExpected :: Word32 -> FamilyExpected
errorExpected code = FamilyExpected code [] Nothing forbidTimer

replayWrite :: Memory -> (Address,Int,Word32) -> Either FamilyRefusal Memory
replayWrite memory (Address address,size,value) = do
  expect FamilyEffect (size `elem` [1,2,4] && address `mod` fromIntegral size == 0)
  let aligned = address .&. complement 3
  old <- wordAt aligned memory
  let shift = fromIntegral (address .&. 3)*8
      mask = if size==4 then maxBound else (1 `shiftL` (size*8))-1
  expect FamilyEffect (value .&. complement mask == 0)
  pure (M.insert (Address aligned) ((old .&. complement (mask `shiftL` shift)) .|. (value `shiftL` shift)) memory)

checkFamilyCall :: FamilyCall c => c -> [TimerWitness] -> Either FamilyRefusal ()
checkFamilyCall call witnesses = do
  trace <- maybe (Left FamilyContinuation) Right (familyTrace call)
  expect FamilyContinuation (familyTraceCompletion trace==FamilyReturnedCompletion)
  replayed <- foldM replayWrite (familyBefore call) (familyWrites call)
  expect FamilyEffect (replayed==familyAfter call)
  expected <- familyExpected call
  forM_ (expectedWords expected) $ \(address,value) -> wordAt address (familyAfter call) >>= expect FamilyEffect . (==value)
  forM_ (expectedOutput expected) $ \(address,value) -> wordAt address (familyAfter call) >>= expect FamilyEffect . (==value)
  expect FamilyReturn (familyR0 call==expectedR0 expected)
  checkContinuation call trace
  checkTimerPath call witnesses expected trace

checkContinuation :: FamilyCall c => c -> FamilyTrace -> Either FamilyRefusal ()
checkContinuation call trace = do
  let history = familyTraceHistory trace
      task = familyTask call
      core = familyCore call
      ownedKinds = [FamilySaveKind,FamilyResumeKind,FamilyReturnKind]
      mine = filter (\boundary -> familyBoundaryKind boundary `elem` ownedKinds && not (isTickPop (familyDomain call) boundary)) history
  forM_ mine $ \boundary -> expect FamilyAssociation (familyBoundaryCore boundary==core && familyBoundaryTask boundary==task)
  forM_ (filter (\boundary -> familyBoundaryKind boundary==FamilySaveKind) history) $ \boundary ->
    case familyBoundaryRegisters boundary of
      registers | length registers==16 -> expect FamilyContinuation (inFamilyWindow (familyDomain call) (registers !! 14))
      _ -> Left FamilyContinuation
  forM_ (filter (\boundary -> familyBoundaryKind boundary==FamilyResumeKind) history) $ \boundary ->
    case familyBoundaryRegisters boundary of
      registers | length registers==16 -> expect FamilyContinuation (inFamilyWindow (familyDomain call) (registers !! 15))
      _ -> Left FamilyContinuation
  case ([boundary | boundary <- history,familyBoundaryKind boundary==FamilySaveKind],[boundary | boundary <- history,familyBoundaryKind boundary==FamilyResumeKind]) of
    (saved:_,resumed:_) -> case (familyBoundaryRegisters saved,familyBoundaryRegisters resumed) of
      (savedRegisters,resumedRegisters) | length savedRegisters==16 && length resumedRegisters==16 -> do
        expect FamilyContinuation (resumedRegisters !! 13 <= savedRegisters !! 13)
        expect FamilyContinuation (familyBoundaryInstructions saved < familyBoundaryInstructions resumed)
      _ -> Left FamilyContinuation
    _ -> pure ()
  case familyTraceAfter trace of
    first : rest | length rest == 15 -> expect FamilyReturn (first==familyR0 call)
    _ -> Left FamilyContinuation
  case (familyTraceBefore trace, familyTraceAfter trace) of
    (beforeRegisters, afterRegisters) | length beforeRegisters==16 && length afterRegisters==16 -> do
      expect FamilyContinuation (all (\register -> beforeRegisters !! register == afterRegisters !! register) [4..11])
      expect FamilyContinuation (familyTraceBeforeCpsr trace .&. 0xc0 == familyTraceAfterCpsr trace .&. 0xc0)
    _ -> Left FamilyContinuation
checkTimerPath :: FamilyCall c => c -> [TimerWitness] -> FamilyExpected -> FamilyTrace -> Either FamilyRefusal ()
checkTimerPath call witnesses expected trace = do
  let history = familyTraceHistory trace
      task = familyTask call
      ownInserts = [(value,boundary) | boundary <- history,familyBoundaryKind boundary==FamilyTimerInsertKind,Just value <- [familyBoundaryValue boundary]]
      ownRemoves = [(value,boundary) | boundary <- history,familyBoundaryKind boundary==FamilyTimerRemoveKind,Just value <- [familyBoundaryValue boundary]]
      ownConsumes = [(value,boundary) | boundary <- history,familyBoundaryKind boundary==FamilyTimerConsumeKind,Just value <- [familyBoundaryValue boundary]]
      returnAt = maximum (0 : map familyBoundaryInstructions history)
      armStart = case ownInserts of
        ((_,boundary):_) -> familyBoundaryInstructions boundary
        [] -> case [timerArmedAt w | w <- witnesses, timerKind w==TimerInserted, timerTask w==task, timerArmedAt w <= returnAt] of
          [] -> 0
          values -> minimum values
      within w = timerArmedAt w >= armStart && timerArmedAt w <= returnAt
      inserts = map fst ownInserts ++ [timerEntry w | w <- witnesses, timerKind w==TimerInserted, timerTask w==task, within w]
      removes = map fst ownRemoves ++ [timerEntry w | w <- witnesses, timerKind w==TimerRemoved, timerTask w==task, within w]
      consumes = map fst ownConsumes ++ [timerEntry w | w <- witnesses, timerKind w==TimerConsumed, timerTask w==task, within w]
      path = expectedTimer expected
  forM_ ownInserts $ \(value,boundary) -> checkTimerBoundary call value boundary True
  forM_ ownRemoves $ \(value,boundary) -> checkTimerBoundary call value boundary False
  when (timerForbid path) $ expect FamilyTimer (null ownInserts && null ownRemoves && null [w | w <- witnesses, timerTask w==task, within w])
  forM_ (timerArm path) $ \value -> expect FamilyTimer (value `elem` inserts)
  forM_ (timerWake path) $ \value -> expect FamilyTimer (value `elem` removes)
  forM_ (timerExpire path) $ \value -> expect FamilyTimer (value `elem` removes || value `elem` consumes)
  when (timerInsertRequired path) $ expect FamilyTimer (not (null inserts))
  unless (timerInsertRequired path) $ forM_ (map fst ownInserts) $ \value -> expect FamilyTimer (value `elem` timerArm path)
  when (timerRemoveAll path) $ forM_ (map fst ownInserts) $ \value -> expect FamilyTimer (value `elem` removes)
  when (timerExpireAll path) $ forM_ (map fst ownInserts) $ \value -> expect FamilyTimer (value `elem` removes || value `elem` consumes)
  let allowed = timerWake path ++ timerExpire path ++ inserts
  forM_ (map fst ownRemoves) $ \value -> expect FamilyTimer (value `elem` allowed)
  forM_ (map fst ownConsumes) $ \value -> expect FamilyTimer (value `elem` allowed)

checkTimerBoundary :: FamilyCall c => c -> Word32 -> FamilyBoundary -> Bool -> Either FamilyRefusal ()
checkTimerBoundary call value boundary inserting = do
  expect FamilyTimer (isTimerEntry (familyDomain call) value)
  case familyBoundaryRegisters boundary of
    registers | length registers>1 -> expect FamilyTimer (registers !! 1==value)
    _ -> Left FamilyTimer
  entryTask <- if inserting
    then wordAt (value+8) (familyAfter call)
    else wordAt (value+8) (familyBefore call)
  expect FamilyAssociation (familyBoundaryTask boundary==familyTask call || entryTask==familyTask call)
  expect FamilyAssociation (familyBoundaryCore boundary==familyCore call || entryTask==familyTask call)
  if inserting then expect FamilyTimer (entryTask==familyTask call) else expect FamilyTimer (entryTask/=0)

isTimerEntry :: Domain -> Word32 -> Bool
isTimerEntry _ value =
  any table [DomainA,DomainB]
 where
  table domain =
    let family = layout domain
    in inTable (layoutFlagTimer family) 120 value || inTable (layoutMailboxTimer family) 120 value
  inTable base count address = address>=base && address<base+count*12 && (address-base) `mod` 12==0

isTickPop :: Domain -> FamilyBoundary -> Bool
isTickPop _ boundary = case familyBoundaryAddress boundary of
  Address address -> address `elem` [0x44d8c,0xd4d4c]

familyExpected :: FamilyCall c => c -> Either FamilyRefusal FamilyExpected
familyExpected call = do
  service <- maybe (Left FamilyReturn) Right (familyService call)
  case service of
    FamilyRelease -> carrierRelease call
    HelperRelease -> carrierRelease call
    HelperBlock -> carrierBlock call
    HelperBlockTimed -> carrierBlockTimed call
    HelperCheck -> carrierCheck call
    FlagCreate -> flagCreate call
    FlagDelete -> flagDelete call
    FlagSet -> flagSet call
    FlagSetHandler -> flagSet call
    FlagClear -> flagClear call
    FlagWait -> flagWaitLike call False False
    FlagPoll -> flagWaitLike call True False
    FlagTimedWait -> flagWaitLike call False True
    MailboxCreate -> mailboxCreate call
    MailboxDelete -> mailboxDelete call
    MailboxSend -> mailboxSend call
    MailboxReceive -> mailboxReceiveLike call False False
    MailboxPoll -> mailboxReceiveLike call True False
    MailboxTimedReceive -> mailboxReceiveLike call False True

resolveFlag :: Domain -> Word32 -> Either FamilyRefusal (Domain,Word32)
resolveFlag entry idWord = do
  let slot = idWord .&. 0xffff
  domain <- case idWord `shiftR` 16 of
    0 -> Right entry
    1 -> Right DomainA
    2 -> Right DomainB
    _ -> Left FamilyReturn
  expect FamilyReturn (slot>=1 && slot<=120)
  pure (domain,slot)

resolveMailbox :: Domain -> Word32 -> Either FamilyRefusal (Domain,Word32)
resolveMailbox entry idWord = do
  let slot = idWord .&. 0xffff
  domain <- case idWord `shiftR` 16 of
    0 -> Right entry
    1 -> Right DomainA
    2 -> Right DomainB
    _ -> Left FamilyReturn
  expect FamilyReturn (slot==1)
  pure (domain,slot)

resolveOwnFlag :: Domain -> Word32 -> Either FamilyRefusal (Domain,Word32)
resolveOwnFlag entry idWord = do
  let slot = idWord .&. 0xffff
  expect FamilyReturn (slot>=1 && slot<=120)
  pure (entry,slot)

resolveOwnMailbox :: Domain -> Word32 -> Either FamilyRefusal (Domain,Word32)
resolveOwnMailbox entry idWord = do
  let slot = idWord .&. 0xffff
  expect FamilyReturn (slot==1)
  pure (entry,slot)

matches :: Word32 -> Word32 -> Word32 -> Bool
matches mode want pattern'
  | mode .&. 0xfffffffe/=0 = False
  | mode .&. 1/=0 = want .&. pattern'/=0
  | otherwise = want .&. pattern'==want

queueWaiters :: Memory -> Word32 -> Either FamilyRefusal [(Word32,Word32)]
queueWaiters memory control = wordAt control memory >>= go (0 :: Int)
 where
  go depth task
    | depth>256 = Left FamilyDestructor
    | task==control = Right []
    | otherwise = do
        wait <- wordAt (task+0x14) memory
        next <- wordAt task memory
        rest <- go (depth+1) next
        pure ((task,wait):rest)

messageChain :: Memory -> Word32 -> Either FamilyRefusal [Word32]
messageChain memory headMessage = go (0 :: Int) headMessage
 where
  go depth message
    | depth>256 = Left FamilyDestructor
    | message==0 = Right []
    | otherwise = do
        next <- wordAt message memory
        rest <- go (depth+1) next
        pure (message:rest)

carrierRelease :: FamilyCall c => c -> Either FamilyRefusal FamilyExpected
carrierRelease call = do
  task <- argAt (familyArguments call) 1
  wait <- wordAt (task+0x14) (familyBefore call)
  timer <- wordAt wait (familyBefore call)
  pure FamilyExpected
    { expectedR0=familyR0 call
    , expectedWords=[(wait,0)]
    , expectedOutput=Nothing
    , expectedTimer=if timer==0 then forbidTimer else TimerPath [] [timer] [] False False False False
    }

carrierBlock :: FamilyCall c => c -> Either FamilyRefusal FamilyExpected
carrierBlock call = do
  queue <- argAt (familyArguments call) 1
  wait <- argAt (familyArguments call) 2
  pure FamilyExpected
    { expectedR0=familyR0 call
    , expectedWords=[(wait,0),(wait+4,queue),(familyTask call+0x14,wait)]
    , expectedOutput=Nothing
    , expectedTimer=forbidTimer
    }

carrierBlockTimed :: FamilyCall c => c -> Either FamilyRefusal FamilyExpected
carrierBlockTimed call = do
  queue <- argAt (familyArguments call) 1
  wait <- argAt (familyArguments call) 2
  timer <- argAt (familyArguments call) 3
  pure FamilyExpected
    { expectedR0=familyR0 call
    , expectedWords=[(wait,timer),(wait+4,queue),(timer+8,familyTask call),(familyTask call+0x14,wait)]
    , expectedOutput=Nothing
    , expectedTimer=TimerPath [timer] [] [] True False False False
    }

carrierCheck :: FamilyCall c => c -> Either FamilyRefusal FamilyExpected
carrierCheck call = do
  control <- argAt (familyArguments call) 0
  want <- argAt (familyArguments call) 1
  mode <- argAt (familyArguments call) 2
  out <- argAt (familyArguments call) 3
  patternBefore <- wordAt (control+0x0c) (familyBefore call)
  config <- wordAt (control+0x08) (familyBefore call)
  attributes <- wordAt config (familyBefore call)
  let satisfied = matches mode want patternBefore
      clear = attributes .&. 4/=0
  pure FamilyExpected
    { expectedR0=familyR0 call
    , expectedWords=if satisfied && clear then [(control+0x0c,0)] else []
    , expectedOutput=if satisfied then Just (out,patternBefore) else Nothing
    , expectedTimer=forbidTimer
    }

flagCreate :: FamilyCall c => c -> Either FamilyRefusal FamilyExpected
flagCreate call = do
  idWord <- argAt (familyArguments call) 0
  request <- argAt (familyArguments call) 1
  case resolveOwnFlag (familyDomain call) idWord of
    Left _ -> pure (errorExpected statusInvalid)
    Right (domain,slot) -> do
      let family = layout domain
          config = flagConfig family slot
          control = flagControl family slot
      exists <- wordAt (config+8) (familyBefore call)
      if exists/=0 then pure (errorExpected statusAlreadyExists) else do
        attributes <- wordAt request (familyBefore call)
        initial <- wordAt (request+4) (familyBefore call)
        pure FamilyExpected
          { expectedR0=0
          , expectedWords=[(config,attributes),(config+4,initial),(config+8,1),(control,control),(control+4,control),(control+0x0c,initial)]
          , expectedOutput=Nothing
          , expectedTimer=forbidTimer
          }

flagDelete :: FamilyCall c => c -> Either FamilyRefusal FamilyExpected
flagDelete call = do
  idWord <- argAt (familyArguments call) 0
  case resolveOwnFlag (familyDomain call) idWord of
    Left _ -> pure (errorExpected statusInvalid)
    Right (domain,slot) -> do
      let family = layout domain
          config = flagConfig family slot
          control = flagControl family slot
      waiters <- queueWaiters (familyBefore call) control
      afterQueue <- wordAt control (familyAfter call)
      expect FamilyDestructor (afterQueue==control)
      forM_ waiters $ \(_,wait) -> wordAt wait (familyAfter call) >>= expect FamilyDestructor . (==statusReleased)
      timers <- forM waiters $ \(_,wait) -> wordAt wait (familyBefore call)
      pure FamilyExpected
        { expectedR0=0
        , expectedWords=[(config,0),(config+4,0),(config+8,0),(control,control),(control+4,control),(control+0x0c,0)] ++ [(wait,statusReleased) | (_,wait) <- waiters]
        , expectedOutput=Nothing
        , expectedTimer=TimerPath [] [timer | timer <- timers,timer/=0] [] False False False False
        }

flagSet :: FamilyCall c => c -> Either FamilyRefusal FamilyExpected
flagSet call = do
  idWord <- argAt (familyArguments call) 0
  bits <- argAt (familyArguments call) 1
  case resolveFlag (familyDomain call) idWord of
    Left _ -> pure (errorExpected statusInvalid)
    Right (domain,slot) -> do
      let family = layout domain
          config = flagConfig family slot
          control = flagControl family slot
      exists <- wordAt (config+8) (familyBefore call)
      if exists==0 then pure (errorExpected statusNotCreated) else do
        patternBefore <- wordAt (control+0x0c) (familyBefore call)
        attributes <- wordAt config (familyBefore call)
        headWaiter <- wordAt control (familyBefore call)
        let patternAfter = patternBefore .|. bits
            clear = attributes .&. 4/=0
        if headWaiter==control then pure FamilyExpected
          { expectedR0=0
          , expectedWords=[(control+0x0c,patternAfter)]
          , expectedOutput=Nothing
          , expectedTimer=noTimer
          }
        else do
          wait <- wordAt (headWaiter+0x14) (familyBefore call)
          want <- wordAt (wait+0x0c) (familyBefore call)
          mode <- wordAt (wait+0x10) (familyBefore call)
          if not (matches mode want patternAfter) then pure FamilyExpected
            { expectedR0=0
            , expectedWords=[(control+0x0c,patternAfter)]
            , expectedOutput=Nothing
            , expectedTimer=noTimer
            }
          else do
            timer <- wordAt wait (familyBefore call)
            next <- wordAt headWaiter (familyBefore call)
            previous <- wordAt (headWaiter+4) (familyBefore call)
            pure FamilyExpected
              { expectedR0=0
              , expectedWords=[(control+0x0c,if clear then 0 else patternAfter),(wait+0x14,patternAfter),(wait,0),(previous,next),(next+4,previous)]
              , expectedOutput=Nothing
              , expectedTimer=if timer==0 then noTimer else TimerPath [] [timer] [] False False False False
              }

flagClear :: FamilyCall c => c -> Either FamilyRefusal FamilyExpected
flagClear call = do
  idWord <- argAt (familyArguments call) 0
  keep <- argAt (familyArguments call) 1
  case resolveFlag (familyDomain call) idWord of
    Left _ -> pure (errorExpected statusInvalid)
    Right (domain,slot) -> do
      let family = layout domain
          config = flagConfig family slot
          control = flagControl family slot
      exists <- wordAt (config+8) (familyBefore call)
      if exists==0 then pure (errorExpected statusNotCreated) else do
        patternBefore <- wordAt (control+0x0c) (familyBefore call)
        pure FamilyExpected
          { expectedR0=0
          , expectedWords=[(control+0x0c,patternBefore .&. keep)]
          , expectedOutput=Nothing
          , expectedTimer=forbidTimer
          }

flagWaitLike :: FamilyCall c => c -> Bool -> Bool -> Either FamilyRefusal FamilyExpected
flagWaitLike call pollOnly timed = do
  idWord <- argAt (familyArguments call) 0
  want <- argAt (familyArguments call) 1
  mode <- argAt (familyArguments call) 2
  out <- argAt (familyArguments call) 3
  case resolveFlag (familyDomain call) idWord of
    Left _ -> pure (errorExpected statusInvalid)
    Right (domain,slot) -> do
      let family = layout domain
          config = flagConfig family slot
          control = flagControl family slot
      timeout <- if pollOnly then pure 0 else if timed then stackTimeout call else pure 0xffffffff
      if signedWord timeout<(-1) then pure (errorExpected statusParam) else
        if want==0 || mode .&. 0xfffffffe/=0 then pure (errorExpected statusParam) else do
          exists <- wordAt (config+8) (familyBefore call)
          if exists==0 then pure (errorExpected statusNotCreated) else do
            headWaiter <- wordAt control (familyBefore call)
            if headWaiter/=control then pure (errorExpected statusOverflow) else do
              patternBefore <- wordAt (control+0x0c) (familyBefore call)
              attributes <- wordAt config (familyBefore call)
              let satisfied = matches mode want patternBefore
                  clear = attributes .&. 4/=0
              if satisfied then pure FamilyExpected
                { expectedR0=0
                , expectedWords=[(control+0x0c,if clear then 0 else patternBefore)]
                , expectedOutput=Just (out,patternBefore)
                , expectedTimer=forbidTimer
                }
              else if signedWord timeout==0 then pure FamilyExpected
                { expectedR0=statusTimeout
                , expectedWords=[]
                , expectedOutput=Nothing
                , expectedTimer=forbidTimer
                }
              else do
                let r0 = familyR0 call
                let allowed = if signedWord timeout==(-1) then [0,statusReleased] else [0,statusReleased,statusTimeout]
                expect FamilyReturn (r0 `elem` allowed)
                outputBefore <- wordAt out (familyBefore call)
                when (r0==0) $ wordAt out (familyAfter call) >>= \outputAfter -> expect FamilyEffect (outputAfter/=0)
                let timers
                      | signedWord timeout==(-1) = forbidTimer
                      | r0==0 || r0==statusReleased = TimerPath [] [] [] True True False False
                      | otherwise = TimerPath [] [] [] True False True False
                pure FamilyExpected
                  { expectedR0=r0
                  , expectedWords=if r0==0 then [] else [(out,outputBefore)]
                  , expectedOutput=Nothing
                  , expectedTimer=timers
                  }

stackTimeout :: FamilyCall c => c -> Either FamilyRefusal Word32
stackTimeout call = case familyTrace call of
  Just trace -> case familyTraceBefore trace of
    registers | length registers==16 -> wordAt (registers !! 13) (familyBefore call)
    _ -> Left FamilyContinuation
  Nothing -> Left FamilyContinuation

mailboxCreate :: FamilyCall c => c -> Either FamilyRefusal FamilyExpected
mailboxCreate call = do
  idWord <- argAt (familyArguments call) 0
  request <- argAt (familyArguments call) 1
  case resolveOwnMailbox (familyDomain call) idWord of
    Left _ -> pure (errorExpected statusInvalid)
    Right (domain,slot) -> do
      let family = layout domain
          config = mailboxConfig family slot
          control = mailboxControl family slot
      exists <- wordAt (config+8) (familyBefore call)
      if exists/=0 then pure (errorExpected statusAlreadyExists) else do
        attributes <- wordAt request (familyBefore call)
        limit <- wordAt (request+4) (familyBefore call)
        pure FamilyExpected
          { expectedR0=0
          , expectedWords=[(config,attributes),(config+4,limit),(config+8,1),(control,control),(control+4,control),(control+0x0c,0),(control+0x10,0)]
          , expectedOutput=Nothing
          , expectedTimer=forbidTimer
          }

mailboxDelete :: FamilyCall c => c -> Either FamilyRefusal FamilyExpected
mailboxDelete call = do
  idWord <- argAt (familyArguments call) 0
  case resolveOwnMailbox (familyDomain call) idWord of
    Left _ -> pure (errorExpected statusInvalid)
    Right (domain,slot) -> do
      let family = layout domain
          config = mailboxConfig family slot
          control = mailboxControl family slot
      waiters <- queueWaiters (familyBefore call) control
      headMessage <- wordAt (control+0x0c) (familyBefore call)
      messages <- messageChain (familyBefore call) headMessage
      afterHead <- wordAt (control+0x0c) (familyAfter call)
      afterTail <- wordAt (control+0x10) (familyAfter call)
      afterQueue <- wordAt control (familyAfter call)
      expect FamilyDestructor (afterHead==0 && afterTail==0 && afterQueue==control)
      forM_ waiters $ \(_,wait) -> wordAt wait (familyAfter call) >>= expect FamilyDestructor . (==statusReleased)
      forM_ messages $ \message -> do
        beforeNext <- wordAt message (familyBefore call)
        afterNext <- wordAt message (familyAfter call)
        expect FamilyDestructor (beforeNext==afterNext)
      timers <- forM waiters $ \(_,wait) -> wordAt wait (familyBefore call)
      pure FamilyExpected
        { expectedR0=0
        , expectedWords=[(config,2),(config+4,32),(config+8,0),(control,control),(control+4,control),(control+0x0c,0),(control+0x10,0)]
          ++ [(wait,statusReleased) | (_,wait) <- waiters]
          ++ [(message,before) | message <- messages,let before=fromMaybeZero (M.lookup (Address message) (familyBefore call))]
        , expectedOutput=Nothing
        , expectedTimer=TimerPath [] [timer | timer <- timers,timer/=0] [] False False False False
        }
 where
  fromMaybeZero (Just value) = value
  fromMaybeZero Nothing = 0

mailboxSend :: FamilyCall c => c -> Either FamilyRefusal FamilyExpected
mailboxSend call = do
  idWord <- argAt (familyArguments call) 0
  message <- argAt (familyArguments call) 1
  case resolveMailbox (familyDomain call) idWord of
    Left _ -> pure (errorExpected statusInvalid)
    Right (domain,slot) -> do
      let family = layout domain
          config = mailboxConfig family slot
          control = mailboxControl family slot
      exists <- wordAt (config+8) (familyBefore call)
      if exists==0 then pure (errorExpected statusNotCreated) else do
        attributes <- wordAt config (familyBefore call)
        limit <- wordAt (config+4) (familyBefore call)
        let priorityOrder = attributes .&. 2/=0
        priority <- if priorityOrder then wordAt (message+4) (familyBefore call) else pure 0
        let signedPriority = signedWord priority
            signedLimit = signedWord limit
        if priorityOrder && (signedPriority<=0 || signedPriority>signedLimit)
          then pure (errorExpected statusParam)
          else do
            headWaiter <- wordAt control (familyBefore call)
            if headWaiter/=control then do
              wait <- wordAt (headWaiter+0x14) (familyBefore call)
              timer <- wordAt wait (familyBefore call)
              next <- wordAt headWaiter (familyBefore call)
              previous <- wordAt (headWaiter+4) (familyBefore call)
              pure FamilyExpected
                { expectedR0=0
                , expectedWords=[(wait+0x0c,message),(wait,0),(previous,next),(next+4,previous)]
                , expectedOutput=Nothing
                , expectedTimer=if timer==0 then noTimer else TimerPath [] [timer] [] False False False False
                }
            else do
              headMessage <- wordAt (control+0x0c) (familyBefore call)
              tailMessage <- wordAt (control+0x10) (familyBefore call)
              inserted <- if priorityOrder
                then priorityInsert (familyBefore call) control priority message
                else pure [(message,0)]
              let appended
                    | priorityOrder = []
                    | headMessage==0 = [(control+0x0c,message),(control+0x10,message)]
                    | otherwise = [(tailMessage,message),(control+0x10,message)]
              pure FamilyExpected
                { expectedR0=0
                , expectedWords=inserted ++ appended ++ [(message+4,priority) | priorityOrder]
                , expectedOutput=Nothing
                , expectedTimer=noTimer
                }

priorityInsert :: Memory -> Word32 -> Word32 -> Word32 -> Either FamilyRefusal [(Word32,Word32)]
priorityInsert memory control priority message = do
  headMessage <- wordAt (control+0x0c) memory
  go (control+0x0c) headMessage
 where
  candidate = signedWord priority
  go link next
    | next==0 = pure [(message,0),(link,message)]
    | otherwise = do
        existing <- wordAt (next+4) memory
        if candidate < signedWord existing
          then pure [(message,next),(link,message)]
          else wordAt next memory >>= go next

mailboxReceiveLike :: FamilyCall c => c -> Bool -> Bool -> Either FamilyRefusal FamilyExpected
mailboxReceiveLike call pollOnly timed = do
  idWord <- argAt (familyArguments call) 0
  out <- argAt (familyArguments call) 1
  case resolveMailbox (familyDomain call) idWord of
    Left _ -> pure (errorExpected statusInvalid)
    Right (domain,slot) -> do
      let family = layout domain
          config = mailboxConfig family slot
          control = mailboxControl family slot
      timeout <- if pollOnly then pure 0 else if timed then argAt (familyArguments call) 2 else pure 0xffffffff
      if signedWord timeout<(-1) then pure (errorExpected statusParam) else do
        exists <- wordAt (config+8) (familyBefore call)
        if exists==0 then pure (errorExpected statusNotCreated) else do
          headMessage <- wordAt (control+0x0c) (familyBefore call)
          if headMessage/=0 then do
            next <- wordAt headMessage (familyBefore call)
            pure FamilyExpected
              { expectedR0=0
              , expectedWords=[(control+0x0c,next)]
              , expectedOutput=Just (out,headMessage)
              , expectedTimer=forbidTimer
              }
          else if signedWord timeout==0 then pure FamilyExpected
            { expectedR0=statusTimeout
            , expectedWords=[]
            , expectedOutput=Nothing
            , expectedTimer=forbidTimer
            }
          else do
            let r0 = familyR0 call
            let allowed = if signedWord timeout==(-1) then [0,statusReleased] else [0,statusReleased,statusTimeout]
            expect FamilyReturn (r0 `elem` allowed)
            outputBefore <- wordAt out (familyBefore call)
            when (r0==0) $ wordAt out (familyAfter call) >>= \outputAfter -> expect FamilyEffect (outputAfter/=0)
            let timers
                  | signedWord timeout==(-1) = forbidTimer
                  | r0==0 || r0==statusReleased = TimerPath [] [] [] True True False False
                  | otherwise = TimerPath [] [] [] True False True False
            pure FamilyExpected
              { expectedR0=r0
              , expectedWords=if r0==0 then [] else [(out,outputBefore)]
              , expectedOutput=Nothing
              , expectedTimer=timers
              }
