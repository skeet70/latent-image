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
  , OwnedMachine (..)
  , Boundary
  , boundaryParse
  , boundaryWire
  , boundaryRetired
  , boundaryServices
  , boundaryWrites
  ) where

import Data.List (intercalate, nub, nubBy, sort)
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

machineEvents, machineStates, modeState, requestState, selectionState, transferState, stepState :: Span
machineEvents = Span 0x014ea4ec 0x014ea5ac
machineStates = Span 0x014ea5ac 0x014ea66c
modeState = Span 0x014ea678 0x014ea6e4
requestState = Span 0x014ea6e4 0x014ea714
selectionState = Span 0x014ea9bc 0x014eaaf4
transferState = Span 0x014eaed8 0x014eb5d0
stepState = Span 0x014eb5d0 0x014eb5d8

-- | A playback machine with an owned implementation.
data OwnedMachine = Machine0 | Machine7 | Machine30 | Machine61
  deriving (Eq, Ord, Show, Enum, Bounded)

machineNumber :: OwnedMachine -> Int
machineNumber m = case m of
  Machine0 -> 0
  Machine7 -> 7
  Machine30 -> 30
  Machine61 -> 61

machineExtent :: OwnedMachine -> Span
machineExtent m = case m of
  Machine0 -> Span 0x011e8ce4 0x011ea434
  Machine7 -> Span 0x011eb824 0x011ec980
  Machine30 -> Span 0x011f31bc 0x011f4810
  Machine61 -> Span 0x011fe4d0 0x011ff0dc

machineServices :: OwnedMachine -> [Service]
machineServices m = case m of
  Machine0 -> machine0Services
  Machine7 -> machine7And30Services
  Machine30 -> machine7And30Services
  Machine61 -> machine61Services

-- | State a machine writes beyond the machine and mode halfwords every boundary watches.
machineWrites :: OwnedMachine -> [Span]
machineWrites m = case m of
  Machine0 -> []
  Machine7 -> [requestState, stepState]
  Machine30 -> [requestState, transferState]
  Machine61 -> [selectionState]

-- | The machines an observation treats as owned: their action extents are retired, and their
-- services and state are watched.
newtype Boundary = Boundary [OwnedMachine]
  deriving (Eq, Show)

-- | Comma-separated machine numbers, each owned and named once.
boundaryParse :: String -> Maybe Boundary
boundaryParse wire = do
  machines <- mapM named (splitOn wire)
  if null machines || nub machines /= machines then Nothing else Just (Boundary (sort machines))
  where
    named text = case [m | m <- [minBound .. maxBound], show (machineNumber m) == text] of
      [m] -> Just m
      _ -> Nothing
    splitOn text = case break (== ',') text of
      (part, ',' : rest) -> part : splitOn rest
      (part, _) -> [part]

boundaryWire :: Boundary -> String
boundaryWire (Boundary machines) = intercalate "," (map (show . machineNumber) machines)

boundaryRetired :: Boundary -> [Span]
boundaryRetired (Boundary machines) = map machineExtent machines

boundaryServices :: Boundary -> [Service]
boundaryServices (Boundary machines) = nubBy (\a b -> serviceAddress a == serviceAddress b) (concatMap machineServices machines)

boundaryWrites :: Boundary -> [Span]
boundaryWrites (Boundary machines) = [machineEvents, machineStates, modeState] ++ nub (concatMap machineWrites machines)

none :: Capture
none = Capture [] []

status :: Word32 -> String -> Service
status address name = Service address name [] ["r0"] none

void :: Word32 -> String -> Service
void address name = Service address name [] [] none

withArgument :: Word32 -> String -> Service
withArgument address name = Service address name ["r0"] ["r0"] none

machine0Services :: [Service]
machine0Services =
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

machine7And30Services :: [Service]
machine7And30Services =
  [ status 0x0120bc20 "dispatch_machine3_events"
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

machine61Services :: [Service]
machine61Services =
  [ status 0x0120c1a8 "service_0120c1a8"
  , status 0x0121eda0 "service_0121eda0"
  , status 0x0121ef68 "service_0121ef68"
  , status 0x0121f0fc "service_0121f0fc"
  , status 0x0121f118 "service_0121f118"
  , status 0x0121f1c4 "service_0121f1c4"
  , status 0x0121f1e8 "service_0121f1e8"
  , status 0x0121f20c "service_0121f20c"
  , status 0x0121f224 "service_0121f224"
  , status 0x0121f254 "service_0121f254"
  , status 0x0121f2a4 "service_0121f2a4"
  , status 0x0121f390 "service_0121f390"
  , status 0x0121f3ec "service_0121f3ec"
  , status 0x0121f500 "service_0121f500"
  , status 0x0121f518 "service_0121f518"
  , status 0x0121f7ac "service_0121f7ac"
  , status 0x0121f83c "service_0121f83c"
  , status 0x0121f85c "service_0121f85c"
  , status 0x012202d0 "service_012202d0"
  , withArgument 0x01229f84 "service_01229f84"
  , withArgument 0x0123265c "service_0123265c"
  , status 0x01243590 "service_01243590"
  , status 0x01243644 "service_01243644"
  , status 0x01243760 "service_01243760"
  , status 0x012437bc "service_012437bc"
  , status 0x01243890 "service_01243890"
  , status 0x012438f0 "service_012438f0"
  , withArgument 0x01243948 "service_01243948"
  , status 0x01243a6c "service_01243a6c"
  , withArgument 0x01245c7c "service_01245c7c"
  ]
