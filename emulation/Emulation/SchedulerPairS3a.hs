module Emulation.SchedulerPairS3a (supportsService, serviceCall, resumeService) where

import Control.Monad (when)
import Data.Bits ((.&.), complement)
import Data.Word (Word32)
import Emulation.SchedulerPairModel
import Emulation.SchedulerPairPlan (Service (..), DispatchPoint (..))
import Emulation.SchedulerPairService
import Replacement.RtosOwnFirst.Regions (Domain)
import Replacement.RtosScheduler.Regions (Helper (..))

supportsService :: Service -> Bool
supportsService service = service `elem` [DisableDispatch,EnableDispatch,ExitTask,RotateReady,InterruptRotateReady,CoreRotateReady,InterruptCoreRotateReady]

serviceCall :: Domain -> Service -> Word32 -> Arguments -> Memory -> Either ModelFailure ServiceExpected
serviceCall domain service cpsr (Arguments [r0,r1,_,_]) memory
  | supportsService service = runService action memory
  | otherwise = Left InvalidArguments
  where
    words' = domainWords domain
    errorCode n = fromIntegral (n :: Int)
    returned result = pure (ServiceReturn,Just result,cpsr,[])
    success = pure (ServiceReturn,Just 0,irqEnabled cpsr,[])
    retry processor = pure (ServiceRetry,Nothing,processor,[])
    dispatch point reason processor bank = pure (ServiceDispatch point (errorCode reason),Just (errorCode reason),processor,bank)
    helper name a b c = helperAction domain name (Arguments [a,b,c,0])
    withLock record body = do
      lock <- load (record+8)
      held <- load lock
      if held /= 0 then retry (irqMasked cpsr) else do
        lockService record
        body
    current body = load (currentContext words') >>= (\record -> withLock record (body record))
    selectRecord selector
      | selector == 0 = load (currentContext words')
      | selector <= 2 = pure (contextTable words'+(selector-1)*0x84)
      | otherwise = VM (const (Left InvalidArguments))
    action
      | service == ExitTask = current $ \record -> do
          enabled <- load (dispatchCell words')
          when (enabled == 0) (store (dispatchCell words') 1)
          task <- load (runningCell words')
          discarded <- load (task+0x30)
          when (discarded == 0) $ do
            _ <- helper Remove record task 0
            running <- load (runningCell words')
            _ <- helper Reset running 0 0
            next <- load (runningCell words')
            flags <- byte (next+0xe)
            when (flags .&. 1 /= 0) $ do
              storeByte (next+0xe) (flags .&. complement 1)
              _ <- helper Prepare record next 0
              pure ()
          unlockService record
          dispatch NoSave (-10) (irqMasked cpsr) []
      | otherwise = do
          valid <- serviceContextValid words' (service `elem` [InterruptRotateReady,InterruptCoreRotateReady]) cpsr
          if not valid then returned (if service == CoreRotateReady then 0 else errorCode (-25)) else case service of
            DisableDispatch -> current $ \record -> do
              task <- load (runningCell words')
              state <- byte (task+0xc)
              if state .&. 1 == 0 then unlockService record >> retry (irqEnabled cpsr) else do
                store (dispatchCell words') 0
                unlockService record
                success
            EnableDispatch -> current $ \record -> do
              store (dispatchCell words') 1
              task <- load (runningCell words')
              discarded <- load (task+0x30)
              if discarded /= 0 then do
                unlockService record
                dispatch NoSave (-92) (irqEnabled cpsr) []
              else do
                running <- load (runningCell words')
                selected <- load (selectedCell words')
                unlockService record
                if running == selected then success else dispatch Save (-92) (irqMasked cpsr) []
            RotateReady -> if r0 > 16 then returned (errorCode (-17)) else current $ \record -> do
              p <- if r0 == 0 then load (runningCell words') >>= (\task -> byte (task+0xd)) else pure (r0-1)
              (changed,_) <- helper Rotate record p 0
              unlockService record
              if changed == 0 then success else dispatch Save (-85) (irqMasked cpsr) []
            InterruptRotateReady -> if r0-1 >= 16 then returned (errorCode (-17)) else current $ \record -> do
              (changed,_) <- helper Rotate record (r0-1) 0
              when (changed /= 0) (store (rescheduleCell words') 1)
              unlockService record
              success
            CoreRotateReady -> if r1-1 >= 16 || r0 >= 3 then returned 0 else coreRotation False
            InterruptCoreRotateReady -> if r1-1 >= 16 then returned (errorCode (-17)) else coreRotation True
            _ -> VM (const (Left InvalidArguments))
    coreRotation interrupt = do
      record <- selectRecord r0
      withLock record $ do
        (changed,_) <- helper Rotate record (r1-1) 0
        (local,bank) <- if changed == 0 then pure (0,[]) else load record >>= pure . notifyService domain . NotificationTarget
        when (interrupt && changed /= 0) $ do
          reschedule <- if local /= 0 then pure 1 else load (rescheduleCell words')
          store (rescheduleCell words') reschedule
        unlockService record
        if not interrupt && local /= 0 then dispatch Save (-4) (irqMasked cpsr) bank
          else pure (ServiceReturn,Just 0,irqEnabled cpsr,bank)
serviceCall _ _ _ _ _ = Left InvalidArguments

resumeService :: Domain -> Service -> Word32 -> Arguments -> Memory -> Either ModelFailure ServiceExpected
resumeService _ service cpsr _
  | service `elem` [EnableDispatch,RotateReady,CoreRotateReady] =
      runService (pure (ServiceReturn,Just 0,irqEnabled cpsr,[]))
  | otherwise = const (Left InvalidArguments)
