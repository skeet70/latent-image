//! Qualification view of the record contexts (issue 117, X-M1 1.50).
//!
//! `composition::qualification` writes slot 2's current handle to the stock bank-2 record
//! handle word at `0x169cf08` (`s53c_record_handles.words[2]`, `-1` absent). The view is
//! exhaustive over the four slots and both generations.

use super::{Contexts, Pair};
use xm1_saclac_interface::stock::record_handle_word;

/// One slot's two generations as the raw handle words stock stores.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SlotView { pub current: Option<u32>, pub previous: Option<u32> }

/// All four slots, in `Context` order (Root, Lens, Live, S1).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextsView { pub slots: [SlotView; 4] }

fn slot(pair: &Pair) -> SlotView {
    SlotView {
        current: pair.current.map(record_handle_word),
        previous: pair.previous.map(record_handle_word),
    }
}

/// The view of `contexts`.
pub fn view(contexts: &Contexts) -> ContextsView {
    ContextsView { slots: [slot(&contexts.slots[0]), slot(&contexts.slots[1]), slot(&contexts.slots[2]), slot(&contexts.slots[3])] }
}
