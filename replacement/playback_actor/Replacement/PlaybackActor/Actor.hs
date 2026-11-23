{-# LANGUAGE OverloadedStrings #-}

-- | The owned playback machines' stock boundary in the X-M1 1.50 plain image: the retired
-- action extents, the retained services the actions call and the state they write.
module Replacement.PlaybackActor.Actor
  ( Span (..)
  , inside
  , Capture (..)
  , Service (..)
  , dispatcher
  , machineEvents
  , machineStates
  , modeState
  , retired
  , services
  , watchedWrites
  ) where

import qualified Data.Text as T
import Data.Word (Word32)

-- | A half-open guest address range.
data Span = Span Word32 Word32
  deriving (Eq, Show)

inside :: Span -> Word32 -> Bool
inside (Span s e) a = s <= a && a < e

-- | Bytes read at a site's entry and at its return, as (register holding the address, count).
data Capture = Capture {captureEntry :: [(Int, Int)], captureReturn :: [(Int, Int)]}

data Service = Service
  { serviceAddress :: Word32
  , serviceName :: String
  , serviceArguments :: [T.Text]
  , serviceResult :: [T.Text]
  , serviceCapture :: Capture
  }

-- | The queue-4 machine dispatcher and the machine event and state halfwords it indexes by
-- machine.
dispatcher :: Word32
dispatcher = 0x011e8bf8

machineEvents, machineStates, modeState :: Span
machineEvents = Span 0x014ea4ec 0x014ea5ac
machineStates = Span 0x014ea5ac 0x014ea66c
modeState = Span 0x014ea678 0x014ea6e4

-- | Machine 0's action extent.
retired :: [Span]
retired = [Span 0x011e8ce4 0x011ea434]

none :: Capture
none = Capture [] []

status :: Word32 -> String -> Service
status address name = Service address name [] ["r0"] none

void :: Word32 -> String -> Service
void address name = Service address name [] [] none

services :: [Service]
services =
  [ status 0x0120bd54 "status_parameter"
  , status 0x0120c268 "status_navigation"
  , status 0x0120c7b4 "status_parameter_transition"
  , status 0x0120da08 "status_machine20"
  , status 0x0120dad8 "status_buffer_mode"
  , status 0x0120e830 "status_effect"
  , status 0x0120f120 "status_machine34"
  , status 0x01212e48 "status_message"
  , void 0x0120c650 "publish_requested_mode"
  , Service 0x0121ca80 "set_work_state" ["r0"] [] none
  , void 0x0121cca4 "submit_indexed_storage_buffer"
  , void 0x0121cd84 "apply_selected_record_flags"
  , void 0x0121cde4 "write_error_trace"
  , void 0x0121ce80 "source_error_notification"
  , void 0x0121d578 "notify_primary"
  , void 0x0121d5ac "consume_secondary"
  , void 0x0121d5e8 "notify_081f"
  , void 0x0121d624 "notify_0819"
  , void 0x0121d660 "notify_084e"
  , void 0x0121d694 "bind_selected_record"
  , void 0x0121d6f8 "notify_0850"
  , void 0x0121d72c "notify_084f"
  , void 0x0121d7b0 "cancel_buffer_step"
  , void 0x0121d7f8 "callback_ready"
  , void 0x0121e624 "post_pending_event"
  , void 0x0121e794 "apply_selected_mode_options"
  , void 0x0121e884 "reset_workspaces"
  , void 0x0121ea08 "notify_0821"
  , void 0x0121eb68 "trace"
  , void 0x0121eb94 "notify_0825"
  , void 0x0121ebc8 "notify_0826"
  , Service 0x0072b680 "device_notification" [] ["r0"] (Capture [(0, 8)] [])
  ]

watchedWrites :: [Span]
watchedWrites = [machineEvents, machineStates, modeState]
