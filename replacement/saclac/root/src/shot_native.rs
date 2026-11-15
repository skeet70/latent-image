#![forbid(unsafe_code)]

use xm1_saclac_interface::{CompletionStatus, RecordHandle};

/// The owned completion services' actual firmware boundary. Implementations must use the
/// canonical profile stores, command blocks and client publications, not a second reply model.
pub trait ProfileReplies {
    fn half_press_reply(&self, record: Option<RecordHandle>, status: CompletionStatus,
        capture_vote: impl FnMut(u32) -> (u32, u16), apply_position: impl FnMut(u32));
    fn capture_reply(&self, record: Option<RecordHandle>, status: CompletionStatus);
    fn setting_reply(&self, record: Option<RecordHandle>, status: CompletionStatus);
}

/// The lens service's take/publication/clear transaction under its interrupt lock.
pub trait AtomicLensChange {
    fn take_lens_change(&self) -> u32;
}
