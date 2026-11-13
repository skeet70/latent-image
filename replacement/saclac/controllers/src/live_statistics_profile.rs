#![forbid(unsafe_code)]

//! The live statistics configuration's leaf operations: the request blocks it builds from the
//! profile, its replies, conditions, change groups and results.

use crate::layers::{
    Adjustment, Block, Change, CommandParameters, Field, ProfileRecord, ProfileShared, Results, StatisticsGeometry,
    StatisticsHelpers, StatisticsSettings,
};
use crate::live_statistics::{Answered, CaptureWord, Condition, Groups, Owed, Port, Reply, Request};
use crate::replies::{Replies, ReplyCommand, Values};
use crate::stock;
use xm1_saclac_interface::statistics::{
    Capture, CaptureCommand, CaptureReport, Command24, Configuration, Exposure, Format, MeasuredWindow, Notice, Setup,
    SetupTail, StatisticsPayload as P, RESULTS_BYTES,
};
use xm1_saclac_interface::{CommandKind as K, CompletionStatus, Parameters, RecordHandle, Statistics};

/// Everything the leaf operations reach outside the configuration.
pub trait Layers {
    type Profile: ProfileRecord;
    type Shared: ProfileShared;
    type Geometry: StatisticsGeometry;
    type Adjustment: Adjustment;
    type Helpers: StatisticsHelpers;
    type Settings: StatisticsSettings;
    type Parameters: CommandParameters;
    type Results: Results;
    type Replies: Replies;
    type Statistics: Statistics;

    fn profile(&mut self) -> &mut Self::Profile;
    fn shared(&mut self) -> &mut Self::Shared;
    fn geometry(&mut self) -> &mut Self::Geometry;
    fn adjustment(&mut self) -> &mut Self::Adjustment;
    fn helpers(&mut self) -> &mut Self::Helpers;
    fn settings(&mut self) -> &mut Self::Settings;
    fn parameters(&mut self) -> &mut Self::Parameters;
    fn results(&mut self) -> &mut Self::Results;
    fn replies(&mut self) -> &mut Self::Replies;
    fn statistics(&mut self) -> &mut Self::Statistics;
    /// The live view last applied the primary profile.
    fn primary_applied(&mut self) -> bool;
    fn fault(&mut self);
}

/// The configuration's port over its layers.
pub struct Leaf<'l, L>(pub &'l mut L);

impl<L: Layers> Port for Leaf<'_, L> {
    fn request(&mut self, request: Request, record: RecordHandle) { send(request, record, self.0) }
    fn reply(&mut self, reply: Reply<'_>, record: RecordHandle) { answer(reply, record, self.0) }
    fn condition(&mut self, condition: Condition, record: RecordHandle) -> bool { holds(condition, record, self.0) }
    fn changes(&mut self, record: RecordHandle) -> Groups { changes(record, self.0) }
    fn deliver_results(&mut self, record: RecordHandle) {
        let callback = read(self.0, record, Field::ResultsCallback);
        if let Some(callback) = xm1_saclac_interface::stock::optional_callback(callback) {
            self.0.results().deliver(callback);
        }
    }
    fn select_descriptor(&mut self, descriptor: u32, record: RecordHandle) { self.0.profile().select_descriptor(record, descriptor) }
    fn fault(&mut self) { self.0.fault() }
}

fn read(layers: &mut impl Layers, record: RecordHandle, field: Field) -> u32 { layers.profile().read(record, field) }

fn block(layers: &mut impl Layers, record: RecordHandle, block: Block) -> [u32; 4] {
    let mut words = [0; 4];
    layers.profile().read_block(record, block, &mut words);
    words
}

fn selected(layers: &mut impl Layers, record: RecordHandle) -> bool { read(layers, record, Field::Selection) != 0 }

fn word(parameters: Parameters<'_>, index: usize) -> u32 {
    parameters.bytes().get(index * 4..index * 4 + 4).map_or(0, |bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn setup(layers: &mut impl Layers, record: RecordHandle, tail: SetupTail) -> Setup {
    let descriptor = read(layers, record, Field::Descriptor);
    let pair = [read(layers, record, Field::DescriptorFirst), read(layers, record, Field::DescriptorSecond)];
    Setup { descriptor, pair, tail }
}

fn format(layers: &mut impl Layers, record: RecordHandle) -> Format {
    let ratio_word = read(layers, record, Field::FormatRatio);
    let ratio = [ratio_word as u16, (ratio_word >> 16) as u16];
    let value = read(layers, record, Field::FormatValue);
    if !selected(layers, record) {
        let class = u32::from(layers.adjustment().first_dispatch(record) == 2);
        return Format { dimensions: [240, 640], ratio, value, class, mode: 0 };
    }
    let mode = read(layers, record, Field::FormatMode);
    let class = if mode.wrapping_sub(6) <= 1 { 9 } else { 8 };
    let dimensions = layers.helpers().scale_capture_window(mode, ratio_word).unwrap_or_default();
    Format { dimensions, ratio, value, class, mode }
}

fn output(layers: &mut impl Layers, record: RecordHandle) -> [u32; 4] {
    [Field::OutputFirst, Field::ViewMode, Field::OutputThird, Field::OutputFourth].map(|field| read(layers, record, field))
}

fn measured_window(layers: &mut impl Layers, record: RecordHandle) -> MeasuredWindow {
    let (window, extent) = layers.shared().measured_window();
    let first = read(layers, record, Field::FactorFirst);
    let second = read(layers, record, Field::FactorSecond);
    let class = read(layers, record, Field::AutofocusClass);
    let corrected = layers.geometry().correct_window(window, first, second, class);
    MeasuredWindow { corrected, level: read(layers, record, Field::WindowLevel) as u16, extent }
}

fn exposure(layers: &mut impl Layers, record: RecordHandle) -> Exposure {
    let fifth = if selected(layers, record) { read(layers, record, Field::ExposureSelected) } else { 1 };
    let refresh = layers.adjustment().refresh_dispatch(record);
    let settings = [
        read(layers, record, Field::ExposureFirst),
        read(layers, record, Field::ExposureSecond),
        read(layers, record, Field::ExposureThird),
        refresh,
        fifth,
        read(layers, record, Field::ExposureSixth),
        read(layers, record, Field::ExposureSeventh),
        read(layers, record, Field::ExposureEighth),
    ];
    let window = measured_window(layers, record);
    let lens = read(layers, record, Field::LensIndex);
    let adjusted = read(layers, record, Field::LensBase).wrapping_add(layers.helpers().lens_offset(lens)) as u16;
    let linear_source = read(layers, record, Field::LinearSource);
    let linear = layers.geometry().linearize(linear_source) as u16;
    let source = layers.shared().source();
    let class = read(layers, record, Field::AutofocusClass) as u8;
    let detail = read(layers, record, Field::Detail) as u8;
    Exposure { settings, window, adjusted, linear, source, class, detail }
}

/// One span word of the capture block from a window dimension and a selection ratio term.
fn span(dimension: u32, ratio: u32) -> u32 {
    (dimension.wrapping_mul(116) / 1000).wrapping_mul(ratio.wrapping_sub(3)).wrapping_add(dimension >> 1)
}

fn capture(layers: &mut impl Layers, record: RecordHandle, word: CaptureWord) -> Capture {
    let position = layers.adjustment().mode_position(record);
    let selection = read(layers, record, Field::Selection);
    let settings = [read(layers, record, Field::CaptureFirst), read(layers, record, Field::CaptureSecond)];
    let retune = match word {
        CaptureWord::Retune => read(layers, record, Field::StatisticsRetune),
        CaptureWord::Resume => 1,
        CaptureWord::Suspend => 0,
    };
    let options = [
        Field::CaptureOptionFirst,
        Field::SelectionCallbackArmed,
        Field::IntervalTracking,
        Field::CaptureOptionFourth,
        Field::CaptureOptionFifth,
    ]
    .map(|field| read(layers, record, field) as u8);
    let ratio = read(layers, record, Field::FormatRatio);
    let dimensions = layers.helpers().scale_capture_window(1, ratio).unwrap_or_default();
    let terms = layers.helpers().selection_ratio(record);
    let spans = [span(dimensions[1], terms[0]), span(dimensions[0], terms[1])];
    let policy = read(layers, record, Field::CapturePolicy) as u16;
    Capture { position, selection, settings, retune, options, spans, dimensions, policy }
}

fn flagged(layers: &mut impl Layers, record: RecordHandle) -> bool {
    read(layers, record, Field::SelectionPending) | read(layers, record, Field::ViewActive) != 0
}

fn configuration(layers: &mut impl Layers, record: RecordHandle) -> Configuration {
    let setup = setup(layers, record, SetupTail::Default);
    let format = format(layers, record);
    let window = block(layers, record, Block::AuxiliaryWindow);
    let output = output(layers, record);
    let exposure = exposure(layers, record);
    let capture = capture(layers, record, CaptureWord::Retune);
    let notice = Notice {
        pending: read(layers, record, Field::SelectionPending),
        results: layers.results().address(),
        callback: stock::RESULTS_READY,
    };
    let flag = u32::from(flagged(layers, record));
    Configuration { setup, format, window, output, exposure, capture, notice, flag }
}

fn parameter(layers: &mut impl Layers, command: K, index: usize) -> u32 { layers.parameters().word(command, index) }

fn send(request: Request, record: RecordHandle, layers: &mut impl Layers) {
    let payload = match request {
        Request::Start { selected: false } => {
            let configuration = configuration(layers, record);
            let mode = if layers.settings().parameter_feature() { 4 } else { 3 };
            layers.profile().write(record, Field::StartMode, mode);
            let feature = read(layers, record, Field::StartHeader);
            P::Start { configuration, feature, callback: stock::STATISTICS_STARTED }
        }
        Request::Start { selected: true } => {
            P::StartSelected { configuration: configuration(layers, record), callback: stock::STATISTICS_STARTED }
        }
        Request::Configure => {
            let configuration = configuration(layers, record);
            let command = CaptureCommand {
                second: parameter(layers, K::Command09, 1),
                third: parameter(layers, K::Command09, 2),
                fourth: parameter(layers, K::Command09, 3),
            };
            let view = read(layers, record, Field::CaptureView);
            let callbacks = [stock::CAPTURE_CONFIGURED, stock::CAPTURE_PROGRESS, stock::CAPTURE_RESULTS];
            P::Configure { configuration, view, command, callbacks }
        }
        Request::Stop { pause, shot_idle } => {
            post(&P::Stop { pause, shot_idle, callback: stock::STATISTICS_STOPPED }, layers);
            match read(layers, record, Field::StopMode) {
                3 => layers.settings().set_parameter_feature(false),
                4 => layers.settings().set_parameter_feature(true),
                _ => {}
            }
            return;
        }
        Request::Release => P::Release { callback: stock::STATISTICS_RELEASED },
        Request::MeasuredWindow => P::MeasuredWindow(measured_window(layers, record)),
        Request::MeasuredSource => {
            let source = layers.shared().source();
            P::MeasuredSource { source, position: layers.adjustment().mode_position(record) }
        }
        Request::Adjust => {
            let tail = SetupTail::Adjustment(parameter(layers, K::Command11, 4));
            P::Adjust { setup: setup(layers, record, tail), callbacks: [stock::ADJUSTED, stock::DESCRIPTOR_CHOSEN] }
        }
        Request::Command13 => P::Command13,
        Request::Prepare => {
            let selection = [Field::SelectionEnables, Field::SelectionFlags, Field::SelectionThird].map(|field| read(layers, record, field));
            let range = block(layers, record, Block::SelectionRange);
            P::Prepare { selection, range, exposure: exposure(layers, record), callback: stock::PREPARED }
        }
        Request::Output => P::Output { output: output(layers, record), callback: stock::OUTPUT_CONFIGURED },
        Request::Window => P::Window { window: block(layers, record, Block::AuxiliaryWindow), callback: stock::WINDOW_CONFIGURED },
        Request::Format => P::Format { format: format(layers, record), callback: stock::FORMAT_CONFIGURED },
        Request::Exposure => P::Exposure { exposure: exposure(layers, record), callback: stock::EXPOSURE_CONFIGURED },
        Request::Capture(word) => P::Capture { capture: capture(layers, record, word), callback: stock::CAPTURE_RETUNED },
        Request::Selection { refresh } => {
            let (pending, flagged) = if refresh {
                (read(layers, record, Field::SelectionPending), flagged(layers, record))
            } else {
                (0, false)
            };
            let results = layers.results().address();
            P::Selection { pending, results, callbacks: [stock::SELECTION_TAKEN, stock::RESULTS_READY], flagged }
        }
        Request::Command24 => {
            let [second, third, fourth, fifth] = [1, 2, 3, 4].map(|index| parameter(layers, K::Command24, index));
            let ratio = layers.helpers().selection_ratio(record);
            P::Command24 { command: Command24 { second, third, fourth, fifth, ratio }, callback: stock::COMMAND24_DONE }
        }
    };
    post(&payload, layers);
}

fn post(payload: &xm1_saclac_interface::statistics::StatisticsPayload, layers: &mut impl Layers) {
    stock::statistics_request(payload, layers.statistics());
}

fn respond(layers: &mut impl Layers, command: ReplyCommand, status: i32, values: Values<'_>) {
    layers.replies().reply(command, CompletionStatus(status), values);
}

fn answered(answered: Answered) -> ReplyCommand {
    match answered {
        Answered::Command0a => ReplyCommand::Command0a,
        Answered::Command13 => ReplyCommand::Command13,
    }
}

fn answer(reply: Reply<'_>, record: RecordHandle, layers: &mut impl Layers) {
    use ReplyCommand as R;
    match reply {
        Reply::Finished(Owed::Command0a) => respond(layers, R::Command09, 0, Values::Words(&[0])),
        Reply::Finished(Owed::Command11) | Reply::Descriptor { selected: false } => {
            let descriptor = read(layers, record, Field::Descriptor);
            respond(layers, R::Command11, 0, Values::Words(&[descriptor]));
        }
        Reply::Descriptor { selected: true } => {
            let descriptor = read(layers, record, Field::Descriptor);
            respond(layers, R::Command11, 18, Values::Words(&[descriptor]));
        }
        Reply::Finished(Owed::Release) => export_diagnostics(record, layers),
        Reply::Finished(Owed::Command1d) => respond(layers, R::Command1d, 0, Values::Words(&[])),
        Reply::Finished(Owed::Command1e) => respond(layers, R::Command1e, 0, Values::Words(&[])),
        Reply::Finished(Owed::Command1f) => respond(layers, R::Command1f, 0, Values::Words(&[])),
        Reply::Finished(Owed::Command20) => respond(layers, R::Command20, 0, Values::Words(&[])),
        Reply::Finished(Owed::Command21) => respond(layers, R::Command21, 0, Values::Words(&[])),
        Reply::Finished(Owed::Command22) => respond(layers, R::Command22, 0, Values::Words(&[])),
        Reply::Accepted(command) => respond(layers, answered(command), 20, Values::Words(&[])),
        Reply::Refused(command) => respond(layers, answered(command), 21, Values::Words(&[])),
        Reply::Prepared => {
            let flags = read(layers, record, Field::SelectionFlags);
            respond(layers, R::Prepare, 19, Values::Words(&[flags]));
        }
        Reply::CaptureConfigured => respond(layers, R::Command09, 16, Values::Words(&[0])),
        Reply::CaptureResults(completion) => report_capture(completion, record, layers),
        Reply::Command24 { refused, completion } => {
            let status = if refused { -94 } else { word(completion, 0) as i32 };
            respond(layers, R::Command24, status, Values::Words(&[word(completion, 1)]));
        }
    }
}

fn report_capture(completion: Parameters<'_>, record: RecordHandle, layers: &mut impl Layers) {
    let mut results = [0; RESULTS_BYTES];
    layers.results().read(&mut results);
    if word(completion, 15) == 1 {
        layers.geometry().reset_default_selection();
    }
    let ratio = read(layers, record, Field::FormatRatio);
    let mode = read(layers, record, Field::FormatMode);
    let dimensions = layers.helpers().scale_capture_window(mode, ratio).unwrap_or_default();
    let report = CaptureReport { dimensions, completion: completion.bytes(), results: &results };
    let bytes = xm1_saclac_interface::stock::statistics::capture_report(&report);
    respond(layers, ReplyCommand::Command09, 17, Values::Record(&bytes));
}

fn export_diagnostics(record: RecordHandle, layers: &mut impl Layers) {
    let exports = layers.settings().diagnostic_exports();
    let buffer = read(layers, record, Field::DiagnosticBuffer);
    if buffer == 0 {
        return;
    }
    if exports.row_data {
        layers.settings().export_row_data(buffer);
    }
    if exports.debug {
        layers.settings().export_debug(buffer);
    }
}

fn holds(condition: Condition, record: RecordHandle, layers: &mut impl Layers) -> bool {
    match condition {
        Condition::StartNeeded => {
            let mode = read(layers, record, Field::ViewMode);
            let output = read(layers, record, Field::OutputFirst);
            let locked = read(layers, record, Field::ViewLocked);
            (mode != 0 && mode != 3) || output != 0 || (mode == 3 && locked == 0)
        }
        Condition::Selected => selected(layers, record),
        Condition::PrimaryApplied => layers.primary_applied(),
        Condition::Command22Cleared => parameter(layers, K::Command22, 1) == 0,
    }
}

fn changes(record: RecordHandle, layers: &mut impl Layers) -> Groups {
    let mut changed = |change: Change| layers.profile().changed(record, change);
    let processing = [
        Change::Exposure,
        Change::ExposureSecond,
        Change::ExposureThird,
        Change::FactorFirst,
        Change::FactorSecond,
        Change::Offset,
        Change::Detail,
    ]
    .into_iter()
    .fold(false, |any, change| changed(change) | any);
    let selected = layers.profile().read(record, Field::Selection) == 1;
    let mut changed = |change: Change| layers.profile().changed(record, change);
    let (geometry, measurement) = if selected {
        (changed(Change::Resource), changed(Change::Range))
    } else {
        let measurement = [
            Change::Retune,
            Change::SelectionMode,
            Change::SelectionOwner,
            Change::SelectionReset,
            Change::CaptureOptionFourth,
            Change::CaptureRatio,
        ]
        .into_iter()
        .fold(false, |any, change| changed(change) | any);
        (changed(Change::Class), measurement)
    };
    Groups { processing, geometry, measurement }
}
