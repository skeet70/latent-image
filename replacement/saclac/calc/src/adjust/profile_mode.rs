//! The lens step a record's profile mode takes (id5 0xf5271c), and the limit flags of a profile
//! selection (0xf5ea70) with the limit levels they weigh its positions against (0xf5e72c,
//! 0xf5e420). Levels are in tenths.

use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::record::{group_00, group_08};
use xm1_saclac_profile_records::stock::shared::group_41;
use xm1_saclac_profile_records::{MeasurementSelection, SharedHeaderByte};

use crate::adjust::configuration::MotionSource;
use crate::adjust::exposure_limits::{self, LimitPorts, Request, Selection, NO_INDEX};
use crate::adjust::limit_steps::{self, StepInputs};
use crate::adjust::scene_settings::{self, EnableInputs, SceneSelection};
use crate::exposure::levels;
use crate::exposure::ports::{Lens, Ports, Properties};
use crate::exposure::settings;
use crate::stock::adjust::motion_enable;
use crate::stock::calculation::Cell;
use crate::stock::capture::FIRST_ELEMENT;
use crate::stock::profile_mode::{self as stock, CellTable};
use crate::stock::property;

/// Image 4's readiness answer.
pub trait DeviceReadiness {
    /// 0x74081c.
    fn device_ready(&mut self) -> bool;
}

/// What the limit flags take for image 4's readiness.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Readiness {
    /// Ask `DeviceReadiness`.
    #[default]
    Ask,
    Ready,
    NotReady,
}

/// The limit flags (0xf5e420's result). The exposure limits read them as their bank.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LimitFlags(pub u8);

impl LimitFlags {
    /// The position is below the first level.
    pub const BELOW: u8 = 1;
    /// The position is at or above the first level.
    pub const AT_OR_ABOVE: u8 = 2;
    /// The limits read the bank's own lower level.
    pub const OWN_LOWER: u8 = 4;
    /// The spread exceeds its margin.
    pub const SPREAD: u8 = 8;
    /// Style 7.
    pub const STYLE_SEVEN: u8 = 0x40;
    /// Style 1, 3 or 5 beside another flag.
    pub const ODD_STYLE: u8 = 0x80;

    pub const fn contains(self, flag: u8) -> bool { self.0 & flag != 0 }
}

/// How far `to` lies above `from`, and which margin it is held to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Spread {
    /// Against `stock::SPREAD_MARGIN_ABOVE` when the second level lies above the position, else
    /// `stock::SPREAD_MARGIN`.
    Configured { from: i16, to: i16 },
    /// Against `stock::BOUNDED_MARGIN`; ends at the position instead of `to` while
    /// `stock::BOUNDED_TO_POSITION` is set.
    Bounded { from: i16, to: i16 },
}

impl Default for Spread {
    fn default() -> Self { Self::Configured { from: 0, to: 0 } }
}

/// The positions the flags weigh.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Positions {
    /// Compared with the first limit level.
    pub position: i16,
    pub spread: Spread,
}

/// What the limit flags read (0xf5ea70's block).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FlagInputs {
    pub measurement_selection: MeasurementSelection,
    /// The selection kind (0xf612dc).
    pub scene: u32,
    /// The selection flags (0xf61584).
    pub flags: u16,
    /// The selection auxiliary byte (0xf6171c): 2 turns the flags off below style 7, and any value
    /// leaves no second level.
    pub auxiliary: u8,
    /// The selection style (0xf615bc).
    pub style: u8,
    /// Record byte 0x32.
    pub class: u32,
    /// Record byte 0x4b.
    pub alternate: bool,
    /// Record byte 0x66.
    pub variant: bool,
    /// Shared header byte 0x63.
    pub armed: bool,
    /// Record byte 0x3e.
    pub choice: u32,
    /// Record byte 0x45.
    pub policy: u32,
    pub threshold: Option<i16>,
    /// The record's measured level is 0.
    pub unmeasured: bool,
    /// Taken off an unmeasured limit, which stays at least 0.
    pub limit_offset: i16,
    pub index: u32,
    pub level: i16,
    /// The record's lens step (record byte 0x44).
    pub profile: u32,
    pub capture_profile: u32,
    pub reference: u32,
    pub tracking: bool,
    pub minimum: i16,
    pub readiness: Readiness,
    pub positions: Positions,
}

impl FlagInputs {
    /// 0xf72c3c's request. 0xf5e72c leaves its auxiliary word (+0x48) unwritten.
    fn request(&self, index: u32) -> Request {
        Request {
            selection: Selection {
                scene: self.scene,
                flags: self.flags,
                alternate: self.alternate,
                variant: self.variant,
                unmeasured: self.unmeasured,
                threshold: self.threshold,
                auxiliary: self.auxiliary != 0,
            },
            class: self.class,
            reference: self.reference,
            policy: self.policy,
            armed: self.armed,
            choice: self.choice,
            tracking: self.tracking,
            bank: 0,
            cell: Cell::default(),
            index,
            level: self.level,
            profile: self.profile,
            fixed_upper: false,
            capture_profile: self.capture_profile,
            minimum: self.minimum,
        }
    }
}

/// The limit levels the flags weigh the position against (0xf5e72c's output).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LimitLevels {
    /// The limits' target less their limit, on a lens step's base level.
    pub first: i16,
    /// The same for the unindexed limits, raised by the scene's second rise. Only an unmeasured
    /// selection without an auxiliary byte has one.
    pub second: Option<i16>,
}

/// Whether the flags weigh the selection's scene, style and armed byte, or take scene 0, style 0
/// and no armed byte.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlagStyle {
    Selection,
    Plain,
}

/// One limit level of the selection's limits at `index`.
fn limit_level(ports: &mut (impl LimitPorts + Lens), inputs: &FlagInputs, index: u32) -> i16 {
    let limits = exposure_limits::limits(ports, &inputs.request(index));
    let limit = if inputs.unmeasured {
        if limits.limit > inputs.limit_offset { limits.limit.wrapping_sub(inputs.limit_offset) } else { 0 }
    } else {
        limits.limit
    };
    let step = if stock::on_record_step(inputs.scene) { inputs.profile } else { limits.near_step };
    limits.target.wrapping_sub(limit).wrapping_add(ports.base_level(step))
}

/// The selection's limit levels and the flags its positions take against them (0xf5e72c).
pub fn limit_levels(ports: &mut (impl LimitPorts + Lens), inputs: &FlagInputs, style: FlagStyle) -> (LimitLevels, LimitFlags) {
    let first = limit_level(ports, inputs, inputs.index);
    let second = (inputs.unmeasured && inputs.auxiliary == 0).then(|| {
        let rise = settings::level(ports, stock::second_rise(inputs.scene)).wrapping_mul(10);
        rise.wrapping_add(limit_level(ports, inputs, NO_INDEX))
    });
    let levels = LimitLevels { first, second };
    let (scene, selection_style, armed) = match style {
        FlagStyle::Selection => (inputs.scene, inputs.style, inputs.armed),
        FlagStyle::Plain => (0, 0, false),
    };
    (levels, classify(ports, scene, selection_style, armed, &inputs.positions, &levels))
}

/// Whether the spread exceeds its margin (0xf5e438..0xf5e4fc).
fn spread_exceeds(ports: &mut impl Properties, positions: &Positions, levels: &LimitLevels) -> bool {
    let (from, to, margin) = match positions.spread {
        Spread::Configured { from, to } => {
            let above = levels.second.unwrap_or(i16::MIN) > positions.position;
            let setting = if above { stock::SPREAD_MARGIN_ABOVE } else { stock::SPREAD_MARGIN };
            (from, to, settings::level(ports, setting).wrapping_mul(10))
        }
        Spread::Bounded { from, to } => {
            let margin = (ports.property(stock::BOUNDED_MARGIN) as i16).wrapping_mul(10);
            let to = if ports.property(stock::BOUNDED_TO_POSITION) != 0 { positions.position } else { to };
            (from, to, margin)
        }
    };
    from < to && i32::from(to) - i32::from(from) > i32::from(margin)
}

/// The flags `positions` take against `levels` (0xf5e420).
fn classify(ports: &mut impl Properties, scene: u32, style: u8, armed: bool, positions: &Positions, levels: &LimitLevels) -> LimitFlags {
    let below = positions.position < levels.first;
    let lowered = |flags| if armed && flags == LimitFlags::BELOW && stock::armed_own_lower(scene) { LimitFlags::OWN_LOWER } else { flags };
    let flags = match style {
        0 | 1 => {
            let flags = if below {
                LimitFlags::BELOW
            } else if spread_exceeds(ports, positions, levels) {
                LimitFlags::SPREAD
            } else {
                0
            };
            if armed && !below && stock::armed_at_or_above(scene) { LimitFlags::AT_OR_ABOVE } else { lowered(flags) }
        }
        2 | 3 | 7 => match (below, stock::below_own_lower(scene)) {
            (false, _) => LimitFlags::AT_OR_ABOVE,
            (true, true) => LimitFlags::OWN_LOWER,
            (true, false) => lowered(LimitFlags::BELOW),
        },
        4 | 5 => {
            if below {
                LimitFlags::OWN_LOWER
            } else {
                LimitFlags::AT_OR_ABOVE
            }
        }
        _ => 0,
    };
    let flags = if matches!(style, 1 | 3 | 5) && flags != 0 { flags | LimitFlags::ODD_STYLE } else { flags };
    LimitFlags(if style == 7 { flags | LimitFlags::STYLE_SEVEN } else { flags })
}

/// The selection's limit flags: none while `property::ADJUSTMENT_FLAGS` turns them off or the
/// selection is disabled, and none for a spread at limit index 2 (0xf5ea70).
pub fn limit_flags(ports: &mut (impl LimitPorts + Lens + DeviceReadiness), inputs: &FlagInputs) -> LimitFlags {
    let enabled = match (inputs.style, inputs.auxiliary) {
        (7, _) => true,
        (_, 2) => false,
        _ if inputs.measurement_selection == MeasurementSelection::Record => false,
        _ => match inputs.readiness {
            Readiness::Ask => ports.device_ready(),
            Readiness::Ready => true,
            Readiness::NotReady => false,
        },
    };
    if !enabled || ports.property(property::ADJUSTMENT_FLAGS) & stock::FLAGS_OFF != 0 {
        return LimitFlags::default();
    }
    let (_, flags) = limit_levels(ports, inputs, FlagStyle::Selection);
    if inputs.index == 2 && flags.contains(LimitFlags::SPREAD) { LimitFlags::default() } else { flags }
}

/// What a profile mode's step is chosen from beside the record (0xf5271c's arguments).
#[derive(Clone, Copy, Debug)]
pub struct ModeInputs<'a> {
    pub record: RecordHandle,
    /// The limit index, and the column of the mode's cell in `cells`.
    pub rate: u32,
    pub cells: &'a CellTable,
    /// The bound lens reference (0x76d9c8).
    pub lens_reference: u32,
}

/// The lens step the record's profile mode takes (0xf5271c).
pub fn mode_step(ports: &mut (impl Ports + DeviceReadiness), mode: &ModeInputs) -> u32 {
    let record = mode.record;
    let levels = levels::start(ports, record, true);
    let measured = levels::measured_level(ports, record);
    let tracking = ports.property(motion_enable(MotionSource::Live)) & 3 != 0;
    let capture_profile = ports.record_profile();
    let minimum = settings::level_step(ports, 3);
    let mut profile = ports.profile();
    let measurement_selection = profile.measurement_selection(record);
    let flags = FlagInputs {
        measurement_selection,
        scene: profile.selection_kind(record, false),
        flags: profile.selection_flags(record, false),
        auxiliary: profile.selection_auxiliary(record, false),
        style: profile.selection_style(record, false),
        class: u32::from(profile.read::<u8, _>(record, &group_00::AT_0032)),
        alternate: profile.read::<u8, _>(record, &group_00::AT_004B) != 0,
        variant: profile.read::<u8, _>(record, &group_00::AT_0066) != 0,
        armed: profile.header(record, SharedHeaderByte::At63) != 0,
        choice: u32::from(profile.read::<u8, _>(record, &group_00::AT_003E)),
        policy: u32::from(profile.read::<u8, _>(record, &group_00::AT_0045)),
        threshold: Selection::threshold_of(profile.read(record, &group_00::AT_0020)),
        unmeasured: measured == 0,
        limit_offset: 0,
        index: profile.read(record, &group_08::AT_01F4.at(FIRST_ELEMENT)),
        level: profile.read(record, &group_00::AT_001E),
        profile: u32::from(profile.read::<u8, _>(record, &group_00::AT_0044)),
        capture_profile,
        reference: mode.lens_reference,
        tracking: tracking && profile.read(record, &group_00::INTERVAL_TRACKING).0 != 0,
        minimum,
        readiness: Readiness::Ask,
        positions: Positions { position: profile.read_shared(record, &group_41::AT_096C), spread: Spread::default() },
    };
    let base = profile.read_shared(record, &group_41::AT_0972).wrapping_sub(profile.read(record, &group_00::AT_001C));
    let second = profile.read_shared(record, &group_41::AT_096E);
    let steps = profile.read::<u32, _>(record, &group_00::AT_0014);
    let enable = EnableInputs {
        selection: SceneSelection { measurement_selection, scene: flags.scene, flags: flags.flags },
        auxiliary: flags.auxiliary,
        hold: steps,
        selector: profile.read(record, &group_00::AT_0051),
    };
    let request_minimum = profile.read(record, &group_00::AT_0024);
    let bank = limit_flags(ports, &flags);
    let request = flags.request(mode.rate);
    let request = Request {
        selection: Selection { unmeasured: levels.unmeasured, ..request.selection },
        bank: bank.0,
        cell: mode.cells.mode_cell(mode.rate),
        minimum: request_minimum,
        ..request
    };
    let inputs = StepInputs {
        request,
        level: levels.start,
        placed_level: base.wrapping_add(flags.positions.position),
        mark: base.wrapping_add(second),
        steps: steps as i32,
        class_offset: 0,
        reference_level: measured,
        from_reference: scene_settings::adjustment_enabled(ports, enable),
    };
    limit_steps::choose(ports, &inputs).step
}
