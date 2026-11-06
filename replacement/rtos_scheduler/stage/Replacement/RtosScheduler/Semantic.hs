module Replacement.RtosScheduler.Semantic (Semantic (..), parseSemantic, semanticText) where

data Semantic = IrqDoorbell | DispatchSave | DispatchResume | DispatchNoSave | DispatchSelect | DispatchFirstEntry | DispatchIrqReturn | DispatchIrqResume | DispatchIrqTail | ServiceExitTask | ServiceRotateReady | ServiceInterruptRotateReady | ServiceCoreRotateReady | ServiceInterruptCoreRotateReady | ServiceDisableDispatch | ServiceEnableDispatch | ServiceActivateTask | ServiceInterruptActivateTask | ServiceCancelActivation | ServiceWakeTask | ServiceInterruptWakeTask | ServiceCancelWakeup | ServiceDeliverTaskException | ServiceDeliverTaskExceptionEntry | ServiceGetTaskPriority | ServiceSleepTask | ServiceSuspendTask | ServiceResumeTask | ServiceForceResumeTask | ServiceRaiseTaskException | ServiceInterruptRaiseTaskException | ServiceDisableTaskException | ServiceEnableTaskException deriving (Eq, Ord, Show)

parseSemantic :: String -> String -> Either String Semantic
parseSemantic kind name = case lookup (kind ++ ":" ++ name) names of
  Just semantic -> Right semantic
  Nothing -> Left "unknown scheduler semantic"

semanticText :: Semantic -> (String, String)
semanticText semantic = case [text | (text, value) <- names, value == semantic] of
  [text] -> let (kind, rest) = break (== ':') text in (kind, drop 1 rest)
  _ -> error "semantic table incomplete"

names :: [(String, Semantic)]
names =
  [ ("irq:doorbell", IrqDoorbell)
  , ("dispatch:save", DispatchSave)
  , ("dispatch:resume", DispatchResume)
  , ("dispatch:no-save", DispatchNoSave)
  , ("dispatch:select", DispatchSelect)
  , ("dispatch:first-entry", DispatchFirstEntry)
  , ("dispatch:irq-return", DispatchIrqReturn)
  , ("dispatch:irq-resume", DispatchIrqResume)
  , ("dispatch:irq-tail", DispatchIrqTail)
  , ("service:exit-task", ServiceExitTask)
  , ("service:rotate-ready", ServiceRotateReady)
  , ("service:interrupt-rotate-ready", ServiceInterruptRotateReady)
  , ("service:core-rotate-ready", ServiceCoreRotateReady)
  , ("service:interrupt-core-rotate-ready", ServiceInterruptCoreRotateReady)
  , ("service:disable-dispatch", ServiceDisableDispatch)
  , ("service:enable-dispatch", ServiceEnableDispatch)
  , ("service:activate-task", ServiceActivateTask)
  , ("service:interrupt-activate-task", ServiceInterruptActivateTask)
  , ("service:cancel-activation", ServiceCancelActivation)
  , ("service:wake-task", ServiceWakeTask)
  , ("service:interrupt-wake-task", ServiceInterruptWakeTask)
  , ("service:cancel-wakeup", ServiceCancelWakeup)
  , ("service:deliver-task-exception", ServiceDeliverTaskException)
  , ("service:deliver-task-exception-entry", ServiceDeliverTaskExceptionEntry)
  , ("service:get-task-priority", ServiceGetTaskPriority)
  , ("service:sleep-task", ServiceSleepTask)
  , ("service:suspend-task", ServiceSuspendTask)
  , ("service:resume-task", ServiceResumeTask)
  , ("service:force-resume-task", ServiceForceResumeTask)
  , ("service:raise-task-exception", ServiceRaiseTaskException)
  , ("service:interrupt-raise-task-exception", ServiceInterruptRaiseTaskException)
  , ("service:disable-task-exception", ServiceDisableTaskException)
  , ("service:enable-task-exception", ServiceEnableTaskException)
  ]
