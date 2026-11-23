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
  , message119Builder
  , message119Sent
  , CopySite (..)
  , messageCopies
  , appCopyLoads
  , appCopyStores
  , provenanceMemory
  ) where

import Data.List (intercalate, nub, sort)
import qualified Data.Text as T
import Data.Word (Word32)
import Numeric (showHex)

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
data OwnedMachine
  = Machine0
  | Machine1
  | Machine2
  | Machine3
  | Machine4
  | Machine5
  | Machine6
  | Machine7
  | Machine8
  | Machine30
  | Machine34
  | Machine44
  | Machine45
  | Machine46
  | Machine47
  | Machine56
  | Machine57
  | Machine61
  | Machine66
  | Machine70
  deriving (Eq, Ord, Show, Enum, Bounded)

machineNumber :: OwnedMachine -> Int
machineNumber m = case m of
  Machine0 -> 0
  Machine1 -> 1
  Machine2 -> 2
  Machine3 -> 3
  Machine4 -> 4
  Machine5 -> 5
  Machine6 -> 6
  Machine7 -> 7
  Machine8 -> 8
  Machine30 -> 30
  Machine34 -> 34
  Machine44 -> 44
  Machine45 -> 45
  Machine46 -> 46
  Machine47 -> 47
  Machine56 -> 56
  Machine57 -> 57
  Machine61 -> 61
  Machine66 -> 66
  Machine70 -> 70

machineExtent :: OwnedMachine -> Span
machineExtent m = case m of
  Machine0 -> Span 0x011e8ce4 0x011ea434
  Machine1 -> Span 0x011ea434 0x011ea5f0
  Machine2 -> Span 0x011ea5f0 0x011eaa54
  Machine3 -> Span 0x011eaa54 0x011eacb0
  Machine4 -> Span 0x011eacb0 0x011eb31c
  Machine5 -> Span 0x011eb31c 0x011eb500
  Machine6 -> Span 0x011eb500 0x011eb824
  Machine7 -> Span 0x011eb824 0x011ec980
  Machine8 -> Span 0x011ec980 0x011ecb1c
  Machine30 -> Span 0x011f31bc 0x011f4810
  Machine34 -> Span 0x011f62c8 0x011f66ec
  Machine44 -> Span 0x011f97b4 0x011fa1b4
  Machine45 -> Span 0x011fa1b4 0x011fa618
  Machine46 -> Span 0x011fa618 0x011facec
  Machine47 -> Span 0x011facec 0x011fb44c
  Machine56 -> Span 0x011fd988 0x011fdc1c
  Machine57 -> Span 0x011fdc1c 0x011fdd38
  Machine61 -> Span 0x011fe4d0 0x011ff0dc
  Machine66 -> Span 0x011ffd3c 0x011ffed0
  Machine70 -> Span 0x01200f58 0x01201814

machineServices :: OwnedMachine -> [Service]
machineServices m = case m of
  Machine0 -> machine0Services
  Machine1 -> machine1Services
  Machine2 -> machine2Services
  Machine3 -> machine3Services
  Machine4 -> machine4Services
  Machine5 -> machine5Services
  Machine6 -> machine6Services
  Machine7 -> machine7And30Services
  Machine8 -> machine8Services
  Machine30 -> machine7And30Services
  Machine34 -> machine34Services
  Machine44 -> machine44Services
  Machine45 -> machine45Services
  Machine46 -> machine46Services
  Machine47 -> machine47Services
  Machine56 -> machine56Services
  Machine57 -> machine57Services
  Machine61 -> machine61Services
  Machine66 -> machine66Services
  Machine70 -> machine70Services

-- | State a machine writes beyond the machine and mode halfwords every boundary watches.
machineWrites :: OwnedMachine -> [Span]
machineWrites m = case m of
  Machine0 -> []
  Machine1 -> []
  Machine2 -> []
  Machine3 -> [Span 0x014eae1c 0x014eae20]
  Machine4 -> [Span 0x01511339 0x0151133a, Span 0x01511348 0x0151134c]
  Machine5 -> [Span 0x014eae84 0x014eae88]
  Machine6 -> [Span 0x014eacb4 0x014eacbc, Span 0x014eacd4 0x014eacd8]
  Machine7 -> [requestState, stepState]
  Machine8 -> []
  Machine30 -> [requestState, transferState]
  Machine34 -> []
  Machine44 -> []
  Machine45 -> []
  Machine46 -> [requestState]
  Machine47 -> []
  Machine56 -> []
  Machine57 -> []
  Machine61 -> [selectionState]
  Machine66 -> []
  Machine70 -> [Span 0x0151316c 0x01513174, Span 0x0151317a 0x0151317b]

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

-- | One site per service address; its arguments and results are those any owned caller passes or
-- reads.
boundaryServices :: Boundary -> [Service]
boundaryServices (Boundary machines) = foldr merge [] (concatMap machineServices machines)
  where
    merge s rest = case break (\t -> serviceAddress t == serviceAddress s) rest of
      (before, t : after) -> s {serviceArguments = nub (serviceArguments s ++ serviceArguments t), serviceResult = nub (serviceResult s ++ serviceResult t)} : before ++ after
      (_, []) -> s : rest

boundaryWrites :: Boundary -> [Span]
boundaryWrites (Boundary machines) = [machineEvents, machineStates, modeState] ++ nub (concatMap machineWrites machines)

-- | The stock builder of playback message 0x119 (image 5 `0x00db2fc0`) up to its send call, at
-- which r1 is the payload length and r2 the payload on the builder's frame.
message119Builder :: Span
message119Builder = Span 0x00db2fc0 0x00db3038

-- | The instruction after the send call; the builder's frame dies there.
message119Sent :: Word32
message119Sent = 0x00db303c

-- | A call of the stock byte copy `0x722054` (r0 destination, r1 source, r2 length) that can carry
-- the message; the copy has returned at the next instruction.
data CopySite = CopySite {copySiteName :: String, copySiteCall :: Word32, copySiteReleasesSource :: Bool}

-- | The dispatch send into a queue buffer, the dispatch receive out of it (the buffer is released
-- after), and every overlay-47 copy longer than 8 bytes out of the request payload `0x014ea6f0`,
-- the restore payload `0x014ea7b4` or a saved request (reader-inventory section 1.4).
messageCopies :: [CopySite]
messageCopies =
  CopySite "dispatch_send" 0x0072517c False
    : CopySite "dispatch_receive" 0x007253f8 True
    : [CopySite ("request_" ++ showHex call "") call False | call <- requestCopies]

requestCopies :: [Word32]
requestCopies =
  [ 0x011f5ae4, 0x011f5b4c, 0x011fc264, 0x0120b440, 0x0120b57c, 0x0120b6b4, 0x0120b6f4, 0x0120bb20
  , 0x0120bddc, 0x0120be30, 0x0120be80, 0x0120c1dc, 0x0120c814, 0x0120c86c, 0x0120c944, 0x0120ca20
  , 0x0120caec, 0x0120cc54, 0x0120ccac, 0x0120ccf0, 0x0120cd30, 0x0120d134, 0x0120d544, 0x0120de5c
  , 0x0120e108, 0x0120e370, 0x0120e624, 0x0120e7b4, 0x0120e93c, 0x0120f098, 0x0120f758, 0x0120fddc
  , 0x012100b0, 0x012102b8, 0x01210758, 0x012107fc, 0x01210b7c, 0x01210d24, 0x01210e4c, 0x01210f0c
  , 0x01210f40, 0x01210fe4, 0x012110a4, 0x012110d8, 0x01211228, 0x01211518, 0x01211624, 0x01211938
  , 0x0121197c, 0x012120b4, 0x012120f8, 0x01212244, 0x012122cc, 0x0121257c, 0x012125c0, 0x012126c8
  , 0x012127f8, 0x01212cd0, 0x01212d14, 0x01212d58, 0x012130a0, 0x0121322c, 0x01213300, 0x01213378
  , 0x012133bc, 0x012134d4, 0x01213594, 0x012135c8, 0x01213b08, 0x01213e08, 0x01213fe4, 0x0121422c
  , 0x01214270, 0x012142b4, 0x01214370, 0x012143c8, 0x01214528, 0x012148d0, 0x01214aa0, 0x01214c9c
  , 0x01214d58, 0x01214e08, 0x01214e60, 0x01214eb8, 0x01214f10, 0x01215168, 0x01215254, 0x01249850
  , 0x01249864, 0x012499c4, 0x012499fc, 0x01249a20, 0x01249a44, 0x01249a68, 0x01249a84, 0x01249ab0
  , 0x01249adc
  ]

-- | The loads and stores of `0x722054`; it branches only on its addresses and length.
appCopyLoads, appCopyStores :: [Word32]
appCopyLoads = [0x00722080, 0x007220c4, 0x007220e8, 0x00722130, 0x00722150, 0x0072215c]
appCopyStores = [0x00722084, 0x007220c8, 0x007220ec, 0x00722134, 0x00722154, 0x00722160]

-- | Physical memory every observation watches for reads and writes of the message: the
-- builder's frames on the task-1 stack (payload at 0x00a4cb04 or 0x00a4cb14 on the card routes),
-- the channel-4 queue buffer at 0x009e68a0 the send fills, the request block from its payload
-- `0x014ea6f0` to the end of the copies' destinations at 0x014ea91c, the saved requests and the
-- copies' other destinations `0x01511524` and `0x015151ec`. A mark created or copied outside them
-- is a finding.
provenanceMemory :: [Span]
provenanceMemory =
  [ Span 0x00a4cb00 0x00a4cc00, Span 0x009e68a0 0x009e6970, Span 0x014ea6f0 0x014ea91c
  , Span 0x01511020 0x01511330, Span 0x01511524 0x01511530, Span 0x015151ec 0x01515254 ]

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
  , -- Void: r0 at its return is whatever its last callee left. Its callers 0x011f3acc and
    -- 0x011f9b14 reach a callee that sets r0 without reading it on every path before any read.
    Service 0x012293a8 "service_012293a8" ["r0"] [] none
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

machine1Services :: [Service]
machine1Services =
  [ status 0x0120b40c "service_0120b40c"
  , status 0x0120cab0 "service_0120cab0"
  , status 0x01210250 "service_01210250"
  , status 0x012142f8 "service_012142f8"
  , void 0x0121eb68 "trace"
  , Service 0x0124c6fc "service_0124c6fc" ["r0"] [] none
  ]

machine2Services :: [Service]
machine2Services =
  [ status 0x00ec026c "service_00ec026c"
  , status 0x00ec03dc "service_00ec03dc"
  , void 0x0120c650 "publish_requested_mode"
  , Service 0x0121ca80 "set_work_state" ["r0"] [] none
  , Service 0x0121daa0 "service_0121daa0" ["r0"] [] none
  , void 0x0121daf4 "service_0121daf4"
  , void 0x0121db3c "service_0121db3c"
  , void 0x0121db7c "service_0121db7c"
  , void 0x0121dbb4 "service_0121dbb4"
  , void 0x0121dc40 "service_0121dc40"
  , Service 0x0121dc78 "service_0121dc78" ["r0"] [] none
  , void 0x0121dcac "service_0121dcac"
  , void 0x0121dd98 "service_0121dd98"
  , Service 0x0121ddd0 "service_0121ddd0" ["r0"] [] none
  , status 0x0121ddf0 "service_0121ddf0"
  , status 0x0121de10 "service_0121de10"
  , status 0x0121eabc "service_0121eabc"
  , void 0x0121eb68 "trace"
  ]

machine3Services :: [Service]
machine3Services =
  [ void 0x0121eb68 "trace"
  , void 0x0122062c "service_0122062c"
  , withArgument 0x01220bdc "service_01220bdc"
  , void 0x01220c94 "service_01220c94"
  , void 0x01220cb8 "service_01220cb8"
  , status 0x01220cc8 "service_01220cc8"
  , status 0x01220dd0 "service_01220dd0"
  , void 0x01220e68 "service_01220e68"
  , withArgument 0x01220f34 "service_01220f34"
  , status 0x01220f4c "service_01220f4c"
  , void 0x01220fd0 "service_01220fd0"
  , void 0x01220fe0 "service_01220fe0"
  , status 0x01220ff0 "service_01220ff0"
  ]

machine4Services :: [Service]
machine4Services =
  [ void 0x0120e6e8 "service_0120e6e8"
  , void 0x0121eb68 "trace"
  , void 0x0123261c "service_0123261c"
  , void 0x012390d4 "service_012390d4"
  , status 0x01239178 "service_01239178"
  , void 0x01249f84 "service_01249f84"
  , void 0x01249fa0 "service_01249fa0"
  , void 0x0124a010 "service_0124a010"
  , status 0x0124a0a8 "service_0124a0a8"
  , status 0x0124a0d4 "service_0124a0d4"
  , status 0x0124a3c0 "service_0124a3c0"
  , void 0x0124a42c "service_0124a42c"
  , void 0x0124a480 "service_0124a480"
  , void 0x0124a4b0 "service_0124a4b0"
  , status 0x0124a518 "service_0124a518"
  , status 0x0124a54c "service_0124a54c"
  , status 0x0124a628 "service_0124a628"
  , void 0x0124a650 "service_0124a650"
  , void 0x0124a66c "service_0124a66c"
  , void 0x0124a688 "service_0124a688"
  , void 0x0124a698 "service_0124a698"
  , void 0x0124a8c0 "service_0124a8c0"
  , Service 0x0124a8d4 "service_0124a8d4" ["r0"] [] none
  , void 0x0124a8fc "service_0124a8fc"
  ]

machine5Services :: [Service]
machine5Services =
  [ void 0x0121eb68 "trace"
  , void 0x012200c4 "service_012200c4"
  , void 0x012200ec "service_012200ec"
  , void 0x01220120 "service_01220120"
  , void 0x01220238 "service_01220238"
  , void 0x0122026c "service_0122026c"
  , void 0x01220294 "service_01220294"
  ]

machine6Services :: [Service]
machine6Services =
  [ void 0x01220314 "service_01220314"
  , void 0x01220340 "service_01220340"
  , void 0x01220378 "service_01220378"
  , void 0x012203a4 "service_012203a4"
  , void 0x012203d0 "service_012203d0"
  , Service 0x012203f4 "service_012203f4" ["r0"] [] none
  , void 0x01220480 "service_01220480"
  , void 0x012204cc "service_012204cc"
  , void 0x01220560 "service_01220560"
  , void 0x0121eb68 "trace"
  ]

machine8Services :: [Service]
machine8Services =
  [ void 0x0121de50 "service_0121de50"
  , void 0x0121de64 "service_0121de64"
  , Service 0x0121de80 "service_0121de80" ["r0"] [] none
  , Service 0x0121ded4 "service_0121ded4" ["r0"] [] none
  , void 0x0121df20 "service_0121df20"
  , void 0x0121df30 "service_0121df30"
  ]

machine34Services :: [Service]
machine34Services =
  [ status 0x0120b9e0 "service_0120b9e0"
  , status 0x0120d22c "service_0120d22c"
  , status 0x0120e42c "service_0120e42c"
  , status 0x0120e830 "status_effect"
  , status 0x0120eb10 "service_0120eb10"
  , status 0x012108d0 "service_012108d0"
  , status 0x012110f8 "service_012110f8"
  , status 0x01211dd0 "service_01211dd0"
  , status 0x01213c50 "service_01213c50"
  , void 0x0121eb68 "trace"
  ]

machine44Services :: [Service]
machine44Services =
  [ status 0x0120bc20 "dispatch_machine3_events"
  , status 0x0120bfd8 "service_0120bfd8"
  , status 0x0120c1a8 "service_0120c1a8"
  , void 0x0121eb68 "trace"
  , void 0x0121ff9c "service_0121ff9c"
  , void 0x0121ffc0 "service_0121ffc0"
  , void 0x012202d0 "service_012202d0"
  , void 0x012205e4 "service_012205e4"
  , Service 0x012293a8 "service_012293a8" ["r0"] [] none
  , Service 0x0123d3d4 "service_0123d3d4" ["r0"] [] none
  , Service 0x0123d3f4 "service_0123d3f4" ["r0"] [] none
  , status 0x0123d414 "service_0123d414"
  , status 0x0123d454 "service_0123d454"
  , status 0x0123d4d0 "service_0123d4d0"
  , void 0x0123d54c "service_0123d54c"
  , status 0x0123d584 "service_0123d584"
  , void 0x0123d87c "service_0123d87c"
  , void 0x0123d89c "service_0123d89c"
  , void 0x0123d8bc "service_0123d8bc"
  , void 0x0123da10 "service_0123da10"
  , void 0x0123db1c "service_0123db1c"
  , void 0x0123dc24 "service_0123dc24"
  , void 0x0123dd1c "service_0123dd1c"
  , void 0x0123dd38 "service_0123dd38"
  , void 0x0123ddc0 "service_0123ddc0"
  , status 0x0123de08 "service_0123de08"
  , void 0x0123de38 "service_0123de38"
  , void 0x0123df40 "service_0123df40"
  , void 0x0123e084 "service_0123e084"
  , void 0x0123e098 "service_0123e098"
  ]

machine45Services :: [Service]
machine45Services =
  [ status 0x0120c1a8 "service_0120c1a8"
  , void 0x0121eb68 "trace"
  , void 0x012202d0 "service_012202d0"
  , Service 0x0123a7cc "service_0123a7cc" ["r0"] [] none
  , Service 0x0123a7ec "service_0123a7ec" ["r0"] [] none
  , status 0x0123a80c "service_0123a80c"
  , status 0x0123aa04 "service_0123aa04"
  , status 0x0123aa84 "service_0123aa84"
  , status 0x0123ab00 "service_0123ab00"
  , Service 0x0123ab84 "service_0123ab84" ["r0"] [] none
  , status 0x0123b228 "service_0123b228"
  ]

machine46Services :: [Service]
machine46Services =
  [ status 0x0120bc20 "dispatch_machine3_events"
  , status 0x0120bfd8 "service_0120bfd8"
  , status 0x0120c1a8 "service_0120c1a8"
  , void 0x0121eb68 "trace"
  , void 0x0121ffc0 "service_0121ffc0"
  , void 0x012202d0 "service_012202d0"
  , void 0x012205e4 "service_012205e4"
  , Service 0x0123b4b4 "service_0123b4b4" ["r0"] [] none
  , Service 0x0123b4d4 "service_0123b4d4" ["r0"] [] none
  , status 0x0123b4f4 "service_0123b4f4"
  , status 0x0123b5a8 "service_0123b5a8"
  , void 0x0123b688 "service_0123b688"
  , void 0x0123b76c "service_0123b76c"
  , void 0x0123b960 "service_0123b960"
  , void 0x0123ba6c "service_0123ba6c"
  , void 0x0123bb74 "service_0123bb74"
  , status 0x0123bf2c "service_0123bf2c"
  ]

machine47Services :: [Service]
machine47Services =
  [ status 0x0120fe64 "service_0120fe64"
  , status 0x0120ff70 "service_0120ff70"
  , status 0x012100d8 "service_012100d8"
  , status 0x012136fc "service_012136fc"
  , void 0x0123a724 "service_0123a724"
  , void 0x0123b470 "service_0123b470"
  , Service 0x0123c2c8 "service_0123c2c8" ["r0"] [] none
  , Service 0x0123c2e8 "service_0123c2e8" ["r0"] [] none
  , Service 0x0123c308 "service_0123c308" ["r0"] [] none
  , void 0x0123c328 "service_0123c328"
  , void 0x0123c490 "service_0123c490"
  , void 0x0123c4c8 "service_0123c4c8"
  , status 0x0123cb4c "service_0123cb4c"
  , void 0x0123cba8 "service_0123cba8"
  , void 0x0123cbe0 "service_0123cbe0"
  , void 0x0123cc18 "service_0123cc18"
  , void 0x0123ce14 "service_0123ce14"
  , void 0x0123cf48 "service_0123cf48"
  , void 0x0123d078 "service_0123d078"
  , void 0x0123d16c "service_0123d16c"
  , status 0x0123d24c "service_0123d24c"
  , void 0x0123d264 "service_0123d264"
  , void 0x0123d390 "service_0123d390"
  , void 0x0123e298 "service_0123e298"
  ]

machine56Services :: [Service]
machine56Services =
  [ status 0x01211250 "service_01211250"
  , status 0x0121132c "service_0121132c"
  , status 0x012135e8 "service_012135e8"
  , void 0x0121eb68 "trace"
  , void 0x0122b3f0 "service_0122b3f0"
  , void 0x0122b440 "service_0122b440"
  , void 0x0122b468 "service_0122b468"
  , status 0x0122b800 "service_0122b800"
  ]

machine57Services :: [Service]
machine57Services =
  [ void 0x0121eb68 "trace"
  , Service 0x0122b48c "service_0122b48c" ["r0"] [] none
  , void 0x0122b4d8 "service_0122b4d8"
  , void 0x0122b504 "service_0122b504"
  ]

machine66Services :: [Service]
machine66Services =
  [ status 0x01211e78 "service_01211e78"
  , status 0x01211fdc "service_01211fdc"
  , status 0x01212120 "service_01212120"
  , status 0x01212310 "service_01212310"
  , void 0x0121eb68 "trace"
  ]

machine70Services :: [Service]
machine70Services =
  [ status 0x00ec03dc "service_00ec03dc"
  , void 0x0121eb68 "trace"
  , status 0x0122c468 "service_0122c468"
  , void 0x01233dcc "service_01233dcc"
  , void 0x01233e0c "service_01233e0c"
  , Service 0x0123265c "service_0123265c" ["r0"] [] none
  , Service 0x01251c80 "service_01251c80" ["r0"] [] none
  , Service 0x01251cd0 "service_01251cd0" ["r0"] [] none
  , void 0x01251de8 "service_01251de8"
  , Service 0x01251e10 "service_01251e10" ["r0"] [] none
  , void 0x01251e40 "service_01251e40"
  , void 0x01251e80 "service_01251e80"
  , void 0x01251ea8 "service_01251ea8"
  , Service 0x01251f24 "service_01251f24" ["r0"] [] none
  , void 0x01251fc8 "service_01251fc8"
  , status 0x0125202c "service_0125202c"
  , Service 0x01252064 "service_01252064" ["r0"] [] none
  , Service 0x012520b0 "service_012520b0" ["r0"] [] none
  , void 0x012520ec "service_012520ec"
  , void 0x01252328 "service_01252328"
  , void 0x0125233c "service_0125233c"
  , Service 0x01252374 "service_01252374" ["r0"] [] none
  , status 0x01252430 "service_01252430"
  , status 0x01252464 "service_01252464"
  , void 0x01252490 "service_01252490"
  , void 0x012524a0 "service_012524a0"
  ]
