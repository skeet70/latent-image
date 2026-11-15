#![cfg(target_arch = "arm")]

use xm1_saclac_interface::{stock::record_handle_word, CompletionStatus, RecordHandle};
use xm1_saclac_shared::stock::Guest;
use crate::shot_native::{AtomicLensChange, ProfileReplies};

impl AtomicLensChange for Guest {
    fn take_lens_change(&self) -> u32 {
        // Canonical task50 state, including the owned interrupt lock/publication/clear transaction.
        unsafe { xm1_power_lifecycle::record_lens_schedule_state::xm1_power_record_helper_76f438() }
    }
}

impl ProfileReplies for Guest {
    fn half_press_reply(&self, record: Option<RecordHandle>, status: CompletionStatus,
        capture_vote: impl FnMut(u32) -> (u32, u16), apply_position: impl FnMut(u32)) {
        unsafe { xm1_power_lifecycle::record_completion_six::complete(record.map(record_handle_word).unwrap_or(u32::MAX), status.0 as u32, capture_vote, apply_position); }
    }
    fn capture_reply(&self, record: Option<RecordHandle>, status: CompletionStatus) {
        unsafe { xm1_power_lifecycle::record_completion_profiles::xm1_power_record_helper_efab18(record.map(record_handle_word).unwrap_or(u32::MAX), status.0 as u32); }
    }
    fn setting_reply(&self, record: Option<RecordHandle>, status: CompletionStatus) {
        unsafe { xm1_power_lifecycle::record_completion_profiles::xm1_power_record_helper_efae88(record.map(record_handle_word).unwrap_or(u32::MAX), status.0 as u32); }
    }
}
