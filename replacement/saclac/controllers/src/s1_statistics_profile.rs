#![forbid(unsafe_code)]

//! The S1 statistics configuration's profile values: what it takes at the S1 begin and the
//! imaging and statistics requests it builds from them.

use crate::children::Children;
use crate::layers::{
    Block, CommandParameters, CorrectedWindow, Field, LensControl, LensCurve, LensMove, ProfileRecord, ProfileShared,
    RecordMode, RemapRequest, Results, S1Values, StatisticsGeometry, StatisticsHelpers, WindowInputs,
};
use crate::replies::Replies;
use crate::s1_statistics::Request;
use crate::stock;
use xm1_saclac_interface::imaging::{ImagingPayload, S1Job};
use xm1_saclac_interface::statistics::{
    Capture, Configuration, Exposure, Format, MeasuredWindow, Notice, Setup, SetupTail, Source, StatisticsPayload as P,
};
use xm1_saclac_interface::{Completion as C, ImgDc, RecordHandle, Statistics};

/// Everything the S1 statistics configuration's leaf reaches outside its own state.
pub trait Layers {
    type Profile: ProfileRecord;
    type Shared: ProfileShared;
    type Helpers: StatisticsHelpers;
    type Geometry: StatisticsGeometry;
    type Values: S1Values;
    type Results: Results;
    type Statistics: Statistics;
    type Imaging: ImgDc;
    type Children: Children;
    type RecordMode: RecordMode;
    type Replies: Replies;
    type Parameters: CommandParameters;
    type Lens: LensControl;
    type Curve: LensCurve;

    fn profile(&mut self) -> &mut Self::Profile;
    fn shared(&mut self) -> &mut Self::Shared;
    fn helpers(&mut self) -> &mut Self::Helpers;
    fn geometry(&mut self) -> &mut Self::Geometry;
    fn values(&mut self) -> &mut Self::Values;
    fn results(&mut self) -> &mut Self::Results;
    fn statistics(&mut self) -> &mut Self::Statistics;
    fn imaging(&mut self) -> &mut Self::Imaging;
    fn children(&mut self) -> &mut Self::Children;
    fn record_mode(&mut self) -> &mut Self::RecordMode;
    fn replies(&mut self) -> &mut Self::Replies;
    fn parameters(&mut self) -> &mut Self::Parameters;
    fn lens(&mut self) -> &mut Self::Lens;
    fn curve(&mut self) -> &mut Self::Curve;
    /// The live statistics configuration's record; the S1 start header, capture geometry and
    /// output block are read from it.
    fn live_statistics_record(&mut self) -> RecordHandle;
    /// The statistics task reads this publication without borrowing the controller.
    fn report_results(&mut self, open: bool);
    /// Command27's lens option.
    fn lens_option(&mut self, record: RecordHandle);
    /// Replies to the autofocus command with the lens state (0xf46c4c).
    fn reply_autofocus_lens(&mut self);
    /// Enters the lens bounds notifier with `detail` and the curve positions (0xefb9f0).
    fn notify_bounds(&mut self, detail: u32, bounds: [u32; 3]);
    /// Tells the measurement round the selection value changed (0xf251c4).
    fn mark_selection_changed(&mut self);
    fn remap_lens_target(&mut self, request: RemapRequest) -> u16;
    /// Posts a relative lens move whose completion is 0x35.
    fn move_lens(&mut self, request: LensMove);
    /// The fault reaction (0xf46c20).
    fn fault(&mut self);
}

/// The values taken at the S1 begin that later requests carry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Taken {
    job: S1Job,
    capture_window: [u32; 2],
    format_value: u32,
    output_fourth: u32,
    exposure: [u32; 8],
    corrected: CorrectedWindow,
    source: Source,
    capture_first: u32,
    option_first: u32,
}

impl Taken {
    pub const fn view_active(&self) -> bool { self.job.view_active != 0 }
}

pub(crate) fn read(layers: &mut impl Layers, record: RecordHandle, field: Field) -> u32 { layers.profile().read(record, field) }

pub(crate) fn block<const N: usize>(layers: &mut impl Layers, record: RecordHandle, block: Block) -> [u32; N] {
    let mut words = [0; N];
    layers.profile().read_block(record, block, &mut words);
    words
}

/// Takes the snapshot (0xf22960) and makes the profile writes that go with it.
pub fn take(record: RecordHandle, layers: &mut impl Layers) -> Taken {
    use Field as F;
    layers.values().trace_snapshot();
    layers.values().release_channel();
    let selection = read(layers, record, F::Selection);
    let scene = read(layers, record, F::SceneKind);
    let option_first = read(layers, record, F::CaptureOptionFirst);
    let ratio = read(layers, record, F::FormatRatio);
    let exposure_first = [F::ExposureFirst, F::ExposureSecond, F::ExposureThird].map(|field| read(layers, record, field));
    let view_mode = read(layers, record, F::ViewMode);
    let format_value = read(layers, record, F::FormatValue);
    let output = read(layers, record, F::OutputFirst);
    let output_fourth = read(layers, record, F::OutputFourth);
    let format_mode = read(layers, record, F::FormatMode);
    let capture_window = layers.helpers().scale_capture_window(format_mode, ratio).unwrap_or_default();
    let window: [u32; 4] = block(layers, record, Block::AuxiliaryWindow);
    let pair = [read(layers, record, F::DescriptorFirst), read(layers, record, F::DescriptorSecond)];
    let descriptor = layers.values().descriptor(record);
    let descriptors = [descriptor, pair[0], pair[1]];
    let setup = descriptors;
    let view_active = read(layers, record, F::ViewActive);
    let refresh = layers.values().refresh(record);
    let exposure_word = read(layers, record, F::ExposureWord);
    let exposure_offset = read(layers, record, F::ExposureOffset) as i16;
    let exposure_last = [F::ExposureSixth, F::ExposureSeventh, F::ExposureEighth].map(|field| read(layers, record, field));
    let class = read(layers, record, F::AutofocusClass);
    let source = layers.shared().source();
    let levels = [F::Level, F::Adjusted, F::LinearLevel].map(|field| read(layers, record, field) as i16);
    let lens_word = read(layers, record, F::LensWord);
    let step = read(layers, record, F::Step);
    let step_limit = read(layers, record, F::StepLimit) as u16;
    let step_mode = read(layers, record, F::StepMode) as u8;
    let lens_index = read(layers, record, F::LensIndex);
    let view_byte = read(layers, record, F::ViewByte) as u8;
    let classes = block::<2>(layers, record, Block::Classes).map(|class| class.min(6));
    let unselected = layers.values().unselected(record);
    let header = read(layers, record, F::StartHeader);
    let interval = read(layers, record, F::IntervalTracking);
    let step = if view_byte != 0 { 12 } else if step == 12 { 20 } else { step };
    let fresh_view = read(layers, record, F::ViewActive);
    let fresh_output = read(layers, record, F::OutputFirst);
    let curve = layers.values().curve([option_first, format_value, fresh_view, header, fresh_output, selection, setup[0], step, interval, class, view_mode]);
    let stop_mode = read(layers, record, F::StopMode);
    let stop_curve = layers.values().stop_curve(stop_mode);
    let capture_first = read(layers, record, F::CaptureFirst);
    let unselected_view = layers.values().unselected_view(record);
    let span = layers.values().span(record);
    let corrected = corrected_window(layers, record, scene);
    reset_release(layers, record);

    let job = S1Job {
        levels,
        lens_index,
        lens_word,
        selection,
        step,
        step_limit,
        step_mode,
        curves: [stop_curve, curve],
        output,
        view_mode,
        descriptors,
        setup,
        window: [window[2], window[3]],
        exposure: exposure_word,
        exposure_offset,
        view_active,
        unselected_view,
        view_byte,
        classes,
        ratio,
        span,
        class,
        unselected,
        scene,
    };
    let exposure = [exposure_first[0], exposure_first[1], exposure_first[2], refresh, exposure_word, exposure_last[0], exposure_last[1], exposure_last[2]];
    Taken { job, capture_window, format_value, output_fourth, exposure, corrected, source, capture_first, option_first }
}

fn corrected_window(layers: &mut impl Layers, record: RecordHandle, scene: u32) -> CorrectedWindow {
    use Field as F;
    let words = [F::WindowFirst, F::WindowSecond, F::WindowThird, F::WindowFourth, F::ExposureSecond].map(|field| read(layers, record, field));
    let packed: [u32; 2] = block(layers, record, Block::WindowBytes);
    let low = packed[0].to_le_bytes();
    let high = packed[1].to_le_bytes();
    let bytes = [low[0], low[1], low[2], low[3], high[0], high[1]];
    let lens = layers.values().lens_value();
    let mode = read(layers, record, F::WindowMode) as u16;
    let (window, extent) = layers.shared().measured_window();
    let measured = (extent != 0).then_some(window);
    let factors = [read(layers, record, F::FactorFirst), read(layers, record, F::FactorSecond)];
    let class = read(layers, record, F::AutofocusClass);
    layers.values().corrected_window(&WindowInputs { scene, words, bytes, lens, mode, measured, factors, class })
}

/// The snapshot's profile writes.
fn reset_release(layers: &mut impl Layers, record: RecordHandle) {
    layers.profile().write(record, Field::ResultsTaken, 0);
    let release: [u32; 12] = block(layers, record, Block::ReleaseRange);
    let options: [u32; 7] = block(layers, record, Block::Options);
    let position = release[4] as u16;
    let steps = if options[1] > 1 { 0 } else { layers.values().release_steps(position as i16, release[5], release[7]) };
    layers.profile().write_block(record, Block::LensTarget, &[u32::from(position) | u32::from(steps) << 16]);
    let mut words: [u32; 33] = block(layers, record, Block::Release);
    words[1] = 0;
    words[24] = 0;
    layers.profile().write_block(record, Block::Release, &words);
}

/// One span word of the capture block from a window dimension and a selection ratio term.
fn span(dimension: u32, ratio: u32) -> u32 {
    (dimension.wrapping_mul(116) / 1000).wrapping_mul(ratio.wrapping_sub(3)).wrapping_add(dimension >> 1)
}

/// The capture block's spans and dimensions from the live statistics record (0xf22814).
fn capture_geometry(layers: &mut impl Layers) -> ([u32; 2], [u32; 2]) {
    let record = layers.live_statistics_record();
    let mode = read(layers, record, Field::CaptureWindowMode);
    let ratio = read(layers, record, Field::FormatRatio);
    let dimensions = layers.helpers().scale_capture_window(mode, ratio).unwrap_or_default();
    let spans = if read(layers, record, Field::SpanMode) <= 1 && read(layers, record, Field::SpanSet) == 1 {
        let spans: [u32; 26] = block(layers, record, Block::Spans);
        [spans[4], spans[5]]
    } else {
        let terms = layers.helpers().selection_ratio(record);
        [span(dimensions[1], terms[0]), span(dimensions[0], terms[1])]
    };
    (spans, dimensions)
}

fn start(taken: &Taken, record: RecordHandle, layers: &mut impl Layers) -> P {
    use Field as F;
    let job = &taken.job;
    let setup = Setup { descriptor: job.setup[0], pair: [job.setup[1], job.setup[2]], tail: SetupTail::S1 };
    let selected = job.selection == 1;
    let format = Format {
        dimensions: if selected { taken.capture_window } else { [240, 640] },
        ratio: [job.ratio as u16, (job.ratio >> 16) as u16],
        value: taken.format_value,
        class: if taken.option_first != 0 { 5 } else { 4 },
        mode: if selected { read(layers, record, F::FormatMode) } else { 0 },
    };
    let window = block(layers, record, Block::AuxiliaryWindow);
    let output = [job.output, job.view_mode, read(layers, record, F::OutputThird), taken.output_fourth];
    let (spans, dimensions) = capture_geometry(layers);
    let options = [
        taken.option_first as u8,
        read(layers, record, F::SelectionCallbackArmed) as u8,
        read(layers, record, F::IntervalTracking) as u8,
        read(layers, record, F::CaptureOptionFourth) as u8,
        read(layers, record, F::CaptureOptionFifth) as u8,
    ];
    // 0xf23088 leaves capture +16 unwritten; the owned S1 start defines it as zero.
    let capture = Capture {
        position: job.scene,
        selection: job.selection,
        settings: [taken.capture_first, read(layers, record, F::CaptureSecond)],
        retune: 0,
        options,
        spans,
        dimensions,
        policy: read(layers, record, F::CapturePolicy) as u16,
    };
    let linear_level = read(layers, record, F::LinearLevel);
    let linear = layers.geometry().linearize(linear_level) as u16;
    let window_block = MeasuredWindow { corrected: taken.corrected.window, level: job.levels[0] as u16, extent: taken.corrected.extent };
    let exposure = Exposure {
        settings: taken.exposure,
        window: window_block,
        adjusted: job.levels[1] as u16,
        linear,
        source: taken.source,
        class: job.class as u8,
        detail: read(layers, record, F::Detail) as u8,
    };
    let notice = Notice { pending: job.view_active, results: layers.results().address(), callback: stock::S1_RESULTS_READY };
    let flag = read(layers, record, F::ViewActive);
    let live = layers.live_statistics_record();
    let header = read(layers, live, F::StartHeader);
    let configuration = Configuration { setup, format, window, output, exposure, capture, notice, flag };
    if capture.selection != 0 {
        P::StartSelected { configuration, callback: stock::S1_STARTED }
    } else {
        P::S1Start { configuration, header, callback: stock::S1_STARTED }
    }
}

/// The capture block the retune sends, read from the profile when it is sent (0xf23584).
fn retune(record: RecordHandle, layers: &mut impl Layers) -> Capture {
    use Field as F;
    let position = read(layers, record, F::SceneKind);
    let selection = read(layers, record, F::Selection);
    let settings = [read(layers, record, F::CaptureFirst), read(layers, record, F::CaptureSecond)];
    let retune = read(layers, record, F::StatisticsRetune);
    // 0xf23584 leaves wire bytes 30 and 33 unwritten; both options are defined as zero.
    let options = [
        read(layers, record, F::CaptureOptionFirst) as u8,
        0,
        read(layers, record, F::IntervalTracking) as u8,
        read(layers, record, F::CaptureOptionFourth) as u8,
        0,
    ];
    let policy = read(layers, record, F::CapturePolicy) as u16;
    let (spans, dimensions) = capture_geometry(layers);
    Capture { position, selection, settings, retune, options, spans, dimensions, policy }
}

fn post(payload: &P, completions: &[C], layers: &mut impl Layers) { stock::post_statistics(payload, completions, layers.statistics()) }

/// Posts `request` to the imaging or statistics task, or sets Command27's lens option.
pub fn request(request: Request, taken: &Taken, record: RecordHandle, layers: &mut impl Layers) {
    match request {
        Request::BeginImaging => {
            let callbacks = [stock::S1_IMAGING_CONFIGURED, stock::S1_IMAGING_ENDED, stock::S1_IMAGING_REPORTED];
            let payload = ImagingPayload::BeginS1 { job: taken.job, callbacks };
            stock::post_imaging(&payload, &[C::Image57, C::Image58, C::Image59], layers.imaging());
        }
        Request::EndImaging => {
            stock::post_imaging(&ImagingPayload::EndS1 { callback: stock::S1_IMAGING_RELEASED }, &[], layers.imaging());
        }
        Request::Start => {
            let payload = start(taken, record, layers);
            post(&payload, &[C::Statistics7b, C::Statistics7f], layers);
        }
        Request::Stop { pause } => {
            post(&P::Stop { pause, shot_idle: true, callback: stock::S1_STOPPED }, &[C::Statistics7c], layers);
        }
        Request::Selection => {
            let results = layers.results().address();
            let payload = P::Selection {
                pending: taken.job.view_active,
                results,
                callbacks: [stock::S1_SELECTED, stock::S1_RESULTS_READY],
                flagged: true,
            };
            post(&payload, &[C::Statistics7e, C::Statistics7f], layers);
        }
        Request::Retune => {
            let capture = retune(record, layers);
            post(&P::Capture { capture, callback: stock::S1_RETUNED }, &[C::Statistics80], layers);
        }
        Request::Output => {
            let live = layers.live_statistics_record();
            let output = [Field::OutputFirst, Field::ViewMode, Field::OutputThird, Field::OutputFourth].map(|field| read(layers, live, field));
            post(&P::Output { output, callback: stock::S1_OUTPUT_CONFIGURED }, &[C::Statistics75], layers);
        }
        Request::LensOption => layers.lens_option(record),
    }
}
