#![forbid(unsafe_code)]

use xm1_saclac_calc::scene_recognition::Recognition;
use xm1_saclac_interface::{CompletionStatus, RecordHandle};

/// Profile effects return before client publication.
pub trait ProfileReplies {
    fn half_press_reply(&self, record: Option<RecordHandle>, status: CompletionStatus,
        capture_vote: impl FnMut(RecordHandle) -> Recognition, apply_position: impl FnMut(RecordHandle));
    fn capture_reply(&self, record: Option<RecordHandle>, status: CompletionStatus);
    fn setting_reply(&self, record: Option<RecordHandle>, status: CompletionStatus);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensChange(pub u32);

/// The lens service's take/publication/clear transaction under its interrupt lock.
pub trait AtomicLensChange {
    fn take_lens_change(&self) -> LensChange;
}
