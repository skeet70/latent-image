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

pub const fn measurement_outcome(word: i32) -> crate::children::ChildOutcome {
    use crate::children::{ChildOutcome, MeasurementOutcome, MeasurementUpdate, Restart, RetainedRoundStatus};
    let (update, restart) = match word {
        0 => return ChildOutcome::Fault,
        1 => return ChildOutcome::Complete,
        2 => return ChildOutcome::Pending,
        3 => return ChildOutcome::Measurement(MeasurementOutcome::Round(RetainedRoundStatus::Status03)),
        5 => (MeasurementUpdate::Window, Restart::Pending),
        6 => (MeasurementUpdate::NoPeerRequest, Restart::Pending),
        7 => (MeasurementUpdate::Source, Restart::Pending),
        8 => (MeasurementUpdate::Window, Restart::Complete),
        9 => (MeasurementUpdate::NoPeerRequest, Restart::Complete),
        10 => (MeasurementUpdate::Source, Restart::Complete),
        12 => return ChildOutcome::Measurement(MeasurementOutcome::Round(RetainedRoundStatus::Status0c)),
        _ => return ChildOutcome::Fault,
    };
    ChildOutcome::Measurement(MeasurementOutcome::Update { update, restart })
}

pub const fn measurement_update_operation(update: crate::children::MeasurementUpdate) -> Option<xm1_saclac_interface::StatisticsOperation> {
    use crate::children::MeasurementUpdate;
    use xm1_saclac_interface::StatisticsOperation;
    match update {
        MeasurementUpdate::Window => Some(StatisticsOperation::Request14),
        MeasurementUpdate::NoPeerRequest => None,
        MeasurementUpdate::Source => Some(StatisticsOperation::Request41),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StockCallbackWord(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensFinishRoute {
    Ordinary,
    Alternate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensPayload {
    pub operation: xm1_saclac_interface::LensDcOperation,
    bytes: [u8; 32],
    length: usize,
}

impl LensPayload {
    pub fn bytes(&self) -> &[u8] { &self.bytes[..self.length] }
}

pub fn adjustment_payload(
    request: crate::adjustment::LensRequest,
    callback: StockCallbackWord,
    finish_route: LensFinishRoute,
) -> Option<LensPayload> {
    use crate::adjustment::LensRequest;
    use xm1_saclac_interface::LensDcOperation;

    let mut bytes = [0; 32];
    let (operation, words, length) = match request {
        LensRequest::ReserveOnly => return None,
        LensRequest::Move { position, amount, scale } => {
            let amount = if scale.0 < 32 { amount.0 << scale.0 } else { 0 };
            (LensDcOperation::Request1c,
                [callback.0, 0, amount, u32::from(position.0 as u16), 2, 1, 0, 13], 32)
        }
        LensRequest::SetPosition { position, amount } =>
            (LensDcOperation::Request1c,
                [callback.0, 1, amount.0, u32::from(position.0 as u16), 2, 1, 0, 13], 32),
        LensRequest::SetPercentage { position, percent } =>
            (LensDcOperation::Request1c,
                [callback.0, 2, u32::from(percent.get()), u32::from(position.0 as u16), 2, 1, 0, 13], 32),
        LensRequest::Stop =>
            (LensDcOperation::Request15, [callback.0, 3, 0, 0, 0, 0, 0, 0], 12),
        LensRequest::Synchronize =>
            (LensDcOperation::Request17, [callback.0, 0, 0, 0, 0, 0, 0, 0], 8),
        LensRequest::Query =>
            (LensDcOperation::Request18, [callback.0, 0, 0, 0, 0, 0, 0, 0], 4),
        LensRequest::Configure { selection, value } => {
            let operation = match finish_route {
                LensFinishRoute::Ordinary => LensDcOperation::Request13,
                LensFinishRoute::Alternate => LensDcOperation::Request1d,
            };
            (operation, [callback.0, selection.0, value.0, 1, 0, 10, 0, 0], 24)
        }
        LensRequest::ConfigureDefault =>
            (LensDcOperation::Request16, [callback.0, 3, 0, 255, 0, 0, 0, 0], 16),
        LensRequest::SetValue(value) =>
            (LensDcOperation::Request19, [callback.0, value.0, 0, 0, 0, 0, 0, 0], 8),
        LensRequest::ConfigureValue(value) =>
            (LensDcOperation::Request1a, [callback.0, value.0, 0, 0, 0, 0, 0, 0], 8),
    };
    for (chunk, word) in bytes.chunks_exact_mut(4).zip(words) {
        chunk.copy_from_slice(&word.to_le_bytes());
    }
    Some(LensPayload { operation, bytes, length })
}
