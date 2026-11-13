//! Adjustment profiles: the profile a measurement chooses, the lens step limits and range set of a
//! profile, the levels and capture profile its range set selects, and the round's settlement of an
//! exposure level into a profile's range.

use xm1_saclac_profile_records::{MeasurementSelection, WindowStatus};

use crate::adjust::configuration;
use crate::exposure::ports::{Lens, Properties, Property, StockMemory};
use crate::exposure::settings;
use crate::stock::profile_adjustment::{self as stock, ChoiceTable, ProfileRecord, RangeSet};

/// An adjustment profile: one row of the profile table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdjustmentProfile(u8);

impl AdjustmentProfile {
    pub const fn new(id: u32) -> Option<Self> { if id < stock::PROFILES as u32 { Some(Self(id as u8)) } else { None } }

    pub const fn id(self) -> u32 { self.0 as u32 }

    pub const fn record(self) -> &'static ProfileRecord { &stock::PROFILE_RECORDS[self.0 as usize] }

    /// The profile whose ranges choose this profile's percent.
    pub const fn auxiliary(self) -> Self { Self(self.record().auxiliary) }

    /// The code every range of the profile carries (0xf78134).
    pub const fn code(self) -> u16 { self.0 as u16 }

    /// The profile's lens steps come from the standard limit table (0xf77ef8's condition).
    const fn standard_steps(self) -> bool {
        let record = self.record();
        record.state == 1 || record.range_limit == 46 || record.adjustment == 1
    }

    const fn fixed(self) -> usize { self.record().fixed_range_set as usize }
}

/// Which limit table a lens step is read from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepTable {
    Standard,
    /// The table the lens's own setting selects.
    Lens,
}

/// The image-4 lens services the profile adjustment reads beside `Lens`.
pub trait LensSteps {
    /// The near lens step at `limit` in tenths, or at the configured limit for 0xffff: 0x746cc8 from
    /// the standard table, 0x746e48 otherwise.
    fn near_step(&mut self, reference: u32, limit: u16, table: StepTable) -> u32;
    /// The far lens step: 0x746d6c from the standard table, 0x746ffc otherwise.
    fn far_step(&mut self, reference: u32, limit: u16, table: StepTable) -> u32;
    /// 0x746804.
    fn lens_step(&mut self, step: u32) -> u32;
}

/// The ratio of a profile code's range at a selector, times 100 (0x1007e44).
pub trait StepRatios {
    fn step_ratio(&mut self, code: u16, selector: u8) -> u32;
}

/// What chooses a measurement's adjustment profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileChoice {
    pub selection: MeasurementSelection,
    /// The result code: the round's statistics word, or the record's second result.
    pub result: u32,
    pub window_status: WindowStatus,
    pub mode: u32,
    pub flagged: bool,
    /// An unselected measurement chooses by percent band.
    pub banded: bool,
    pub percent: u16,
    pub held: bool,
    pub held_variant: u8,
}

/// The mode whose selected profiles come from the wide column.
const WIDE_MODE: u32 = 46;

fn choice_table(properties: &mut impl Properties, status: WindowStatus) -> &'static ChoiceTable {
    match status.0 {
        1 => &stock::CHOICE_ONE,
        2 => &stock::CHOICE_TWO,
        3 => &stock::CHOICE_THREE,
        4 => &stock::CHOICE_FOUR,
        _ if properties.property(stock::ALTERNATE_CHOICE) as u8 == 1 => &stock::CHOICE_FOUR,
        _ => &stock::CHOICE_THREE,
    }
}

const fn band(percent: u16, fourth_band: u16) -> usize {
    match percent {
        0..=10 => 0,
        11..=20 => 1,
        21..=30 => 2,
        _ if percent <= fourth_band => 3,
        _ => 4,
    }
}

const fn flagged_column(result: u32) -> usize {
    match result {
        2 => 0,
        11 => 1,
        12 => 2,
        14 => 3,
        17 => 4,
        18 | 31 => 5,
        19 => 6,
        26 => 7,
        27 => 8,
        _ => 9,
    }
}

const fn plain_column(result: u32) -> usize {
    match result {
        20 => 0,
        13 => 1,
        17 => 2,
        10 => 3,
        11 => 4,
        12 => 5,
        26 => 6,
        14 => 7,
        19 => 8,
        27 => 9,
        18 | 31 => 10,
        2 => 11,
        _ => 12,
    }
}

const fn selected_banded_column(result: u32) -> usize {
    match result {
        4 | 22 => 0,
        5 | 24 => 1,
        15 | 23 => 2,
        16 | 25 => 3,
        _ => 4,
    }
}

const fn selected_wide_column(result: u32) -> usize {
    match result {
        4 => 0,
        5 => 1,
        11 => 2,
        15 => 3,
        16 => 4,
        22 => 5,
        23 => 6,
        24 => 7,
        25 => 8,
        _ => 9,
    }
}

const fn selected_column(result: u32) -> usize {
    match result {
        4 => 0,
        5 => 1,
        7 | 28 => 2,
        8 | 29 => 3,
        9 | 30 => 4,
        11 => 5,
        12 => 6,
        15 => 7,
        16 => 8,
        21 => 9,
        22 => 10,
        23 => 11,
        24 => 12,
        25 => 13,
        _ => 14,
    }
}

/// The measurement's adjustment profile; `None` where the table chooses none and the caller's
/// profile stands (0xf77c8c).
pub fn choose(properties: &mut impl Properties, choice: &ProfileChoice) -> Option<AdjustmentProfile> {
    let table = choice_table(properties, choice.window_status);
    let result = choice.result;
    let id = if choice.selection != MeasurementSelection::Unselected {
        if choice.banded {
            table.selected_banded[selected_banded_column(result)]
        } else if choice.mode == WIDE_MODE {
            table.selected_wide[selected_wide_column(result)]
        } else {
            table.selected[selected_column(result)]
        }
    } else if choice.banded {
        let group = match result {
            13 | 20 => 0,
            2 | 19 | 27 => 1,
            _ => 2,
        };
        table.banded[group][band(choice.percent, table.fourth_band)]?
    } else if !choice.held {
        if choice.flagged { table.flagged[flagged_column(result)] } else { table.plain[plain_column(result)] }
    } else {
        match result {
            14 => table.held[0],
            13 | 20 => table.held[1],
            _ if choice.held_variant == 2 => table.held[2],
            _ => table.held[3],
        }
    };
    AdjustmentProfile::new(u32::from(id))
}

/// The profile's range set; a configured class beyond the table reads class 0 (0xf780d0).
pub fn range_set(properties: &mut impl Properties, profile: AdjustmentProfile) -> &'static RangeSet {
    let record = profile.record();
    let set = if record.fixed_range_set {
        record.range_key as usize
    } else {
        let class = properties.property(Property(record.range_key)) as u8;
        let classes = stock::RANGE_CLASS_SETS.get(usize::from(class)).unwrap_or(&stock::RANGE_CLASS_SETS[0]);
        usize::from(classes[record.policy as usize])
    };
    &stock::RANGE_SET_TABLE[set]
}

/// One selector's range: its lower and upper bound settings and the setting of its option.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Range {
    pub lower: Property,
    pub upper: Property,
    pub option: Property,
}

pub const SELECTORS: u8 = 5;
/// The selector a forced or alternate resolution takes.
const LAST_SELECTOR: u8 = 5;

/// The range at `selector`, 0 to 5 (0xf78440).
pub fn range(properties: &mut impl Properties, profile: AdjustmentProfile, selector: u8) -> Range {
    let [lower, upper] = range_set(properties, profile).bounds[usize::from(selector)];
    let record = profile.record();
    let option = match stock::OPTION_SETTINGS.get(record.scale as usize) {
        Some(settings) => settings[usize::from((properties.property(record.option) as u8 >> selector) & 1)],
        None => record.option,
    };
    Range { lower, upper, option }
}

fn configured_level(properties: &mut impl Properties, setting: Property) -> i16 { settings::signed_word(properties, setting) }

/// A configured bound reads 0xffff when unset.
const UNSET: i16 = -1;

/// A selection setting's value: 2 for 4, 1 for 2, else 0 (0xf73bc8).
fn selection_value(properties: &mut impl Properties, setting: Property) -> u16 {
    match properties.property(setting) as u8 {
        4 => 2,
        2 => 1,
        _ => 0,
    }
}

/// The profile's first selection in hundredths (0xf79140).
pub fn selection_percent(properties: &mut impl Properties, profile: AdjustmentProfile) -> i16 {
    let first = range_set(properties, profile).selections[0];
    (selection_value(properties, first) * 100) as i16
}

/// The capture profile of a range's option; an option value beyond the table reads 0 (0xf73bf4).
fn capture_profile(properties: &mut impl Properties, range: Range) -> u32 {
    stock::OPTION_CAPTURE_PROFILES.get(usize::from(properties.property(range.option) as u8)).copied().unwrap_or(0)
}

/// The near and far lens steps of a profile at a lens reference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensLimits {
    pub near: u32,
    pub far: u32,
}

fn step_limit(properties: &mut impl Properties, setting: Property) -> u16 {
    let limit = settings::word(properties, setting);
    if limit == u16::MAX { limit } else { limit.wrapping_mul(10) }
}

/// 0xf7809c: near 0xf77ef8, far 0xf77f84.
pub fn lens_limits(ports: &mut (impl Properties + LensSteps), profile: AdjustmentProfile, reference: u32) -> LensLimits {
    let table = if profile.standard_steps() { StepTable::Standard } else { StepTable::Lens };
    let near_limit = step_limit(ports, stock::NEAR_LIMIT[profile.fixed()]);
    let near = ports.near_step(reference, near_limit, table);
    let far_limit = step_limit(ports, stock::FAR_LIMIT[profile.fixed()]);
    let far = ports.far_step(reference, far_limit, table);
    LensLimits { near, far }
}

/// Where a target level falls in a profile's three selections.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Window {
    /// The values of the range set's first, second and third selection settings.
    pub selections: [u16; 3],
    /// The second and third selection above the first, in hundredths.
    pub spans: [u16; 2],
    /// 0 to 2.
    pub chosen: usize,
}

impl Window {
    pub const fn selection(&self) -> u16 { self.selections[self.chosen] }

    pub const fn offset(&self) -> u16 {
        match self.chosen {
            0 => 0,
            chosen => self.spans[chosen - 1],
        }
    }
}

/// The selection a target level reaches: past the upper neighbour's level the first, past the
/// lower's the second, else the third. Profiles with the secondary gate measure from the lens's
/// level; the others from the far and near limits, and only for a target (0xf789b4).
pub fn window(ports: &mut (impl Properties + Lens + LensSteps), profile: AdjustmentProfile, target: Option<i16>, lens: u32, reference: u32) -> Window {
    let set = range_set(ports, profile);
    let selections = set.selections.map(|setting| selection_value(ports, setting));
    let spans = [1, 2].map(|index| selections[index].wrapping_sub(selections[0]).wrapping_mul(100));
    let mut window = Window { selections, spans, chosen: 0 };
    let secondary = profile.record().secondary_gate;
    let levels = if secondary {
        let level = i32::from(ports.level(lens, reference));
        Some([level, level])
    } else if target.is_some() {
        let limits = lens_limits(ports, profile, reference);
        let far = ports.lens_step(limits.far);
        let far = i32::from(ports.level(far, reference));
        let near = ports.lens_step(limits.near);
        let near = i32::from(ports.level(near, reference));
        Some([far, near])
    } else {
        None
    };
    if let Some([upper_level, lower_level]) = levels {
        let target = target.map_or(-1, i32::from);
        let neighbours = set.neighbours;
        window.chosen = if target > upper_level + i32::from(configured_level(ports, neighbours.upper)) - i32::from(spans[0] as i16) {
            0
        } else if target > lower_level + i32::from(configured_level(ports, neighbours.lower)) - i32::from(spans[1] as i16) {
            1
        } else {
            2
        };
    }
    window
}

/// The bounds a lens step's neighbours give, each widened by the level difference to the
/// neighbouring step when flagged and the step lies inside `[lower, upper]` (0xf78188).
fn neighbour_bounds(ports: &mut (impl Properties + Lens), profile: AdjustmentProfile, step: u32, lower: u32, upper: u32, reference: u32) -> [i16; 2] {
    let neighbours = range_set(ports, profile).neighbours;
    let mut difference = |other: u32| {
        let here = i32::from(ports.level(step, reference));
        let there = i32::from(ports.level(other, reference));
        (here - there).unsigned_abs() as u16
    };
    let below = if neighbours.lower_flagged && step > lower { difference(step.wrapping_sub(1)) } else { 0 };
    let above = if neighbours.upper_flagged && step < upper { difference(step.wrapping_add(1)) } else { 0 };
    let minimum = configured_level(ports, neighbours.lower).wrapping_sub(below as i16);
    let maximum = configured_level(ports, neighbours.upper).wrapping_add(above as i16);
    [minimum, maximum]
}

/// The level, in tenths, of the last auxiliary range whose bound reaches `threshold`; forced, the
/// last selector's (0xf7838c).
fn auxiliary_level(properties: &mut impl Properties, profile: AdjustmentProfile, threshold: i32, forced: bool) -> i16 {
    let auxiliary = range_set(properties, profile).auxiliary;
    if forced {
        return settings::level(properties, auxiliary[usize::from(LAST_SELECTOR)][1]).wrapping_mul(10);
    }
    let mut level = 0;
    for [bound, setting] in auxiliary.into_iter().take(usize::from(SELECTORS)) {
        let bound = configured_level(properties, bound);
        if bound == UNSET {
            break;
        }
        if i32::from(bound) >= threshold {
            level = settings::level(properties, setting).wrapping_mul(10);
        }
    }
    level
}

/// A level split into the part the profile's auxiliary ranges hold and the rest up to the cap.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Split {
    pub held: i16,
    pub rest: i16,
}

/// Splits `total` at `level`: the auxiliary level held, the rest up to the profile's cap; both
/// at least 0 (0xf7882c).
pub fn split(properties: &mut impl Properties, profile: AdjustmentProfile, level: i16, total: i32) -> Split {
    let cap_setting = range_set(properties, profile).cap;
    let cap = settings::level(properties, cap_setting);
    let held = auxiliary_level(properties, profile, i32::from(level), false);
    let split = if i32::from(held) >= total {
        Split { held: total as i16, rest: 0 }
    } else {
        let room = cap.wrapping_mul(10).wrapping_sub(held).max(0);
        let rest = if i32::from(room) < total - i32::from(held) { room } else { (total as i16).wrapping_sub(held) };
        Split { held, rest }
    };
    Split { held: split.held.max(0), rest: split.rest.max(0) }
}

/// The percent of the auxiliary profile's last range whose bound lies above `threshold` at the
/// lens step `step` (0xf784f8).
pub fn choose_percent(ports: &mut (impl Properties + Lens + LensSteps + StepRatios), profile: AdjustmentProfile, threshold: i32, reference: u32, step: u32) -> u16 {
    let auxiliary = profile.auxiliary();
    let lens = ports.lens_step(step);
    let base = ports.level(lens, reference);
    let mut selected = 0;
    for selector in 0..SELECTORS {
        let lower = range(ports, auxiliary, selector).lower;
        let bound = configured_level(ports, lower);
        if bound == UNSET {
            break;
        }
        if i32::from(bound.wrapping_add(base)) > threshold {
            selected = selector;
        }
    }
    (ports.step_ratio(auxiliary.code(), selected) / 100) as u16
}

/// The selector a request settles on: the first whose ratio is within the request's limit, the
/// one before a ratio of 1, else 5. A forced request takes its limit from the configured ratio;
/// a request without a limit keeps `selector` (0xf785b8).
fn request_selector(ports: &mut (impl Properties + StepRatios), request: u32, profile: AdjustmentProfile, selector: u8, forced: bool) -> u8 {
    let limit = if forced {
        let divisor = u32::from(settings::word(ports, stock::RATIO_DIVISOR)).max(1);
        let ratio = u32::from(settings::word(ports, stock::RATIO_DIVIDEND)).wrapping_mul(1000) / divisor;
        let rounded = if ratio % 10 > 4 { ratio / 10 + 1 } else { ratio / 10 };
        u32::from(rounded as u16) / 100
    } else {
        let setting = stock::request_limit(request).map_or(0, |setting| ports.property(setting) as u8).min(5);
        if request == 0 || setting == 0 {
            return selector;
        }
        u32::from(setting) * 10
    };
    for candidate in 0..SELECTORS {
        let ratio = (ports.step_ratio(profile.code(), candidate) / 100) & 0xffff;
        if ratio == 1 {
            return candidate.saturating_sub(1);
        }
        if limit >= ratio {
            return candidate;
        }
    }
    SELECTORS
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Settled {
    level: i16,
    rest: i16,
    offset: u16,
}

/// The level at the window, capped, then raised to the upper bound of the last range whose lower
/// bound reaches it (0xf78bc8 unforced).
fn settle(ports: &mut (impl Properties + Lens + LensSteps), profile: AdjustmentProfile, level: i16, lens: u32, base: i16, reference: u32) -> Settled {
    let window = window(ports, profile, (level != UNSET).then_some(level), lens, reference);
    let offset = window.offset();
    let reached = (level as u16).wrapping_add(offset).wrapping_sub(base as u16) as i16;
    let capped = configured_level(ports, stock::LEVEL_CAP[profile.fixed()]).min(reached);
    let mut settled = Settled { level: capped, ..Settled::default() };
    let below = (base as u16).wrapping_sub((level as u16).wrapping_add(offset));
    for selector in 0..SELECTORS {
        let range = range(ports, profile, selector);
        let lower = configured_level(ports, range.lower);
        if lower == UNSET {
            break;
        }
        if lower >= capped {
            let upper = configured_level(ports, range.upper);
            settled = Settled { level: upper, rest: below.wrapping_add(upper as u16) as i16, offset };
        }
    }
    settled
}

/// The highest level the profile's ranges select at its near lens limit, less the third
/// selection's span and the cap; 100 more for a percent of 100 or more where the profile raises
/// with it (0xf78d24).
pub fn selected_level(ports: &mut (impl Properties + Lens + LensSteps), profile: AdjustmentProfile, percent: i32, reference: u32) -> i16 {
    let mut last = SELECTORS - 1;
    for selector in 0..SELECTORS {
        let upper = range(ports, profile, selector).upper;
        if configured_level(ports, upper) == UNSET {
            last = selector.saturating_sub(1);
            break;
        }
    }
    let top_setting = range(ports, profile, last).upper;
    let top = configured_level(ports, top_setting);
    let cap_setting = range_set(ports, profile).cap;
    let cap = settings::level(ports, cap_setting);
    let near = lens_limits(ports, profile, reference).near;
    let lens = ports.lens_step(near);
    let span = window(ports, profile, None, lens, reference).spans[1];
    let level = ports.level(lens, reference).wrapping_add(top).wrapping_sub(span as i16).wrapping_sub(cap.wrapping_mul(10));
    if profile.record().raises_with_percent && percent > 99 { level.wrapping_add(100) } else { level }
}

/// 0xf5f57c lowering: the offset is clamped to 200 either way.
fn lowered(value: i16, offset: i32) -> i16 { value.wrapping_sub(offset.clamp(-200, 200) as i16) }

/// The round's settlement of an exposure into a profile's ranges.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Settlement {
    pub level: i16,
    pub settled: i16,
    pub lens: u32,
    pub split: Split,
    pub offset: i16,
    /// The percent was 100 or more.
    pub raised: bool,
}

/// What a settlement starts from: `start + preset - deduction` plus the profile's selection
/// percent, lowered by `percent` (100 taken off a percent of 100 or more).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettlementInputs {
    pub percent: i32,
    pub reference: u32,
    pub start: i16,
    pub deduction: i16,
    pub preset: i16,
}

/// The level a settlement aims at, and 100 when the percent was 100 or more.
struct Start {
    target: i16,
    raised_by: i32,
}

fn start(inputs: &SettlementInputs, selection: i16) -> Start {
    let level = inputs.start.wrapping_add(inputs.preset).wrapping_sub(inputs.deduction).wrapping_add(selection);
    let (percent, raised_by) = if inputs.percent > 99 { (i32::from((inputs.percent - 100) as i16), 100) } else { (inputs.percent, 0) };
    Start { target: lowered(level, percent), raised_by }
}

/// Settles at the given lens step (0xf797f0).
pub fn settle_at_lens(ports: &mut (impl Properties + Lens + LensSteps), profile: AdjustmentProfile, inputs: &SettlementInputs, lens: u32) -> Settlement {
    let selection = selection_percent(ports, profile);
    let start = start(inputs, selection);
    let base = ports.level(lens, inputs.reference);
    let level = selected_level(ports, profile, start.raised_by, inputs.reference).max(start.target);
    let settled = settle(ports, profile, level, lens, base, inputs.reference);
    let split = split(ports, profile, settled.level, i32::from(settled.rest));
    Settlement { level, settled: settled.level, lens, split, offset: settled.offset as i16, raised: start.raised_by != 0 }
}

/// Settles at the lens step, walking down from the far limit while the target lies at or below the
/// midpoint of a step's and the next lower step's neighbour bounds (0xf79b94).
pub fn settle_by_steps(ports: &mut (impl Properties + Lens + LensSteps), profile: AdjustmentProfile, inputs: &SettlementInputs) -> Settlement {
    let reference = inputs.reference;
    let limits = lens_limits(ports, profile, reference);
    let selection = selection_percent(ports, profile);
    let start = start(inputs, selection);
    let span = window(ports, profile, None, limits.far, reference).spans[0];
    let mut step = limits.far;
    while step > limits.near {
        let lower = step - 1;
        let [minimum, _] = neighbour_bounds(ports, profile, step, limits.near, limits.far, reference);
        let here = (minimum as u16).wrapping_sub(span).wrapping_add(ports.level(step, reference) as u16) as i16;
        let [_, maximum] = neighbour_bounds(ports, profile, lower, limits.near, limits.far, reference);
        let there = (maximum as u16).wrapping_sub(span).wrapping_add(ports.level(lower, reference) as u16) as i16;
        let midpoint = ((i32::from(here) + i32::from(there)) / 2) as i16;
        if i32::from(start.target) > i32::from(midpoint) {
            break;
        }
        step = lower;
    }
    let level = selected_level(ports, profile, start.raised_by, reference).max(start.target);
    let base = ports.level(step, reference);
    let settled = settle(ports, profile, level, step, base, reference);
    let split = split(ports, profile, settled.level, i32::from(settled.rest));
    let level = settled.level.wrapping_sub(split.rest.wrapping_add(split.held)).wrapping_add(ports.level(step, reference));
    Settlement { level, settled: settled.level, lens: step, split, offset: selection.wrapping_add(settled.offset as i16), raised: start.raised_by != 0 }
}

/// What a resolution of a settled level reads beside the profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolutionInputs {
    pub lens: u32,
    pub reference: u32,
    pub adjustment: i16,
    pub mode: u32,
    pub forced: bool,
    pub primary: bool,
    pub alternate: bool,
    pub request: u32,
}

/// The capture profile a settled level resolves to, with the range's code and selector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Resolution {
    pub capture_profile: u32,
    pub code: u16,
    pub selector: u8,
}

/// Modes 0, 3, 4 and 5 take the forced request's range.
const fn forced_mode(mode: u32) -> bool { mode <= 5 && (0x39 >> mode) & 1 != 0 }

/// The capture profile of the last range whose lower bound lies at or above the settled level at
/// the window (the first, primary; the last selector's, alternate), then moved by the request. A
/// forced request in a forced mode raises `level` to its range's upper bound and splits the rest
/// again (0xf7a0cc).
pub fn resolve(
    ports: &mut (impl Properties + Lens + LensSteps + StepRatios + StockMemory),
    profile: AdjustmentProfile,
    inputs: &ResolutionInputs,
    level: &mut i16,
    split: &mut Split,
) -> Resolution {
    let span = (split.rest as u16).wrapping_add(split.held as u16);
    let base = ports.level(inputs.lens, inputs.reference) as u16;
    let target = base.wrapping_add((*level as u16).wrapping_sub(inputs.adjustment as u16).wrapping_sub(span)) as i16;
    let window = window(ports, profile, (target != UNSET).then_some(target), inputs.lens, inputs.reference);
    let percent = selection_percent(ports, profile);
    let code = profile.code();
    if inputs.primary {
        let first = range(ports, profile, 0);
        return Resolution { capture_profile: capture_profile(ports, first), code, selector: 0 };
    }
    let (mut selector, mut capture) = if inputs.alternate {
        let last = range(ports, profile, LAST_SELECTOR);
        (LAST_SELECTOR, capture_profile(ports, last))
    } else {
        let first = range(ports, profile, 0);
        let mut chosen = (0, capture_profile(ports, first));
        for candidate in 0..SELECTORS {
            let range = range(ports, profile, candidate);
            let lower = configured_level(ports, range.lower);
            if lower == UNSET {
                break;
            }
            let reach = base.wrapping_sub(window.offset()).wrapping_sub(percent as u16).wrapping_add(lower as u16) as i16;
            if target <= reach {
                chosen = (candidate, capture_profile(ports, range));
            }
        }
        chosen
    };
    if inputs.forced || configuration::shared::feature_enabled(ports) {
        if forced_mode(inputs.mode) && !profile.record().tertiary_gate {
            selector = request_selector(ports, inputs.request, profile, selector, true);
            let range = range(ports, profile, selector);
            capture = capture_profile(ports, range);
            let upper = configured_level(ports, range.upper);
            if upper > *level {
                let raise = (upper as u16).wrapping_sub(*level as u16);
                *level = upper;
                *split = self::split(ports, profile, upper, i32::from(span.wrapping_add(raise) as i16));
            }
        }
    } else if inputs.request != 0 {
        selector = request_selector(ports, inputs.request, profile, selector, false);
        let range = range(ports, profile, selector);
        capture = capture_profile(ports, range);
    }
    Resolution { capture_profile: capture, code, selector }
}

/// The byte offset, in a block of halfwords, of the entry 0xf7a9ec copies for a row, column and
/// index.
pub const fn halfword_offset(row: u32, column: u32, index: u32) -> u32 {
    match (row <= 5, column <= 2) {
        (true, true) => 2 * (9 * row + 3 * column + index) + 404,
        (true, false) => 2 * (3 * row + index) + 350,
        (false, true) => 2 * (3 * column + index) + 386,
        (false, false) => 2 * (index + 172),
    }
}
