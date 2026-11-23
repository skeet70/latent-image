//! The retained services, guest state and the action exports the republished action-table
//! rows name.

use crate::machine0::{self, Call, Source, State, WorkState};
use crate::machine30;
use crate::machine7;
use crate::machine61;
use crate::DevicePacket;
use core::mem::MaybeUninit;
use core::ptr::{read_volatile, write_volatile};

const PACKET_FRAME_WORDS: usize = 20;

const MACHINE_STATES: u32 = 0x014e_a5ac;
const MACHINE0_STATE: u32 = MACHINE_STATES;
const MACHINE7_STATE: u32 = MACHINE_STATES + 2 * 7;
const MACHINE30_STATE: u32 = MACHINE_STATES + 2 * 30;
const MACHINE61_STATE: u32 = MACHINE_STATES + 2 * 61;
const MODE_STATE: u32 = 0x014e_a678;
const BUFFER_STEP: u32 = MODE_STATE + 0x18;
const MODE_VALUE: u32 = MODE_STATE + 0x1c;
const REQUEST: u32 = crate::request::BLOCK;
const REQUEST_EVENT: u32 = crate::request::Message119::Event.address();
const TRANSFER: u32 = 0x014e_aed8;
const TRANSFER_STEP: u32 = TRANSFER + machine30::Word::Step as u32;
const STEP: u32 = 0x014e_b5d0;
const STEP_KIND: u32 = STEP + 4;
const SELECTION: u32 = 0x014e_a9bc;

extern "C" {
    fn playback_status_parameter() -> i32;
    fn playback_status_navigation() -> i32;
    fn playback_status_parameter_transition() -> i32;
    fn playback_status_machine20() -> i32;
    fn playback_status_buffer_mode() -> i32;
    fn playback_status_effect() -> i32;
    fn playback_status_machine34() -> i32;
    fn playback_status_message() -> i32;
    fn playback_publish_requested_mode();
    fn playback_set_work_state(state: u32);
    fn playback_submit_indexed_storage_buffer();
    fn playback_apply_selected_record_flags();
    fn playback_write_error_trace();
    fn playback_source_error_notification();
    fn playback_notify_primary();
    fn playback_consume_secondary();
    fn playback_notify_081f();
    fn playback_notify_0819();
    fn playback_notify_084e();
    fn playback_bind_selected_record();
    fn playback_notify_0850();
    fn playback_notify_084f();
    fn playback_cancel_buffer_step();
    fn playback_callback_ready();
    fn playback_post_pending_event();
    fn playback_apply_selected_mode_options();
    fn playback_reset_workspaces();
    fn playback_notify_0821();
    fn playback_trace() -> i32;
    fn playback_notify_0825();
    fn playback_notify_0826();
    fn playback_device_notification(packet: *const u32) -> i32;
}

extern "C" {
    fn playback_service_0120ec28() -> i32;
    fn playback_service_01212a48() -> i32;
    fn playback_service_0120d618() -> i32;
    fn playback_service_0120e6e8() -> i32;
    fn playback_service_0120e964() -> i32;
    fn playback_service_0120f328() -> i32;
    fn playback_service_0120f420() -> i32;
    fn playback_service_0120fc08() -> i32;
    fn playback_service_0121164c() -> i32;
    fn playback_service_01211c8c() -> i32;
    fn playback_service_012128a0() -> i32;
    fn playback_service_012138ac() -> i32;
    fn playback_service_0121406c() -> i32;
    fn playback_service_012290cc(argument: u32) -> i32;
    fn playback_service_012290fc(argument: u32) -> i32;
    fn playback_service_012390d4() -> i32;
    fn playback_service_01239134() -> i32;
    fn playback_service_01239178() -> i32;
}

extern "C" {
    fn playback_dispatch_machine3_events() -> i32;
    fn playback_dispatch_machine46_events() -> i32;
    fn playback_select_source_kind22() -> i32;
    fn playback_fill_control_mode_frame() -> i32;
    fn playback_prepare_selected_source_event() -> i32;
    fn playback_prepare_selected_transfer_event() -> i32;
    fn playback_service_0120ce60() -> i32;
    fn playback_service_012119a4() -> i32;
    fn playback_service_0121c9f8() -> i32;
    fn playback_service_012205e4() -> i32;
    fn playback_service_0122586c() -> i32;
    fn playback_service_012258bc() -> i32;
    fn playback_service_01225928() -> i32;
    fn playback_service_01225984(argument: u32) -> i32;
    fn playback_service_012259a4(argument: u32) -> i32;
    fn playback_service_01225a98() -> i32;
    fn playback_service_01225b20() -> i32;
    fn playback_service_01225bfc() -> i32;
    fn playback_service_01225d08() -> i32;
    fn playback_service_01225f64() -> i32;
    fn playback_service_01225fb0() -> i32;
    fn playback_service_01226014() -> i32;
    fn playback_service_01226230() -> i32;
    fn playback_service_012262fc(argument: u32) -> i32;
    fn playback_service_012263ec() -> i32;
    fn playback_service_012264fc() -> i32;
    fn playback_service_01226518() -> i32;
    fn playback_service_012265cc() -> i32;
    fn playback_service_01226684() -> i32;
    fn playback_service_0122669c() -> i32;
    fn playback_service_01229068() -> i32;
    fn playback_service_012293a8(argument: u32) -> i32;
    fn playback_service_012294cc() -> i32;
    fn playback_service_0122962c() -> i32;
    fn playback_service_0122968c() -> i32;
    fn playback_service_012296dc(argument: u32) -> i32;
    fn playback_service_01229800() -> i32;
    fn playback_service_01229824() -> i32;
    fn playback_service_012298a4() -> i32;
    fn playback_service_01236698() -> i32;
    fn playback_service_012377d4() -> i32;
    fn playback_service_0123c224() -> i32;
    fn playback_service_0123c268() -> i32;
    fn playback_service_01242fa4(argument: u32) -> i32;
    fn playback_service_01243178(argument: u32) -> i32;
    fn playback_service_01243198(argument: u32) -> i32;
    fn playback_service_012431b8() -> i32;
    fn playback_service_01243868() -> i32;
    fn playback_service_01243e48() -> i32;
    fn playback_service_01243fa0() -> i32;
    fn playback_service_012441d0(argument: u32) -> i32;
    fn playback_service_012441f0() -> i32;
}

extern "C" {
    fn playback_service_0120c1a8() -> i32;
    fn playback_service_0121eda0() -> i32;
    fn playback_service_0121ef68() -> i32;
    fn playback_service_0121f0fc() -> i32;
    fn playback_service_0121f118() -> i32;
    fn playback_service_0121f1c4() -> i32;
    fn playback_service_0121f1e8() -> i32;
    fn playback_service_0121f20c() -> i32;
    fn playback_service_0121f224() -> i32;
    fn playback_service_0121f254() -> i32;
    fn playback_service_0121f2a4() -> i32;
    fn playback_service_0121f390() -> i32;
    fn playback_service_0121f3ec() -> i32;
    fn playback_service_0121f500() -> i32;
    fn playback_service_0121f518() -> i32;
    fn playback_service_0121f7ac() -> i32;
    fn playback_service_0121f83c() -> i32;
    fn playback_service_0121f85c() -> i32;
    fn playback_service_012202d0() -> i32;
    fn playback_service_01229f84(argument: u32) -> i32;
    fn playback_service_0123265c(argument: u32) -> i32;
    fn playback_service_01243590() -> i32;
    fn playback_service_01243644() -> i32;
    fn playback_service_01243760() -> i32;
    fn playback_service_012437bc() -> i32;
    fn playback_service_01243890() -> i32;
    fn playback_service_012438f0() -> i32;
    fn playback_service_01243948(argument: u32) -> i32;
    fn playback_service_01243a6c() -> i32;
    fn playback_service_01245c7c(argument: u32) -> i32;
}

struct Stock;

fn notify(packet: &DevicePacket) {
    let mut frame = MaybeUninit::<[u32; PACKET_FRAME_WORDS]>::uninit();
    let words = frame.as_mut_ptr() as *mut u32;
    unsafe {
        write_volatile(words, packet.code);
        write_volatile(words.add(1), packet.value);
        playback_device_notification(words);
    }
}

fn mode_value() -> u32 {
    unsafe { read_volatile(MODE_VALUE as *const u32) }
}

impl machine0::Services for Stock {
    fn status(&mut self, source: Source) -> i32 {
        unsafe {
            match source {
                Source::Parameter => playback_status_parameter(),
                Source::Navigation => playback_status_navigation(),
                Source::ParameterTransition => playback_status_parameter_transition(),
                Source::Machine20 => playback_status_machine20(),
                Source::BufferMode => playback_status_buffer_mode(),
                Source::Effect => playback_status_effect(),
                Source::Machine34 => playback_status_machine34(),
                Source::Message => playback_status_message(),
            }
        }
    }

    fn call(&mut self, call: Call) {
        unsafe {
            match call {
                Call::PublishRequestedMode => playback_publish_requested_mode(),
                Call::SubmitIndexedStorageBuffer => playback_submit_indexed_storage_buffer(),
                Call::ApplySelectedRecordFlags => playback_apply_selected_record_flags(),
                Call::WriteErrorTrace => playback_write_error_trace(),
                Call::SourceErrorNotification => playback_source_error_notification(),
                Call::NotifyPrimary => playback_notify_primary(),
                Call::ConsumeSecondary => playback_consume_secondary(),
                Call::Notify081f => playback_notify_081f(),
                Call::Notify0819 => playback_notify_0819(),
                Call::Notify084e => playback_notify_084e(),
                Call::BindSelectedRecord => playback_bind_selected_record(),
                Call::Notify0850 => playback_notify_0850(),
                Call::Notify084f => playback_notify_084f(),
                Call::CancelBufferStep => playback_cancel_buffer_step(),
                Call::CallbackReady => playback_callback_ready(),
                Call::PostPendingEvent => playback_post_pending_event(),
                Call::ApplySelectedModeOptions => playback_apply_selected_mode_options(),
                Call::ResetWorkspaces => playback_reset_workspaces(),
                Call::Notify0821 => playback_notify_0821(),
                Call::Trace => {
                    playback_trace();
                }
                Call::Notify0825 => playback_notify_0825(),
                Call::Notify0826 => playback_notify_0826(),
            }
        }
    }

    fn set_state(&mut self, state: State) {
        unsafe { write_volatile(MACHINE0_STATE as *mut u16, state as u16) }
    }

    fn set_work_state(&mut self, state: WorkState) {
        unsafe { playback_set_work_state(state as u32) }
    }

    fn notify_device(&mut self, packet: &DevicePacket) {
        notify(packet)
    }

    fn mode_value(&self) -> u32 {
        mode_value()
    }

    fn clear_buffer_step(&mut self) {
        unsafe { write_volatile(BUFFER_STEP as *mut u32, 0) }
    }
}

impl machine7::Services for Stock {
    fn call(&mut self, call: machine7::Call) -> i32 {
        use machine7::Call as C;
        unsafe {
            match call {
                C::Fn120d618 => playback_service_0120d618(),
                C::Fn120e6e8 => playback_service_0120e6e8(),
                C::Fn120e964 => playback_service_0120e964(),
                C::Fn120ec28 => playback_service_0120ec28(),
                C::Fn120f328 => playback_service_0120f328(),
                C::Fn120f420 => playback_service_0120f420(),
                C::Fn120fc08 => playback_service_0120fc08(),
                C::Fn121164c => playback_service_0121164c(),
                C::Fn1211c8c => playback_service_01211c8c(),
                C::Fn12128a0 => playback_service_012128a0(),
                C::Fn1212a48 => playback_service_01212a48(),
                C::Fn12138ac => playback_service_012138ac(),
                C::Fn121406c => playback_service_0121406c(),
                C::Trace => playback_trace(),
                C::Fn12390d4 => playback_service_012390d4(),
                C::Fn1239134 => playback_service_01239134(),
                C::Fn1239178 => playback_service_01239178(),
            }
        }
    }

    fn call_with(&mut self, call: machine7::CallWith, argument: u32) {
        unsafe {
            match call {
                machine7::CallWith::Fn12290cc => playback_service_012290cc(argument),
                machine7::CallWith::Fn12290fc => playback_service_012290fc(argument),
            };
        }
    }

    fn set_state(&mut self, state: machine7::State) {
        unsafe { write_volatile(MACHINE7_STATE as *mut u16, state as u16) }
    }

    fn request_event(&self) -> u32 {
        unsafe { read_volatile(REQUEST_EVENT as *const u32) }
    }

    fn step(&self) -> u32 {
        unsafe { read_volatile(STEP as *const u32) }
    }

    fn step_kind(&self) -> u16 {
        unsafe { read_volatile(STEP_KIND as *const u16) }
    }

    fn set_step_kind(&mut self, kind: u16) {
        unsafe { write_volatile(STEP_KIND as *mut u16, kind) }
    }
}

impl machine30::Services for Stock {
    fn call(&mut self, call: machine30::Call) -> i32 {
        use machine30::Call as C;
        unsafe {
            match call {
                C::Dispatch3Events => playback_dispatch_machine3_events(),
                C::Fn120ce60 => playback_service_0120ce60(),
                C::Fn120ec28 => playback_service_0120ec28(),
                C::Dispatch46Events => playback_dispatch_machine46_events(),
                C::Fn12119a4 => playback_service_012119a4(),
                C::Fn1212a48 => playback_service_01212a48(),
                C::Fn121c9f8 => playback_service_0121c9f8(),
                C::Trace => playback_trace(),
                C::Fn12205e4 => playback_service_012205e4(),
                C::Fn122586c => playback_service_0122586c(),
                C::Fn12258bc => playback_service_012258bc(),
                C::Fn1225928 => playback_service_01225928(),
                C::Fn1225a98 => playback_service_01225a98(),
                C::Fn1225b20 => playback_service_01225b20(),
                C::Fn1225bfc => playback_service_01225bfc(),
                C::Fn1225d08 => playback_service_01225d08(),
                C::Fn1225f64 => playback_service_01225f64(),
                C::Fn1225fb0 => playback_service_01225fb0(),
                C::SelectSourceKind22 => playback_select_source_kind22(),
                C::Fn1226014 => playback_service_01226014(),
                C::Fn1226230 => playback_service_01226230(),
                C::Fn12263ec => playback_service_012263ec(),
                C::Fn12264fc => playback_service_012264fc(),
                C::Fn1226518 => playback_service_01226518(),
                C::Fn12265cc => playback_service_012265cc(),
                C::Fn1226684 => playback_service_01226684(),
                C::Fn122669c => playback_service_0122669c(),
                C::FillControlModeFrame => playback_fill_control_mode_frame(),
                C::Fn1229068 => playback_service_01229068(),
                C::PrepareSelectedSourceEvent => playback_prepare_selected_source_event(),
                C::PrepareSelectedTransferEvent => playback_prepare_selected_transfer_event(),
                C::Fn12294cc => playback_service_012294cc(),
                C::Fn122962c => playback_service_0122962c(),
                C::Fn122968c => playback_service_0122968c(),
                C::Fn1229800 => playback_service_01229800(),
                C::Fn1229824 => playback_service_01229824(),
                C::Fn12298a4 => playback_service_012298a4(),
                C::Fn1236698 => playback_service_01236698(),
                C::Fn12377d4 => playback_service_012377d4(),
                C::Fn123c224 => playback_service_0123c224(),
                C::Fn123c268 => playback_service_0123c268(),
                C::Fn12431b8 => playback_service_012431b8(),
                C::Fn1243868 => playback_service_01243868(),
                C::Fn1243e48 => playback_service_01243e48(),
                C::Fn1243fa0 => playback_service_01243fa0(),
                C::Fn12441f0 => playback_service_012441f0(),
                C::Fn1243178Step => playback_service_01243178(TRANSFER_STEP),
                C::Fn1242fa4Step => playback_service_01242fa4(TRANSFER_STEP),
                C::Fn12293a8Step => playback_service_012293a8(TRANSFER_STEP),
            }
        }
    }

    fn call_with(&mut self, call: machine30::CallWith, argument: u32) -> i32 {
        use machine30::CallWith as C;
        unsafe {
            match call {
                C::SetPhase => playback_service_01225984(argument),
                C::StartStep => playback_service_012259a4(argument),
                C::Fn12262fc => playback_service_012262fc(argument),
                C::Fn12296dc => playback_service_012296dc(argument),
                C::Fn1243198 => playback_service_01243198(argument),
                C::Fn12441d0 => playback_service_012441d0(argument),
            }
        }
    }

    fn set_state(&mut self, state: machine30::State) {
        unsafe { write_volatile(MACHINE30_STATE as *mut u16, state as u16) }
    }

    fn word(&self, word: machine30::Word) -> u32 {
        unsafe { read_volatile((TRANSFER + word as u32) as *const u32) }
    }

    fn set_word(&mut self, word: machine30::Word, value: u32) {
        unsafe { write_volatile((TRANSFER + word as u32) as *mut u32, value) }
    }

    fn byte(&self, byte: machine30::Byte) -> u8 {
        unsafe { read_volatile((TRANSFER + byte as u32) as *const u8) }
    }

    fn set_byte(&mut self, byte: machine30::Byte, value: u8) {
        unsafe { write_volatile((TRANSFER + byte as u32) as *mut u8, value) }
    }

    fn mode_value(&self) -> u32 {
        mode_value()
    }

    fn notify_device(&mut self, packet: &DevicePacket) {
        notify(packet)
    }

    fn post_request(&mut self, code: u16, event: u32) {
        unsafe {
            write_volatile(REQUEST as *mut u16, code);
            write_volatile(REQUEST_EVENT as *mut u32, event);
        }
    }
}

impl machine61::Services for Stock {
    fn call(&mut self, call: machine61::Call) -> i32 {
        use machine61::Call as C;
        unsafe {
            match call {
                C::Fn120c1a8 => playback_service_0120c1a8(),
                C::Trace => playback_trace(),
                C::Fn121eda0 => playback_service_0121eda0(),
                C::Fn121ef68 => playback_service_0121ef68(),
                C::Fn121f0fc => playback_service_0121f0fc(),
                C::Fn121f118 => playback_service_0121f118(),
                C::Fn121f1c4 => playback_service_0121f1c4(),
                C::Fn121f1e8 => playback_service_0121f1e8(),
                C::Fn121f20c => playback_service_0121f20c(),
                C::Fn121f224 => playback_service_0121f224(),
                C::Fn121f254 => playback_service_0121f254(),
                C::Fn121f2a4 => playback_service_0121f2a4(),
                C::Fn121f390 => playback_service_0121f390(),
                C::Fn121f3ec => playback_service_0121f3ec(),
                C::Fn121f500 => playback_service_0121f500(),
                C::Fn121f518 => playback_service_0121f518(),
                C::Fn121f7ac => playback_service_0121f7ac(),
                C::Fn121f83c => playback_service_0121f83c(),
                C::Fn121f85c => playback_service_0121f85c(),
                C::Fn12202d0 => playback_service_012202d0(),
                C::Fn1243590 => playback_service_01243590(),
                C::Fn1243644 => playback_service_01243644(),
                C::Fn1243760 => playback_service_01243760(),
                C::Fn12437bc => playback_service_012437bc(),
                C::Fn1243868 => playback_service_01243868(),
                C::Fn1243890 => playback_service_01243890(),
                C::Fn12438f0 => playback_service_012438f0(),
                C::Fn1243a6c => playback_service_01243a6c(),
            }
        }
    }

    fn call_with(&mut self, call: machine61::CallWith, argument: u32) -> i32 {
        use machine61::CallWith as C;
        unsafe {
            match call {
                C::Fn1229f84 => playback_service_01229f84(argument),
                C::Fn123265c => playback_service_0123265c(argument),
                C::Fn1243198 => playback_service_01243198(argument),
                C::Fn1243948 => playback_service_01243948(argument),
                C::Fn1245c7c => playback_service_01245c7c(argument),
            }
        }
    }

    fn set_state(&mut self, state: machine61::State) {
        unsafe { write_volatile(MACHINE61_STATE as *mut u16, state as u16) }
    }

    fn request(&self, word: machine61::Request) -> u32 {
        unsafe { read_volatile((REQUEST + word as u32) as *const u32) }
    }

    fn byte_at(&self, address: u32) -> u8 {
        unsafe { read_volatile(address as *const u8) }
    }

    fn selection(&self, word: machine61::Selection) -> u32 {
        unsafe { read_volatile((SELECTION + word as u32) as *const u32) }
    }

    fn set_selection(&mut self, word: machine61::Selection, value: u32) {
        unsafe { write_volatile((SELECTION + word as u32) as *mut u32, value) }
    }
}

macro_rules! actions {
    ($machine:ident { $($export:ident => $action:ident),* $(,)? }) => {
        $(
            #[no_mangle]
            pub extern "C" fn $export() -> i32 {
                $machine::$action(&mut Stock)
            }
        )*
    };
}

actions! { machine0 {
    xm1_playback_m0_action02 => action02, xm1_playback_m0_action03 => action03,
    xm1_playback_m0_action04 => action04, xm1_playback_m0_action05 => action05,
    xm1_playback_m0_action06 => action06, xm1_playback_m0_action07 => action07,
    xm1_playback_m0_action08 => action08, xm1_playback_m0_action09 => action09,
    xm1_playback_m0_action10 => action10, xm1_playback_m0_action11 => action11,
    xm1_playback_m0_action12 => action12, xm1_playback_m0_action13 => action13,
    xm1_playback_m0_action14 => action14, xm1_playback_m0_action15 => action15,
    xm1_playback_m0_action16 => action16, xm1_playback_m0_action17 => action17,
    xm1_playback_m0_action18 => action18, xm1_playback_m0_action19 => action19,
    xm1_playback_m0_action20 => action20, xm1_playback_m0_action21 => action21,
    xm1_playback_m0_action22 => action22, xm1_playback_m0_action23 => action23,
    xm1_playback_m0_action24 => action24, xm1_playback_m0_action25 => action25,
    xm1_playback_m0_action26 => action26, xm1_playback_m0_action27 => action27,
    xm1_playback_m0_action28 => action28, xm1_playback_m0_action29 => action29,
    xm1_playback_m0_action30 => action30, xm1_playback_m0_action31 => action31,
    xm1_playback_m0_action32 => action32,
} }

actions! { machine7 {
    xm1_playback_m7_action02 => action02, xm1_playback_m7_action03 => action03,
    xm1_playback_m7_action04 => action04, xm1_playback_m7_action05 => action05,
    xm1_playback_m7_action06 => action06, xm1_playback_m7_action07 => action07,
    xm1_playback_m7_action08 => action08, xm1_playback_m7_action09 => action09,
    xm1_playback_m7_action10 => action10, xm1_playback_m7_action11 => action11,
    xm1_playback_m7_action12 => action12, xm1_playback_m7_action13 => action13,
    xm1_playback_m7_action14 => action14, xm1_playback_m7_action15 => action15,
    xm1_playback_m7_action16 => action16, xm1_playback_m7_action17 => action17,
    xm1_playback_m7_action18 => action18, xm1_playback_m7_action19 => action19,
    xm1_playback_m7_action20 => action20, xm1_playback_m7_action21 => action21,
    xm1_playback_m7_action22 => action22, xm1_playback_m7_action23 => action23,
    xm1_playback_m7_action24 => action24, xm1_playback_m7_action25 => action25,
    xm1_playback_m7_action26 => action26, xm1_playback_m7_action27 => action27,
    xm1_playback_m7_action28 => action28, xm1_playback_m7_action29 => action29,
    xm1_playback_m7_action30 => action30, xm1_playback_m7_action31 => action31,
    xm1_playback_m7_action32 => action32, xm1_playback_m7_action33 => action33,
    xm1_playback_m7_action34 => action34, xm1_playback_m7_action35 => action35,
    xm1_playback_m7_action36 => action36, xm1_playback_m7_action37 => action37,
    xm1_playback_m7_action38 => action38, xm1_playback_m7_action39 => action39,
    xm1_playback_m7_action40 => action40, xm1_playback_m7_action41 => action41,
    xm1_playback_m7_action42 => action42, xm1_playback_m7_action43 => action43,
    xm1_playback_m7_action44 => action44, xm1_playback_m7_action45 => action45,
    xm1_playback_m7_action46 => action46, xm1_playback_m7_action47 => action47,
    xm1_playback_m7_action48 => action48, xm1_playback_m7_action49 => action49,
    xm1_playback_m7_action50 => action50,
} }

actions! { machine30 {
    xm1_playback_m30_action02 => action02, xm1_playback_m30_action03 => action03,
    xm1_playback_m30_action04 => action04, xm1_playback_m30_action05 => action05,
    xm1_playback_m30_action06 => action06, xm1_playback_m30_action07 => action07,
    xm1_playback_m30_action08 => action08, xm1_playback_m30_action09 => action09,
    xm1_playback_m30_action10 => action10, xm1_playback_m30_action11 => action11,
    xm1_playback_m30_action12 => action12, xm1_playback_m30_action13 => action13,
    xm1_playback_m30_action14 => action14, xm1_playback_m30_action15 => action15,
    xm1_playback_m30_action16 => action16, xm1_playback_m30_action17 => action17,
    xm1_playback_m30_action18 => action18, xm1_playback_m30_action19 => action19,
    xm1_playback_m30_action20 => action20, xm1_playback_m30_action21 => action21,
    xm1_playback_m30_action22 => action22, xm1_playback_m30_action23 => action23,
    xm1_playback_m30_action24 => action24, xm1_playback_m30_action25 => action25,
    xm1_playback_m30_action26 => action26, xm1_playback_m30_action27 => action27,
    xm1_playback_m30_action28 => action28, xm1_playback_m30_action29 => action29,
    xm1_playback_m30_action30 => action30, xm1_playback_m30_action31 => action31,
    xm1_playback_m30_action32 => action32, xm1_playback_m30_action33 => action33,
    xm1_playback_m30_action34 => action34, xm1_playback_m30_action35 => action35,
} }

actions! { machine61 {
    xm1_playback_m61_action02 => action02, xm1_playback_m61_action03 => action03,
    xm1_playback_m61_action04 => action04, xm1_playback_m61_action05 => action05,
    xm1_playback_m61_action06 => action06, xm1_playback_m61_action07 => action07,
    xm1_playback_m61_action08 => action08, xm1_playback_m61_action09 => action09,
    xm1_playback_m61_action10 => action10, xm1_playback_m61_action11 => action11,
    xm1_playback_m61_action12 => action12, xm1_playback_m61_action13 => action13,
    xm1_playback_m61_action14 => action14, xm1_playback_m61_action15 => action15,
    xm1_playback_m61_action16 => action16, xm1_playback_m61_action17 => action17,
    xm1_playback_m61_action18 => action18, xm1_playback_m61_action19 => action19,
    xm1_playback_m61_action20 => action20, xm1_playback_m61_action21 => action21,
    xm1_playback_m61_action22 => action22, xm1_playback_m61_action23 => action23,
    xm1_playback_m61_action24 => action24, xm1_playback_m61_action25 => action25,
    xm1_playback_m61_action26 => action26, xm1_playback_m61_action27 => action27,
} }
