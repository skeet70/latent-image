#![forbid(unsafe_code)]

//! The live view's operations on the profile: preparation, parameter application, view
//! requests, the conditions the controller branches on and the effects it asks for.

use crate::layers::{
    clear_words, Block, Change, Configuration, DisplayRecords, Field, Geometry, GeometryCheck, GeometryFlag, GeometryValue,
    LensControl, MirrorSources, Pipeline, ProfileRecord, RecordChannel, RecordMode, SharedHelpers,
};
use crate::live_view::{
    Application, ChangeCode, Condition, Effect, MotionSetting, Preparation, ReplyStatus, RestoreOptions, StopMode,
    StopReason, ViewRequest,
};
use crate::replies::{Replies, ReplyCommand as R, Values};
use crate::stock;
use xm1_saclac_interface::{Command, CommandKind as K, CompletionStatus, ImgDc, Parameters, RecordHandle};

pub(crate) fn word(parameters: Parameters<'_>, index: usize) -> u32 {
    let start = index * 4;
    parameters.bytes().get(start..start + 4).map_or(0, |bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Command17's ten parameter words: word 1 masks the selection bits word 2 sets.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionWords(pub [u32; 10]);

impl SelectionWords {
    pub fn of(parameters: Parameters<'_>) -> Self { Self(core::array::from_fn(|index| word(parameters, index))) }
}

/// Sets the profile's preparation enables and, for each enabled bit, the preparation flag
/// `values` gives it. The S1 statistics configuration applies them too.
pub fn apply_preparation(enables: u32, values: u32, record: RecordHandle, profile: &mut impl ProfileRecord) {
    let enabled = profile.read(record, Field::SelectionEnables);
    let flags = (0..4).map(|bit| 1 << bit).filter(|bit| enables & bit != 0).fold(enabled, |flags, bit| {
        if values & bit != 0 { flags | bit } else { flags & !bit }
    });
    profile.write(record, Field::SelectionEnables, enables);
    profile.write(record, Field::SelectionFlags, flags);
}

/// Whether the profile's preparation is enabled and flagged.
pub fn preparation_requested(record: RecordHandle, profile: &mut impl ProfileRecord) -> bool {
    let flags = profile.read(record, Field::SelectionFlags);
    profile.read(record, Field::SelectionEnables) & 1 != 0 && flags & 1 != 0
}

/// A preparation mode a command leaves in the profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode { Off, On, Full }

impl Mode {
    const fn word(self) -> u32 {
        match self { Self::Off => 0, Self::On => 1, Self::Full => 2 }
    }
}

/// What the view configuration carries about the preparation before it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Prepared { #[default] None, Gated, Ungated, Refresh, Autofocus }

impl Prepared {
    pub const fn word(self) -> u32 {
        match self { Self::None => 0, Self::Gated => 1, Self::Ungated => 2, Self::Refresh => 3, Self::Autofocus => 4 }
    }
}

/// The adjustment request's values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Adjustment { pub selection: u32, pub mode: u32, pub first: u32, pub second: u32 }

/// One of the two identical value groups the view configuration carries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ValueGroup {
    pub first: u32,
    pub second: u16,
    pub third: u8,
    pub fourth: u32,
    pub fifth: u32,
    pub sixth: u16,
    /// Already corrected by the image configuration for `fourth`.
    pub seventh: u16,
    pub eighth: u16,
    pub ninth: u32,
    pub tenth: u16,
    pub eleventh: u32,
}

/// The profile layers' values the view configuration request carries unchanged.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ConfigurationValues {
    pub head: [u32; 3],
    pub windows: [[u32; 3]; 2],
    pub block: [u32; 4],
    pub group: ValueGroup,
    pub converted: [u16; 2],
    pub properties: [u16; 4],
    pub tail: [u32; 3],
    pub restorable: u8,
    pub level: u16,
    pub detail: u8,
    pub classes: [u8; 2],
    pub scale: u8,
    pub capture: u8,
    pub rate: u8,
    pub restarts: u32,
}

/// Reads what the view configuration request carries from the profile layers.
pub trait ConfigurationSource {
    fn values(&mut self, record: RecordHandle) -> ConfigurationValues;
    /// Takes the profile's pending offset for the view, clearing it.
    fn take_offset(&mut self, record: RecordHandle) -> u16;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionClass { Unselected, Selected, Flagged }

/// The view configuration request's content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Configured {
    pub values: ConfigurationValues,
    pub selection: u32,
    pub selection_mode: u32,
    pub selection_class: SelectionClass,
    pub refresh: u32,
    pub offset: u16,
    pub motion: u8,
    pub active: u8,
    pub view_mode: u8,
    pub direct_start: bool,
    pub prepared: Prepared,
}

/// A view request with the values its payload carries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ViewPayload {
    Start { value: u32, measured: bool },
    Restore(RestoreOptions),
    Refresh { value: u32, motion: MotionSetting },
    Configure(Configured),
    Stop(StopMode),
    Suspend,
    Resume,
    Adjust(Adjustment),
    Window(u32),
}

/// Everything the leaf operations reach outside the live view.
pub trait Layers {
    type Profile: ProfileRecord;
    type Geometry: Geometry;
    type Configuration: Configuration;
    type Display: DisplayRecords;
    type RecordMode: RecordMode;
    type Helpers: SharedHelpers;
    type Pipeline: Pipeline;
    type Source: ConfigurationSource;
    type ImgDc: ImgDc;
    type Replies: Replies;
    type Lens: LensControl;
    type Sources: MirrorSources;

    fn profile(&mut self) -> &mut Self::Profile;
    fn geometry(&mut self) -> &mut Self::Geometry;
    fn configuration(&mut self) -> &mut Self::Configuration;
    fn display(&mut self) -> &mut Self::Display;
    fn record_mode(&mut self) -> &mut Self::RecordMode;
    fn helpers(&mut self) -> &mut Self::Helpers;
    fn pipeline(&mut self) -> &mut Self::Pipeline;
    fn source(&mut self) -> &mut Self::Source;
    fn imgdc(&mut self) -> &mut Self::ImgDc;
    fn replies(&mut self) -> &mut Self::Replies;
    fn lens(&mut self) -> &mut Self::Lens;
    fn mirror_sources(&mut self) -> &mut Self::Sources;
}

/// The words of the suspension block and the one the suspend marks.
const SUSPENSION_WORDS: usize = 19;
const SUSPENDED_WORD: usize = 17;
const SUSPENDED: u32 = 4;

const REJECTED: CompletionStatus = CompletionStatus(21);
/// The status Command17's reply carries for a rejected selection.
const SELECTION_REJECTED: CompletionStatus = CompletionStatus(-96);
/// The status the live view answers Command09 with, whatever its outcome.
const CAPTURE_ANSWERED: CompletionStatus = CompletionStatus(15);

/// The leaf state the live view's operations keep between entries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LiveProfile {
    selection_active: bool,
    primary_applied: bool,
    prepared: Prepared,
    selection_mode: u32,
}

impl LiveProfile {
    pub const fn new() -> Self {
        Self { selection_active: false, primary_applied: false, prepared: Prepared::None, selection_mode: 0 }
    }

    /// Prepares the profile for `command`; `None` prepares without one.
    pub fn prepare(&mut self, command: Option<Command<'_>>, record: RecordHandle, layers: &mut impl Layers) -> Preparation {
        self.prepared = Prepared::None;
        let mut preparation = Preparation::default();
        match command.map(|command| (command.kind, command.parameters)) {
            Some((K::Command02, parameters)) => {
                layers.configuration().claim_selection();
                reset_inactive_view(record, layers);
                layers.helpers().update_selection_range(record);
                if read(layers, record, Field::SceneKind) == 57 {
                    layers.geometry().set(GeometryFlag::Alternate, true);
                }
                let sequence = word(parameters, 3);
                layers.profile().write(record, Field::PreparationFirst, sequence);
                preparation.sequence = sequence == 0;
                if sequence < 2 {
                    set_modes(layers, record, [None, Some(Mode::On), Some(Mode::On)]);
                    notify_selection(record, layers);
                } else {
                    set_modes(layers, record, [None, Some(Mode::Full), Some(Mode::On)]);
                    if read(layers, record, Field::SelectionNotifies) != 0 {
                        notify_selection(record, layers);
                    }
                }
                layers.profile().write(record, Field::PendingOutput, 0);
                self.prepared = if read(layers, record, Field::SelectionNotifies) != 0 { Prepared::Gated } else { Prepared::Ungated };
            }
            Some((K::Command03, _)) => preparation.stop = StopReason::SecondaryProfile,
            Some((K::Command09, _)) => {
                if layers.geometry().alternate() {
                    set_modes(layers, record, [Some(Mode::Full), Some(Mode::Full), Some(Mode::Full)]);
                } else {
                    set_modes(layers, record, [Some(Mode::Off), Some(Mode::Full), Some(Mode::Off)]);
                }
                layers.geometry().set(GeometryFlag::Alternate, false);
                layers.geometry().set(GeometryFlag::Capture, true);
            }
            Some((K::Command0a, _)) => layers.geometry().set(GeometryFlag::Capture, false),
            Some((K::HalfRelease, _)) => {
                layers.configuration().claim_selection();
                reset_inactive_view(record, layers);
                if read(layers, record, Field::ReleaseRange) == 0 {
                    layers.helpers().update_selection_range(record);
                }
                let first = if read(layers, record, Field::Selection) == 0 { Mode::Off } else { Mode::Full };
                set_modes(layers, record, [Some(first), Some(Mode::Full), Some(Mode::On)]);
                if read(layers, record, Field::ReleaseNotifies) == 1 {
                    notify_selection(record, layers);
                }
                self.prepared = Prepared::Gated;
            }
            Some((K::Autofocus, _)) => {
                let stop = [
                    (Change::AutofocusArea, 2),
                    (Change::SelectionMode, 3),
                    (Change::StopFive, 5),
                    (Change::StopFour, 4),
                    (Change::StopTwo, 2),
                ]
                .into_iter()
                .find(|&(change, _)| layers.profile().changed(record, change))
                .and_then(|(_, code)| ChangeCode::new(code));
                if let Some(code) = stop { preparation.stop = StopReason::Change(code); }
                set_modes(layers, record, [Some(Mode::Full), Some(Mode::Full), Some(Mode::Full)]);
                self.prepared = Prepared::Autofocus;
            }
            Some((K::Command17, parameters)) => {
                let mask = word(parameters, 1);
                let on = |bit: u32| (mask & bit != 0).then_some(Mode::On);
                set_modes(layers, record, [on(1), on(4), on(0x20)]);
                if mask & 8 != 0 { layers.profile().write(record, Field::PendingOutput, 0); }
            }
            Some((K::Command1d, _)) => {
                set_modes(layers, record, [Some(Mode::Full), Some(Mode::Full), None]);
                self.prepared = Prepared::Refresh;
            }
            Some((K::Command22, parameters)) if word(parameters, 1) == 1 => {
                if let Some(code) = ChangeCode::new(2) { preparation.stop = StopReason::Change(code); }
            }
            Some((K::Command22, _)) => set_modes(layers, record, [Some(Mode::Full), Some(Mode::Full), Some(Mode::Full)]),
            _ => set_modes(layers, record, [Some(Mode::Full), Some(Mode::Full), Some(Mode::On)]),
        }
        preparation
    }

    /// Applies `command`'s parameters to the profile.
    pub fn apply(&mut self, command: Command<'_>, record: RecordHandle, layers: &mut impl Layers) -> Application {
        let parameters = command.parameters;
        let restart_if = |restart: bool| if restart { Application::Restart } else { Application::Applied };
        match command.kind {
            K::Command02 => {
                self.primary_applied = true;
                select(record, word(parameters, 2), layers);
                self.selection_mode = crate::live_mirror::primary(record, layers);
                Application::Applied
            }
            K::Command03 => {
                self.primary_applied = false;
                select(record, word(parameters, 3), layers);
                Application::Applied
            }
            K::Autofocus => crate::live_autofocus::apply(self, parameters, record, layers),
            K::Command11 => {
                let descriptor = word(parameters, 1);
                if descriptor != u32::MAX { layers.profile().select_descriptor(record, descriptor); }
                layers.profile().set_descriptor_pair(record, word(parameters, 2), word(parameters, 3));
                Application::Applied
            }
            K::Command17 => {
                let (requested, offered) = (word(parameters, 1), word(parameters, 2));
                if !layers.geometry().admits(requested, offered) { return Application::Refused; }
                self.apply_selection(&SelectionWords::of(parameters), record, layers.profile());
                let outgrown = requested == 2 && layers.geometry().outgrown(record, GeometryCheck::Command17);
                restart_if(outgrown || !direct_start(record, layers))
            }
            K::Prepare => {
                apply_preparation(word(parameters, 1), word(parameters, 2), record, layers.profile());
                layers.profile().write(record, Field::PrepareFirst, word(parameters, 3));
                layers.profile().write(record, Field::PrepareSecond, word(parameters, 4));
                Application::Applied
            }
            K::Command1d => {
                layers.profile().write(record, Field::Command1dFirst, word(parameters, 1));
                layers.profile().write(record, Field::Command1dSecond, word(parameters, 2));
                Application::Applied
            }
            K::Command1e => {
                layers.profile().write(record, Field::Command1eValue, word(parameters, 1));
                Application::Applied
            }
            K::Command1f => {
                let words = [word(parameters, 1), word(parameters, 2), word(parameters, 3), word(parameters, 4)];
                layers.profile().write_block(record, Block::Command1f, &words);
                Application::Applied
            }
            K::Command20 => {
                layers.profile().write(record, Field::Command20Value, word(parameters, 1));
                restart_if(layers.geometry().outgrown(record, GeometryCheck::Command20))
            }
            K::Command21 => {
                layers.profile().write(record, Field::Command21Value, word(parameters, 1));
                Application::Restart
            }
            K::Command23 => {
                layers.profile().write(record, Field::SelectionTaken, word(parameters, 1));
                layers.profile().write(record, Field::Command23Second, word(parameters, 2));
                let outgrown = layers.geometry().outgrown(record, GeometryCheck::Command23);
                restart_if(outgrown || read(layers, record, Field::ViewMode) == 3)
            }
            K::Prepare26 => {
                layers.profile().write_block(record, Block::Prepare26, &[word(parameters, 1), 1]);
                Application::Applied
            }
            _ => Application::Applied,
        }
    }

    /// Copies Command17's selection words into the profile and records whether any
    /// selection bit is left set. The S1 statistics configuration applies them too.
    pub fn apply_selection(&mut self, words: &SelectionWords, record: RecordHandle, profile: &mut impl ProfileRecord) {
        let [_, mask, values, ..] = words.0;
        let mut flags = profile.read(record, Field::Command17Selection);
        for (bit, field, index) in [
            (1, Some(Field::Command17First), 3),
            (2, Some(Field::Command17Second), 4),
            (4, Some(Field::Command17Third), 5),
            (0x20, None, 0),
            (8, Some(Field::Command17Fourth), 6),
        ] {
            if mask & bit == 0 { continue; }
            let set = values & bit != 0;
            flags = if set { flags | bit } else { flags & !bit };
            if let Some(field) = field {
                profile.write(record, field, if set { words.0[index] } else { 0 });
            }
        }
        profile.write(record, Field::Command17Fifth, words.0[8]);
        profile.write(record, Field::Command17Sixth, words.0[9]);
        self.selection_active = flags != 0;
        profile.write(record, Field::Command17Selection, flags);
    }

    pub(crate) fn set_selection_mode(&mut self, mode: u32) { self.selection_mode = mode; }

    /// Whether the profile's selection mode differs from the one recorded last, where
    /// only modes 51 and 52 are told apart.
    pub(crate) fn selection_mode_needs_restore(&self, record: RecordHandle, layers: &mut impl Layers) -> bool {
        let mode = layers.geometry().value(record, GeometryValue::SelectionMode);
        match self.selection_mode {
            recorded @ (51 | 52) => mode != recorded,
            _ => matches!(mode, 51 | 52),
        }
    }

    pub(crate) fn notice_owner(record: RecordHandle, layers: &mut impl Layers) {
        layers.helpers().clear_calculations();
        if read(layers, record, Field::SelectionCallbackArmed) == 1 {
            layers.profile().notify_selection_owner(record);
        }
    }

    pub fn condition(&mut self, condition: Condition, command: Option<Command<'_>>, record: RecordHandle, layers: &mut impl Layers) -> bool {
        match condition {
            Condition::SelectionPending => read(layers, record, Field::SelectionPending) == 1,
            Condition::DirectStart => direct_start(record, layers),
            Condition::ViewGeometryChanged => {
                let changed = layers.profile().changed(record, Change::ViewGeometry);
                !direct_start(record, layers) && changed
            }
            Condition::AutofocusAreaChanged => layers.profile().changed(record, Change::AutofocusArea),
            Condition::AutofocusModeChanged => layers.profile().changed(record, Change::AutofocusMode),
            Condition::AutofocusRetune => {
                let pending = read(layers, record, Field::SelectionPending) == 1;
                read(layers, record, Field::ViewActive) == 1 || pending
            }
            Condition::Command1dRestart => {
                if read(layers, record, Field::Command1dRestarts) != 0 { return true; }
                let mode_three = read(layers, record, Field::ViewMode) == 3;
                let requested_three = command.is_some_and(|command| word(command.parameters, 1) == 3);
                mode_three == requested_three
            }
            Condition::PreparationRequested => preparation_requested(record, layers.profile()),
            Condition::PreparationRestorable => read(layers, record, Field::PreparationRestorable) == 0,
            Condition::StatisticsRetune => read(layers, record, Field::StatisticsRetune) == 1,
            Condition::PrimaryApplied => self.primary_applied,
            Condition::SelectionActive => self.selection_active,
        }
    }

    pub fn effect(&mut self, effect: Effect, record: RecordHandle, layers: &mut impl Layers) {
        match effect {
            Effect::Hold => layers.configuration().hold(true),
            Effect::ReleaseHold => layers.configuration().hold(false),
            Effect::StoreAdjustment => {
                let adjustment = layers.geometry().value(record, GeometryValue::Adjustment);
                layers.profile().write(record, Field::AdjustmentTarget, adjustment);
            }
            Effect::BeginRecordMode => {
                let mode = layers.record_mode();
                if mode.initialize() && mode.available(RecordChannel::LiveView) {
                    mode.begin(RecordChannel::LiveView);
                }
            }
            Effect::FinishRecordMode => {
                let mode = layers.record_mode();
                if mode.available(RecordChannel::LiveView) { mode.finish(); }
            }
            Effect::QueryRecordMode => { layers.record_mode().initialize(); }
            Effect::StatisticsPreparation(on) => {
                let flags = read(layers, record, Field::SelectionFlags);
                layers.profile().write(record, Field::SelectionFlags, if on { flags | 1 } else { flags & !1 });
            }
            Effect::SelectionTaken => layers.profile().write(record, Field::SelectionTaken, 1),
        }
    }

    /// Answers the command the live view finished and clears the focus state it leaves stale.
    /// Stock reads Command17's selection from the live measurement's record; this reads `record`.
    pub fn reply(&mut self, command: Option<K>, status: ReplyStatus, record: RecordHandle, layers: &mut impl Layers) {
        let status = match status { ReplyStatus::Accepted => CompletionStatus(0), ReplyStatus::Rejected => REJECTED };
        let Some(command) = command else { return };
        let answered = match command {
            K::Command03 => {
                clear_focus(record, layers.profile());
                clear_words::<HISTORY_WORDS>(layers.profile(), record, Block::FocusHistory, &[0, 1]);
                return;
            }
            K::Autofocus => {
                clear_words::<HISTORY_WORDS>(layers.profile(), record, Block::FocusHistory, &[0]);
                return;
            }
            K::Command09 => return respond(layers, R::Command09, CAPTURE_ANSWERED, &[0]),
            K::Command0a => return respond(layers, R::Command09, status, &[0]),
            K::Command17 if status == REJECTED => return respond(layers, R::Command17, SELECTION_REJECTED, &[]),
            K::Command17 => {
                if read(layers, record, Field::Command17Selection) == 0 {
                    // The history is written twice, first with word 0 alone cleared (0xf2fab8).
                    clear_focus(record, layers.profile());
                    clear_words::<HISTORY_WORDS>(layers.profile(), record, Block::FocusHistory, &[0]);
                    clear_words::<HISTORY_WORDS>(layers.profile(), record, Block::FocusHistory, &[0, 1]);
                }
                R::Command17
            }
            K::Prepare => {
                let flags = read(layers, record, Field::SelectionFlags);
                return respond(layers, R::Prepare, status, &[flags]);
            }
            K::HalfRelease => R::HalfRelease,
            K::Command1d => R::Command1d,
            K::Command1e => R::Command1e,
            K::Command20 => R::Command20,
            K::Command21 => R::Command21,
            K::Command22 => R::Command22,
            K::Command23 => R::Command23,
            K::Prepare26 => R::Prepare26,
            K::Command29 => R::Command29,
            _ => return,
        };
        respond(layers, answered, status, &[]);
    }

    pub fn motion_setting(&mut self, layers: &mut impl Layers) -> MotionSetting {
        MotionSetting(layers.geometry().motion())
    }

    pub fn view(&mut self, request: ViewRequest, record: RecordHandle, layers: &mut impl Layers) {
        let payload = match request {
            ViewRequest::Start { measured } => {
                ViewPayload::Start { value: layers.geometry().value(record, GeometryValue::StartValue), measured }
            }
            ViewRequest::Restore(options) => ViewPayload::Restore(options),
            ViewRequest::Refresh(motion) => ViewPayload::Refresh { value: read(layers, record, Field::RefreshValue), motion },
            ViewRequest::Configure(_) => ViewPayload::Configure(self.configure(record, layers)),
            ViewRequest::Stop(mode) => {
                let queried = layers.pipeline().stop_queried();
                layers.profile().write(record, Field::StopQueried, u32::from(queried));
                ViewPayload::Stop(mode)
            }
            ViewRequest::Suspend => {
                if read(layers, record, Field::SelectionCallbackArmed) == 1 {
                    let mut block = [0; SUSPENSION_WORDS];
                    layers.profile().read_block(record, Block::Suspension, &mut block);
                    block[SUSPENDED_WORD] = SUSPENDED;
                    layers.profile().write_block(record, Block::Suspension, &block);
                }
                ViewPayload::Suspend
            }
            ViewRequest::Resume => {
                stock::view_request(ViewPayload::Resume, layers.imgdc());
                if read(layers, record, Field::Selection) != 1 {
                    layers.geometry().reset_selection();
                }
                return;
            }
            ViewRequest::Adjust { selection } => {
                let selection = if selection {
                    let active = read(layers, record, Field::ViewActive);
                    active | read(layers, record, Field::SelectionPending)
                } else {
                    0
                };
                ViewPayload::Adjust(Adjustment {
                    selection,
                    mode: layers.geometry().value(record, GeometryValue::SelectionMode),
                    first: read(layers, record, Field::AdjustmentFirst),
                    second: read(layers, record, Field::AdjustmentSecond),
                })
            }
            ViewRequest::Window(window) => ViewPayload::Window(window),
        };
        stock::view_request(payload, layers.imgdc());
    }

    fn configure(&mut self, record: RecordHandle, layers: &mut impl Layers) -> Configured {
        let refresh = read(layers, record, Field::RefreshValue);
        let motion = layers.geometry().motion() as u8;
        layers.geometry().set(GeometryFlag::Configuring, true);
        let selection = read(layers, record, Field::Selection);
        let view_mode = read(layers, record, Field::ViewMode) as u8;
        let direct_start = direct_start(record, layers);
        let active = read(layers, record, Field::ViewActive) as u8 | read(layers, record, Field::SelectionPending) as u8;
        let selection_mode = layers.geometry().value(record, GeometryValue::SelectionMode);
        let mut values = layers.source().values(record);
        values.classes = values.classes.map(|class| class.min(6));
        let selection_class = if read(layers, record, Field::Command17Selection) & 8 != 0 {
            SelectionClass::Flagged
        } else if selection == 0 {
            SelectionClass::Unselected
        } else {
            SelectionClass::Selected
        };
        let offset = layers.source().take_offset(record);
        let configured = Configured {
            values, selection, selection_mode, selection_class, refresh, offset, motion, active, view_mode, direct_start,
            prepared: self.prepared,
        };
        crate::live_mirror::configured(&configured, record, layers);
        self.selection_mode = selection_mode;
        configured
    }
}

fn read(layers: &mut impl Layers, record: RecordHandle, field: Field) -> u32 { layers.profile().read(record, field) }

fn respond(layers: &mut impl Layers, command: R, status: CompletionStatus, words: &[u32]) {
    layers.replies().reply(command, status, Values::Words(words));
}

const HISTORY_WORDS: usize = 20;

fn clear_focus(record: RecordHandle, profile: &mut impl ProfileRecord) {
    clear_words::<33>(profile, record, Block::Release, &[1, 24]);
    clear_words::<8>(profile, record, Block::FocusConfirmation, &[0]);
    clear_words::<5>(profile, record, Block::MeasuredWindow, &[3]);
}

fn direct_start(record: RecordHandle, layers: &mut impl Layers) -> bool {
    let mode = read(layers, record, Field::ViewMode);
    !(mode == 3 && read(layers, record, Field::ViewLocked) == 1)
}

fn reset_inactive_view(record: RecordHandle, layers: &mut impl Layers) {
    if read(layers, record, Field::ViewActive) == 0 {
        layers.display().reset();
    }
}

/// Lets the selection owner know of the new profile and resets the selection buffers.
fn notify_selection(record: RecordHandle, layers: &mut impl Layers) {
    LiveProfile::notice_owner(record, layers);
    layers.geometry().reset_selection();
}

/// Clears the selection a profile command replaces and selects its descriptor.
fn select(record: RecordHandle, descriptor: u32, layers: &mut impl Layers) {
    layers.profile().clear_selection(record);
    if descriptor != u32::MAX { layers.profile().select_descriptor(record, descriptor); }
}

fn set_modes(layers: &mut impl Layers, record: RecordHandle, modes: [Option<Mode>; 3]) {
    for (mode, field) in modes.into_iter().zip([Field::PreparationFirst, Field::PreparationSecond, Field::PreparationThird]) {
        if let Some(mode) = mode { layers.profile().write(record, field, mode.word()); }
    }
}
