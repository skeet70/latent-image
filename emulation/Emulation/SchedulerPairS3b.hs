module Emulation.SchedulerPairS3b (NotifyPolicy (..), serviceCall, serviceCallWithNotify, supportsService, resumeService, releaseWait) where

import Control.Monad (when)
import Data.Bits ((.&.), (.|.), complement, shiftR)
import Data.Word (Word32)
import Emulation.SchedulerPairModel
import Emulation.SchedulerPairPlan (Address, DispatchPoint (Save), Service (..))
import Emulation.SchedulerPairService
import Replacement.RtosOwnFirst.Regions (Domain)
import Replacement.RtosScheduler.Regions (Helper (..))

supportsService :: Service -> Bool
supportsService service = service `elem` [InterruptActivateTask,ActivateTask,CancelActivation,WakeTask,InterruptWakeTask,CancelWakeup]

data NotifyPolicy = StockNotify | PredicateNotify NotificationTarget NotifyPredicate

serviceCall :: Domain -> Service -> Word32 -> Arguments -> Memory -> Either ModelFailure ServiceExpected
serviceCall = serviceCallWithNotify StockNotify

serviceCallWithNotify :: NotifyPolicy -> Domain -> Service -> Word32 -> Arguments -> Memory -> Either ModelFailure ServiceExpected
serviceCallWithNotify policy domain service cpsr (Arguments [handle,_,_,_])
 | supportsService service = runService $ do
  valid <- serviceContextValid words' handler cpsr
  if not valid then returned cpsr (-25) else do
    selected <- resolveTask words' (not handler) True handle
    case selected of
      Left failure -> pure (ServiceReturn,Just failure,cpsr,[])
      Right (record,task) -> do
        definition <- load (task+8)
        exists <- load (definition+0x24)
        if exists == 0 then returned cpsr (-42) else do
          lockService record
          state <- byte (task+12)
          flags <- byte (task+14)
          running <- load (runningCell words')
          discarded <- if task == running then load (task+0x30) else pure 0
          if cancel then do
            result <- if (wake && state == 0) || discarded /= 0 then pure (fromIntegral (-41 :: Int)) else do
              storeByte (task+14) (flags .&. complement bit)
              pure (if flags .&. bit /= 0 then 1 else 0)
            unlockService record
            pure (ServiceReturn,Just result,irqEnabled cpsr,[])
          else if wake && (state == 0 || (not handler && discarded /= 0)) then do
            unlockService record
            returned (irqEnabled cpsr) (-41)
          else if (not wake && state == 0) || (wake && state .&. 8 /= 0) then do
            changed <- if wake then releaseWait domain record task else fst <$> helperAction domain Prepare (Arguments [record,task,0,0])
            (local,bank) <- if changed == 0 then pure (0,[]) else load record >>= notification . NotificationTarget
            when (handler && local /= 0) (store (rescheduleCell words') 1)
            unlockService record
            pure (if not handler && local /= 0 then ServiceDispatch Save reason else ServiceReturn,
              Just (if not handler && local /= 0 then reason else 0),if not handler && local /= 0 then irqMasked cpsr else irqEnabled cpsr,bank)
          else do
            result <- if flags .&. bit /= 0 then pure (fromIntegral (-43 :: Int)) else storeByte (task+14) (flags .|. bit) >> pure 0
            unlockService record
            pure (ServiceReturn,Just result,irqEnabled cpsr,[])
 where
  words' = domainWords domain
  handler = service `elem` [InterruptActivateTask,InterruptWakeTask]
  wake = service `elem` [WakeTask,InterruptWakeTask,CancelWakeup]
  cancel = service `elem` [CancelActivation,CancelWakeup]
  bit = if wake then 2 else 1
  reason = fromIntegral (if wake then (-19 :: Int) else -7)
  notification target = case policy of
    StockNotify -> pure (notifyService domain target)
    PredicateNotify expected predicate
      | target==expected -> pure (notifyWithPredicate predicate domain target)
      | otherwise -> VM (const (Left InvalidArguments))
serviceCallWithNotify _ _ _ _ _ = const (Left InvalidArguments)

returned :: Word32 -> Int -> VM (ServiceCompletion,Maybe Word32,Word32,[(Address,Word32)])
returned cpsr result = pure (ServiceReturn,Just (fromIntegral result),cpsr,[])
resumeService :: Domain -> Service -> Word32 -> Arguments -> Memory -> Either ModelFailure ServiceExpected
resumeService _ service cpsr _
  | service `elem` [ActivateTask,WakeTask] = runService (returned (irqEnabled cpsr) 0)
  | otherwise = const (Left InvalidArguments)

releaseWait :: Domain -> Word32 -> Word32 -> VM Word32
releaseWait domain record task = do
  wait <- load (task+0x14)
  timer <- load wait
  when (timer /= 0) (removeTimer record timer)
  store wait 0
  terminating <- load (task+0x24)
  if terminating /= 0 then do
    _ <- helperAction domain Reset (Arguments [task,0,0,0])
    store (task+0x30) 1
    flags <- byte (task+14)
    if flags .&. 1 == 0 then pure 0 else do
      storeByte (task+14) (flags .&. complement 1)
      changed <- fst <$> helperAction domain Prepare (Arguments [record,task,0,0])
      pure (if changed /= 0 then 1 else 0)
  else do
    changing <- load (task+0x28)
    when (changing /= 0) $ do
      new <- load (task+0x34)
      storeByte (task+13) new
      store (task+0x28) 0
    store (task+0x2c) 0
    state <- byte (task+12)
    if state .&. 4 /= 0 then storeByte (task+12) 4 >> pure 0 else insertReady record task

removeTimer :: Word32 -> Word32 -> VM ()
removeTimer record entry = do
  index <- load entry
  count <- field record 0x38
  heap <- load (record+0x34)
  base <- field record 0x3c
  let remaining = count-1
      node i = heap+(i-1)*8
      copy to from = do
        expiry <- load (node from)
        pointer <- load (node from+4)
        store (node to) expiry
        store (node to+4) pointer
        store pointer to
      up i expiry = if i <= 1 then pure i else do
        parent <- load (node (i `shiftR` 1))
        if parent-base <= expiry-base then pure i else copy i (i `shiftR` 1) >> up (i `shiftR` 1) expiry
      down i expiry = if i*2 > remaining then pure i else do
        let left=i*2; right=left+1
        smaller <- if right > remaining then pure left else do
          l <- load (node left); r <- load (node right)
          pure (if r-base >= l-base then left else right)
        child <- load (node smaller)
        if expiry-base <= child-base then pure i else copy i smaller >> down smaller expiry
  lastExpiry <- load (node count)
  setField record 0x38 remaining
  when (remaining /= 0) $ do
    upward <- if index <= 1 then pure False else do
      parent <- load (node (index `shiftR` 1))
      pure (lastExpiry-base < parent-base)
    destination <- if upward then copy index (index `shiftR` 1) >> up (index `shiftR` 1) lastExpiry else down index lastExpiry
    copy destination count
