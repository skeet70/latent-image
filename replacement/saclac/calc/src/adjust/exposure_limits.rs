//! The exposure limits of a measurement (id5 0xf6fb74..0xf72c3c): the level range and lens steps
//! its scene allows, the target and threshold levels inside them, and the band, margin and limits
//! an unmeasured exposure is held to. Levels are in tenths.

use crate::adjust::profile_adjustment::{LensSteps, StepTable};
use crate::exposure::ports::{Properties, Property};
use crate::exposure::program::{PrimaryInputs, PrimaryRange};
use crate::exposure::settings::{self, signed_tenths};
use crate::stock::calculation::Cell;
use crate::stock::exposure_limits::{self as stock, BandSource, LimitKey, ParameterField, SetSelector};

/// The configuration layer's lens channel levels.
pub trait ChannelLevels {
    /// The channel's level for value 0, less its class offset when `offset` is set (0x747c40).
    fn channel_level(&mut self, channel: u32, offset: bool) -> i16;
    /// The step's compensated level in the reference lens's row (0x746a80).
    fn compensated_level(&mut self, step: u32, reference: u32) -> i16;
}

/// The scene layer's response limits.
pub trait SceneLevels {
    /// The limit the capture profile's response sets at `level` for limit `index` (0xf5f3c0).
    fn response_limit(&mut self, capture_profile: u32, level: i16, index: u32) -> i16;
    /// `level - placed + offset` less the step's base level, within -200..=200 (0xf5f324).
    fn excess(&mut self, level: i16, placed: i16, step: u32, offset: i16) -> i16;
}

/// The display layer's grid cell levels.
pub trait CellLevels {
    /// 0xef7380.
    fn level_class(&mut self, cell: Cell) -> u32;
    /// The configured level of the cell's row (0xef74d0).
    fn row_level(&mut self, cell: Cell) -> i16;
}

/// Everything the limits read.
pub trait LimitPorts: Properties + LensSteps + ChannelLevels + SceneLevels + CellLevels {}

impl<T: Properties + LensSteps + ChannelLevels + SceneLevels + CellLevels> LimitPorts for T {}

/// Selection flags bit 1 replaces most limit settings with fixed ones.
const FLAGGED: u16 = 2;
/// The limit index that names no indexed limit.
pub const NO_INDEX: u32 = 4;

/// Scenes whose band is the request's level, whose limits are uncapped and whose thresholds are
/// ignored (0xf713bc).
pub(crate) const fn level_pinned(scene: u32) -> bool { matches!(scene, 24 | 45 | 47 | 55 | 56) }

/// The record's selection as the limits read it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Selection {
    /// The selection kind (0xf612dc).
    pub scene: u32,
    /// The selection flags (0xf61584).
    pub flags: u16,
    /// Gives scenes 0, 43, 44, 48 and 57 their alternate settings, and the target its lens class
    /// offset.
    pub alternate: bool,
    /// Takes the second setting of scenes 46 and 54.
    pub variant: bool,
    pub unmeasured: bool,
    /// The level the measurement is held to.
    pub threshold: Option<i16>,
    /// Gives scene 0 the lens limits of scene 48.
    pub auxiliary: bool,
}

impl Selection {
    /// A threshold as the record carries it: 0x7fff for none.
    pub const fn threshold_of(raw: i16) -> Option<i16> { if raw == i16::MAX { None } else { Some(raw) } }

    const fn flagged(&self) -> bool { self.flags & FLAGGED != 0 }

    /// 0xf704c4.
    const fn accepting(&self) -> bool { self.alternate && !self.flagged() && matches!(self.scene, 0 | 43 | 44 | 48 | 57) }

    /// The threshold, unless the scene ignores it (0xf714d4).
    const fn active_threshold(&self) -> Option<i16> {
        if matches!(self.scene, 53 | 54 | 57) || level_pinned(self.scene) {
            None
        } else {
            self.threshold
        }
    }
}

/// Which limit of a pair a stage reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitSet {
    /// Stages 3 to 5.
    First,
    Second,
}

impl LimitSet {
    pub const fn of_stage(stage: u32) -> Self { if matches!(stage, 3..=5) { Self::First } else { Self::Second } }
}

/// What chooses a clamped mode limit beside the selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LimitQuery {
    /// Indexes 0 to 2 lower the limit to an indexed one; any other leaves the mode limit.
    pub index: u32,
    /// Record byte 0x45: which of five limits scenes 0, 43..=48 and 54..=57 take.
    pub policy: u32,
    /// Shared header byte 0x63: scenes 0 and 48 take their armed limits.
    pub armed: bool,
    pub set: LimitSet,
    /// Record byte 0x3e; 0 leaves the choice to the scene.
    pub choice: u32,
    /// The least an indexed limit reaches, step 3 of the level table included.
    pub minimum: i16,
}

fn byte_tenths(ports: &mut impl Properties, id: Property) -> i16 { settings::level(ports, id).wrapping_mul(10) }

fn step_three(ports: &mut impl Properties) -> i16 { settings::level_step(ports, 3) }

fn mode_limit(ports: &mut impl Properties, key: &LimitKey, set: LimitSet, flagged: bool) -> i16 {
    settings::signed_word(ports, stock::mode_limit(key, flagged).setting(set == LimitSet::First, 0))
}

/// The mode limit of the selection, lowered to an indexed limit for indexes 0 to 2 but not below
/// `minimum` less step 3 (0xf72214).
pub fn clamped_limit(ports: &mut impl Properties, selection: &Selection, query: &LimitQuery) -> i16 {
    let key = LimitKey { scene: selection.scene, policy: query.policy, choice: query.choice, armed: query.armed };
    let limit = mode_limit(ports, &key, query.set, selection.flagged());
    if query.index > 2 {
        return limit;
    }
    let indexed = match stock::indexed_limit(&key) {
        Some(pair) => settings::signed_word(ports, pair.setting(query.set == LimitSet::First, query.index)),
        None => limit,
    };
    let step = step_three(ports);
    let indexed = if i32::from(query.minimum) > i32::from(step) + i32::from(indexed) { query.minimum.wrapping_sub(step) } else { indexed };
    indexed.min(limit)
}

/// What a measurement's limits are read for (0xf72c3c's argument).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Request {
    pub selection: Selection,
    /// The record's class; class 5 without a parameter bank takes no indexed limit.
    pub class: u32,
    /// The lens reference the steps are read for.
    pub reference: u32,
    /// Record byte 0x45.
    pub policy: u32,
    /// Shared header byte 0x63.
    pub armed: bool,
    /// Record byte 0x3e.
    pub choice: u32,
    pub tracking: bool,
    /// Nonzero reads the second half of the parameter set and the lower level by bank; bit 2 reads
    /// the bank's own lower level.
    pub bank: u8,
    pub cell: Cell,
    /// The indexed limit that lowers the limit (see `LimitQuery::index`).
    pub index: u32,
    /// The band of the level-pinned scenes; their response limits apply at or below 100.
    pub level: i16,
    /// The current lens step; its cap bounds the upper level of scenes 46 and 54.
    pub profile: u32,
    /// The upper level is the parameter set's, uncapped.
    pub fixed_upper: bool,
    pub capture_profile: u32,
    pub minimum: i16,
}

/// The step at which the threshold level changes, and the levels above and at or below it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Threshold {
    pub step: u32,
    pub above: i16,
    pub at_or_below: i16,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Band {
    pub high: i16,
    pub low: i16,
}

/// A measurement's exposure limits (0xf72c3c's result).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Limits {
    pub upper: i16,
    pub lower: i16,
    pub far_step: u32,
    pub near_step: u32,
    pub target: i16,
    /// The target without the tracking offset.
    pub plain_target: i16,
    pub ceiling: i16,
    pub threshold: Threshold,
    pub band: Band,
    pub margin: i16,
    pub limit: i16,
    /// The limit with no index.
    pub floor: i16,
}

struct Parameters {
    above: i16,
    at_or_below: i16,
    threshold_limit: u16,
    upper: i16,
    ceiling: i16,
    target_ceiling: i16,
    target_floor: i16,
}

/// The selection's parameter set (0xf71520, 0xf71f88). A selector byte outside 1..=10, which stock
/// reads past its table, takes the first set.
fn parameter_set(ports: &mut impl Properties, selection: &Selection) -> u32 {
    let selector = match stock::set_selector(selection.scene, selection.variant, selection.flagged()) {
        SetSelector::Setting(setting) => setting,
        SetSelector::Thresholded { setting, wide } => match selection.active_threshold() {
            None if selection.accepting() => stock::ACCEPTING_SET,
            None => setting,
            Some(threshold) if !wide || selection.unmeasured => {
                if threshold > settings::signed_word(ports, stock::SET_THRESHOLD) {
                    setting
                } else {
                    stock::LOW_SET
                }
            }
            Some(_) if selection.accepting() => stock::ACCEPTING_SET,
            Some(_) => stock::LOW_SET,
        },
    };
    let index = ports.property(selector).checked_sub(1);
    index.and_then(|index| stock::PARAMETER_SETS.get(index as usize)).copied().unwrap_or(stock::PARAMETER_SETS[0])
}

impl Parameters {
    /// 0xf71fa8.
    fn read(ports: &mut impl Properties, selection: &Selection, second_bank: bool) -> Self {
        let set = parameter_set(ports, selection);
        let field = |field| stock::parameter(set, second_bank, field);
        Self {
            above: byte_tenths(ports, field(ParameterField::Above)),
            at_or_below: byte_tenths(ports, field(ParameterField::AtOrBelow)),
            threshold_limit: settings::word(ports, field(ParameterField::ThresholdLimit)).wrapping_mul(10),
            upper: byte_tenths(ports, field(ParameterField::Upper)),
            ceiling: byte_tenths(ports, field(ParameterField::Ceiling)),
            target_ceiling: signed_tenths(ports, field(ParameterField::TargetCeiling)),
            target_floor: signed_tenths(ports, field(ParameterField::TargetFloor)),
        }
    }
}

/// A lens step limit in tenths, 0xffff for none (0xf70d54, 0xf70fac).
fn lens_limit(ports: &mut impl Properties, setting: Option<Property>) -> u16 {
    let Some(setting) = setting else { return u16::MAX };
    let word = settings::word(ports, setting);
    if word == u16::MAX {
        word
    } else {
        word.wrapping_mul(10)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CapRule {
    pub scene: u32,
    pub class: u32,
    /// The profile class 5 is not capped at.
    pub first: u32,
}

/// `limit` capped for `profile`: by the class-5 cap away from the first profile, then by the
/// profile's own; the level-pinned scenes are uncapped (0xf723d4).
pub(crate) fn capped(ports: &mut impl Properties, rule: CapRule, profile: u32, limit: i16) -> i16 {
    if level_pinned(rule.scene) {
        return limit;
    }
    let limit = if rule.class == 5 && profile != rule.first { limit.min(signed_tenths(ports, stock::CLASS_FIVE_CAP)) } else { limit };
    limit.min(byte_tenths(ports, stock::profile_cap(profile)))
}

/// 0xf72474.
fn upper(ports: &mut impl Properties, request: &Request, parameters: &Parameters, far_step: u32) -> i16 {
    let scene = request.selection.scene;
    if request.fixed_upper || level_pinned(scene) {
        return parameters.upper;
    }
    let profile = if matches!(scene, 46 | 54) { request.profile } else { far_step };
    let capped = capped(ports, CapRule { scene, class: 0, first: far_step }, profile, parameters.upper);
    match request.cell.column {
        0 => capped,
        column => capped.min(parameters.ceiling.wrapping_sub(100 * i16::from(column))),
    }
}

/// 0xf72094.
fn target(ports: &mut impl LimitPorts, parameters: &Parameters, reference: u32, selection: &Selection, tracking: bool) -> i16 {
    let adjustment = if tracking { signed_tenths(ports, stock::TRACKING_OFFSET) } else { 0 };
    let level = ports.channel_level(reference, selection.alternate);
    let offset = signed_tenths(ports, stock::TARGET_OFFSET);
    let target = level.wrapping_sub(offset).wrapping_sub(adjustment).min(parameters.target_ceiling);
    let target = i32::from(target).max(i32::from(parameters.target_floor) - i32::from(adjustment)) as i16;
    match selection.active_threshold() {
        Some(threshold) if selection.unmeasured => target.min(threshold),
        _ => target,
    }
}

/// 0xf72160.
fn lower(ports: &mut impl LimitPorts, request: &Request, parameters: &Parameters) -> i16 {
    let selection = &request.selection;
    if request.bank == 0 || level_pinned(selection.scene) {
        signed_tenths(ports, stock::lower_level(selection.scene, selection.accepting(), selection.variant, selection.flagged()))
    } else if request.bank & 4 != 0 {
        signed_tenths(ports, stock::bank_lower_level(selection.scene, selection.variant))
    } else {
        target(ports, parameters, request.reference, selection, request.tracking)
    }
}

/// 0xf722c8.
fn band(ports: &mut impl Properties, selection: &Selection, level: i16, tracking: bool) -> Band {
    if let Some(threshold) = selection.active_threshold() {
        return Band { high: threshold, low: threshold };
    }
    let (scene, accepting, flagged) = (selection.scene, selection.accepting(), selection.flagged());
    let source = if tracking { stock::tracking_band(scene, accepting, flagged) } else { stock::still_band(scene, accepting, flagged) };
    match source {
        BandSource::Level => Band { high: level, low: level },
        BandSource::Settings(high, low) => Band { high: byte_tenths(ports, high), low: byte_tenths(ports, low) },
    }
}

/// 0xf72334.
fn margin(ports: &mut impl Properties, selection: &Selection, tracking: bool) -> i16 {
    let setting = if tracking { stock::tracking_margin(selection.scene, selection.accepting(), selection.flagged()) } else { stock::still_margin(selection.scene, selection.accepting()) };
    let margin = match setting {
        Some(setting) => byte_tenths(ports, setting),
        None => 0,
    };
    margin.wrapping_sub(step_three(ports)).max(0)
}

impl Limits {
    /// Keeps lower ≤ target ≤ upper, target ≤ at_or_below ≤ above and near step ≤ threshold step
    /// ≤ far step (0xf729d0).
    fn normalize(&mut self) {
        self.lower = self.lower.min(self.upper);
        self.target = self.target.min(self.upper).max(self.lower);
        self.far_step = self.far_step.max(self.near_step);
        let threshold = &mut self.threshold;
        threshold.at_or_below = threshold.at_or_below.min(threshold.above);
        threshold.above = threshold.above.max(self.target);
        threshold.at_or_below = threshold.at_or_below.max(self.target);
        threshold.step = threshold.step.max(self.near_step).min(self.far_step);
    }

    /// 0xf72ac8.
    fn reconcile(&mut self, selection: &Selection, tracking: bool) {
        let active = selection.active_threshold().is_some();
        self.band.high = self.band.high.max(self.band.low);
        if !active {
            self.band.high = self.band.high.max(self.target);
            self.band.low = self.band.low.max(self.target);
        } else {
            if self.band.high < self.lower {
                self.band = Band { high: self.lower, low: self.lower };
            }
            self.raise_thresholds(self.band.high);
        }
        if tracking {
            self.plain_target = self.plain_target.max(self.lower).max(self.target);
            let plain = self.plain_target;
            self.threshold.above = self.threshold.above.max(plain);
            self.threshold.at_or_below = self.threshold.at_or_below.max(plain);
            if !active {
                self.band.high = self.band.high.max(plain);
            }
        }
        self.limit = self.limit.min(self.floor);
        self.margin = self.margin.min(self.limit);
        if self.band.high == self.band.low {
            self.margin = self.limit;
        }
    }

    fn raise_thresholds(&mut self, level: i16) {
        let threshold = &mut self.threshold;
        if threshold.above < level {
            threshold.above = level;
            threshold.at_or_below = level;
        } else {
            threshold.at_or_below = threshold.at_or_below.max(level);
        }
    }
}

/// The limits of a measurement (0xf72c3c).
pub fn limits(ports: &mut impl LimitPorts, request: &Request) -> Limits {
    let selection = &request.selection;
    let (scene, accepting, flagged) = (selection.scene, selection.accepting(), selection.flagged());
    let index = if request.class == 5 && request.bank == 0 { NO_INDEX } else { request.index };
    let parameters = Parameters::read(ports, selection, request.bank != 0);
    let far_limit = lens_limit(ports, stock::far_limit(scene, accepting, selection.auxiliary, flagged));
    let far_step = ports.far_step(request.reference, far_limit, StepTable::Standard);
    let near_limit = lens_limit(ports, stock::near_limit(scene, accepting, selection.auxiliary, flagged));
    let near_step = ports.near_step(request.reference, near_limit, StepTable::Standard);
    let tracked_target = target(ports, &parameters, request.reference, selection, request.tracking);
    let plain_target = target(ports, &parameters, request.reference, selection, false);
    // A limit in tenths is even, so never the 0xffff that reads the configured far step.
    let threshold_step = ports.far_step(request.reference, parameters.threshold_limit, StepTable::Standard);
    let mut limits = Limits {
        upper: upper(ports, request, &parameters, far_step),
        lower: lower(ports, request, &parameters),
        far_step,
        near_step,
        target: tracked_target,
        plain_target,
        ceiling: parameters.ceiling,
        threshold: Threshold { step: threshold_step, above: parameters.above, at_or_below: parameters.at_or_below },
        ..Limits::default()
    };
    limits.normalize();
    if !selection.unmeasured {
        limits.band = Band { high: limits.target, low: limits.target };
        return limits;
    }
    limits.band = band(ports, selection, request.level, request.tracking);
    let query = LimitQuery { index, policy: request.policy, armed: request.armed, set: LimitSet::Second, choice: request.choice, minimum: request.minimum };
    limits.limit = clamped_limit(ports, selection, &query);
    limits.floor = clamped_limit(ports, selection, &LimitQuery { index: NO_INDEX, ..query });
    limits.margin = margin(ports, selection, request.tracking);
    if matches!(ports.level_class(request.cell), 4 | 5) {
        let row = ports.row_level(request.cell);
        (limits.margin, limits.limit, limits.floor) = (row, row, row);
    }
    if level_pinned(scene) && request.level <= 100 {
        let indexed = ports.response_limit(request.capture_profile, request.level, index);
        let unindexed = ports.response_limit(request.capture_profile, request.level, NO_INDEX);
        limits.limit = indexed.min(limits.limit);
        limits.margin = limits.limit;
        limits.floor = unindexed.min(limits.floor);
    }
    limits.reconcile(selection, request.tracking);
    limits
}

/// The range 0xf64530 reads for an unselected measurement: its argument fixes no class, bank or
/// cell, index 4, no request level and the parameter set's upper level.
pub fn primary_range(ports: &mut impl LimitPorts, inputs: &PrimaryInputs) -> PrimaryRange {
    let request = Request {
        selection: Selection {
            scene: inputs.kind,
            flags: inputs.flags,
            alternate: inputs.focus_class != 0,
            variant: inputs.variant != 0,
            unmeasured: inputs.unmeasured,
            threshold: Selection::threshold_of(inputs.record_20),
            auxiliary: inputs.auxiliary != 0,
        },
        class: 0,
        reference: inputs.lens_reference,
        policy: inputs.policy,
        armed: inputs.armed != 0,
        choice: u32::from(inputs.record_3e),
        tracking: inputs.interval_tracking.0 != 0,
        bank: 0,
        cell: Cell::default(),
        index: NO_INDEX,
        level: i16::MAX,
        profile: u32::from(inputs.record_44),
        fixed_upper: true,
        capture_profile: inputs.curve,
        minimum: inputs.step_three,
    };
    let limits = limits(ports, &request);
    PrimaryRange { upper: limits.upper, lower: limits.lower, far_lens: limits.far_step, near_lens: limits.near_step, floor: limits.floor }
}
