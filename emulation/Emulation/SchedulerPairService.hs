module Emulation.SchedulerPairService where

import Data.Bits ((.&.), (.|.), complement, shiftR)
import Data.Int (Int16, Int32)
import Data.Word (Word32)
import Emulation.SchedulerPairModel
import Emulation.SchedulerPairPlan (Address (..), DispatchPoint (..))
import Replacement.RtosOwnFirst.Regions (Domain (..))

data ServiceCompletion = ServiceReturn | ServiceDispatch DispatchPoint Word32 | ServiceRetry | ServiceCallback Word32 [Word32] deriving (Eq,Show)
data ServiceExpected = ServiceExpected
  { serviceExpected :: Expected, serviceCompletion :: ServiceCompletion, serviceCpsr :: Word32, serviceR0 :: Maybe Word32 } deriving (Eq,Show)
data DomainState = DomainState
  { contextWord :: Word32, currentContext :: Word32, contextTable :: Word32, runningCell :: Word32,
    rescheduleCell :: Word32, dispatchCell :: Word32, selectedCell :: Word32, sleepResults :: Word32 } deriving (Eq,Show)
domainWords :: Domain -> DomainState
domainWords DomainA = DomainState 0x400545a4 0x400545c8 0x400545cc 0x40054760 0x400546d8 0x40054764 0x40054768 0x4005477c
domainWords DomainB = DomainState 0x400e4564 0x400e4588 0x400e458c 0x400e4720 0x400e4698 0x400e4724 0x400e4728 0x400e473c

runService :: VM (ServiceCompletion,Maybe Word32,Word32,[(Address,Word32)]) -> Memory -> Either ModelFailure ServiceExpected
runService action memory = do
  ((completion,result,cpsr,bank),after,writes) <- runVM action memory
  pure (ServiceExpected (Expected (maybe 0 id result) after writes bank) completion cpsr result)
newtype NotificationTarget = NotificationTarget Word32 deriving (Eq,Show)
data NotifyPredicate = PerDomainPredicate | PrimaryPredicate

notifyService :: Domain -> NotificationTarget -> (Word32,[(Address,Word32)])
notifyService = notifyWithPredicate PerDomainPredicate

notifyWithPredicate :: NotifyPredicate -> Domain -> NotificationTarget -> (Word32,[(Address,Word32)])
notifyWithPredicate predicate domain (NotificationTarget owner)
  | local = (1,[])
  | otherwise = (0,[(Address (if owner `shiftR` 1 == 0 then 0xfffb30cc else 0xfffb40cc),0x1000000)])
 where
  local = case predicate of
    PerDomainPredicate -> owner == 0 || owner == (if domain == DomainA then 1 else 2)
    PrimaryPredicate -> owner <= 1

serviceContextValid :: DomainState -> Bool -> Word32 -> VM Bool
serviceContextValid words' handler cpsr = do
  context <- load (contextWord words')
  pure ((context /= 0) == handler && cpsr .&. 0x80 == 0)
resolveTask :: DomainState -> Bool -> Bool -> Word32 -> VM (Either Word32 (Word32,Word32))
resolveTask words' allowZero zeroIsRunning handle = do
  let owner = fromIntegral (fromIntegral (handle `shiftR` 16) :: Int16) :: Int
      number = fromIntegral (fromIntegral handle :: Int16) :: Int
  if owner < 0 || owner > 2 || number < 0 || (number == 0 && not allowZero)
    then pure (Left (fromIntegral (-18 :: Int)))
    else do
      record <- if owner == 0 then load (currentContext words') else pure (contextTable words' + fromIntegral (owner-1)*0x84)
      count <- load (record+0x48)
      if number > 0 && number > fromIntegral (fromIntegral count :: Int32)
        then pure (Left (fromIntegral (-18 :: Int)))
        else do
          task <- if number == 0 && zeroIsRunning then load (runningCell words') else do
            base <- load (record+0x44)
            pure (base + fromIntegral (number-1)*0x38)
          pure (Right (record,task))
lockService :: Word32 -> VM ()
lockService record = load (record+8) >>= (`store` 1)
unlockService :: Word32 -> VM ()
unlockService record = load (record+8) >>= (`store` 0)
irqMasked, irqEnabled :: Word32 -> Word32
irqMasked cpsr = cpsr .|. 0x80
irqEnabled cpsr = cpsr .&. complement 0x80
