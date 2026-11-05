module Emulation.SchedulerPairS3c (serviceCall, supportsService, resumeService) where

import Data.Bits ((.&.), (.|.), complement)
import Data.Word (Word32)
import Emulation.SchedulerPairModel
import Emulation.SchedulerPairPlan (Address, DispatchPoint (Save), Service (..))
import Emulation.SchedulerPairService
import Replacement.RtosOwnFirst.Regions (Domain)

supportsService :: Service -> Bool
supportsService service = service `elem` [GetTaskPriority,SleepTask,SuspendTask,ResumeTask,ForceResumeTask,RaiseTaskException,InterruptRaiseTaskException,DisableTaskException,EnableTaskException,DeliverTaskException,DeliverTaskExceptionEntry]
type Outcome = (ServiceCompletion,Maybe Word32,Word32,[(Address,Word32)])
result :: Word32 -> Int -> VM Outcome
result cpsr value = pure (ServiceReturn,Just (fromIntegral value),cpsr,[])

serviceCall :: Domain -> Service -> Word32 -> Arguments -> Memory -> Either ModelFailure ServiceExpected
serviceCall domain service cpsr (Arguments [handle,argument,_,_])
  | supportsService service = runService action
 where
  words' = domainWords domain
  handler = service == InterruptRaiseTaskException
  action
    | service `elem` [DeliverTaskException,DeliverTaskExceptionEntry] && cpsr .&. 0x80 == 0 = VM (const (Left InvalidArguments))
    | service == DeliverTaskException = do
        record <- load (currentContext words')
        locked <- load (record+8) >>= load
        if locked == 1 then deliveryStep words' cpsr else VM (const (Left InvalidArguments))
    | service == DeliverTaskExceptionEntry = do
        record <- load (currentContext words')
        lockService record
        task <- load (runningCell words')
        flags <- byte (task+14)
        pending <- load (task+16)
        if flags .&. 4 /= 0 && pending /= 0 then deliveryStep words' cpsr else do
          unlockService record
          pure (ServiceReturn,Nothing,cpsr,[])
    | otherwise = do
        valid <- serviceContextValid words' handler cpsr
        if not valid then result cpsr (-25) else case service of
          SleepTask -> sleepAction
          DisableTaskException -> exceptionControl False
          EnableTaskException -> exceptionControl True
          _ -> do
            selected <- resolveTask words' (service `notElem` [ResumeTask,ForceResumeTask]) (not handler) handle
            case selected of
              Left failure -> pure (ServiceReturn,Just failure,cpsr,[])
              Right (record,task) -> do
                definition <- load (task+8)
                if service `elem` [RaiseTaskException,InterruptRaiseTaskException]
                  then if argument == 0 then result cpsr (-17) else raiseAction record task definition
                  else do
                    exists <- load (definition+0x24)
                    if exists == 0 then result cpsr (-42) else taskAction record task
  finish record value = unlockService record >> result (irqEnabled cpsr) value
  retry record = unlockService record >> pure (ServiceRetry,Nothing,irqEnabled cpsr,[])
  taskAction record task = do
    running <- load (runningCell words')
    acquired <- if service == SuspendTask && task == running then load (currentContext words') else pure record
    lockService acquired
    state <- byte (task+12)
    discarded <- if task == running then load (task+0x30) else pure 0
    case service of
      GetTaskPriority -> if state == 0 || discarded /= 0 then finish record (-41) else do
        p <- byte (task+13)
        store argument (p+1)
        finish record 0
      SuspendTask -> if task == running && state .&. 1 == 0 then retry acquired else do
        enabled <- if task == running then load (dispatchCell words') else pure 1
        if enabled == 0 then finish record (-25)
        else if state == 0 then finish record (-41)
        else if state .&. 1 /= 0 then do
          storeByte (task+12) 4
          changed <- removeReady record task
          dispatch record changed (-22)
        else if state .&. 4 /= 0 then finish record (-43)
        else storeByte (task+12) (state .|. 4) >> finish record 0
      ResumeTask -> resumeAction record task state discarded
      ForceResumeTask -> resumeAction record task state discarded
      _ -> VM (const (Left InvalidArguments))
  resumeAction record task state discarded
    | state .&. 4 == 0 || discarded /= 0 = finish record (-41)
    | state .&. 2 /= 0 = storeByte (task+12) (state .&. complement 4) >> finish record 0
    | otherwise = insertReady record task >>= \changed -> dispatch record changed (-23)
  dispatch record changed reason = do
    (local,bank) <- if changed == 0 then pure (0,[]) else notifyService domain <$> load record
    unlockService record
    pure (if local /= 0 then ServiceDispatch Save (fromIntegral (reason :: Int)) else ServiceReturn,
      Just (if local /= 0 then fromIntegral reason else 0),if local /= 0 then irqMasked cpsr else irqEnabled cpsr,bank)
  sleepAction = do
    enabled <- load (dispatchCell words')
    if enabled == 0 then result cpsr (-25) else do
      initial <- load (runningCell words')
      definition <- load (initial+8)
      slot <- load (definition+0x20)
      record <- load (currentContext words')
      lockService record
      task <- load (runningCell words')
      state <- byte (task+12)
      if state .&. 1 == 0 then retry record else do
        flags <- byte (task+14)
        if flags .&. 2 /= 0 then storeByte (task+14) (flags .&. complement 2) >> finish record 0 else do
          storeByte (task+12) 10
          _ <- removeReady record task
          current <- load (runningCell words')
          let wait = sleepResults words'+(slot-1)*4
          store (current+0x14) wait
          store wait 0
          unlockService record
          pure (ServiceDispatch Save (fromIntegral (-17 :: Int)),Just (fromIntegral (-17 :: Int)),irqMasked cpsr,[])
  exceptionControl enable = do
    record <- load (currentContext words')
    lockService record
    task <- load (runningCell words')
    state <- if enable then byte (task+12) else pure 1
    if state .&. 1 == 0 then retry record else do
      definition <- load (task+8)
      callback <- load (definition+0x1c)
      discarded <- load (task+0x30)
      if callback == 0 || discarded /= 0 then finish record (-41) else do
        flags <- byte (task+14)
        storeByte (task+14) (if enable then flags .|. 4 else flags .&. complement 4)
        pending <- if enable then load (task+16) else pure 0
        if pending /= 0 then deliveryStep words' cpsr else finish record 0
  raiseAction record task definition = do
    lockService record
    state <- byte (task+12)
    callback <- load (definition+0x1c)
    running <- load (runningCell words')
    discarded <- if not handler && task == running then load (task+0x30) else pure 0
    if state == 0 || callback == 0 || discarded /= 0 then finish record (-41) else do
      pending <- load (task+16)
      store (task+16) (pending .|. argument)
      owner <- load record
      let (local,bank) = notifyService domain owner
      flags <- byte (task+14)
      if local /= 0 && task == running && flags .&. 4 /= 0
        then if handler then do
          store (rescheduleCell words') 1
          unlockService record
          pure (ServiceReturn,Just 0,irqEnabled cpsr,bank)
        else do
          (completion,value,status,innerBank) <- deliveryStep words' cpsr
          pure (completion,value,status,bank++innerBank)
        else do
          unlockService record
          pure (ServiceReturn,Just 0,irqEnabled cpsr,bank)
serviceCall _ _ _ _ = const (Left InvalidArguments)

deliveryStep :: DomainState -> Word32 -> VM Outcome
deliveryStep words' cpsr = do
  task <- load (runningCell words')
  pending <- load (task+16)
  flags <- byte (task+14)
  storeByte (task+14) (flags .&. complement 4)
  store (task+16) 0
  record <- load (currentContext words')
  unlockService record
  current <- load (runningCell words')
  definition <- load (current+8)
  callback <- load (definition+0x1c)
  user <- load (definition+4)
  pure (ServiceCallback callback [pending,user],Nothing,irqEnabled cpsr,[])

resumeService :: Domain -> Service -> Word32 -> Arguments -> Memory -> Either ModelFailure ServiceExpected
resumeService domain service cpsr _
  | service `elem` [SuspendTask,ResumeTask,ForceResumeTask] = runService (result (irqEnabled cpsr) 0)
  | service == SleepTask = runService $ do
      task <- load (runningCell words')
      wait <- load (task+0x14)
      value <- load wait
      pure (ServiceReturn,Just value,irqEnabled cpsr,[])
  | service `elem` [RaiseTaskException,EnableTaskException,DeliverTaskException,DeliverTaskExceptionEntry] = runService $ do
      record <- load (currentContext words')
      lockService record
      task <- load (runningCell words')
      pending <- load (task+16)
      if pending /= 0 then deliveryStep words' cpsr else do
        flags <- byte (task+14)
        storeByte (task+14) (flags .|. 4)
        if service == DeliverTaskException
          then pure (ServiceReturn,Nothing,irqMasked cpsr,[])
          else do
            unlockService record
            pure (ServiceReturn,if service == DeliverTaskExceptionEntry then Nothing else Just 0,
              if service == DeliverTaskExceptionEntry then irqMasked cpsr else irqEnabled cpsr,[])
  | otherwise = const (Left InvalidArguments)
 where words' = domainWords domain
