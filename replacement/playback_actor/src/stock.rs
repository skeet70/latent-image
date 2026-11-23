//! The retained services, guest state and the action exports the republished action-table
//! rows name.

use crate::machine0::{self, Call, Source, State, WorkState};
use crate::machine1;
use crate::machine2;
use crate::machine3;
use crate::machine30;
use crate::machine34;
use crate::machine4;
use crate::machine44;
use crate::machine45;
use crate::machine46;
use crate::machine47;
use crate::machine5;
use crate::machine56;
use crate::machine57;
use crate::machine6;
use crate::machine61;
use crate::machine66;
use crate::machine7;
use crate::machine70;
use crate::machine8;
use crate::DevicePacket;
use core::mem::MaybeUninit;
use core::ptr::{read_volatile, write_volatile};

const PACKET_FRAME_WORDS: usize = 20;

const MACHINE_STATES: u32 = 0x014e_a5ac;
const MACHINE0_STATE: u32 = MACHINE_STATES;
const MACHINE1_STATE: u32 = MACHINE_STATES + 2;
const MACHINE2_STATE: u32 = MACHINE_STATES + 2 * 2;
const MACHINE3_STATE: u32 = MACHINE_STATES + 2 * 3;
const MACHINE4_STATE: u32 = MACHINE_STATES + 2 * 4;
const MACHINE5_STATE: u32 = MACHINE_STATES + 2 * 5;
const MACHINE6_STATE: u32 = MACHINE_STATES + 2 * 6;
const MACHINE7_STATE: u32 = MACHINE_STATES + 2 * 7;
const MACHINE30_STATE: u32 = MACHINE_STATES + 2 * 30;
const MACHINE34_STATE: u32 = MACHINE_STATES + 2 * 34;
const MACHINE44_STATE: u32 = MACHINE_STATES + 2 * 44;
const MACHINE45_STATE: u32 = MACHINE_STATES + 2 * 45;
const MACHINE46_STATE: u32 = MACHINE_STATES + 2 * 46;
const MACHINE47_STATE: u32 = MACHINE_STATES + 2 * 47;
const MACHINE56_STATE: u32 = MACHINE_STATES + 2 * 56;
const MACHINE57_STATE: u32 = MACHINE_STATES + 2 * 57;
const MACHINE61_STATE: u32 = MACHINE_STATES + 2 * 61;
const MACHINE66_STATE: u32 = MACHINE_STATES + 2 * 66;
const MACHINE70_STATE: u32 = MACHINE_STATES + 2 * 70;
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
const IMAGE: u32 = 0x014e_ad40;
const JPEG: u32 = 0x014e_ae2c;
const STORAGE: u32 = 0x0151_1338;
const STORAGE_ACTIVE: u32 = STORAGE + 1;
const OVERLAY47: u32 = 0x014f_f2f4;
const BLOCK_14FF3C4: u32 = 0x014f_f3c4;
const MACHINE46_BLOCK: u32 = 0x014f_f36c;
const STREAM: u32 = 0x014e_f894;
const SEQUENCE: u32 = 0x0151_3154;
const IO: u32 = 0x014e_aca8;
const IO_COUNT: u32 = IO + 0x20;
const IMAGE_CONTROL: u32 = 0x0151_14f4;
const MEDIA_OUTPUT: u32 = 0x0151_49a0;

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

extern "C" {
    fn playback_service_0120b9e0() -> i32;
    fn playback_service_0120d22c() -> i32;
    fn playback_service_0120e42c() -> i32;
    fn playback_service_0120eb10() -> i32;
    fn playback_service_012108d0() -> i32;
    fn playback_service_012110f8() -> i32;
    fn playback_service_01211dd0() -> i32;
    fn playback_service_01213c50() -> i32;
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

impl machine34::Services for Stock {
    fn call(&mut self, call: machine34::Call) -> i32 {
        use machine34::Call as C;
        unsafe {
            match call {
                C::Fn120b9e0 => playback_service_0120b9e0(),
                C::Fn120d22c => playback_service_0120d22c(),
                C::Fn120e42c => playback_service_0120e42c(),
                C::Fn120e830 => playback_status_effect(),
                C::Fn120eb10 => playback_service_0120eb10(),
                C::Fn12108d0 => playback_service_012108d0(),
                C::Fn12110f8 => playback_service_012110f8(),
                C::Fn1211dd0 => playback_service_01211dd0(),
                C::Fn1213c50 => playback_service_01213c50(),
                C::Trace => playback_trace(),
            }
        }
    }

    fn set_state(&mut self, state: machine34::State) {
        unsafe { write_volatile(MACHINE34_STATE as *mut u16, state as u16) }
    }
}

extern "C" {
    fn playback_service_00ec026c() -> i32;
    fn playback_service_00ec03dc() -> i32;
    fn playback_service_0121daa0(argument: u32) -> i32;
    fn playback_service_0121daf4() -> i32;
    fn playback_service_0121db3c() -> i32;
    fn playback_service_0121db7c() -> i32;
    fn playback_service_0121dbb4() -> i32;
    fn playback_service_0121dc40() -> i32;
    fn playback_service_0121dc78(argument: u32) -> i32;
    fn playback_service_0121dcac() -> i32;
    fn playback_service_0121dd98() -> i32;
    fn playback_service_0121ddd0(argument: u32) -> i32;
    fn playback_service_0121ddf0() -> i32;
    fn playback_service_0121de10() -> i32;
    fn playback_service_0121eabc() -> i32;
}

impl machine2::Services for Stock {
    fn call(&mut self, call: machine2::Call) {
        use machine2::Call as C;
        unsafe {
            match call {
                C::PublishRequestedMode => playback_publish_requested_mode(),
                C::Fn121daf4 => {
                    playback_service_0121daf4();
                }
                C::Fn121db3c => {
                    playback_service_0121db3c();
                }
                C::Fn121db7c => {
                    playback_service_0121db7c();
                }
                C::Fn121dbb4 => {
                    playback_service_0121dbb4();
                }
                C::Fn121dc40 => {
                    playback_service_0121dc40();
                }
                C::Fn121dcac => {
                    playback_service_0121dcac();
                }
                C::Fn121dd98 => {
                    playback_service_0121dd98();
                }
                C::Trace => {
                    playback_trace();
                }
            }
        }
    }

    fn status(&mut self, status: machine2::Status) -> i32 {
        use machine2::Status as S;
        unsafe {
            match status {
                S::Fn0ec026c => playback_service_00ec026c(),
                S::Fn0ec03dc => playback_service_00ec03dc(),
                S::Fn121ddf0 => playback_service_0121ddf0(),
                S::Fn121de10 => playback_service_0121de10(),
                S::Fn121eabc => playback_service_0121eabc(),
            }
        }
    }

    fn call_with(&mut self, call: machine2::CallWith, argument: u32) {
        use machine2::CallWith as C;
        unsafe {
            match call {
                C::Fn121daa0 => playback_service_0121daa0(argument),
                C::Fn121dc78 => playback_service_0121dc78(argument),
                C::Fn121ddd0 => playback_service_0121ddd0(argument),
            };
        }
    }

    fn set_work_state(&mut self, state: machine2::WorkState) {
        unsafe { playback_set_work_state(state as u32) }
    }

    fn set_state(&mut self, state: machine2::State) {
        unsafe { write_volatile(MACHINE2_STATE as *mut u16, state as u16) }
    }

    fn request(&self, word: machine2::Request) -> u32 {
        unsafe { read_volatile((REQUEST + word as u32) as *const u32) }
    }
}

extern "C" {
    fn playback_service_0122062c() -> i32;
    fn playback_service_01220bdc(argument: u32) -> i32;
    fn playback_service_01220c94() -> i32;
    fn playback_service_01220cb8() -> i32;
    fn playback_service_01220cc8() -> i32;
    fn playback_service_01220dd0() -> i32;
    fn playback_service_01220e68() -> i32;
    fn playback_service_01220f34(argument: u32) -> i32;
    fn playback_service_01220f4c() -> i32;
    fn playback_service_01220fd0() -> i32;
    fn playback_service_01220fe0() -> i32;
    fn playback_service_01220ff0() -> i32;
}

impl machine3::Services for Stock {
    fn call(&mut self, call: machine3::Call) -> i32 {
        use machine3::Call as C;
        unsafe {
            match call {
                C::Trace => playback_trace(),
                C::Fn122062c => playback_service_0122062c(),
                C::Fn1220c94 => playback_service_01220c94(),
                C::Fn1220cb8 => playback_service_01220cb8(),
                C::Fn1220cc8 => playback_service_01220cc8(),
                C::Fn1220dd0 => playback_service_01220dd0(),
                C::Fn1220e68 => playback_service_01220e68(),
                C::Fn1220f4c => playback_service_01220f4c(),
                C::Fn1220fd0 => playback_service_01220fd0(),
                C::Fn1220fe0 => playback_service_01220fe0(),
                C::Fn1220ff0 => playback_service_01220ff0(),
            }
        }
    }

    fn call_with(&mut self, call: machine3::CallWith, argument: u32) -> i32 {
        use machine3::CallWith as C;
        unsafe {
            match call {
                C::Fn1220bdc => playback_service_01220bdc(argument),
                C::Fn1220f34 => playback_service_01220f34(argument),
            }
        }
    }

    fn set_state(&mut self, state: machine3::State) {
        unsafe { write_volatile(MACHINE3_STATE as *mut u16, state as u16) }
    }

    fn word(&self, word: machine3::Word) -> u32 {
        unsafe { read_volatile((IMAGE + word as u32) as *const u32) }
    }

    fn set_word(&mut self, word: machine3::Word, value: u32) {
        unsafe { write_volatile((IMAGE + word as u32) as *mut u32, value) }
    }

    fn byte(&self, byte: machine3::Byte) -> u8 {
        unsafe { read_volatile((IMAGE + byte as u32) as *const u8) }
    }
}

extern "C" {
    fn playback_service_0123261c() -> i32;
    fn playback_service_01249f84() -> i32;
    fn playback_service_01249fa0() -> i32;
    fn playback_service_0124a010() -> i32;
    fn playback_service_0124a0a8() -> i32;
    fn playback_service_0124a0d4() -> i32;
    fn playback_service_0124a3c0() -> i32;
    fn playback_service_0124a42c() -> i32;
    fn playback_service_0124a480() -> i32;
    fn playback_service_0124a4b0() -> i32;
    fn playback_service_0124a518() -> i32;
    fn playback_service_0124a54c() -> i32;
    fn playback_service_0124a628() -> i32;
    fn playback_service_0124a650() -> i32;
    fn playback_service_0124a66c() -> i32;
    fn playback_service_0124a688() -> i32;
    fn playback_service_0124a698() -> i32;
    fn playback_service_0124a8c0() -> i32;
    fn playback_service_0124a8d4(argument: u32) -> i32;
    fn playback_service_0124a8fc() -> i32;
}

impl machine4::Services for Stock {
    fn call(&mut self, call: machine4::Call) -> i32 {
        use machine4::Call as C;
        unsafe {
            match call {
                C::Fn120e6e8 => playback_service_0120e6e8(),
                C::Trace => playback_trace(),
                C::Fn123261c => playback_service_0123261c(),
                C::Fn12390d4 => playback_service_012390d4(),
                C::Fn1239178 => playback_service_01239178(),
                C::Fn1249f84 => playback_service_01249f84(),
                C::Fn1249fa0 => playback_service_01249fa0(),
                C::Fn124a010 => playback_service_0124a010(),
                C::Fn124a0a8 => playback_service_0124a0a8(),
                C::Fn124a0d4 => playback_service_0124a0d4(),
                C::Fn124a3c0 => playback_service_0124a3c0(),
                C::Fn124a42c => playback_service_0124a42c(),
                C::Fn124a480 => playback_service_0124a480(),
                C::Fn124a4b0 => playback_service_0124a4b0(),
                C::Fn124a518 => playback_service_0124a518(),
                C::Fn124a54c => playback_service_0124a54c(),
                C::Fn124a628 => playback_service_0124a628(),
                C::Fn124a650 => playback_service_0124a650(),
                C::Fn124a66c => playback_service_0124a66c(),
                C::Fn124a688 => playback_service_0124a688(),
                C::Fn124a698 => playback_service_0124a698(),
                C::Fn124a8c0 => playback_service_0124a8c0(),
                C::Fn124a8fc => playback_service_0124a8fc(),
            }
        }
    }

    fn complete(&mut self, bit: machine4::Completion) {
        unsafe {
            playback_service_0124a8d4(bit as u32);
        }
    }

    fn set_state(&mut self, state: machine4::State) {
        unsafe { write_volatile(MACHINE4_STATE as *mut u16, state as u16) }
    }

    fn storage(&self, word: machine4::Storage) -> u32 {
        unsafe { read_volatile((STORAGE + word as u32) as *const u32) }
    }

    fn set_storage(&mut self, word: machine4::Storage, value: u32) {
        unsafe { write_volatile((STORAGE + word as u32) as *mut u32, value) }
    }

    fn set_active(&mut self) {
        unsafe { write_volatile(STORAGE_ACTIVE as *mut u8, 1) }
    }
}

extern "C" {
    fn playback_service_012200c4() -> i32;
    fn playback_service_012200ec() -> i32;
    fn playback_service_01220120() -> i32;
    fn playback_service_01220238() -> i32;
    fn playback_service_0122026c() -> i32;
    fn playback_service_01220294() -> i32;
}

impl machine5::Services for Stock {
    fn call(&mut self, call: machine5::Call) {
        use machine5::Call as C;
        unsafe {
            match call {
                C::Trace => playback_trace(),
                C::Fn12200c4 => playback_service_012200c4(),
                C::Fn12200ec => playback_service_012200ec(),
                C::Fn1220120 => playback_service_01220120(),
                C::Fn1220238 => playback_service_01220238(),
                C::Fn122026c => playback_service_0122026c(),
                C::Fn1220294 => playback_service_01220294(),
            };
        }
    }

    fn set_state(&mut self, state: machine5::State) {
        unsafe { write_volatile(MACHINE5_STATE as *mut u16, state as u16) }
    }

    fn word(&self, word: machine5::Word) -> u32 {
        unsafe { read_volatile((JPEG + word as u32) as *const u32) }
    }

    fn set_word(&mut self, word: machine5::Word, value: u32) {
        unsafe { write_volatile((JPEG + word as u32) as *mut u32, value) }
    }

    fn byte(&self, byte: machine5::Byte) -> u8 {
        unsafe { read_volatile((JPEG + byte as u32) as *const u8) }
    }
}

extern "C" {
    fn playback_service_0123a7cc(argument: u32);
    fn playback_service_0123a7ec(argument: u32);
    fn playback_service_0123a80c() -> i32;
    fn playback_service_0123aa04() -> i32;
    fn playback_service_0123aa84() -> i32;
    fn playback_service_0123ab00() -> i32;
    fn playback_service_0123ab84(argument: u32);
    fn playback_service_0123b228() -> i32;
}

impl machine45::Services for Stock {
    fn call(&mut self, call: machine45::Call) -> i32 {
        use machine45::Call as C;
        unsafe {
            match call {
                C::Fn120c1a8 => playback_service_0120c1a8(),
                C::Trace => playback_trace(),
                C::Fn12202d0 => playback_service_012202d0(),
                C::Fn123a80c => playback_service_0123a80c(),
                C::Fn123aa04 => playback_service_0123aa04(),
                C::Fn123aa84 => playback_service_0123aa84(),
                C::Fn123ab00 => playback_service_0123ab00(),
                C::Fn123b228 => playback_service_0123b228(),
            }
        }
    }

    fn call_with(&mut self, call: machine45::CallWith, argument: u32) {
        use machine45::CallWith as C;
        unsafe {
            match call {
                C::Fn123a7cc => playback_service_0123a7cc(argument),
                C::Fn123a7ec => playback_service_0123a7ec(argument),
                C::Fn123ab84 => playback_service_0123ab84(argument),
            }
        }
    }

    fn set_state(&mut self, state: machine45::State) {
        unsafe { write_volatile(MACHINE45_STATE as *mut u16, state as u16) }
    }

    fn overlay47(&self, word: machine45::Overlay47) -> u32 {
        unsafe { read_volatile((OVERLAY47 + word as u32) as *const u32) }
    }

    fn word_at(&self, address: u32) -> u32 {
        unsafe { read_volatile(address as *const u32) }
    }
}

extern "C" {
    fn playback_service_0120fe64() -> i32;
    fn playback_service_0120ff70() -> i32;
    fn playback_service_012100d8() -> i32;
    fn playback_service_012136fc() -> i32;
    fn playback_service_0123a724() -> i32;
    fn playback_service_0123b470() -> i32;
    fn playback_service_0123c2c8(argument: u32) -> i32;
    fn playback_service_0123c2e8(argument: u32) -> i32;
    fn playback_service_0123c308(argument: u32) -> i32;
    fn playback_service_0123c328() -> i32;
    fn playback_service_0123c490() -> i32;
    fn playback_service_0123c4c8() -> i32;
    fn playback_service_0123cb4c() -> i32;
    fn playback_service_0123cba8() -> i32;
    fn playback_service_0123cbe0() -> i32;
    fn playback_service_0123cc18() -> i32;
    fn playback_service_0123ce14() -> i32;
    fn playback_service_0123cf48() -> i32;
    fn playback_service_0123d078() -> i32;
    fn playback_service_0123d16c() -> i32;
    fn playback_service_0123d24c() -> i32;
    fn playback_service_0123d264() -> i32;
    fn playback_service_0123d390() -> i32;
    fn playback_service_0123e298() -> i32;
}

impl machine47::Services for Stock {
    fn call(&mut self, call: machine47::Call) -> i32 {
        use machine47::Call as C;
        unsafe {
            match call {
                C::Fn120fe64 => playback_service_0120fe64(),
                C::Fn120ff70 => playback_service_0120ff70(),
                C::Fn12100d8 => playback_service_012100d8(),
                C::Fn12136fc => playback_service_012136fc(),
                C::Trace => playback_trace(),
                C::Fn123a724 => playback_service_0123a724(),
                C::Fn123b470 => playback_service_0123b470(),
                C::Fn123c328 => playback_service_0123c328(),
                C::Fn123c490 => playback_service_0123c490(),
                C::Fn123c4c8 => playback_service_0123c4c8(),
                C::Fn123cb4c => playback_service_0123cb4c(),
                C::Fn123cba8 => playback_service_0123cba8(),
                C::Fn123cbe0 => playback_service_0123cbe0(),
                C::Fn123cc18 => playback_service_0123cc18(),
                C::Fn123ce14 => playback_service_0123ce14(),
                C::Fn123cf48 => playback_service_0123cf48(),
                C::Fn123d078 => playback_service_0123d078(),
                C::Fn123d16c => playback_service_0123d16c(),
                C::Fn123d24c => playback_service_0123d24c(),
                C::Fn123d264 => playback_service_0123d264(),
                C::Fn123d390 => playback_service_0123d390(),
                C::Fn123e298 => playback_service_0123e298(),
            }
        }
    }

    fn call_with(&mut self, call: machine47::CallWith, argument: u32) -> i32 {
        use machine47::CallWith as C;
        unsafe {
            match call {
                C::Fn123c2c8 => playback_service_0123c2c8(argument),
                C::Fn123c2e8 => playback_service_0123c2e8(argument),
                C::Fn123c308 => playback_service_0123c308(argument),
            }
        }
    }

    fn set_state(&mut self, state: machine47::State) {
        unsafe { write_volatile(MACHINE47_STATE as *mut u16, state as u16) }
    }

    fn block_14ff3c4(&self, word: machine47::Block14ff3c4) -> u32 {
        unsafe { read_volatile((BLOCK_14FF3C4 + word as u32) as *const u32) }
    }
}

extern "C" {
    fn playback_service_0122b48c(argument: u32) -> i32;
    fn playback_service_0122b4d8() -> i32;
    fn playback_service_0122b504() -> i32;
}

impl machine57::Services for Stock {
    fn call(&mut self, call: machine57::Call) -> i32 {
        use machine57::Call as C;
        unsafe {
            match call {
                C::Trace => playback_trace(),
                C::Fn122b4d8 => playback_service_0122b4d8(),
                C::Fn122b504 => playback_service_0122b504(),
            }
        }
    }

    fn call_with(&mut self, call: machine57::CallWith, argument: u32) -> i32 {
        use machine57::CallWith as C;
        unsafe {
            match call {
                C::Fn122b48c => playback_service_0122b48c(argument),
            }
        }
    }

    fn set_state(&mut self, state: machine57::State) {
        unsafe { write_volatile(MACHINE57_STATE as *mut u16, state as u16) }
    }

    fn request(&self, word: machine57::Request) -> u32 {
        unsafe { read_volatile((REQUEST + word as u32) as *const u32) }
    }
}

extern "C" {
    fn playback_service_01211e78() -> i32;
    fn playback_service_01211fdc() -> i32;
    fn playback_service_01212120() -> i32;
    fn playback_service_01212310() -> i32;
}

impl machine66::Services for Stock {
    fn call(&mut self, call: machine66::Call) -> i32 {
        use machine66::Call as C;
        unsafe {
            match call {
                C::Fn1211e78 => playback_service_01211e78(),
                C::Fn1211fdc => playback_service_01211fdc(),
                C::Fn1212120 => playback_service_01212120(),
                C::Fn1212310 => playback_service_01212310(),
                C::Trace => playback_trace(),
            }
        }
    }

    fn set_state(&mut self, state: machine66::State) {
        unsafe { write_volatile(MACHINE66_STATE as *mut u16, state as u16) }
    }

    fn request(&self, word: machine66::Request) -> u32 {
        unsafe { read_volatile((REQUEST + word as u32) as *const u32) }
    }
}

extern "C" {
    fn playback_service_0120b40c() -> i32;
    fn playback_service_0120cab0() -> i32;
    fn playback_service_01210250() -> i32;
    fn playback_service_012142f8() -> i32;
    fn playback_service_0124c6fc(argument: u32) -> i32;
}

impl machine1::Services for Stock {
    fn call(&mut self, call: machine1::Call) -> i32 {
        use machine1::Call as C;
        unsafe {
            match call {
                C::Fn120b40c => playback_service_0120b40c(),
                C::Fn120cab0 => playback_service_0120cab0(),
                C::Fn1210250 => playback_service_01210250(),
                C::Fn12142f8 => playback_service_012142f8(),
                C::Trace => playback_trace(),
            }
        }
    }

    fn call_with(&mut self, call: machine1::CallWith, argument: u32) {
        unsafe {
            match call {
                machine1::CallWith::Fn124c6fc => playback_service_0124c6fc(argument),
            };
        }
    }

    fn set_state(&mut self, state: machine1::State) {
        unsafe { write_volatile(MACHINE1_STATE as *mut u16, state as u16) }
    }

    fn image_control(&self, byte: machine1::ImageControl) -> u8 {
        unsafe { read_volatile((IMAGE_CONTROL + byte as u32) as *const u8) }
    }

    fn media_output(&self, word: machine1::MediaOutput) -> u32 {
        unsafe { read_volatile((MEDIA_OUTPUT + word as u32) as *const u32) }
    }
}

extern "C" {
    fn playback_service_01220314() -> i32;
    fn playback_service_01220340() -> i32;
    fn playback_service_01220378() -> i32;
    fn playback_service_012203a4() -> i32;
    fn playback_service_012203d0() -> i32;
    fn playback_service_012203f4(argument: u32) -> i32;
    fn playback_service_01220480() -> i32;
    fn playback_service_012204cc() -> i32;
    fn playback_service_01220560() -> i32;
}

impl machine6::Services for Stock {
    fn call(&mut self, call: machine6::Call) {
        use machine6::Call as C;
        unsafe {
            match call {
                C::Fn1220314 => playback_service_01220314(),
                C::Fn1220340 => playback_service_01220340(),
                C::Fn1220378 => playback_service_01220378(),
                C::Fn12203a4 => playback_service_012203a4(),
                C::Fn12203d0 => playback_service_012203d0(),
                C::Fn1220480 => playback_service_01220480(),
                C::Fn12204cc => playback_service_012204cc(),
                C::Fn1220560 => playback_service_01220560(),
                C::Trace => playback_trace(),
            };
        }
    }

    fn call_with(&mut self, call: machine6::CallWith, argument: u32) {
        unsafe {
            match call {
                machine6::CallWith::Fn12203f4 => playback_service_012203f4(argument),
            };
        }
    }

    fn set_state(&mut self, state: machine6::State) {
        unsafe { write_volatile(MACHINE6_STATE as *mut u16, state as u16) }
    }

    fn request(&self, word: machine6::Request) -> u32 {
        unsafe { read_volatile((REQUEST + word as u32) as *const u32) }
    }

    fn io(&self, word: machine6::Io) -> u32 {
        unsafe { read_volatile((IO + word as u32) as *const u32) }
    }

    fn set_io(&mut self, word: machine6::Io, value: u32) {
        unsafe { write_volatile((IO + word as u32) as *mut u32, value) }
    }

    fn io_count(&self) -> i64 {
        unsafe { read_volatile(IO_COUNT as *const i64) }
    }
}

extern "C" {
    fn playback_service_0121de50() -> i32;
    fn playback_service_0121de64() -> i32;
    fn playback_service_0121de80(argument: u32) -> i32;
    fn playback_service_0121ded4(argument: u32) -> i32;
    fn playback_service_0121df20() -> i32;
    fn playback_service_0121df30() -> i32;
}

impl machine8::Services for Stock {
    fn call(&mut self, call: machine8::Call) {
        use machine8::Call as C;
        unsafe {
            match call {
                C::Fn121de50 => playback_service_0121de50(),
                C::Fn121de64 => playback_service_0121de64(),
                C::Fn121df20 => playback_service_0121df20(),
                C::Fn121df30 => playback_service_0121df30(),
            };
        }
    }

    fn call_with(&mut self, call: machine8::CallWith, argument: u32) {
        use machine8::CallWith as C;
        unsafe {
            match call {
                C::Fn121de80 => playback_service_0121de80(argument),
                C::Fn121ded4 => playback_service_0121ded4(argument),
            };
        }
    }

    fn set_peer_state(&mut self, peer: machine8::Peer, state: u16) {
        unsafe { write_volatile((MACHINE_STATES + 2 * peer as u32) as *mut u16, state) }
    }
}

extern "C" {
    fn playback_service_0120bfd8() -> i32;
    fn playback_service_0121ff9c() -> i32;
    fn playback_service_0121ffc0() -> i32;
    fn playback_service_0123d3d4(argument: u32) -> i32;
    fn playback_service_0123d3f4(argument: u32) -> i32;
    fn playback_service_0123d414() -> i32;
    fn playback_service_0123d454() -> i32;
    fn playback_service_0123d4d0() -> i32;
    fn playback_service_0123d54c() -> i32;
    fn playback_service_0123d584() -> i32;
    fn playback_service_0123d87c() -> i32;
    fn playback_service_0123d89c() -> i32;
    fn playback_service_0123d8bc() -> i32;
    fn playback_service_0123da10() -> i32;
    fn playback_service_0123db1c() -> i32;
    fn playback_service_0123dc24() -> i32;
    fn playback_service_0123dd1c() -> i32;
    fn playback_service_0123dd38() -> i32;
    fn playback_service_0123ddc0() -> i32;
    fn playback_service_0123de08() -> i32;
    fn playback_service_0123de38() -> i32;
    fn playback_service_0123df40() -> i32;
    fn playback_service_0123e084() -> i32;
    fn playback_service_0123e098() -> i32;
}

impl machine44::Services for Stock {
    fn call(&mut self, call: machine44::Call) -> i32 {
        use machine44::Call as C;
        unsafe {
            match call {
                C::Dispatch3Events => playback_dispatch_machine3_events(),
                C::Fn120bfd8 => playback_service_0120bfd8(),
                C::Fn120c1a8 => playback_service_0120c1a8(),
                C::Trace => playback_trace(),
                C::Fn121ff9c => playback_service_0121ff9c(),
                C::Fn121ffc0 => playback_service_0121ffc0(),
                C::Fn12202d0 => playback_service_012202d0(),
                C::Fn12205e4 => playback_service_012205e4(),
                C::Fn123d414 => playback_service_0123d414(),
                C::Fn123d454 => playback_service_0123d454(),
                C::Fn123d4d0 => playback_service_0123d4d0(),
                C::Fn123d54c => playback_service_0123d54c(),
                C::Fn123d584 => playback_service_0123d584(),
                C::Fn123d87c => playback_service_0123d87c(),
                C::Fn123d89c => playback_service_0123d89c(),
                C::Fn123d8bc => playback_service_0123d8bc(),
                C::Fn123da10 => playback_service_0123da10(),
                C::Fn123db1c => playback_service_0123db1c(),
                C::Fn123dc24 => playback_service_0123dc24(),
                C::Fn123dd1c => playback_service_0123dd1c(),
                C::Fn123dd38 => playback_service_0123dd38(),
                C::Fn123ddc0 => playback_service_0123ddc0(),
                C::Fn123de08 => playback_service_0123de08(),
                C::Fn123de38 => playback_service_0123de38(),
                C::Fn123df40 => playback_service_0123df40(),
                C::Fn123e084 => playback_service_0123e084(),
                C::Fn123e098 => playback_service_0123e098(),
            }
        }
    }

    fn call_with(&mut self, call: machine44::CallWith, argument: u32) -> i32 {
        use machine44::CallWith as C;
        unsafe {
            match call {
                C::Fn12293a8 => playback_service_012293a8(argument),
                C::Fn123d3d4 => playback_service_0123d3d4(argument),
                C::Fn123d3f4 => playback_service_0123d3f4(argument),
            }
        }
    }

    fn set_state(&mut self, state: machine44::State) {
        unsafe { write_volatile(MACHINE44_STATE as *mut u16, state as u16) }
    }

    fn word(&self, word: machine44::Word) -> u32 {
        unsafe { read_volatile((STREAM + word as u32) as *const u32) }
    }

    fn byte(&self, byte: machine44::Byte) -> u8 {
        unsafe { read_volatile((STREAM + byte as u32) as *const u8) }
    }

    fn word_at(&self, address: u32) -> u32 {
        unsafe { read_volatile(address as *const u32) }
    }

    fn set_word_at(&mut self, address: u32, value: u32) {
        unsafe { write_volatile(address as *mut u32, value) }
    }

    fn request_code(&self) -> u16 {
        unsafe { read_volatile(REQUEST as *const u16) }
    }
}

extern "C" {
    fn playback_service_0123b4b4(argument: u32) -> i32;
    fn playback_service_0123b4d4(argument: u32) -> i32;
    fn playback_service_0123b4f4() -> i32;
    fn playback_service_0123b5a8() -> i32;
    fn playback_service_0123b688() -> i32;
    fn playback_service_0123b76c() -> i32;
    fn playback_service_0123b960() -> i32;
    fn playback_service_0123ba6c() -> i32;
    fn playback_service_0123bb74() -> i32;
    fn playback_service_0123bf2c() -> i32;
}

impl machine46::Services for Stock {
    fn call(&mut self, call: machine46::Call) -> i32 {
        use machine46::Call as C;
        unsafe {
            match call {
                C::Dispatch3Events => playback_dispatch_machine3_events(),
                C::Fn120bfd8 => playback_service_0120bfd8(),
                C::Fn120c1a8 => playback_service_0120c1a8(),
                C::Trace => playback_trace(),
                C::Fn121ffc0 => playback_service_0121ffc0(),
                C::Fn12202d0 => playback_service_012202d0(),
                C::Fn12205e4 => playback_service_012205e4(),
                C::Fn123b4f4 => playback_service_0123b4f4(),
                C::Fn123b5a8 => playback_service_0123b5a8(),
                C::Fn123b688 => playback_service_0123b688(),
                C::Fn123b76c => playback_service_0123b76c(),
                C::Fn123b960 => playback_service_0123b960(),
                C::Fn123ba6c => playback_service_0123ba6c(),
                C::Fn123bb74 => playback_service_0123bb74(),
                C::Fn123bf2c => playback_service_0123bf2c(),
            }
        }
    }

    fn call_with(&mut self, call: machine46::CallWith, argument: u32) {
        use machine46::CallWith as C;
        unsafe {
            match call {
                C::Fn123b4b4 => playback_service_0123b4b4(argument),
                C::Fn123b4d4 => playback_service_0123b4d4(argument),
            };
        }
    }

    fn set_state(&mut self, state: machine46::State) {
        unsafe { write_volatile(MACHINE46_STATE as *mut u16, state as u16) }
    }

    fn clear_request_code(&mut self) {
        unsafe { write_volatile(REQUEST as *mut u16, 0) }
    }

    fn word(&self, word: machine46::Word) -> u32 {
        unsafe { read_volatile((MACHINE46_BLOCK + word as u32) as *const u32) }
    }

    fn byte(&self, byte: machine46::Byte) -> u8 {
        unsafe { read_volatile((MACHINE46_BLOCK + byte as u32) as *const u8) }
    }

    fn word_at(&self, address: u32) -> u32 {
        unsafe { read_volatile(address as *const u32) }
    }

    fn set_word_at(&mut self, address: u32, value: u32) {
        unsafe { write_volatile(address as *mut u32, value) }
    }
}

extern "C" {
    fn playback_service_01211250() -> i32;
    fn playback_service_0121132c() -> i32;
    fn playback_service_012135e8() -> i32;
    fn playback_service_0122b3f0() -> i32;
    fn playback_service_0122b440() -> i32;
    fn playback_service_0122b468() -> i32;
    fn playback_service_0122b800() -> i32;
}

impl machine56::Services for Stock {
    fn call(&mut self, call: machine56::Call) -> i32 {
        use machine56::Call as C;
        unsafe {
            match call {
                C::Fn1211250 => playback_service_01211250(),
                C::Fn121132c => playback_service_0121132c(),
                C::Fn12135e8 => playback_service_012135e8(),
                C::Trace => playback_trace(),
                C::Fn122b3f0 => playback_service_0122b3f0(),
                C::Fn122b440 => playback_service_0122b440(),
                C::Fn122b468 => playback_service_0122b468(),
                C::Fn122b800 => playback_service_0122b800(),
            }
        }
    }

    fn set_state(&mut self, state: machine56::State) {
        unsafe { write_volatile(MACHINE56_STATE as *mut u16, state as u16) }
    }

    fn request(&self, word: machine56::Request) -> u32 {
        unsafe { read_volatile((REQUEST + word as u32) as *const u32) }
    }
}

extern "C" {
    fn playback_service_0122c468() -> i32;
    fn playback_service_01233dcc() -> i32;
    fn playback_service_01233e0c() -> i32;
    fn playback_service_01251c80(argument: u32) -> i32;
    fn playback_service_01251cd0(argument: u32) -> i32;
    fn playback_service_01251de8() -> i32;
    fn playback_service_01251e10(argument: u32) -> i32;
    fn playback_service_01251e40() -> i32;
    fn playback_service_01251e80() -> i32;
    fn playback_service_01251ea8() -> i32;
    fn playback_service_01251f24(argument: u32) -> i32;
    fn playback_service_01251fc8() -> i32;
    fn playback_service_0125202c() -> i32;
    fn playback_service_01252064(argument: u32) -> i32;
    fn playback_service_012520b0(argument: u32) -> i32;
    fn playback_service_012520ec() -> i32;
    fn playback_service_01252328() -> i32;
    fn playback_service_0125233c() -> i32;
    fn playback_service_01252374(argument: u32) -> i32;
    fn playback_service_01252430() -> i32;
    fn playback_service_01252464() -> i32;
    fn playback_service_01252490() -> i32;
    fn playback_service_012524a0() -> i32;
}

impl machine70::Services for Stock {
    fn call(&mut self, call: machine70::Call) -> i32 {
        use machine70::Call as C;
        unsafe {
            match call {
                C::Fn0ec03dc => playback_service_00ec03dc(),
                C::Trace => playback_trace(),
                C::Fn122c468 => playback_service_0122c468(),
                C::Fn1233dcc => playback_service_01233dcc(),
                C::Fn1233e0c => playback_service_01233e0c(),
                C::Fn1251de8 => playback_service_01251de8(),
                C::Fn1251e40 => playback_service_01251e40(),
                C::Fn1251e80 => playback_service_01251e80(),
                C::Fn1251ea8 => playback_service_01251ea8(),
                C::Fn1251fc8 => playback_service_01251fc8(),
                C::Fn125202c => playback_service_0125202c(),
                C::Fn12520ec => playback_service_012520ec(),
                C::Fn1252328 => playback_service_01252328(),
                C::Fn125233c => playback_service_0125233c(),
                C::Fn1252430 => playback_service_01252430(),
                C::Fn1252464 => playback_service_01252464(),
                C::Fn1252490 => playback_service_01252490(),
                C::Fn12524a0 => playback_service_012524a0(),
            }
        }
    }

    fn call_with(&mut self, call: machine70::CallWith, argument: u32) {
        use machine70::CallWith as C;
        unsafe {
            match call {
                C::Fn123265c => playback_service_0123265c(argument),
                C::Fn1251c80 => playback_service_01251c80(argument),
                C::Fn1251cd0 => playback_service_01251cd0(argument),
                C::Fn1251e10 => playback_service_01251e10(argument),
                C::Fn1251f24 => playback_service_01251f24(argument),
                C::Fn1252064 => playback_service_01252064(argument),
                C::Fn12520b0 => playback_service_012520b0(argument),
                C::Fn1252374 => playback_service_01252374(argument),
            };
        }
    }

    fn set_state(&mut self, state: machine70::State) {
        unsafe { write_volatile(MACHINE70_STATE as *mut u16, state as u16) }
    }

    fn request(&self, word: machine70::Request) -> u32 {
        unsafe { read_volatile((REQUEST + word as u32) as *const u32) }
    }

    fn word(&self, word: machine70::Word) -> u32 {
        unsafe { read_volatile((SEQUENCE + word as u32) as *const u32) }
    }

    fn set_word(&mut self, word: machine70::Word, value: u32) {
        unsafe { write_volatile((SEQUENCE + word as u32) as *mut u32, value) }
    }

    fn byte(&self, byte: machine70::Byte) -> u8 {
        unsafe { read_volatile((SEQUENCE + byte as u32) as *const u8) }
    }

    fn set_byte(&mut self, byte: machine70::Byte, value: u8) {
        unsafe { write_volatile((SEQUENCE + byte as u32) as *mut u8, value) }
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

actions! { machine34 {
    xm1_playback_m34_action02 => action02, xm1_playback_m34_action03 => action03,
    xm1_playback_m34_action04 => action04, xm1_playback_m34_action05 => action05,
    xm1_playback_m34_action06 => action06, xm1_playback_m34_action07 => action07,
    xm1_playback_m34_action08 => action08, xm1_playback_m34_action09 => action09,
    xm1_playback_m34_action10 => action10, xm1_playback_m34_action11 => action11,
    xm1_playback_m34_action12 => action12, xm1_playback_m34_action13 => action13,
    xm1_playback_m34_action14 => action14,
} }

actions! { machine2 {
    xm1_playback_m2_action02 => action02, xm1_playback_m2_action03 => action03,
    xm1_playback_m2_action04 => action04, xm1_playback_m2_action05 => action05,
    xm1_playback_m2_action06 => action06, xm1_playback_m2_action07 => action07,
    xm1_playback_m2_action08 => action08, xm1_playback_m2_action09 => action09,
    xm1_playback_m2_action10 => action10, xm1_playback_m2_action11 => action11,
    xm1_playback_m2_action12 => action12, xm1_playback_m2_action13 => action13,
    xm1_playback_m2_action14 => action14, xm1_playback_m2_action15 => action15,
} }

actions! { machine3 {
    xm1_playback_m3_action02 => action02, xm1_playback_m3_action03 => action03,
    xm1_playback_m3_action04 => action04, xm1_playback_m3_action05 => action05,
    xm1_playback_m3_action06 => action06, xm1_playback_m3_action07 => action07,
    xm1_playback_m3_action08 => action08, xm1_playback_m3_action09 => action09,
    xm1_playback_m3_action10 => action10,
} }

actions! { machine4 {
    xm1_playback_m4_action02 => action02, xm1_playback_m4_action03 => action03,
    xm1_playback_m4_action04 => action04, xm1_playback_m4_action05 => action05,
    xm1_playback_m4_action06 => action06, xm1_playback_m4_action07 => action07,
    xm1_playback_m4_action08 => action08, xm1_playback_m4_action09 => action09,
    xm1_playback_m4_action10 => action10, xm1_playback_m4_action11 => action11,
    xm1_playback_m4_action12 => action12, xm1_playback_m4_action13 => action13,
    xm1_playback_m4_action14 => action14, xm1_playback_m4_action15 => action15,
    xm1_playback_m4_action16 => action16, xm1_playback_m4_action17 => action17,
    xm1_playback_m4_action18 => action18, xm1_playback_m4_action19 => action19,
} }

actions! { machine5 {
    xm1_playback_m5_action02 => action02, xm1_playback_m5_action03 => action03,
    xm1_playback_m5_action04 => action04, xm1_playback_m5_action05 => action05,
    xm1_playback_m5_action06 => action06, xm1_playback_m5_action07 => action07,
    xm1_playback_m5_action08 => action08, xm1_playback_m5_action09 => action09,
    xm1_playback_m5_action10 => action10,
} }

actions! { machine45 {
    xm1_playback_m45_action02 => action02, xm1_playback_m45_action03 => action03,
    xm1_playback_m45_action04 => action04, xm1_playback_m45_action05 => action05,
    xm1_playback_m45_action06 => action06, xm1_playback_m45_action07 => action07,
    xm1_playback_m45_action08 => action08, xm1_playback_m45_action09 => action09,
    xm1_playback_m45_action10 => action10,
} }

actions! { machine47 {
    xm1_playback_m47_action02 => action02, xm1_playback_m47_action03 => action03,
    xm1_playback_m47_action04 => action04, xm1_playback_m47_action05 => action05,
    xm1_playback_m47_action06 => action06, xm1_playback_m47_action07 => action07,
    xm1_playback_m47_action08 => action08, xm1_playback_m47_action09 => action09,
    xm1_playback_m47_action10 => action10, xm1_playback_m47_action11 => action11,
    xm1_playback_m47_action12 => action12, xm1_playback_m47_action13 => action13,
} }

actions! { machine57 {
    xm1_playback_m57_action02 => action02, xm1_playback_m57_action03 => action03,
    xm1_playback_m57_action04 => action04, xm1_playback_m57_action05 => action05,
    xm1_playback_m57_action06 => action06, xm1_playback_m57_action07 => action07,
    xm1_playback_m57_action08 => action08,
} }

actions! { machine66 {
    xm1_playback_m66_action02 => action02, xm1_playback_m66_action03 => action03,
    xm1_playback_m66_action04 => action04, xm1_playback_m66_action05 => action05,
    xm1_playback_m66_action06 => action06, xm1_playback_m66_action07 => action07,
    xm1_playback_m66_action08 => action08,
} }

actions! { machine1 {
    xm1_playback_m1_action02 => action02, xm1_playback_m1_action03 => action03,
    xm1_playback_m1_action04 => action04, xm1_playback_m1_action05 => action05,
    xm1_playback_m1_action06 => action06, xm1_playback_m1_action07 => action07,
    xm1_playback_m1_action08 => action08, xm1_playback_m1_action09 => action09,
    xm1_playback_m1_action10 => action10, xm1_playback_m1_action11 => action11,
    xm1_playback_m1_action12 => action12, xm1_playback_m1_action13 => action13,
    xm1_playback_m1_action14 => action14, xm1_playback_m1_action15 => action15,
} }

actions! { machine6 {
    xm1_playback_m6_action02 => action02, xm1_playback_m6_action03 => action03,
    xm1_playback_m6_action04 => action04, xm1_playback_m6_action05 => action05,
    xm1_playback_m6_action06 => action06, xm1_playback_m6_action07 => action07,
    xm1_playback_m6_action08 => action08, xm1_playback_m6_action09 => action09,
    xm1_playback_m6_action10 => action10, xm1_playback_m6_action11 => action11,
} }

actions! { machine8 {
    xm1_playback_m8_action02 => action02, xm1_playback_m8_action03 => action03,
    xm1_playback_m8_action04 => action04, xm1_playback_m8_action05 => action05,
    xm1_playback_m8_action06 => action06, xm1_playback_m8_action07 => action07,
    xm1_playback_m8_action08 => action08, xm1_playback_m8_action09 => action09,
    xm1_playback_m8_action10 => action10, xm1_playback_m8_action11 => action11,
} }

actions! { machine44 {
    xm1_playback_m44_action02 => action02, xm1_playback_m44_action03 => action03,
    xm1_playback_m44_action04 => action04, xm1_playback_m44_action05 => action05,
    xm1_playback_m44_action06 => action06, xm1_playback_m44_action07 => action07,
    xm1_playback_m44_action08 => action08, xm1_playback_m44_action09 => action09,
    xm1_playback_m44_action10 => action10, xm1_playback_m44_action11 => action11,
    xm1_playback_m44_action12 => action12, xm1_playback_m44_action13 => action13,
    xm1_playback_m44_action14 => action14, xm1_playback_m44_action15 => action15,
    xm1_playback_m44_action16 => action16, xm1_playback_m44_action17 => action17,
    xm1_playback_m44_action18 => action18, xm1_playback_m44_action19 => action19,
    xm1_playback_m44_action20 => action20,
} }

actions! { machine46 {
    xm1_playback_m46_action02 => action02, xm1_playback_m46_action03 => action03,
    xm1_playback_m46_action04 => action04, xm1_playback_m46_action05 => action05,
    xm1_playback_m46_action06 => action06, xm1_playback_m46_action07 => action07,
    xm1_playback_m46_action08 => action08, xm1_playback_m46_action09 => action09,
    xm1_playback_m46_action10 => action10, xm1_playback_m46_action11 => action11,
    xm1_playback_m46_action12 => action12, xm1_playback_m46_action13 => action13,
    xm1_playback_m46_action14 => action14, xm1_playback_m46_action15 => action15,
} }

actions! { machine56 {
    xm1_playback_m56_action02 => action02, xm1_playback_m56_action03 => action03,
    xm1_playback_m56_action04 => action04, xm1_playback_m56_action05 => action05,
    xm1_playback_m56_action06 => action06, xm1_playback_m56_action07 => action07,
    xm1_playback_m56_action08 => action08, xm1_playback_m56_action09 => action09,
    xm1_playback_m56_action10 => action10, xm1_playback_m56_action11 => action11,
    xm1_playback_m56_action12 => action12, xm1_playback_m56_action13 => action13,
    xm1_playback_m56_action14 => action14, xm1_playback_m56_action15 => action15,
    xm1_playback_m56_action16 => action16, xm1_playback_m56_action17 => action17,
    xm1_playback_m56_action18 => action18,
} }

actions! { machine70 {
    xm1_playback_m70_action02 => action02, xm1_playback_m70_action03 => action03,
    xm1_playback_m70_action04 => action04, xm1_playback_m70_action05 => action05,
    xm1_playback_m70_action06 => action06, xm1_playback_m70_action07 => action07,
    xm1_playback_m70_action08 => action08, xm1_playback_m70_action09 => action09,
    xm1_playback_m70_action10 => action10, xm1_playback_m70_action11 => action11,
    xm1_playback_m70_action12 => action12, xm1_playback_m70_action13 => action13,
    xm1_playback_m70_action14 => action14, xm1_playback_m70_action15 => action15,
    xm1_playback_m70_action16 => action16, xm1_playback_m70_action17 => action17,
    xm1_playback_m70_action18 => action18, xm1_playback_m70_action19 => action19,
    xm1_playback_m70_action20 => action20, xm1_playback_m70_action21 => action21,
    xm1_playback_m70_action22 => action22,
} }
