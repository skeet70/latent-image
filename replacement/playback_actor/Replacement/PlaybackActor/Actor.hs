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

machineEvents, machineStates, modeState, requestState, transferState, stepState :: Span
machineEvents = Span 0x014ea4ec 0x014ea5ac
machineStates = Span 0x014ea5ac 0x014ea66c
modeState = Span 0x014ea678 0x014ea6e4
requestState = Span 0x014ea6e4 0x014ea714
transferState = Span 0x014eaed8 0x014eb5d0
stepState = Span 0x014eb5d0 0x014eb5d8

-- | The owned machines' action extents.
retired :: [Span]
retired = [Span 0x011e8ce4 0x011ea434, Span 0x011eb824 0x011ec980, Span 0x011f31bc 0x011f4810]

none :: Capture
none = Capture [] []

status :: Word32 -> String -> Service
status address name = Service address name [] ["r0"] none

void :: Word32 -> String -> Service
void address name = Service address name [] [] none

withArgument :: Word32 -> String -> Service
withArgument address name = Service address name ["r0"] ["r0"] none

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
  , status 0x0120bc20 "dispatch_machine3_events"
  , status 0x0120ce60 "service_0120ce60"
  , status 0x0120ec28 "service_0120ec28"
  , status 0x012101bc "dispatch_machine46_events"
  , status 0x012119a4 "service_012119a4"
  , status 0x01212a48 "service_01212a48"
  , status 0x0121c9f8 "service_0121c9f8"
  , status 0x012205e4 "service_012205e4"
  , status 0x0122586c "service_0122586c"
  , status 0x012258bc "service_012258bc"
  , status 0x01225928 "service_01225928"
  , withArgument 0x01225984 "service_01225984"
  , withArgument 0x012259a4 "service_012259a4"
  , status 0x01225a98 "service_01225a98"
  , status 0x01225b20 "service_01225b20"
  , status 0x01225bfc "service_01225bfc"
  , status 0x01225d08 "service_01225d08"
  , status 0x01225f64 "service_01225f64"
  , status 0x01225fb0 "service_01225fb0"
  , status 0x01225fdc "select_source_kind22"
  , status 0x01226014 "service_01226014"
  , status 0x01226230 "service_01226230"
  , withArgument 0x012262fc "service_012262fc"
  , status 0x012263ec "service_012263ec"
  , status 0x012264fc "service_012264fc"
  , status 0x01226518 "service_01226518"
  , status 0x012265cc "service_012265cc"
  , status 0x01226684 "service_01226684"
  , status 0x0122669c "service_0122669c"
  , status 0x01229038 "fill_control_mode_frame"
  , status 0x01229068 "service_01229068"
  , status 0x0122925c "prepare_selected_source_event"
  , status 0x012292a4 "prepare_selected_transfer_event"
  , withArgument 0x012293a8 "service_012293a8"
  , status 0x012294cc "service_012294cc"
  , status 0x0122962c "service_0122962c"
  , status 0x0122968c "service_0122968c"
  , withArgument 0x012296dc "service_012296dc"
  , status 0x01229800 "service_01229800"
  , status 0x01229824 "service_01229824"
  , status 0x012298a4 "service_012298a4"
  , status 0x01236698 "service_01236698"
  , status 0x012377d4 "service_012377d4"
  , status 0x0123c224 "service_0123c224"
  , status 0x0123c268 "service_0123c268"
  , withArgument 0x01242fa4 "service_01242fa4"
  , withArgument 0x01243178 "service_01243178"
  , withArgument 0x01243198 "service_01243198"
  , status 0x012431b8 "service_012431b8"
  , status 0x01243868 "service_01243868"
  , status 0x01243e48 "service_01243e48"
  , status 0x01243fa0 "service_01243fa0"
  , withArgument 0x012441d0 "service_012441d0"
  , status 0x012441f0 "service_012441f0"
  , status 0x0120d618 "service_0120d618"
  , void 0x0120e6e8 "service_0120e6e8"
  , status 0x0120e964 "service_0120e964"
  , status 0x0120f328 "service_0120f328"
  , status 0x0120f420 "service_0120f420"
  , status 0x0120fc08 "service_0120fc08"
  , status 0x0121164c "service_0121164c"
  , status 0x01211c8c "service_01211c8c"
  , status 0x012128a0 "service_012128a0"
  , status 0x012138ac "service_012138ac"
  , status 0x0121406c "service_0121406c"
  , Service 0x012290cc "service_012290cc" ["r0"] [] none
  , Service 0x012290fc "service_012290fc" ["r0"] [] none
  , void 0x012390d4 "service_012390d4"
  , void 0x01239134 "service_01239134"
  , status 0x01239178 "service_01239178"
  ]

watchedWrites :: [Span]
watchedWrites = [machineEvents, machineStates, modeState, requestState, transferState, stepState]
