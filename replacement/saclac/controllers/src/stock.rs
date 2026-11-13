#![forbid(unsafe_code)]

use xm1_saclac_interface::stock::StockCallbackWord;

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

/// X-M1 1.50 keeps the S1 progress in bits 0..3 of the byte at 0x169d508, which nothing else
/// writes; the shot sequence reads capture, bridge and release through 0xf153a0, 0xf15414 and
/// 0xf154b0 (from 0xf17dbc, 0xf17dcc, 0xf17ddc).
pub const fn s1_progress(waiting: crate::s1::Waiting) -> u8 {
    waiting.shot as u8 | (waiting.capture as u8) << 1 | (waiting.bridge as u8) << 2 | (waiting.release as u8) << 3
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SynchronousExport {
    StartRecord,
    StartCaptureRecord,
    UpdateRecord,
    ReserveClient,
    LensPosition,
    ReserveClient2,
    ReserveClient3,
    ReserveClient4,
    ReserveClient5,
    ReserveClient6,
    ReserveClient7,
    ReserveClient8,
    SetLensPosition,
    MoveLens,
    SetLensPercentage,
    StopLens,
    LensMotion,
    SynchronizeLens,
    QueryLens,
    SetLensValue,
    ConfigureLens,
    DefaultLens,
    ConfigureLensValue,
    NotifyPrimary,
    ReleaseParameters,
    PrimaryImageStatus,
    SecondaryImageStatus,
    CaptureValue,
    NotifySecondary,
    PrimaryMovieStatus,
    SecondaryMovieStatus,
    SelectionRejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExportAddress(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExportAbi {
    pub address: ExportAddress,
    pub export: SynchronousExport,
    pub argument_words: u8,
}

pub const SYNCHRONOUS_EXPORTS: [ExportAbi; 32] = {
    use SynchronousExport::*;
    [
        ExportAbi { address: ExportAddress(0xefbe74), export: StartRecord, argument_words: 10 },
        ExportAbi { address: ExportAddress(0xefbf58), export: StartCaptureRecord, argument_words: 14 },
        ExportAbi { address: ExportAddress(0xefc1f0), export: UpdateRecord, argument_words: 6 },
        ExportAbi { address: ExportAddress(0xefc2f8), export: ReserveClient, argument_words: 1 },
        ExportAbi { address: ExportAddress(0xefc33c), export: LensPosition, argument_words: 0 },
        ExportAbi { address: ExportAddress(0xefc360), export: ReserveClient2, argument_words: 1 },
        ExportAbi { address: ExportAddress(0xefc3a4), export: ReserveClient3, argument_words: 1 },
        ExportAbi { address: ExportAddress(0xefc3e8), export: ReserveClient4, argument_words: 1 },
        ExportAbi { address: ExportAddress(0xefc42c), export: ReserveClient5, argument_words: 1 },
        ExportAbi { address: ExportAddress(0xefc470), export: ReserveClient6, argument_words: 1 },
        ExportAbi { address: ExportAddress(0xefc4b4), export: ReserveClient7, argument_words: 1 },
        ExportAbi { address: ExportAddress(0xefc4f8), export: ReserveClient8, argument_words: 1 },
        ExportAbi { address: ExportAddress(0xefc53c), export: SetLensPosition, argument_words: 3 },
        ExportAbi { address: ExportAddress(0xefc5ac), export: MoveLens, argument_words: 3 },
        ExportAbi { address: ExportAddress(0xefc640), export: SetLensPercentage, argument_words: 3 },
        ExportAbi { address: ExportAddress(0xefc6b8), export: StopLens, argument_words: 1 },
        ExportAbi { address: ExportAddress(0xefc708), export: LensMotion, argument_words: 0 },
        ExportAbi { address: ExportAddress(0xefc73c), export: SynchronizeLens, argument_words: 1 },
        ExportAbi { address: ExportAddress(0xefc790), export: QueryLens, argument_words: 1 },
        ExportAbi { address: ExportAddress(0xefc7e0), export: SetLensValue, argument_words: 2 },
        ExportAbi { address: ExportAddress(0xefc830), export: ConfigureLens, argument_words: 3 },
        ExportAbi { address: ExportAddress(0xefc8e8), export: DefaultLens, argument_words: 1 },
        ExportAbi { address: ExportAddress(0xefc948), export: ConfigureLensValue, argument_words: 2 },
        ExportAbi { address: ExportAddress(0xefc998), export: NotifyPrimary, argument_words: 1 },
        ExportAbi { address: ExportAddress(0xefc9b4), export: ReleaseParameters, argument_words: 0 },
        ExportAbi { address: ExportAddress(0xefc9c8), export: PrimaryImageStatus, argument_words: 0 },
        ExportAbi { address: ExportAddress(0xefc9d8), export: SecondaryImageStatus, argument_words: 0 },
        ExportAbi { address: ExportAddress(0xefc9e8), export: CaptureValue, argument_words: 0 },
        ExportAbi { address: ExportAddress(0xefc9f8), export: NotifySecondary, argument_words: 1 },
        ExportAbi { address: ExportAddress(0xefca14), export: PrimaryMovieStatus, argument_words: 0 },
        ExportAbi { address: ExportAddress(0xefca30), export: SecondaryMovieStatus, argument_words: 0 },
        ExportAbi { address: ExportAddress(0xefca4c), export: SelectionRejected, argument_words: 0 },
    ]
};

pub const fn lens_availability(word: u32) -> crate::synchronous::Availability {
    if word == 2 { crate::synchronous::Availability::Unavailable }
    else { crate::synchronous::Availability::Available }
}

pub const fn readiness_status(readiness: crate::synchronous::Readiness) -> xm1_saclac_interface::CompletionStatus {
    match readiness {
        crate::synchronous::Readiness::Ready => xm1_saclac_interface::CompletionStatus(0),
        crate::synchronous::Readiness::Busy => xm1_saclac_interface::CompletionStatus(-94),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordWord(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordHalfword(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordMode {
    Default,
    First,
    Second,
}

impl RecordMode {
    pub const fn from_word(word: u32) -> Self {
        match word { 1 => Self::First, 2 => Self::Second, _ => Self::Default }
    }
    const fn word(self) -> u32 {
        match self { Self::Default => 0, Self::First => 1, Self::Second => 2 }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordSample {
    pub leading: RecordWord,
    pub components: [RecordHalfword; 4],
    pub trailing: RecordWord,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordStart {
    pub channel: RecordWord,
    pub mode: RecordMode,
    pub settings: [RecordWord; 2],
    pub range: [RecordHalfword; 2],
    pub source: RecordWord,
    pub sample: RecordSample,
}

pub fn record_start_payload(input: RecordStart, callback: StockCallbackWord) -> [u8; 68] {
    let mut bytes = [0; 68];
    bytes[0..4].copy_from_slice(&input.channel.0.to_le_bytes());
    bytes[4..8].copy_from_slice(&input.mode.word().to_le_bytes());
    bytes[8..12].copy_from_slice(&input.settings[0].0.to_le_bytes());
    bytes[12..16].copy_from_slice(&input.settings[1].0.to_le_bytes());
    bytes[16..18].copy_from_slice(&input.range[0].0.to_le_bytes());
    bytes[18..20].copy_from_slice(&input.range[1].0.to_le_bytes());
    bytes[24..28].copy_from_slice(&input.source.0.to_le_bytes());
    bytes[28..30].copy_from_slice(&100_u16.to_le_bytes());
    bytes[30..32].copy_from_slice(&16_u16.to_le_bytes());
    bytes[32..36].copy_from_slice(&input.sample.leading.0.to_le_bytes());
    for (chunk, component) in bytes[36..44].chunks_exact_mut(2).zip(input.sample.components) {
        chunk.copy_from_slice(&component.0.to_le_bytes());
    }
    bytes[44..48].copy_from_slice(&input.sample.trailing.0.to_le_bytes());
    bytes[64..68].copy_from_slice(&callback.0.to_le_bytes());
    bytes
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordUpdate {
    pub mode: crate::adjustment::ExposureMode,
    pub duration: crate::adjustment::ExposureDuration,
    pub value: RecordWord,
    pub position: crate::adjustment::LensPosition,
    pub auxiliary: RecordWord,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordLimits {
    pub exposure: crate::adjustment::ExposureLimits,
    pub minimum_position: crate::adjustment::LensPosition,
    pub maximum_position: crate::adjustment::LensPosition,
}

pub fn record_update_payload(input: RecordUpdate, limits: RecordLimits, callback: StockCallbackWord) -> [u8; 20] {
    let position = if input.position.0 < limits.minimum_position.0 {
        limits.minimum_position
    } else if input.position.0 > limits.maximum_position.0 {
        limits.maximum_position
    } else {
        input.position
    };
    let duration = crate::adjustment::exposure_duration(input.mode, input.duration, limits.exposure);
    let mut bytes = [0; 20];
    bytes[..4].copy_from_slice(&duration.0.to_le_bytes());
    bytes[4..8].copy_from_slice(&input.value.0.to_le_bytes());
    bytes[8..10].copy_from_slice(&position.0.to_le_bytes());
    bytes[12..16].copy_from_slice(&input.auxiliary.0.to_le_bytes());
    bytes[16..].copy_from_slice(&callback.0.to_le_bytes());
    bytes
}

pub fn capture_record_payload(
    input: crate::capture::Capture,
    callbacks: crate::capture::Callbacks,
    dimensions: crate::capture::Dimensions,
    encoded_position: u8,
    height: crate::capture::HeightSplit,
    total: crate::capture::HeightSplit,
    enabled: bool,
) -> [u8; 176] {
    let mut bytes = [0; 176];
    for (chunk, word) in bytes[..24].chunks_exact_mut(4).zip([
        callbacks.completion.0, callbacks.auxiliary.0, callbacks.sample.0,
        0, callbacks.progress.0, 0,
    ]) {
        chunk.copy_from_slice(&word.to_le_bytes());
    }
    let body = &mut bytes[24..];
    for (offset, source) in [(0, input.sources[0]), (16, input.sources[1])] {
        for (chunk, word) in body[offset..offset + 20].chunks_exact_mut(4).zip([
            source.identity, source.parameters[0], source.parameters[1], source.parameters[2], source.length,
        ]) {
            chunk.copy_from_slice(&word.0.to_le_bytes());
        }
    }
    for (offset, word) in [(32, input.channel.0), (72, 2), (76, input.channel.0),
        (80, input.policy.0), (92, input.auxiliary.0)] {
        body[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    for (offset, halfword) in [
        (36, input.dimensions.width.0), (38, input.dimensions.height.0),
        (60, dimensions.width.0), (62, dimensions.height.0), (104, 100), (106, 16),
        (112, input.position.0 as u16), (116, input.dimensions.width.0),
        (118, height.height.0), (120, height.excess.0), (122, total.height.0), (124, total.excess.0),
    ] {
        body[offset..offset + 2].copy_from_slice(&halfword.to_le_bytes());
    }
    body[137] = encoded_position;
    body[138] = input.source_option;
    body[140] = input.option;
    body[141] = u8::from(enabled);
    body[142] = u8::from(input.multiple_frames);
    body[147] = u8::from(input.mode != crate::capture::CaptureMode::ModeSix);
    body[148] = 1;
    bytes
}

pub const fn measurement_control_code(control: crate::children::MeasurementControl) -> u16 {
    use crate::children::MeasurementControl;
    match control {
        MeasurementControl::Initialize => 0xb7,
        MeasurementControl::Stop => 0xb8,
        MeasurementControl::Reconfigure => 0xbb,
        MeasurementControl::Suspend => 0xb9,
        MeasurementControl::Resume => 0xba,
    }
}

pub const fn adjustment_reply_status(reply: crate::adjustment::Reply) -> xm1_saclac_interface::CompletionStatus {
    use crate::adjustment::Reply;
    xm1_saclac_interface::CompletionStatus(match reply {
        Reply::Success => 0,
        Reply::Busy => -95,
        Reply::SelectionOutOfRange => -97,
    })
}

/// The live view's completion entries; `view_request` names the completion each posts.
const VIEW_STARTED: StockCallbackWord = StockCallbackWord(0xf2f7c8);
const VIEW_RESTORED: StockCallbackWord = StockCallbackWord(0xf2f7a0);
const VIEW_CONFIGURED: StockCallbackWord = StockCallbackWord(0xf2f778);
const VIEW_ACCESSOR: StockCallbackWord = StockCallbackWord(0xf2f6d8);
const VIEW_ACCESSOR_VALUE: StockCallbackWord = StockCallbackWord(0xf2f6ac);
const VIEW_STOPPED: StockCallbackWord = StockCallbackWord(0xf2f750);
const VIEW_SUSPENDED: StockCallbackWord = StockCallbackWord(0xf2f728);
const VIEW_RESUMED: StockCallbackWord = StockCallbackWord(0xf2f700);
const VIEW_ADJUSTED: StockCallbackWord = StockCallbackWord(0xf2f684);
const VIEW_WINDOWED: StockCallbackWord = StockCallbackWord(0xf2f65c);

/// The view configuration's largest payload, 0x3f26's.
const VIEW_PAYLOAD_BYTES: usize = 232;

struct Payload { bytes: [u8; VIEW_PAYLOAD_BYTES] }

impl Payload {
    fn word(&mut self, offset: usize, value: u32) { self.bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }
    fn half(&mut self, offset: usize, value: u16) { self.bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes()); }
    fn byte(&mut self, offset: usize, value: u8) { self.bytes[offset] = value; }
    fn words(&mut self, offset: usize, values: &[u32]) {
        for (index, value) in values.iter().enumerate() { self.word(offset + index * 4, *value); }
    }
}

pub const fn restore_word(options: crate::live_view::RestoreOptions) -> u32 {
    (options.first as u32) | ((options.second as u32) << 1) | ((options.negative as u32) << 31)
}

pub const fn stop_word(mode: crate::live_view::StopMode) -> u32 {
    use crate::live_view::StopMode;
    match mode {
        StopMode::Plain => 0,
        StopMode::Secondary => 1,
        StopMode::SecondaryNegative => 6,
        StopMode::Change(code) => code.get() as u32,
    }
}

/// Lays out one of the live view's image requests and sends it. Senders 0xfea438..0xfea820
/// and 0xfead6c drop the send's result.
pub fn view_request(payload: crate::live_profile::ViewPayload, imgdc: &mut impl xm1_saclac_interface::ImgDc) {
    use crate::live_profile::ViewPayload;
    use xm1_saclac_interface::{Completion as C, ImgDcOperation as Op, ImgDcRequest};
    let mut out = Payload { bytes: [0; VIEW_PAYLOAD_BYTES] };
    let (operation, length, completions): (Op, usize, &[C]) = match payload {
        ViewPayload::Start { value, measured } => {
            out.words(0, &[VIEW_STARTED.0, value, u32::from(measured)]);
            (Op::Request13, 12, &[C::Image3f])
        }
        ViewPayload::Restore(options) => {
            out.words(0, &[VIEW_RESTORED.0, 0, 0, restore_word(options)]);
            (Op::Request16, 16, &[C::Image40])
        }
        ViewPayload::Refresh { value, motion } => {
            out.words(0, &[VIEW_RESTORED.0, value, motion.0, 3]);
            (Op::Request16, 16, &[C::Image40])
        }
        ViewPayload::Configure(configured) => {
            configuration_payload(&configured, &mut out);
            (Op::Request26, VIEW_PAYLOAD_BYTES, &[C::Image41, C::Accessor4a, C::Accessor4b])
        }
        ViewPayload::Stop(mode) => {
            out.words(0, &[VIEW_STOPPED.0, stop_word(mode)]);
            (Op::Request27, 8, &[C::Image42])
        }
        ViewPayload::Suspend => {
            out.word(0, VIEW_SUSPENDED.0);
            (Op::Request28, 4, &[C::Image43])
        }
        ViewPayload::Resume => {
            out.words(0, &[VIEW_RESUMED.0, 0]);
            (Op::Request29, 8, &[C::Image44])
        }
        ViewPayload::Adjust(adjustment) => {
            out.words(0, &[VIEW_ADJUSTED.0, adjustment.selection, adjustment.mode, adjustment.first, adjustment.second]);
            (Op::Request34, 20, &[C::Image63])
        }
        ViewPayload::Window(window) => {
            out.words(0, &[VIEW_WINDOWED.0, window]);
            (Op::Request18, 8, &[C::Image65])
        }
    };
    let request = ImgDcRequest { operation, parameters: xm1_saclac_interface::stock::parameters(&out.bytes[..length]), completions };
    let _ = imgdc.request(request);
}

/// 0x3f26's layout from 0xf30f64..0xf31540 (payload offset = 0xf4 minus the frame
/// offset below r11); the bytes it leaves unwritten stay zero here.
fn configuration_payload(configured: &crate::live_profile::Configured, out: &mut Payload) {
    use crate::live_profile::SelectionClass;
    let values = &configured.values;
    out.words(0, &[VIEW_CONFIGURED.0, VIEW_ACCESSOR.0, VIEW_ACCESSOR_VALUE.0, values.head[0], configured.selection, values.head[1], values.head[2]]);
    out.words(0x1c, &values.windows[0]);
    out.words(0x28, &values.windows[1]);
    out.words(0x34, &values.block);
    out.word(0x44, match configured.selection_class {
        SelectionClass::Flagged => 4,
        SelectionClass::Unselected => 2,
        SelectionClass::Selected => 3,
    });
    for base in [0x48, 0x6c] {
        let group = &values.group;
        out.word(base, group.first);
        out.half(base + 4, group.second);
        out.byte(base + 6, group.third);
        out.word(base + 8, group.fourth);
        out.word(base + 0xc, group.fifth);
        out.half(base + 0x10, group.sixth);
        out.half(base + 0x12, group.seventh);
        out.half(base + 0x14, group.eighth);
        out.word(base + 0x18, group.ninth);
        out.half(base + 0x1c, group.tenth);
        out.word(base + 0x20, group.eleventh);
    }
    out.byte(0x90, 16);
    out.byte(0x91, 16);
    out.half(0x92, 100);
    out.half(0x94, 100);
    out.half(0x9a, values.converted[0]);
    out.half(0x9c, values.converted[1]);
    out.byte(0x9e, 7);
    out.byte(0x9f, 7);
    for (offset, property) in [0xa2, 0xa0, 0xa6, 0xa4].into_iter().zip(values.properties) { out.half(offset, property); }
    out.half(0xa8, values.converted[0]);
    out.half(0xaa, values.converted[1]);
    out.words(0xac, &[values.tail[0], configured.selection_mode, values.tail[1], values.tail[2]]);
    out.byte(0xbc, values.restorable);
    out.half(0xc4, values.level);
    out.byte(0xc8, values.detail);
    out.byte(0xc9, values.classes[0]);
    out.byte(0xca, values.classes[1]);
    out.words(0xd0, &[configured.prepared.word(), configured.refresh]);
    out.half(0xd8, configured.offset);
    out.byte(0xda, configured.motion);
    out.byte(0xdb, values.scale);
    out.byte(0xdc, configured.active);
    out.byte(0xdd, configured.view_mode);
    out.byte(0xdf, u8::from(configured.direct_start));
    out.byte(0xe0, values.capture);
    out.byte(0xe1, values.rate);
    out.word(0xe4, values.restarts);
}

/// The live statistics configuration's completion entries, 0xf3a768..0xf3aa40; each posts the
/// completion it is named for.
pub const STATISTICS_STARTED: StockCallbackWord = StockCallbackWord(0xf3aa18);
pub const STATISTICS_STOPPED: StockCallbackWord = StockCallbackWord(0xf3a9f0);
pub const CAPTURE_CONFIGURED: StockCallbackWord = StockCallbackWord(0xf3a9c8);
pub const CAPTURE_PROGRESS: StockCallbackWord = StockCallbackWord(0xf3a9a0);
pub const CAPTURE_RESULTS: StockCallbackWord = StockCallbackWord(0xf3aa40);
pub const STATISTICS_RELEASED: StockCallbackWord = StockCallbackWord(0xf3a978);
pub const ADJUSTED: StockCallbackWord = StockCallbackWord(0xf3a950);
pub const DESCRIPTOR_CHOSEN: StockCallbackWord = StockCallbackWord(0xf3a924);
pub const PREPARED: StockCallbackWord = StockCallbackWord(0xf3a8ac);
pub const OUTPUT_CONFIGURED: StockCallbackWord = StockCallbackWord(0xf3a884);
pub const WINDOW_CONFIGURED: StockCallbackWord = StockCallbackWord(0xf3a85c);
pub const FORMAT_CONFIGURED: StockCallbackWord = StockCallbackWord(0xf3a834);
pub const EXPOSURE_CONFIGURED: StockCallbackWord = StockCallbackWord(0xf3a80c);
pub const CAPTURE_RETUNED: StockCallbackWord = StockCallbackWord(0xf3a7e4);
pub const SELECTION_TAKEN: StockCallbackWord = StockCallbackWord(0xf3a7bc);
pub const RESULTS_READY: StockCallbackWord = StockCallbackWord(0xf3a794);
pub const COMMAND24_DONE: StockCallbackWord = StockCallbackWord(0xf3a768);

/// Encodes one of the live statistics configuration's requests and posts it to the statistics
/// task.
pub fn statistics_request(payload: &xm1_saclac_interface::statistics::StatisticsPayload, statistics: &mut impl xm1_saclac_interface::Statistics) {
    use xm1_saclac_interface::statistics::StatisticsPayload as P;
    use xm1_saclac_interface::Completion as C;
    let completions: &[C] = match payload {
        P::Start { .. } | P::StartSelected { .. } => &[C::Statistics69, C::Statistics7f],
        P::S1Start { .. } => &[C::Statistics7b, C::Statistics7f],
        P::Configure { .. } => &[C::Statistics6c, C::Statistics78, C::Statistics79, C::Statistics7f],
        P::Stop { .. } => &[C::Statistics6b],
        P::Release { .. } => &[C::Statistics6d],
        P::MeasuredWindow(_) | P::MeasuredSource { .. } | P::Command13 => &[],
        P::Adjust { .. } => &[C::Statistics6e, C::Statistics76],
        P::Prepare { .. } => &[C::Statistics77],
        P::Output { .. } => &[C::Statistics75],
        P::Window { .. } => &[C::Statistics72],
        P::Format { .. } => &[C::Statistics71],
        P::Exposure { .. } => &[C::Statistics73],
        P::Capture { .. } => &[C::Statistics80],
        P::Selection { .. } => &[C::Statistics7e, C::Statistics7f],
        P::Command24 { .. } => &[C::Statistics82],
    };
    post_statistics(payload, completions, statistics);
}

/// The senders 0x1039638..0x103b040 return their post's status; no caller reads it.
pub fn post_statistics(
    payload: &xm1_saclac_interface::statistics::StatisticsPayload,
    completions: &[xm1_saclac_interface::Completion],
    statistics: &mut impl xm1_saclac_interface::Statistics,
) {
    let (operation, message) = xm1_saclac_interface::stock::statistics::statistics_payload(payload);
    let request = xm1_saclac_interface::StatisticsRequest { operation, parameters: xm1_saclac_interface::stock::parameters(message.bytes()), completions };
    let _ = statistics.request(request);
}
