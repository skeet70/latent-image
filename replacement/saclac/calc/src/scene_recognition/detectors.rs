//! The detectors the votes ask: the scene detectors that name a scene for the frame, the change
//! detectors that compare it with the noted frames, the rate filter that tests a scene group, and
//! the stationary decision.

use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::record::{group_00, group_04};
use xm1_saclac_profile_records::stock::shared::{group_27, group_33};
use xm1_saclac_profile_records::MeasurementSelection;

use super::history::push_front;
use super::ports::{DetectorPorts, Overlay, Steadiness, SubjectQuery, SubjectRequest};
use super::{read_grid, Features, FilterOutcome, Grid, Mode, Scene, SceneFlags, SceneRecognition};
use crate::adjust::configuration::MotionRecord;
use crate::exposure::ports::{ProfileAccess, Properties, Property, StockMemory};
use crate::exposure::settings;
use crate::stock::scene_recognition as stock;

/// The frame record's words for the shot's measurement.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureFrame {
    pub level: i16,
    pub primary_level: i16,
    pub alternate_level: i16,
    pub status: u32,
    pub position: i16,
    pub reference: u32,
    pub state: u32,
    pub selected: u32,
    /// The measurement's own answer to the level-difference test.
    pub difference: u32,
}

/// The frame record's words for live view's measurement.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LiveFrame {
    pub upper_level: i16,
    pub lower_level: i16,
    pub primary_level: i16,
    pub alternate_level: i16,
    pub third_level: i16,
    pub reference: u32,
    pub state: u32,
    pub at_4c: u32,
    pub at_3c: u32,
    pub position: i16,
    pub at_38: i16,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameRecord {
    pub capture: CaptureFrame,
    pub live: LiveFrame,
}

pub(crate) fn frame_record(ports: &mut impl ProfileAccess, handle: RecordHandle) -> FrameRecord {
    stock::frame_record(&ports.profile().read_shared(handle, &group_33::AT_07A0))
}

pub(crate) fn record_selected(ports: &mut impl ProfileAccess, handle: RecordHandle) -> bool {
    ports.profile().measurement_selection(handle) == MeasurementSelection::Record
}

/// The second property of `pair` for a record with the record measurement selection, else the first.
fn by_selection(ports: &mut impl ProfileAccess, handle: RecordHandle, pair: [Property; 2]) -> Property {
    pair[usize::from(record_selected(ports, handle))]
}

/// A switch read only for a record without the record measurement selection; 0 otherwise.
pub(crate) fn switch(ports: &mut (impl Properties + ProfileAccess), handle: RecordHandle, property: Property) -> u32 {
    if record_selected(ports, handle) { 0 } else { ports.property(property) }
}

/// The scene detectors, numbered 1..=5 as the configuration names them (table 0xf52c28).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SceneDetector {
    Portrait,
    Landscape,
    Night,
    Closeup,
    /// Answers no scene on any path (0xf4b26c).
    Silent,
}

impl SceneDetector {
    /// 0 names none; values above 5 name the last.
    pub const fn from_index(index: u32) -> Option<Self> {
        match index {
            0 => None,
            1 => Some(Self::Portrait),
            2 => Some(Self::Landscape),
            3 => Some(Self::Night),
            4 => Some(Self::Closeup),
            _ => Some(Self::Silent),
        }
    }

    pub const fn index(self) -> u8 {
        match self {
            Self::Portrait => 1,
            Self::Landscape => 2,
            Self::Night => 3,
            Self::Closeup => 4,
            Self::Silent => 5,
        }
    }

    /// The detector that names `scene`, for the scenes the majority can settle on.
    pub fn naming(scene: Scene) -> Option<Self> {
        match scene.0 {
            1 | 4 | 8 | 9 | 14 | 19 | 20 | 22 | 23 | 26 | 27 | 35 | 36 | 38 | 39 | 41 | 42 => Some(Self::Portrait),
            5 | 18 | 21 | 25 | 34 | 37 | 40 => Some(Self::Landscape),
            11 => Some(Self::Night),
            48 => Some(Self::Closeup),
            _ => None,
        }
    }

    /// The scene the frame's features show, or AUTO.
    pub fn ask(self, ports: &mut (impl Properties + ProfileAccess), handle: RecordHandle, features: &Features, mode: Mode) -> Scene {
        match self {
            Self::Portrait => portrait(ports, handle, features, mode),
            Self::Landscape => landscape(ports, handle, features, mode),
            Self::Night => night(ports, handle, features, mode),
            Self::Closeup => closeup(ports, handle, features, mode),
            Self::Silent => Scene::AUTO,
        }
    }
}

fn reaches_tenths(ports: &mut impl Properties, value: i16, threshold: Property) -> bool {
    i32::from(value) >= i32::from(settings::tenths(ports, threshold))
}

fn portrait(ports: &mut (impl Properties + ProfileAccess), handle: RecordHandle, features: &Features, mode: Mode) -> Scene {
    if features.at_005d == 0 || features.subjects == 0 {
        return Scene::AUTO;
    }
    if ports.property(stock::BELOW_FIRST) != 0 {
        if below(ports, handle, features) {
            Scene::NIGHT_PORTRAIT
        } else if exceeds(ports, handle, features, mode) {
            Scene::BACKLIT_PORTRAIT
        } else {
            Scene::PORTRAIT
        }
    } else if exceeds(ports, handle, features, mode) {
        Scene::BACKLIT_PORTRAIT
    } else if below(ports, handle, features) {
        Scene::NIGHT_PORTRAIT
    } else {
        Scene::PORTRAIT
    }
}

/// The level difference test: the shot's measurement answers it itself, live view's spread
/// between its upper and lower levels must pass the limit.
fn exceeds(ports: &mut (impl Properties + ProfileAccess), handle: RecordHandle, features: &Features, mode: Mode) -> bool {
    if ports.property(stock::DIFFERENCE_TEST) == 0 {
        return false;
    }
    match mode {
        Mode::Capture => features.difference_answer == 1,
        Mode::Live => {
            let limit = by_selection(ports, handle, stock::DIFFERENCE_LIMIT);
            let difference = features.upper_level.wrapping_sub(features.lower_level);
            i32::from(difference) > i32::from(settings::tenths(ports, limit))
        }
    }
}

fn below(ports: &mut (impl Properties + ProfileAccess), handle: RecordHandle, features: &Features) -> bool {
    if ports.property(stock::BELOW_TEST) == 0 {
        return false;
    }
    let limit = by_selection(ports, handle, stock::BELOW_LIMIT);
    !reaches_tenths(ports, features.level, limit)
}

/// Live view's gate on the lens state the frame record reports, then `near`.
fn gated(ports: &mut impl Properties, features: &Features, near: bool) -> bool {
    let key = if features.at_4c < u32::from(settings::word(ports, stock::GATE_SWITCH)) { stock::GATE_BELOW } else { stock::GATE_ABOVE };
    let state_passes = match ports.property(key) {
        0 => true,
        1 => features.at_3c == 0,
        _ => matches!(features.at_3c, 0 | 2 | 3),
    };
    state_passes && near
}

fn level_at_least(ports: &mut (impl Properties + ProfileAccess), handle: RecordHandle, level: i16, pair: [Property; 2]) -> bool {
    let threshold = by_selection(ports, handle, pair);
    level >= settings::signed_tenths(ports, threshold)
}

fn landscape(ports: &mut (impl Properties + ProfileAccess), handle: RecordHandle, features: &Features, mode: Mode) -> Scene {
    let near = features.position <= features.response;
    let passes = match mode {
        Mode::Capture => features.status <= 1 && near,
        Mode::Live => gated(ports, features, near),
    };
    if passes && level_at_least(ports, handle, features.level, stock::LANDSCAPE_LEVEL) { Scene::LANDSCAPE } else { Scene::AUTO }
}

fn night(ports: &mut (impl Properties + ProfileAccess), handle: RecordHandle, features: &Features, mode: Mode) -> Scene {
    let threshold = by_selection(ports, handle, stock::NIGHT_LEVEL);
    if features.level > settings::signed_tenths(ports, threshold) {
        return Scene::AUTO;
    }
    let near = features.position <= features.response;
    match mode {
        Mode::Capture => {
            if ports.property(stock::NIGHT_GATED) != 0 && (features.status > 1 || !near) {
                return Scene::AUTO;
            }
            if ports.property(stock::NIGHT_SELECTED) != 0 && features.selected == 0 {
                return Scene::AUTO;
            }
        }
        Mode::Live => {
            if ports.property(stock::NIGHT_LIVE) == 0 {
                return Scene::AUTO;
            }
        }
    }
    if ports.property(stock::NIGHT_GATED) == 0 || gated(ports, features, near) { Scene::HANDHELD_NIGHT } else { Scene::AUTO }
}

fn closeup(ports: &mut (impl Properties + ProfileAccess), handle: RecordHandle, features: &Features, mode: Mode) -> Scene {
    let near = features.position >= features.distance;
    let passes = match mode {
        Mode::Capture => near,
        Mode::Live => gated(ports, features, near),
    };
    if passes && level_at_least(ports, handle, features.level, stock::CLOSEUP_LEVEL) { Scene::CLOSEUP } else { Scene::AUTO }
}

/// A portrait at a landscape's level is a portrait in a landscape (0xf4bc58).
pub fn with_landscape(ports: &mut (impl Properties + ProfileAccess), handle: RecordHandle, features: &Features, scene: Scene) -> Scene {
    let promoted = match scene {
        Scene::PORTRAIT => Scene::LANDSCAPE_PORTRAIT,
        Scene::BACKLIT_PORTRAIT => Scene::LANDSCAPE_BACKLIT_PORTRAIT,
        _ => return scene,
    };
    if level_at_least(ports, handle, features.level, stock::LANDSCAPE_LEVEL) { promoted } else { scene }
}

/// A night scene on a stationary camera is a tripod night scene while property 0x76dd allows it
/// (0xf4acfc).
pub fn on_tripod(ports: &mut impl Properties, features: &Features, scene: Scene) -> Scene {
    if ports.property(stock::TRIPOD) == 0 || !features.stationary {
        return scene;
    }
    match scene {
        Scene::HANDHELD_NIGHT => Scene::NIGHT,
        Scene::NIGHT_PORTRAIT => Scene::NIGHT_PORTRAIT_TRIPOD,
        _ => scene,
    }
}

/// Whether `scene` may be published; scenes without a switch may not (0xf4a57c).
pub fn enabled(ports: &mut impl Properties, scene: Scene) -> bool {
    match stock::scene_enable(scene) {
        Some(property) => ports.property(property) != 0,
        None => false,
    }
}

/// Adds digital shake reduction to `flags` for a handheld frame whose level lies within the
/// scene's bounds (0xf4b49c).
pub fn with_shake_reduction(ports: &mut (impl Properties + ProfileAccess), handle: RecordHandle, features: &Features, scene: Scene, flags: SceneFlags) -> SceneFlags {
    let (allowed, style, at_0054, at_0032) = {
        let profile = ports.profile();
        let allowed = profile.read(handle, &group_00::AT_0065) != 0;
        (allowed, profile.read(handle, &group_00::AT_003F), profile.read(handle, &group_00::AT_0054), profile.read(handle, &group_00::AT_0032))
    };
    if !allowed || ports.property(stock::SHAKE_REDUCTION) == 0 || record_selected(ports, handle) {
        return flags;
    }
    if matches!(style, 2 | 3 | 7) || at_0054 == 1 || at_0032 != 0 {
        return flags;
    }
    let (level, bounds) = match scene {
        Scene::AUTO | Scene::CLOSEUP => (features.primary_level, features.bounds[0]),
        Scene::HANDHELD_NIGHT => (features.alternate_level, features.bounds[1]),
        _ => return flags,
    };
    if level < bounds.low || level > bounds.high {
        return flags;
    }
    if switch(ports, handle, stock::KEEP_STATIONARY) != 0 && features.stationary {
        return flags;
    }
    flags.with(SceneFlags::SHAKE_REDUCTION)
}

/// The scene groups the rate filter tests: a base scene and the two scenes after it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SceneGroup {
    Beach,
    Snow,
    Sunset,
    BlueSky,
    Green,
    SkyAndGreen,
}

impl SceneGroup {
    const ALL: [Self; 6] = [Self::Beach, Self::Snow, Self::Sunset, Self::BlueSky, Self::Green, Self::SkyAndGreen];

    /// 0xf49fd8.
    pub fn of(scene: Scene) -> Option<Self> { Self::ALL.into_iter().find(|group| scene.0.wrapping_sub(group.base().0) < 3) }

    pub const fn base(self) -> Scene {
        Scene(match self {
            Self::Beach => 18,
            Self::Snow => 21,
            Self::Sunset => 25,
            Self::BlueSky => 34,
            Self::Green => 37,
            Self::SkyAndGreen => 40,
        })
    }
}

/// `scene` within its group as the portrait kind `person` shows it: a portrait or landscape
/// portrait takes the group's first variant, a backlit one its second (0xf4a154).
pub fn with_person(person: Scene, scene: Scene) -> Scene {
    let Some(group) = SceneGroup::of(scene) else { return scene };
    let variant = scene.0 - group.base().0;
    match person {
        Scene::PORTRAIT | Scene::LANDSCAPE_PORTRAIT if variant < 2 => Scene(group.base().0 + 1),
        Scene::BACKLIT_PORTRAIT | Scene::LANDSCAPE_BACKLIT_PORTRAIT if variant != 1 => Scene(group.base().0 + 2),
        _ => scene,
    }
}

/// Tests the frame against the scene group `group` names: `group` when the frame fits it or the
/// group's level test is off, else `candidate`, with the tests that passed (0xf4a848).
pub fn filter(ports: &mut impl Properties, features: &Features, group: Scene, candidate: Scene) -> (Scene, FilterOutcome) {
    let rejected = |position| (candidate, FilterOutcome { position, level: false });
    let Some(scene_group) = SceneGroup::of(group) else { return rejected(false) };
    let index = scene_group as usize;
    if ports.property(stock::FILTER_ENABLE[index]) == 0 {
        return rejected(false);
    }
    let position = ports.property(stock::FILTER_POSITION[index]) != 0;
    if position && features.position > features.response {
        return rejected(false);
    }
    if ports.property(stock::FILTER_LEVEL[index]) == 0 {
        return (group, FilterOutcome { position, level: false });
    }
    let low = i32::from(settings::tenths(ports, stock::FILTER_LOW[index]));
    let high = if scene_group == SceneGroup::Sunset { i32::from(settings::tenths(ports, stock::FILTER_SUNSET_HIGH)) } else { i32::from(i16::MAX) };
    let level = i32::from(features.level);
    if level < low || level > high { rejected(position) } else { (group, FilterOutcome { position, level: true }) }
}

/// The change detectors of the live vote's tables, by slot 1..=6 (tables 0xf52c54 and 0xf52c6c).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeDetector {
    Grid,
    Position,
    Subject,
    Reference,
    Motion,
    Stationary,
}

impl ChangeDetector {
    /// 0 names none; values above 6 name the last.
    pub const fn from_slot(slot: u32) -> Option<Self> {
        match slot {
            0 => None,
            1 => Some(Self::Grid),
            2 => Some(Self::Position),
            3 => Some(Self::Subject),
            4 => Some(Self::Reference),
            5 => Some(Self::Motion),
            _ => Some(Self::Stationary),
        }
    }

    pub const fn slot(self) -> u8 {
        match self {
            Self::Grid => 1,
            Self::Position => 2,
            Self::Subject => 3,
            Self::Reference => 4,
            Self::Motion => 5,
            Self::Stationary => 6,
        }
    }
}

/// The shot vote's change detectors, by slot; slots 3, 5 and 6 never answer (table 0xf52c3c).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureDetector {
    GridMatch,
    LensMoved,
    Reference,
    Silent,
}

impl CaptureDetector {
    /// 0 names none; values above 6 name the last.
    pub const fn from_slot(slot: u32) -> Option<Self> {
        match slot {
            0 => None,
            1 => Some(Self::GridMatch),
            2 => Some(Self::LensMoved),
            4 => Some(Self::Reference),
            _ => Some(Self::Silent),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureChange {
    None,
    Changed,
    /// True: the shifted grid lies above the reference grid, or the reference position beyond
    /// the shot's.
    Toward(bool),
}

/// A cell's weight in the grid comparisons, by property 0x75c3: every cell, the centre four,
/// or each cell's zone setting.
fn cell_weight(ports: &mut impl Properties, cell: usize) -> u32 {
    match ports.property(stock::GRID_WEIGHTING) {
        1 => 1,
        2 => u32::from(stock::CENTRE.contains(&cell)),
        _ => ports.property(stock::zone_weight(cell)),
    }
}

/// The weighted mean absolute difference of two grids, a weight sum of 0 counting as 1.
fn weighted_difference(ports: &mut impl Properties, grid: &Grid, other: &Grid) -> u16 {
    let (mut sum, mut weights) = (0u32, 0u32);
    for (cell, (left, right)) in grid.iter().zip(other).enumerate() {
        let weight = cell_weight(ports, cell);
        sum = sum.wrapping_add((i32::from(*left) - i32::from(*right)).unsigned_abs().wrapping_mul(weight));
        weights = weights.wrapping_add(weight);
    }
    (sum / weights.max(1)) as u16
}

/// Whether a mean, read as signed, reaches a threshold in tenths.
fn mean_reaches(ports: &mut impl Properties, mean: u16, threshold: Property) -> bool { reaches_tenths(ports, mean as i16, threshold) }

/// The distance between two words' low bytes read as signed.
fn byte_distance(left: u32, right: u32) -> u8 { (i32::from(left as u8 as i8) - i32::from(right as u8 as i8)).unsigned_abs() as u8 }

fn position_distance(left: i16, right: i16) -> u16 { (i32::from(left) - i32::from(right)).unsigned_abs() as u16 }

/// Motion classes 2 and 3 count as the same motion.
fn motion_differs(motion: u32, noted: u32) -> bool { motion != noted && !(matches!(motion, 2 | 3) && matches!(noted, 2 | 3)) }

pub(crate) fn subject_request(ports: &mut impl ProfileAccess, handle: RecordHandle) -> SubjectRequest {
    let profile = ports.profile();
    let scale = profile.read(handle, &group_00::AT_0000);
    SubjectRequest {
        chosen: profile.chosen_word(handle),
        at_0164: profile.read(handle, &group_04::AT_0164),
        at_0168: profile.read(handle, &group_04::AT_0168),
        row_scale: [scale as u16, (scale >> 16) as u16],
    }
}

impl SceneRecognition {
    /// The subject query's count, or the last count when it refuses.
    pub(crate) fn query_subjects(&mut self, ports: &mut (impl ProfileAccess + SubjectQuery), handle: RecordHandle) -> u32 {
        let request = subject_request(ports, handle);
        let subjects = ports.subjects(&request).unwrap_or(self.last_subjects);
        self.last_subjects = subjects;
        subjects
    }

    /// Decides whether the camera stands still: when the lens reports it steady, or while fewer
    /// unsteady decisions than property 0x76de have been counted since. Draws the overlay line
    /// while property 0xeb is set (0xf4c210).
    pub fn decide_stationary(&mut self, ports: &mut (impl Properties + Steadiness + Overlay), counting: bool) -> bool {
        let steady = ports.steady();
        let record = &mut self.evaluation.against_reference.stationary;
        let stationary = if steady {
            record.count = 0;
            true
        } else {
            if counting {
                record.count = record.count.saturating_add(1);
            }
            u32::from(record.count) < ports.property(stock::STATIONARY_LIMIT)
        };
        record.stationary = stationary;
        record.steady = steady;
        if ports.property(stock::OVERLAY) != 0 {
            let mut line = [0u8; 11];
            line[..10].copy_from_slice(stock::STATIONARY_TEXT);
            line[10] = b'1';
            let length = if steady { 11 } else { 10 };
            ports.text(&line[..length], 2, 4, 2, 9);
        }
        stationary
    }

    /// The source grid the motion measurement keeps; without a source, none.
    fn source_grid(&self, memory: &mut impl StockMemory) -> Option<Grid> {
        self.evaluation.against_reference.source.map(|source| read_grid(memory, source))
    }

    /// Whether the frame changed from the reference frame by `detector`'s measure (table 0xf52c54).
    pub fn changed_from_reference(&mut self, ports: &mut impl DetectorPorts, handle: RecordHandle, detector: ChangeDetector) -> bool {
        let noted = self.evaluation.against_reference.reference.note;
        match detector {
            ChangeDetector::Grid => {
                let Some(grid) = self.source_grid(ports) else { return false };
                let mean = weighted_difference(ports, &grid, &noted.grid);
                self.evaluation.against_reference.compared = grid;
                self.evaluation.against_reference.mean = mean;
                let threshold = match SceneGroup::of(self.settled.scene) {
                    Some(group) => stock::GROUP_GRID_THRESHOLD[group as usize],
                    None => stock::MATCH_THRESHOLD,
                };
                mean_reaches(ports, mean, threshold)
            }
            ChangeDetector::Position => {
                if frame_record(ports, handle).live.at_3c > 1 {
                    return false;
                }
                let comparison = &mut self.evaluation.against_reference;
                let change = position_distance(comparison.position, noted.position);
                comparison.position_change = change;
                comparison.previous_position = comparison.position;
                change != 0
            }
            ChangeDetector::Subject => self.subject_changed(ports, handle, noted.subject),
            ChangeDetector::Reference => {
                let reference = frame_record(ports, handle).live.reference;
                byte_distance(reference, noted.reference) as u32 >= ports.property(stock::REFERENCE_THRESHOLD)
            }
            ChangeDetector::Motion => self.motion_changed(ports, handle, noted.motion),
            ChangeDetector::Stationary => !record_selected(ports, handle) && self.decide_stationary(ports, true) != noted.stationary,
        }
    }

    /// Whether the frame changed from the snapshot by `detector`'s measure; the stationary
    /// detector also records its decision in the stationary history (table 0xf52c6c).
    pub fn changed_from_snapshot(&mut self, ports: &mut impl DetectorPorts, handle: RecordHandle, detector: ChangeDetector) -> bool {
        let noted = self.evaluation.snapshot;
        match detector {
            ChangeDetector::Grid => {
                let Some(grid) = self.source_grid(ports) else { return false };
                let mean = weighted_difference(ports, &grid, &noted.grid);
                self.evaluation.against_snapshot.compared = grid;
                self.evaluation.against_snapshot.mean = mean;
                mean_reaches(ports, mean, stock::SNAPSHOT_GRID_THRESHOLD)
            }
            ChangeDetector::Position => {
                let reference = frame_record(ports, handle).live.reference;
                let position = self.evaluation.against_reference.position;
                let change = position_distance(position, noted.position);
                self.evaluation.against_snapshot.position_change = change;
                self.evaluation.against_snapshot.previous_position = position;
                change > settings::word(ports, stock::snapshot_position_threshold(reference))
            }
            ChangeDetector::Subject => self.subject_changed(ports, handle, noted.subject),
            ChangeDetector::Reference => {
                let reference = frame_record(ports, handle).live.reference;
                byte_distance(reference, noted.reference) as u32 >= ports.property(stock::SNAPSHOT_REFERENCE_THRESHOLD)
            }
            ChangeDetector::Motion => self.motion_changed(ports, handle, noted.motion),
            ChangeDetector::Stationary => {
                if record_selected(ports, handle) {
                    return false;
                }
                self.evaluation.features.index_recorded = true;
                let changed = self.decide_stationary(ports, true) != noted.stationary;
                push_front(&mut self.stationary_history, self.evaluation.against_reference.stationary);
                changed
            }
        }
    }

    /// What the shot vote's `detector` finds against the reference frame (table 0xf52c3c).
    pub fn capture_change(&self, ports: &mut (impl Properties + ProfileAccess), handle: RecordHandle, detector: CaptureDetector) -> CaptureChange {
        let noted = &self.evaluation.against_reference.reference.note;
        match detector {
            CaptureDetector::GridMatch => {
                let shift = settings::level_step(ports, 2).wrapping_sub(self.level_offset);
                let (mut reference_sum, mut shifted_sum, mut difference_sum, mut weights) = (0i32, 0i32, 0u32, 0u32);
                for (cell, (reference, shifted)) in noted.grid.iter().zip(&self.shifted_grid).enumerate() {
                    let weight = cell_weight(ports, cell);
                    let reference = i32::from(*reference) + i32::from(shift);
                    let shifted = i32::from(*shifted);
                    reference_sum = reference_sum.wrapping_add(reference.wrapping_mul(weight as i32));
                    shifted_sum = shifted_sum.wrapping_add(shifted.wrapping_mul(weight as i32));
                    difference_sum = difference_sum.wrapping_add((reference - shifted).unsigned_abs().wrapping_mul(weight));
                    weights = weights.wrapping_add(weight);
                }
                let mean = (difference_sum / weights.max(1)) as u16;
                if mean_reaches(ports, mean, stock::MATCH_THRESHOLD) { CaptureChange::Toward(shifted_sum > reference_sum) } else { CaptureChange::None }
            }
            CaptureDetector::LensMoved => {
                let position = frame_record(ports, handle).capture.position;
                if position == noted.position { CaptureChange::None } else { CaptureChange::Toward(noted.position > position) }
            }
            CaptureDetector::Reference => {
                let reference = frame_record(ports, handle).capture.reference;
                if byte_distance(reference, noted.reference) as u32 >= ports.property(stock::REFERENCE_THRESHOLD) { CaptureChange::Changed } else { CaptureChange::None }
            }
            CaptureDetector::Silent => CaptureChange::None,
        }
    }

    fn subject_changed(&mut self, ports: &mut (impl ProfileAccess + SubjectQuery), handle: RecordHandle, noted: bool) -> bool {
        if ports.profile().read(handle, &group_00::AT_005D) == 0 {
            return false;
        }
        let present = self.query_subjects(ports, handle) != 0;
        self.evaluation.against_reference.subject_present = present;
        present != noted
    }

    fn motion_changed(&mut self, ports: &mut impl ProfileAccess, handle: RecordHandle, noted: u32) -> bool {
        if record_selected(ports, handle) {
            return false;
        }
        let motion = MotionRecord::decode(ports.profile().read_shared(handle, &group_27::AT_07EC)).dominant;
        self.evaluation.against_reference.motion = motion;
        motion_differs(motion, noted)
    }
}
