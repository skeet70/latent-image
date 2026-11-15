#![cfg(target_arch = "arm")]

use xm1_saclac_calc::scene_recognition::Recognition;
use xm1_saclac_interface::{stock::{record_handle_from_a2, record_handle_word}, CompletionStatus, RecordHandle};
use xm1_saclac_shared::stock::Guest;
use crate::shot_native::{AtomicLensChange, LensChange, ProfileReplies};

impl AtomicLensChange for Guest {
    fn take_lens_change(&self) -> LensChange {
        LensChange(unsafe { xm1_power_lifecycle::record_lens_schedule_state::xm1_power_record_helper_76f438() })
    }
}

impl ProfileReplies for Guest {
    fn half_press_reply(&self, record: Option<RecordHandle>, status: CompletionStatus,
        mut capture_vote: impl FnMut(RecordHandle) -> Recognition, mut apply_position: impl FnMut(RecordHandle)) {
        unsafe {
            xm1_power_lifecycle::record_completion_six::complete(
                record.map(record_handle_word).unwrap_or(u32::MAX), status.0 as u32,
                |profile| {
                    let vote = capture_vote(record_handle_from_a2(profile));
                    (vote.scene.0, vote.flags.0)
                },
                |profile| apply_position(record_handle_from_a2(profile)),
            );
        }
    }
    fn capture_reply(&self, record: Option<RecordHandle>, status: CompletionStatus) {
        unsafe { xm1_power_lifecycle::record_completion_profiles::xm1_power_record_helper_efab18(record.map(record_handle_word).unwrap_or(u32::MAX), status.0 as u32); }
    }
    fn setting_reply(&self, record: Option<RecordHandle>, status: CompletionStatus) {
        unsafe { xm1_power_lifecycle::record_completion_profiles::xm1_power_record_helper_efae88(record.map(record_handle_word).unwrap_or(u32::MAX), status.0 as u32); }
    }
}
