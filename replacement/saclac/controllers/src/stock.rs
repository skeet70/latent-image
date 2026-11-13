#![forbid(unsafe_code)]

use crate::statistics::{Changes, Group};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordFlags {
    selected: u64,
    permitted: u64,
}

impl RecordFlags {
    pub const fn new(selected: u64, permitted: u64) -> Self {
        Self { selected, permitted }
    }

    fn selected(self, mask: u64) -> bool {
        self.selected & mask == mask
    }

    fn permitted(self, mask: u64) -> bool {
        self.permitted & mask == mask
    }
}

pub fn autofocus_changes(request: RecordFlags, capture: RecordFlags, live: bool) -> Changes {
    let processing = request.selected(1 << 58)
        || request.selected(1 << 8)
        || request.selected(1 << 10)
        || request.selected(1 << 30)
        || request.selected(1 << 31)
        || capture.selected(1 << 11)
        || capture.permitted(1 << 19);
    let geometry = request.selected(if live { 1 << 6 } else { 1 << 7 });
    let measurement = if live {
        request.selected(1 << 37)
    } else {
        request.selected(1 << 38)
            || request.selected(1 << 1)
            || capture.selected(1 << 39)
            || capture.permitted(1)
            || capture.permitted(1 << 8)
            || capture.selected(1 << 26)
    };
    Changes { processing, geometry, measurement }
}

pub const fn configuration_operation(group: Group) -> u16 {
    match group {
        Group::Processing => 0x4012,
        Group::Geometry => 0x4010,
        Group::Measurement => 0x402c,
    }
}

pub const fn configuration_completed(wire: u8) -> Option<Group> {
    match wire {
        0x73 => Some(Group::Processing),
        0x71 => Some(Group::Geometry),
        0x80 => Some(Group::Measurement),
        _ => None,
    }
}

pub const fn synchronous_completion(status: xm1_saclac_interface::CompletionStatus) -> crate::synchronous::Completion {
    match status.0 {
        0 | 2 => crate::synchronous::Completion::Finished,
        _ => crate::synchronous::Completion::Progress,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiagnosticRow(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiagnosticState(pub u16);

pub struct DiagnosticSnapshot {
    pub rows: [DiagnosticRow; 32],
    pub states: [DiagnosticState; 32],
}

pub fn write_diagnostic(snapshot: &DiagnosticSnapshot, output: &mut [u8; 128]) {
    for (chunk, row) in output[..64].chunks_exact_mut(2).zip(&snapshot.rows) {
        chunk.copy_from_slice(&row.0.to_le_bytes());
    }
    for (chunk, state) in output[64..].chunks_exact_mut(2).zip(&snapshot.states) {
        chunk.copy_from_slice(&state.0.to_le_bytes());
    }
}
