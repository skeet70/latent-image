//! The retained services, guest state and the action exports the republished action-table
//! rows name.

use crate::machine0::{self, Call, Source, State, WorkState};
use crate::DevicePacket;
use core::mem::MaybeUninit;
use core::ptr::{read_volatile, write_volatile};

const PACKET_FRAME_WORDS: usize = 20;

const MACHINE0_STATE: u32 = 0x014e_a5ac;
const MODE_STATE: u32 = 0x014e_a678;
const BUFFER_STEP: u32 = MODE_STATE + 0x18;
const MODE_VALUE: u32 = MODE_STATE + 0x1c;

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
    fn playback_trace();
    fn playback_notify_0825();
    fn playback_notify_0826();
    fn playback_device_notification(packet: *const u32) -> i32;
}

struct Stock;

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
                Call::Trace => playback_trace(),
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
        let mut frame = MaybeUninit::<[u32; PACKET_FRAME_WORDS]>::uninit();
        let words = frame.as_mut_ptr() as *mut u32;
        unsafe {
            write_volatile(words, packet.code);
            write_volatile(words.add(1), packet.value);
            playback_device_notification(words);
        }
    }

    fn mode_value(&self) -> u32 {
        unsafe { read_volatile(MODE_VALUE as *const u32) }
    }

    fn clear_buffer_step(&mut self) {
        unsafe { write_volatile(BUFFER_STEP as *mut u32, 0) }
    }
}

macro_rules! machine0_actions {
    ($($export:ident => $action:ident),* $(,)?) => {
        $(
            #[no_mangle]
            pub extern "C" fn $export() -> i32 {
                machine0::$action(&mut Stock)
            }
        )*
    };
}

machine0_actions! {
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
}
