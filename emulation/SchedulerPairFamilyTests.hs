{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import qualified Data.ByteString.Char8 as B
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.SchedulerPairCheck
import Emulation.SchedulerPairFamily (TimerKind (..), TimerWitness (..))
import Emulation.SchedulerPairModel (Arguments (..), Memory)
import Emulation.SchedulerPairPlan
import qualified Emulation.SchedulerPairS2 as D
import Replacement.RtosOwnFirst.Regions (Domain (..))
import Replacement.RtosScheduler.Regions (Helper (..))

assert :: String -> Bool -> IO ()
assert label value = unless value (error label)

assertLeft :: String -> Refusal -> Either Refusal () -> IO ()
assertLeft label expected result = case result of
  Left actual | actual==expected -> pure ()
  Left actual -> error (label ++ ": expected " ++ show expected ++ ", got " ++ show actual)
  Right () -> error (label ++ ": expected refusal")

assertRight :: String -> Either Refusal () -> IO ()
assertRight label result = case result of
  Right () -> pure ()
  Left actual -> error (label ++ ": unexpected " ++ show actual)

registers :: [Word32] -> D.Registers
registers values = D.Registers (take 16 (values ++ repeat 0)) 0x13 0 0 0

withStack :: Word32 -> Word32 -> [Word32]
withStack sp r0 = [r0,0,0,0,0,0,0,0,0,0,0,0,0,sp,0,0]

traceOf :: [Word32] -> [Boundary] -> Completion -> CallTrace
traceOf values history completion = CallTrace (registers values) (registers values) completion history Nothing

callOf :: Domain -> Word32 -> Semantic -> [Word32] -> Memory -> Memory -> Word32 -> CallTrace -> [Written] -> Call
callOf domain address semantic arguments before after r0 trace writes =
  Call domain (Address address) semantic (if domain==DomainA then 0 else 1) 0x13 (Arguments arguments)
    before after r0 0x13 writes [] 1 10 (Just trace) 0x9000 1 Nothing [] []

applyWrites :: Memory -> [Written] -> Memory
applyWrites = foldl (\memory (Written (Address address) _ value) -> M.insert (Address address) value memory)

flagConfigAddress, flagControlAddress, flagWaitAddress, flagTimerAddress :: Word32
flagConfigAddress = 0x4005d178
flagControlAddress = 0x400624b4
flagWaitAddress = 0x40055fdc
flagTimerAddress = 0x40055a3c

mailboxConfigAddress, mailboxControlAddress :: Word32
mailboxConfigAddress = 0x4005d16c
mailboxControlAddress = 0x40062c34

flagBefore :: Memory
flagBefore = M.fromList
  [ (Address flagConfigAddress, 0)
  , (Address (flagConfigAddress+4), 0)
  , (Address (flagConfigAddress+8), 1)
  , (Address flagControlAddress, 0x9000)
  , (Address (flagControlAddress+4), 0x9000)
  , (Address (flagControlAddress+8), flagConfigAddress)
  , (Address (flagControlAddress+0x0c), 5)
  , (Address 0x9000, flagControlAddress)
  , (Address 0x9004, flagControlAddress)
  , (Address 0x9014, flagWaitAddress)
  , (Address flagWaitAddress, 0)
  , (Address (flagWaitAddress+4), flagControlAddress)
  , (Address (flagWaitAddress+8), 1)
  , (Address (flagWaitAddress+0x0c), 1)
  , (Address (flagWaitAddress+0x10), 0)
  , (Address (flagWaitAddress+0x14), 0)
  ]

flagSetWrites :: [Written]
flagSetWrites =
  [ Written (Address (flagControlAddress+0x0c)) 4 5
  , Written (Address (flagWaitAddress+0x14)) 4 5
  , Written (Address flagWaitAddress) 4 0
  , Written (Address flagControlAddress) 4 flagControlAddress
  , Written (Address (flagControlAddress+4)) 4 flagControlAddress
  ]

flagSetCall :: [Written] -> Word32 -> CallTrace -> Call
flagSetCall writes r0 trace =
  callOf DomainA 0x4813c (FamilySemantic FlagSet) [1,4,0,0] flagBefore (applyWrites flagBefore writes) r0 trace writes

flagSetTrace :: Word32 -> CallTrace
flagSetTrace r0 = traceOf (withStack 0 r0) [] Returned

flagWaitBefore :: Memory
flagWaitBefore = M.union (M.fromList
  [ (Address flagControlAddress, flagControlAddress)
  , (Address (flagControlAddress+4), flagControlAddress)
  , (Address (flagControlAddress+0x0c), 0)
  , (Address (flagTimerAddress+4), 0x44318)
  , (Address (flagTimerAddress+8), 0x9000)
  , (Address 0x8000, 10)
  , (Address 0x8100, 0xdead)
  ]) flagBefore

flagWaitWrites :: Word32 -> [Written]
flagWaitWrites timer =
  [ Written (Address flagControlAddress) 4 0x9000
  , Written (Address (flagControlAddress+4)) 4 0x9000
  , Written (Address 0x9000) 4 flagControlAddress
  , Written (Address 0x9004) 4 flagControlAddress
  , Written (Address 0x9014) 4 flagWaitAddress
  , Written (Address (flagWaitAddress+0x0c)) 4 1
  , Written (Address (flagWaitAddress+0x10)) 4 0
  , Written (Address flagWaitAddress) 4 timer
  , Written (Address (flagWaitAddress+4)) 4 flagControlAddress
  , Written (Address (flagWaitAddress+8)) 4 1
  , Written (Address (flagTimerAddress+4)) 4 0x44318
  , Written (Address (flagTimerAddress+8)) 4 0x9000
  , Written (Address 0x8100) 4 1
  ]

timerBoundary :: BoundaryKind -> Word32 -> Word32 -> Boundary
timerBoundary kind entry task =
  Boundary kind (Address (if kind==TimerInsertBoundary then 0x44c60 else 0x44ca8)) 1 0
    (registers [1,entry]) M.empty [] [] (Just entry) task [] []

tickBoundary :: Word32 -> Boundary
tickBoundary entry = Boundary TimerConsumeBoundary (Address 0x44d8c) 2 0 (registers [entry]) M.empty [] [] (Just entry) 0x9000 [] []

flagWaitCall :: [Written] -> Word32 -> [Boundary] -> Call
flagWaitCall writes r0 history =
  callOf DomainA 0x48b30 (FamilySemantic FlagTimedWait) [1,1,0,0x8100] flagWaitBefore (applyWrites flagWaitBefore writes) r0 (traceOf (withStack 0x8000 r0) history Returned) writes

flagDeleteWrites :: [Written]
flagDeleteWrites =
  [ Written (Address flagConfigAddress) 4 0
  , Written (Address (flagConfigAddress+4)) 4 0
  , Written (Address (flagConfigAddress+8)) 4 0
  , Written (Address flagControlAddress) 4 flagControlAddress
  , Written (Address (flagControlAddress+4)) 4 flagControlAddress
  , Written (Address (flagControlAddress+0x0c)) 4 0
  , Written (Address flagWaitAddress) 4 0xffffffcf
  ]

flagDeleteCall :: [Written] -> Call
flagDeleteCall writes =
  callOf DomainA 0x4d434 (FamilySemantic FlagDelete) [1] flagBefore (applyWrites flagBefore writes) 0 (flagSetTrace 0) writes

flagDeleteTimerBefore :: Memory
flagDeleteTimerBefore = M.insert (Address (flagTimerAddress+8)) 0x9000 (M.insert (Address (flagTimerAddress+4)) 0x44318 (M.insert (Address flagWaitAddress) flagTimerAddress flagBefore))

flagDeleteTimerCall :: [Written] -> [Boundary] -> Call
flagDeleteTimerCall writes history =
  callOf DomainA 0x4d434 (FamilySemantic FlagDelete) [1] flagDeleteTimerBefore (applyWrites flagDeleteTimerBefore writes) 0 (traceOf (withStack 0 0) history Returned) writes

mailboxBefore :: Memory
mailboxBefore = M.fromList
  [ (Address mailboxConfigAddress, 2)
  , (Address (mailboxConfigAddress+4), 32)
  , (Address (mailboxConfigAddress+8), 1)
  , (Address mailboxControlAddress, mailboxControlAddress)
  , (Address (mailboxControlAddress+4), mailboxControlAddress)
  , (Address (mailboxControlAddress+0x0c), 0x9100)
  , (Address (mailboxControlAddress+0x10), 0x9200)
  , (Address 0x9100, 0x9200)
  , (Address 0x9104, 2)
  , (Address 0x9200, 0)
  , (Address 0x9204, 5)
  , (Address 0x9300, 0)
  , (Address 0x9304, 5)
  ]

mailboxSendCall :: [Written] -> Call
mailboxSendCall writes =
  callOf DomainA 0x4aa44 (FamilySemantic MailboxSend) [1,0x9300] mailboxBefore (applyWrites mailboxBefore writes) 0 (flagSetTrace 0) writes

mailboxDeleteWrites :: [Written]
mailboxDeleteWrites =
  [ Written (Address mailboxConfigAddress) 4 2
  , Written (Address (mailboxConfigAddress+4)) 4 32
  , Written (Address (mailboxConfigAddress+8)) 4 0
  , Written (Address mailboxControlAddress) 4 mailboxControlAddress
  , Written (Address (mailboxControlAddress+4)) 4 mailboxControlAddress
  , Written (Address (mailboxControlAddress+0x0c)) 4 0
  , Written (Address (mailboxControlAddress+0x10)) 4 0
  ]

mailboxDeleteCall :: [Written] -> Call
mailboxDeleteCall writes =
  callOf DomainA 0x4d654 (FamilySemantic MailboxDelete) [1] mailboxBefore (applyWrites mailboxBefore writes) 0 (flagSetTrace 0) writes

misassociated :: Boundary
misassociated = Boundary ResumeBoundary (Address 0x43640) 1 0 (registers (withStack 0x8000 0)) M.empty [] [] Nothing 0x9999 [] []

familyCatalogJson :: B.ByteString
familyCatalogJson = B.pack "{\"fill_word\":\"0xe7f000f0\",\"domains\":{\"a\":{\"regions\":[{\"start\":\"0x4813c\",\"end\":\"0x4839c\",\"entries\":[{\"entry\":\"0x4813c\",\"semantic\":{\"kind\":\"family\",\"name\":\"family-flag-set\"}}]}]},\"b\":{\"regions\":[{\"start\":\"0xd80fc\",\"end\":\"0xd835c\",\"entries\":[{\"entry\":\"0xd80fc\",\"semantic\":{\"kind\":\"family\",\"name\":\"family-flag-set\"}}]}]}}}"

loaders :: M.Map Domain B.ByteString
loaders = M.fromList [(DomainA,B.replicate 0xdae0 'x'),(DomainB,B.replicate 0xdaa0 'x')]

familyOnly :: Catalog
familyOnly = Catalog 0xe7f000f0 [Entry DomainA (FamilySemantic FlagSet) (Address 0x4813c)] [Window DomainA (Address 0x4813c) (Address 0x4839c)] []

schedulerEntries :: [Entry]
schedulerEntries = [Entry DomainA (HelperSemantic Notify) (Address 0x44018),Entry DomainB (HelperSemantic Notify) (Address 0xd3fd4)]

unguardedFamily :: Catalog
unguardedFamily = Catalog 0xe7f000f0 (schedulerEntries ++ [Entry DomainA (FamilySemantic FlagSet) (Address 0x4813c)])
  [Window DomainA (Address 0x44018) (Address 0x44044),Window DomainB (Address 0xd3fd4) (Address 0xd4004)] []

guardedFamily :: Catalog
guardedFamily = unguardedFamily { catalogWindows = catalogWindows unguardedFamily ++ [Window DomainA (Address 0x4813c) (Address 0x4839c)] }

schedulerOnly :: Catalog
schedulerOnly = Catalog 0xe7f000f0 schedulerEntries
  [Window DomainA (Address 0x44018) (Address 0x44044),Window DomainB (Address 0xd3fd4) (Address 0xd4004)] []

isLeft :: Either a b -> Bool
isLeft result = case result of Left _ -> True; Right _ -> False

main :: IO ()
main = do
  let correctCall = flagSetCall flagSetWrites 0 (flagSetTrace 0)
  assertRight "correct flag set" (checkFamilyCall correctCall [])
  let forgedResult = [Written (Address (flagWaitAddress+0x14)) 4 6
                     ,Written (Address (flagControlAddress+0x0c)) 4 5
                     ,Written (Address flagWaitAddress) 4 0
                     ,Written (Address flagControlAddress) 4 flagControlAddress
                     ,Written (Address (flagControlAddress+4)) 4 flagControlAddress]
  assertLeft "forged matched flag result" FamilyEffect (checkFamilyCall (flagSetCall forgedResult 0 (flagSetTrace 0)) [])
  assertLeft "wrong status" FamilyReturn (checkFamilyCall (flagSetCall flagSetWrites 1 (flagSetTrace 1)) [])
  let missingResult = [Written (Address (flagControlAddress+0x0c)) 4 5
                      ,Written (Address flagWaitAddress) 4 0
                      ,Written (Address flagControlAddress) 4 flagControlAddress
                      ,Written (Address (flagControlAddress+4)) 4 flagControlAddress]
  assertLeft "missing wait result" FamilyEffect (checkFamilyCall (flagSetCall missingResult 0 (flagSetTrace 0)) [])
  let validSend = [Written (Address 0x9300) 4 0,Written (Address 0x9200) 4 0x9300,Written (Address 0x9304) 4 5]
      forgedSend = [Written (Address 0x9300) 4 0x9200,Written (Address 0x9100) 4 0x9300,Written (Address 0x9304) 4 5]
  assertRight "priority send equal order" (checkFamilyCall (mailboxSendCall validSend) [])
  assertLeft "priority send wrong order" FamilyEffect (checkFamilyCall (mailboxSendCall forgedSend) [])
  let waitWrites = flagWaitWrites flagTimerAddress
      expiryWrites = [write | write@(Written (Address address) _ _) <- waitWrites,address/=0x8100]
  assertLeft "orphan timer" FamilyTimer (checkFamilyCall (flagWaitCall waitWrites 0 [timerBoundary TimerInsertBoundary flagTimerAddress 0x9000]) [])
  assertRight "wake removes timer" (checkFamilyCall (flagWaitCall waitWrites 0 [timerBoundary TimerInsertBoundary flagTimerAddress 0x9000,timerBoundary TimerRemoveBoundary flagTimerAddress 0x9000]) [])
  assertLeft "expiry without pop" FamilyTimer (checkFamilyCall (flagWaitCall expiryWrites 0xffffffce [timerBoundary TimerInsertBoundary flagTimerAddress 0x9000]) [])
  assertRight "expiry tick pop" (checkFamilyCall (flagWaitCall expiryWrites 0xffffffce [timerBoundary TimerInsertBoundary flagTimerAddress 0x9000,tickBoundary flagTimerAddress]) [])
  let returnBoundary = Boundary ReturnBoundary (Address 0x43640) 20 0 (registers [0]) M.empty [] [] Nothing 0x9000 [] []
      expiryCall = flagWaitCall expiryWrites 0xffffffce [timerBoundary TimerInsertBoundary flagTimerAddress 0x9000,returnBoundary]
      consumeWitness armedAt = TimerWitness flagTimerAddress 0x9000 10 armedAt TimerConsumed
  assertRight "consume witness inside arm-to-return" (checkFamilyCall expiryCall [consumeWitness 10])
  assertLeft "consume witness after return" FamilyTimer (checkFamilyCall expiryCall [consumeWitness 21])
  assertRight "flag delete releases waiter" (checkFamilyCall (flagDeleteCall flagDeleteWrites) [])
  assertRight "delete removes waiter timer" (checkFamilyCall (flagDeleteTimerCall flagDeleteWrites [timerBoundary TimerRemoveBoundary flagTimerAddress 0x9000]) [])
  assertLeft "delete leaves waiter timer" FamilyTimer (checkFamilyCall (flagDeleteTimerCall flagDeleteWrites []) [])
  let stranded = [Written (Address flagConfigAddress) 4 0
                 ,Written (Address (flagConfigAddress+4)) 4 0
                 ,Written (Address (flagConfigAddress+8)) 4 0
                 ,Written (Address flagControlAddress) 4 0x9000
                 ,Written (Address (flagControlAddress+4)) 4 flagControlAddress
                 ,Written (Address (flagControlAddress+0x0c)) 4 0
                 ,Written (Address flagWaitAddress) 4 0]
  assertLeft "stranded delete waiter" FamilyDestructor (checkFamilyCall (flagDeleteCall stranded) [])
  assertRight "mailbox delete drain" (checkFamilyCall (mailboxDeleteCall mailboxDeleteWrites) [])
  let reversed = mailboxDeleteWrites ++ [Written (Address 0x9100) 4 0,Written (Address 0x9200) 4 0x9100]
  assertLeft "drain order" FamilyDestructor (checkFamilyCall (mailboxDeleteCall reversed) [])
  let misassociatedCall = flagSetCall flagSetWrites 0 (traceOf (withStack 0 0) [misassociated] Returned)
  assertLeft "misassociated resume" FamilyAssociation (checkFamilyCall misassociatedCall [])
  let outside = flagSetWrites ++ [Written (Address 0x12345678) 4 1]
  assertLeft "out-of-window write" FamilyEffect (checkFamilyCall (flagSetCall outside 0 (flagSetTrace 0)) [])
  case parseCatalog familyCatalogJson of
    Left failure -> error ("family catalog parse: " ++ failure)
    Right catalog -> do
      assert "family catalog entry count" (length (catalogEntries catalog)==2)
      assert "family catalog semantic" (case catalogEntries catalog of Entry _ (FamilySemantic FlagSet) _:_ -> True; _ -> False)
  assert "family-only plan refused" (isLeft (derivePlan Owned B.empty [familyOnly] loaders))
  assert "family without guard refused" (isLeft (derivePlan Owned B.empty [unguardedFamily] loaders))
  case derivePlan Owned B.empty [guardedFamily] loaders of
    Left failure -> error ("guarded family plan: " ++ failure)
    Right plan -> do
      let familyProbes' = [probe | probe <- planProbes plan,probeDomain probe==DomainA,probeKind probe `elem` [TimerInsert,TimerRemove,TimerConsume]]
      assert "family timer probe count" (length familyProbes'==5)
      assert "family timer insert address" (any (\probe -> probeAddress probe==Address 0x44c60 && probeKind probe==TimerInsert) familyProbes')
      assert "family timer remove address" (any (\probe -> probeAddress probe==Address 0x44ca8 && probeKind probe==TimerRemove) familyProbes')
      assert "family timer consume address" (any (\probe -> probeAddress probe==Address 0x44318 && probeKind probe==TimerConsume && probeRegister probe==Register 0) familyProbes')
  case derivePlan Owned B.empty [schedulerOnly] loaders of
    Left failure -> error ("scheduler-only plan: " ++ failure)
    Right plan -> assert "scheduler plan has no family probes" (null [probe | probe <- planProbes plan,probeKind probe `elem` [TimerInsert,TimerRemove,TimerConsume]])
