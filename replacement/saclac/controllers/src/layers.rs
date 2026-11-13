#![forbid(unsafe_code)]

//! The stock layers saclac's controllers call outside their own state. Each trait is
//! bound at saclac's composition root to the part that owns its layer.

use xm1_saclac_interface::stock::StockCallbackWord;
use xm1_saclac_interface::{CommandKind, ParameterValue, RecordHandle};

/// A profile-record field the controllers read or write. Its value means what the
/// record's other readers make of it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Field {
    ViewActive,
    SceneKind,
    Selection,
    SelectionNotifies,
    PreparationFirst,
    PreparationSecond,
    PreparationThird,
    PendingOutput,
    /// Zero sends a half release through the selection range update.
    ReleaseRange,
    ReleaseNotifies,
    SelectionCallbackArmed,
    SelectionPending,
    SelectionEnables,
    SelectionFlags,
    SelectionTaken,
    ViewMode,
    ViewLocked,
    PreparationRestorable,
    Command1dRestarts,
    AdjustmentTarget,
    StopQueried,
    RefreshValue,
    AdjustmentFirst,
    AdjustmentSecond,
    Command17Selection,
    Command17First,
    Command17Second,
    Command17Third,
    Command17Fourth,
    Command17Fifth,
    Command17Sixth,
    PrepareFirst,
    PrepareSecond,
    Command1dFirst,
    Command1dSecond,
    Command1eValue,
    Command20Value,
    Command21Value,
    Command23Second,
    /// Read from machine 26's record.
    StatisticsRetune,
    AutofocusClass,
    AutofocusApplied,
    AutofocusRelease,
    LensRange,
    // The statistics configuration's blocks.
    Descriptor,
    DescriptorFirst,
    DescriptorSecond,
    FormatRatio,
    FormatValue,
    FormatMode,
    // The output block's second word is `ViewMode`.
    OutputFirst,
    OutputThird,
    OutputFourth,
    ExposureFirst,
    ExposureSecond,
    ExposureThird,
    /// The exposure block's fifth setting while a selection is set.
    ExposureSelected,
    ExposureSixth,
    ExposureSeventh,
    ExposureEighth,
    FactorFirst,
    FactorSecond,
    WindowLevel,
    LensBase,
    LensIndex,
    LinearSource,
    Detail,
    CaptureFirst,
    CaptureSecond,
    CaptureOptionFirst,
    IntervalTracking,
    CaptureOptionFourth,
    CaptureOptionFifth,
    CapturePolicy,
    SelectionThird,
    /// The word the statistics start forwards.
    StartHeader,
    CaptureView,
    /// Written with 4 or 3 at a start, by the parameter feature.
    StartMode,
    /// Read at a stop: 3 clears the parameter feature, 4 sets it.
    StopMode,
    /// The code word that takes the statistics task's results; zero when none.
    ResultsCallback,
    /// The diagnostic buffer's address; zero when none.
    DiagnosticBuffer,
    // The S1 statistics snapshot's values.
    /// The first entry of the capture-window mode table.
    CaptureWindowMode,
    SpanMode,
    SpanSet,
    Level,
    Adjusted,
    LinearLevel,
    LensWord,
    Step,
    StepLimit,
    StepMode,
    ExposureWord,
    ExposureOffset,
    ViewByte,
    WindowFirst,
    WindowSecond,
    WindowThird,
    WindowFourth,
    WindowMode,
    /// Written 0 at the S1 snapshot and 1 when the S1 results are delivered.
    ResultsTaken,
    /// The count of the S1 results' records.
    ResultsCount,
    /// The time the S1 results were taken.
    ResultsTime,
    /// Byte 41 of the selection record; zero lets any result replace the selection.
    SelectionTracked,
    /// Word 0 of the lens designation.
    LensDesignation,
    /// 1 converts the lens target through the remap.
    LensConversion,
    /// The lens reference the target remap starts from.
    LensReference,
}

/// A profile word block the controllers copy whole.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Block {
    Command1f,
    Prepare26,
    /// The record copy a view suspend marks.
    Suspension,
    Lens(LensSlot),
    /// Four words: the statistics window block.
    AuxiliaryWindow,
    /// Four words.
    SelectionRange,
    /// 26 words; words 4 and 5 are spans the profile fixes.
    Spans,
    /// Two words.
    Classes,
    /// Six bytes in two words.
    WindowBytes,
    /// 12 words.
    ReleaseRange,
    /// 7 words.
    Options,
    /// Two halfwords: the lens target position and its steps.
    LensTarget,
    /// 33 words.
    Release,
    /// 8 words.
    FocusConfirmation,
    /// 20 words.
    FocusHistory,
    /// 5 words: the measured window.
    MeasuredWindow,
    /// 26 words: one of the ten S1 result records.
    ResultRecord(ResultIndex),
}

/// One of the profile's five lens pairs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensSlot { Reported, Current, Next }

/// The index of one of the ten S1 result records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResultIndex(u8);

impl ResultIndex {
    pub const COUNT: usize = 10;

    pub const fn new(index: u8) -> Option<Self> { if (index as usize) < Self::COUNT { Some(Self(index)) } else { None } }

    pub const fn get(self) -> u8 { self.0 }
}

/// A group of profile fields the last parameter change may have touched, named for
/// what the live view does when it was touched.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Change {
    ViewGeometry,
    AutofocusArea,
    AutofocusMode,
    /// Decides an autofocus restart by the view mode alone.
    ModeDecides,
    SelectionMode,
    SelectionOwner,
    SelectionNotice,
    SelectionReset,
    StopTwo,
    StopFour,
    StopFive,
    Resource,
    Shift,
    Offset,
    Scale,
    Level,
    Exposure,
    Forced,
    Class,
    Geometry,
    /// Restarts the view while the search property is set; five groups.
    PropertyGated(u8),
    Restarting(Restarting),
    Range,
    Output,
    Lens,
    LensPosition,
    LensRange,
    LensWindow,
    LensValue,
    // The fields the statistics blocks carry; `Exposure`, `Offset`, `Resource`, `Class`,
    // `Range`, `SelectionMode`, `SelectionOwner` and `SelectionReset` cover the others.
    ExposureSecond,
    ExposureThird,
    FactorFirst,
    FactorSecond,
    Retune,
    CaptureRatio,
    CaptureOptionFourth,
    Detail,
}

/// The autofocus changes that restart the view, each its own group in the record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Restarting { First, Second, Third, Fourth, Fifth, Sixth }

pub trait ProfileRecord {
    fn read(&mut self, record: RecordHandle, field: Field) -> u32;
    fn write(&mut self, record: RecordHandle, field: Field, value: u32);
    fn changed(&mut self, record: RecordHandle, change: Change) -> bool;
    fn read_block(&mut self, record: RecordHandle, block: Block, out: &mut [u32]);
    fn write_block(&mut self, record: RecordHandle, block: Block, words: &[u32]);
    /// Clears the selection a profile command replaces.
    fn clear_selection(&mut self, record: RecordHandle);
    fn select_descriptor(&mut self, record: RecordHandle, descriptor: u32);
    fn set_descriptor_pair(&mut self, record: RecordHandle, first: u32, second: u32);
    /// Enters the callback the selection owner left in the profile, if it left one.
    fn notify_selection_owner(&mut self, record: RecordHandle);
}

/// Rewrites `block`'s `N` words with the words at `cleared` set to zero.
pub(crate) fn clear_words<const N: usize>(profile: &mut impl ProfileRecord, record: RecordHandle, block: Block, cleared: &[usize]) {
    let mut words = [0; N];
    profile.read_block(record, block, &mut words);
    for &index in cleared {
        words[index] = 0;
    }
    profile.write_block(record, block, &words);
}

/// What the profile geometry layer gives the live view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeometryValue { StartValue, SelectionMode, Adjustment }

/// The command whose parameters a geometry check compares with the configured view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeometryCheck { Autofocus, Command17, Command20, Command23 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeometryFlag { Alternate, Capture, Configuring }

pub trait Geometry {
    fn value(&mut self, record: RecordHandle, value: GeometryValue) -> u32;
    fn motion(&mut self) -> u32;
    /// Whether the profile no longer fits the configured view.
    fn outgrown(&mut self, record: RecordHandle, check: GeometryCheck) -> bool;
    /// Whether the Command17 selection is admitted.
    fn admits(&mut self, requested: u32, offered: u32) -> bool;
    fn set(&mut self, flag: GeometryFlag, on: bool);
    fn alternate(&mut self) -> bool;
    /// Whether an autofocus change refreshes the lens pair.
    fn lens_follows(&mut self) -> bool;
    fn reset_selection(&mut self);
}

/// The image configuration values an autofocus change publishes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Publication { Mode, SelectionMode, Range, Position, Window }

pub trait Configuration {
    /// Holds or releases the configuration channel the view changes on.
    fn hold(&mut self, held: bool);
    fn claim_selection(&mut self);
    /// Publishes the profile's current value for `publication`.
    fn publish(&mut self, record: RecordHandle, publication: Publication);
    fn search_property(&mut self) -> u32;
    fn combine_lens_pair(&mut self, first: u32, second: u32, third: u32) -> u32;
}

/// The display-record mirror of the profile a live-view step refreshes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mirror {
    Primary,
    Configured,
    Autofocus(AutofocusMirror),
    Range,
    Output,
}

/// The groups an autofocus change copies into the display record.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AutofocusMirror {
    pub selection_mode: bool,
    pub resource: bool,
    /// The resource is converted into the record only when nothing is selected.
    pub convert_resource: bool,
    pub shift: bool,
    pub offset: bool,
    pub scale: bool,
    pub level: bool,
    pub exposure: bool,
    pub geometry: bool,
}

impl AutofocusMirror {
    pub const fn any(self) -> bool {
        self.selection_mode || self.resource || self.shift || self.offset || self.scale || self.level || self.exposure
            || self.geometry
    }
}

/// The profile geometry layer's display parameters and status.
pub trait DisplayRecords {
    fn reset(&mut self);
    /// Copies the profile into the display parameters and status the mirror names.
    fn mirror(&mut self, record: RecordHandle, mirror: Mirror);
}

/// The channel a measurement holds the display record mode on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordChannel { LiveView, S1 }

/// The profile geometry layer's display record mode.
pub trait RecordMode {
    /// True when the record mode was already initialized.
    fn initialize(&mut self) -> bool;
    fn available(&mut self, channel: RecordChannel) -> bool;
    fn begin(&mut self, channel: RecordChannel);
    fn finish(&mut self);
}

/// saclac's own helpers the live view shares with other saclac modules.
pub trait SharedHelpers {
    fn update_selection_range(&mut self, record: RecordHandle);
    fn clear_calculations(&mut self);
    /// The lens pair the autofocus change computes from the search state.
    fn searched_lens_pair(&mut self, record: RecordHandle) -> u32;
    /// Converts the profile's resource for the autofocus decision without keeping it.
    fn convert_resource(&mut self, record: RecordHandle);
}

/// The image pipeline's state as the view stop reads it.
pub trait Pipeline {
    fn stop_queried(&mut self) -> bool;
}

/// The profile geometry layer's statistics corrections.
pub trait StatisticsGeometry {
    /// The measured window after the correction factors `first` and `second` for `class`.
    fn correct_window(&mut self, window: [u16; 3], first: u32, second: u32, class: u32) -> [u16; 3];
    fn linearize(&mut self, value: u32) -> u32;
    fn reset_default_selection(&mut self);
}

/// The adjustment layer's values the statistics configuration carries.
pub trait Adjustment {
    fn mode_position(&mut self, record: RecordHandle) -> u32;
    fn refresh_dispatch(&mut self, record: RecordHandle) -> u32;
    fn first_dispatch(&mut self, record: RecordHandle) -> u32;
}

/// Shared saclac helpers the statistics configuration uses.
pub trait StatisticsHelpers {
    /// The capture window's dimensions for `mode` and the format ratio, when the pair has a
    /// window for that mode.
    fn scale_capture_window(&mut self, mode: u32, ratio: u32) -> Option<[u32; 2]>;
    fn selection_ratio(&mut self, record: RecordHandle) -> [u32; 2];
    fn lens_offset(&mut self, index: u32) -> u32;
}

/// Which diagnostic exports the configuration enables.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DiagnosticExports { pub row_data: bool, pub debug: bool }

pub trait StatisticsSettings {
    fn parameter_feature(&mut self) -> bool;
    fn set_parameter_feature(&mut self, on: bool);
    fn diagnostic_exports(&mut self) -> DiagnosticExports;
    fn export_row_data(&mut self, buffer: u32);
    fn export_debug(&mut self, buffer: u32);
}

/// The parameter words the client left with its last command.
pub trait CommandParameters {
    fn word(&mut self, command: CommandKind, index: usize) -> u32;
}

/// The profile's shared block, as the statistics configuration reads it.
pub trait ProfileShared {
    /// The measured window before correction, with its extent.
    fn measured_window(&mut self) -> ([u16; 3], u32);
    fn source(&mut self) -> xm1_saclac_interface::statistics::Source;
}

/// The buffer the statistics task writes its results into.
pub trait Results {
    fn address(&mut self) -> xm1_saclac_interface::calc::SharedAddress;
    fn read(&mut self, out: &mut [u8; xm1_saclac_interface::statistics::RESULTS_BYTES]);
    /// Enters `callback` with the results.
    fn deliver(&mut self, callback: xm1_saclac_interface::ClientCallback);
}

/// The measurement-window inputs the S1 snapshot hands the calc chain that corrects them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WindowInputs {
    pub scene: u32,
    pub words: [u32; 5],
    pub bytes: [u8; 6],
    pub lens: u32,
    pub mode: u16,
    /// `None` when the shared block has no measured window.
    pub measured: Option<[u16; 3]>,
    pub factors: [u32; 2],
    pub class: u32,
}

/// The corrected measurement window and the extent the calc chain reports.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CorrectedWindow { pub window: [u16; 3], pub extent: u32 }

/// Values the S1 statistics configuration takes from layers beside the profile record.
pub trait S1Values {
    /// The profile's descriptor with the capture policy's offset (0xf51b94).
    fn descriptor(&mut self, record: RecordHandle) -> u32;
    fn lens_value(&mut self) -> u32;
    fn refresh(&mut self, record: RecordHandle) -> u32;
    /// The imaging curve from its eleven inputs in stock's argument order.
    fn curve(&mut self, inputs: [u32; 11]) -> u32;
    fn stop_curve(&mut self, stop_mode: u32) -> u32;
    fn unselected_view(&mut self, record: RecordHandle) -> u32;
    fn span(&mut self, record: RecordHandle) -> u32;
    /// 1 in the unselected profile mode (0xf47330).
    fn unselected(&mut self, record: RecordHandle) -> u32;
    fn corrected_window(&mut self, inputs: &WindowInputs) -> CorrectedWindow;
    /// The lens target's step count for `position` and two release-range words.
    fn release_steps(&mut self, position: i16, first: u32, second: u32) -> u16;
    /// Records the snapshot in the diagnostic trace (operation 0x07000000).
    fn trace_snapshot(&mut self);
    fn release_channel(&mut self);
    /// The lens pair the autofocus change computes from the search state.
    fn searched_lens_pair(&mut self, record: RecordHandle) -> u32;
    /// Whether the results' record query reads the extended bank (property 0x1307).
    fn extended_results(&mut self) -> bool;
    /// Queries the result records into `out`: a count word and ten 104-byte records.
    fn query_records(&mut self, query: &RecordQuery, out: &mut [u8; RECORD_QUERY_BYTES]);
    fn ticks(&mut self) -> u32;
    /// Whether the profile geometry admits Command17's requested selection against the offered.
    fn admits_selection(&mut self, requested: u32, offered: u32) -> bool;
}

/// The bytes the result record query writes.
pub const RECORD_QUERY_BYTES: usize = 4 + 104 * ResultIndex::COUNT;

/// The S1 results' record query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordQuery { pub bank: u32, pub mode: u32, pub descriptor: u32, pub pair: [u32; 2], pub ratio: u32 }

/// The lens control's probes, in the order the replies read them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensProbe { First, Second, Third }

/// The lens control state the controllers read.
pub trait LensControl {
    fn overridden(&mut self, probe: LensProbe) -> bool;
    /// The lens value the profile replies carry.
    fn value(&mut self) -> u32;
    fn position(&mut self) -> u32;
    /// The mode-table index lens moves and curve queries name.
    fn mode_index(&mut self) -> u32;
    fn reference(&mut self) -> u32;
    fn step(&mut self) -> u32;
    /// The lens channel and the sixteen-bit fraction of the way to the next.
    fn channel(&mut self) -> (u32, u16);
    /// The last of the three notice words in the lens control block other tasks share.
    fn third_notice(&mut self) -> u32;
}

/// A lens drive's progress, which the LensDrive command's client receives with status 18.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriveProgress {
    /// The configuration channel lens control reports the drive at.
    pub channel: u32,
    /// LensDrive's word 4 is other than 1.
    pub fourth_carried: bool,
    pub reported: ParameterValue,
}

/// The LensDrive command's replies, which the shared layer composes.
pub trait LensDriveReplies {
    fn report_progress(&mut self, progress: DriveProgress);
}

/// What the bounds notice asks the configuration curve for, after the level and channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CurveQuery { pub range: u32, pub reference: u32, pub mode_index: u32, pub step: u32 }

/// The configuration table's lens curve and values.
pub trait LensCurve {
    /// The curve level `steps` reach from `base`.
    fn level(&mut self, steps: u16, base: u32, reference: u32, mode_index: u32) -> i16;
    /// Whether curve positions are read between lens channels (0x746018).
    fn interpolates(&mut self) -> bool;
    /// The centre, lower and upper curve positions of `level` on `channel`.
    fn positions(&mut self, level: i16, channel: u32, query: CurveQuery) -> [u32; 3];
    /// `positions` between `channel` and the next, `fraction` of the way across; `None` while
    /// the table is disabled.
    fn interpolated_positions(&mut self, level: i16, channel: u32, fraction: u16, query: CurveQuery) -> Option<[u32; 3]>;
    /// The table value a lens pair maps to, second word first (0x746864).
    fn mapped_value(&mut self, second: u32, first: u32) -> u32;
    fn combine_lens_pair(&mut self, first: u32, second: u32, third: u32) -> u32;
}

/// A lens target the remap converts (0xf47300).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemapRequest { pub position: i16, pub reference: u32, pub target: u32, pub convert: bool, pub mode_index: u32 }

/// A relative lens move (lensdc 0x420b).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensMove { pub callback: StockCallbackWord, pub steps: i16, pub mode_index: u32, pub words: [u32; 4] }
