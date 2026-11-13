//! The lens step and level a measurement takes within its exposure limits (id5 0xf72ea4 with
//! 0xf718c4..0xf71e70 and 0xf72528..0xf7283c). Levels are in tenths.

use crate::adjust::exposure_limits::{self, capped, level_pinned, Band, CapRule, LimitPorts, Limits, Request};
use crate::exposure::ports::{CaptureProfiles, Lens};
use crate::exposure::settings;

/// Everything choosing a step reads.
pub trait StepPorts: LimitPorts + Lens + CaptureProfiles {}

impl<T: LimitPorts + Lens + CaptureProfiles> StepPorts for T {}

/// The request level that names none.
pub const NO_REQUEST_LEVEL: i16 = i16::MIN;
/// The level the level-pinned scenes take without a request level (0xf71e54).
pub const UNREQUESTED_LEVEL: i16 = -1182;

/// What a step is chosen for (0xf72ea4's block). `request.level` is the level the pinned scenes
/// hold to, `NO_REQUEST_LEVEL` for none; `request.profile` is the current step.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StepInputs {
    pub request: Request,
    /// The level the opening and the pinned scenes start from.
    pub level: i16,
    /// The level the other scenes place.
    pub placed_level: i16,
    /// The level the excess of scenes 47 and 56 is measured from.
    pub mark: i16,
    /// How many steps scenes 44 and 57 move: positive toward the near step.
    pub steps: i32,
    /// Added to the level of a class-2 pinned scene.
    pub class_offset: i16,
    /// The level the reference step is found from; 0 takes the start.
    pub reference_level: i16,
    /// The other scenes choose from the reference step rather than the threshold.
    pub from_reference: bool,
}

/// What 0xf72ea4 returns: the limits the level falls outside, and whether a walk fell short.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StepFlags(pub u8);

impl StepFlags {
    /// Below the near step's reach.
    pub const BELOW: u8 = 1;
    /// Above the far step's reach.
    pub const ABOVE: u8 = 2;
    /// The chosen level is below the target.
    pub const UNDER_TARGET: u8 = 4;
    /// Scenes 44 and 57 moved fewer steps than asked.
    pub const SHORT: u8 = 32;
}

/// The chosen step (0xf72ea4's stores into its block).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChosenStep {
    /// The current step, made the far step for scene 24.
    pub profile: u32,
    pub step: u32,
    pub level: i16,
    /// The cap of the chosen step's level.
    pub cap: i16,
    pub target: i16,
    pub correction: i16,
    pub floor: i16,
    pub start: i16,
    /// The chosen step's compensated level.
    pub compensated: i16,
    pub above: bool,
    pub flags: StepFlags,
}

#[derive(Clone, Copy)]
struct Placement {
    step: u32,
    level: i16,
    cap: i16,
    correction: i16,
}

struct Opening {
    start: i16,
    rise: i16,
    drop: i16,
}

/// Where an unmeasured exposure opens: at least the larger of the minimum and step 3, at most
/// `reach`, and never below the level plus its cell row's level (0xf718c4).
fn opening(ports: &mut impl StepPorts, inputs: &StepInputs, reach: i16) -> Opening {
    let request = &inputs.request;
    let step = settings::level_step(ports, 3);
    let row = ports.row_level(request.cell);
    let mut least = request.minimum.max(0).max(step).min(reach);
    if request.bank != 0 && !level_pinned(request.selection.scene) {
        least = least.max(step);
    }
    let base = inputs.level.wrapping_add(row);
    let start = base.max(least);
    Opening { start, rise: start.wrapping_sub(base), drop: start.wrapping_sub(step) }
}

impl Limits {
    fn base_level(ports: &mut impl Lens, step: u32) -> i32 { i32::from(ports.base_level(step)) }

    /// The level is below the band at the near step (0xf71af4).
    fn short_of_band(&self, ports: &mut impl Lens, unmeasured: bool, level: i16) -> bool {
        unmeasured && i32::from(level) < Self::base_level(ports, self.near_step) + i32::from(self.band.high)
    }

    /// The correction that lifts `level` toward the band at the near step (0xf719e8).
    fn correction(&self, ports: &mut impl Lens, level: i16) -> i16 {
        if self.limit <= 0 {
            return 0;
        }
        let base = ports.base_level(self.near_step);
        let first = base.wrapping_add(self.band.high);
        if first <= level {
            return 0;
        }
        if self.band.high.wrapping_sub(self.margin).wrapping_add(base) < level {
            return first.wrapping_sub(level);
        }
        let second = self.band.low.wrapping_sub(self.margin).wrapping_add(base);
        if second <= level {
            self.margin
        } else if self.band.low.wrapping_sub(self.limit).wrapping_add(base) >= level {
            self.limit
        } else {
            second.wrapping_add(self.margin).wrapping_sub(level)
        }
    }

    /// The threshold level of a step (0xf71c80).
    const fn threshold_level(&self, step: u32) -> i16 { if self.threshold.step >= step { self.threshold.at_or_below } else { self.threshold.above } }

    /// The farthest step whose base level plus threshold level is at most `level`, else the near
    /// step (0xf71ca8).
    fn threshold_step(&self, ports: &mut impl Lens, level: i16) -> u32 {
        let mut step = self.far_step;
        while step > self.near_step {
            if i32::from(level) >= Self::base_level(ports, step) + i32::from(self.threshold_level(step)) {
                return step;
            }
            step -= 1;
        }
        step
    }

    /// The farthest step whose next nearer step's base level is below the reference level, from
    /// the near step and less step 3 (0xf71d04).
    fn reference_step(&self, ports: &mut impl StepPorts, reference_level: i16, start: i16) -> u32 {
        let level = if reference_level != 0 { reference_level } else { start };
        let near_base = ports.base_level(self.near_step);
        let boundary = level.wrapping_add(near_base).wrapping_sub(settings::level_step(ports, 3));
        let mut step = self.far_step;
        while step > self.near_step {
            if boundary > ports.base_level(step - 1) {
                return step;
            }
            step -= 1;
        }
        step
    }

    /// The farthest step whose base level, averaged with the next nearer one's, is below `level`
    /// (0xf71810).
    fn midpoint_step(&self, ports: &mut impl Lens, level: i16) -> u32 {
        let mut step = self.far_step;
        while step > self.near_step {
            let middle = (Self::base_level(ports, step) + Self::base_level(ports, step - 1)) / 2;
            if i32::from(level) > middle {
                return step;
            }
            step -= 1;
        }
        step
    }

    /// The first step from `from` toward the far step whose capped level reaches `level`, else
    /// the far step (0xf726e8).
    fn reaching_step(&self, ports: &mut impl StepPorts, rule: CapRule, from: u32, level: i16) -> u32 {
        let mut step = from;
        while step < self.far_step {
            let cap = capped(ports, rule, step, self.upper);
            if i32::from(level) <= Self::base_level(ports, step) + i32::from(cap) {
                return step;
            }
            step += 1;
        }
        step
    }

    /// The level at `step`, within its cap and the lower level.
    fn placed(&self, ports: &mut impl StepPorts, rule: CapRule, step: u32, level: i16, correction: i16) -> Placement {
        let cap = capped(ports, rule, step, self.upper);
        let level = correction.wrapping_add(level).wrapping_sub(ports.base_level(step));
        Placement { step, level: level.min(cap).max(self.lower), cap, correction }
    }

    /// The limits with the band held at `level` and the margin at the limit.
    const fn held_at(&self, level: i16) -> Self { Self { band: Band { high: level, low: level }, margin: self.limit, ..*self } }
}

/// The step of a level-pinned scene: the current step, at the request level within the limits
/// (0xf71e70).
fn pinned_step(ports: &mut impl StepPorts, inputs: &StepInputs, limits: &Limits, profile: u32, level: i16) -> Placement {
    let request = &inputs.request;
    if request.level == NO_REQUEST_LEVEL {
        return Placement { step: profile, level: UNREQUESTED_LEVEL, cap: limits.upper, correction: 0 };
    }
    let held = request.level.min(limits.upper).max(limits.lower);
    let view = Limits { near_step: profile, ..limits.held_at(held) };
    let correction = if view.short_of_band(ports, request.selection.unmeasured, level) { view.correction(ports, level) } else { 0 };
    let held = if request.class == 2 { held.wrapping_add(inputs.class_offset).min(limits.upper).max(limits.lower) } else { held };
    Placement { step: profile, level: held, cap: limits.upper, correction }
}

/// The step of scenes 45 and 55: the near step while the level is short of the band, else the
/// step at the level's midpoint (0xf71b34).
fn midpoint_step(ports: &mut impl StepPorts, inputs: &StepInputs, limits: &Limits, level: i16) -> Placement {
    let request = &inputs.request;
    let held = request.level.min(limits.upper).max(limits.lower);
    let view = limits.held_at(held);
    let (step, correction) = if view.short_of_band(ports, request.selection.unmeasured, level) {
        (limits.near_step, view.correction(ports, level))
    } else {
        (limits.midpoint_step(ports, level.wrapping_sub(held)), 0)
    };
    Placement { step, level: held, cap: limits.upper, correction }
}

/// The step of scenes 46 and 54: the current step (0xf72760).
fn current_step(ports: &mut impl StepPorts, inputs: &StepInputs, limits: &Limits, profile: u32, level: i16) -> Placement {
    let request = &inputs.request;
    let view = Limits { near_step: profile, ..*limits };
    let correction = if view.short_of_band(ports, request.selection.unmeasured, level) { view.correction(ports, level) } else { 0 };
    let rule = CapRule { scene: request.selection.scene, class: 0, first: profile };
    limits.placed(ports, rule, profile, level, correction)
}

/// The step of the other scenes: the near step while the level is short of the band, else the
/// step its threshold or the reference step gives (0xf7283c).
fn threshold_step(ports: &mut impl StepPorts, inputs: &StepInputs, limits: &Limits, level: i16, start: i16) -> Placement {
    let request = &inputs.request;
    let rule = CapRule { scene: request.selection.scene, class: request.class, first: limits.far_step };
    if limits.short_of_band(ports, request.selection.unmeasured, level) {
        let correction = limits.correction(ports, level);
        return limits.placed(ports, rule, limits.near_step, level, correction);
    }
    let step = if inputs.from_reference {
        let reference = limits.reference_step(ports, inputs.reference_level, start);
        let cap = capped(ports, rule, reference, limits.upper);
        let base = ports.base_level(reference);
        if level > cap.wrapping_add(base) {
            limits.reaching_step(ports, rule, reference, level)
        } else if level >= limits.threshold_level(reference).wrapping_add(base) {
            reference
        } else {
            limits.threshold_step(ports, level)
        }
    } else {
        limits.threshold_step(ports, level)
    };
    limits.placed(ports, rule, step, level, 0)
}

/// Moves scenes 44 and 57 up to `steps` steps, stopping before a step past the limits' steps or
/// one whose level falls outside its cap or below the lower level (0xf72528).
fn walk(ports: &mut impl StepPorts, inputs: &StepInputs, limits: &Limits, placement: &mut Placement, target: i16) -> u8 {
    let steps = inputs.steps;
    if steps == 0 {
        return 0;
    }
    let rule = CapRule { scene: inputs.request.selection.scene, class: 0, first: limits.far_step };
    let start = placement.step as i32;
    let mut moved: i32 = 0;
    for count in 1..=steps.unsigned_abs() {
        let next = if steps > 0 { start.wrapping_sub(count as i32) } else { start.wrapping_add(count as i32) };
        let open = if steps > 0 { next >= limits.near_step as i32 } else { next <= limits.far_step as i32 };
        if !open {
            break;
        }
        let level = target.wrapping_sub(ports.base_level(next as u32));
        let fits = if steps > 0 { level <= capped(ports, rule, next as u32, limits.upper) } else { level >= limits.lower };
        if !fits {
            break;
        }
        moved = next.wrapping_sub(start);
    }
    if moved == 0 {
        return StepFlags::SHORT;
    }
    let step = start.wrapping_add(moved) as u32;
    let cap = capped(ports, rule, step, limits.upper);
    let level = target.wrapping_sub(ports.base_level(step));
    *placement = Placement { step, level: cap.min(level).max(limits.lower), cap, correction: placement.correction };
    if moved.unsigned_abs() == steps.unsigned_abs() {
        0
    } else {
        StepFlags::SHORT
    }
}

/// 0xf71d78.
fn range_flags(ports: &mut impl StepPorts, inputs: &StepInputs, limits: &Limits, placement: &Placement, level: i16) -> u8 {
    let request = &inputs.request;
    let scene = request.selection.scene;
    let mut flags = 0;
    if !matches!(scene, 47 | 56) {
        let (far, near) = if matches!(scene, 46 | 54) { (placement.step, placement.step) } else { (limits.far_step, limits.near_step) };
        let (upper, lower) = if level_pinned(scene) { (placement.level, placement.level) } else { (limits.upper, limits.lower) };
        let top = upper.wrapping_add(ports.base_level(far));
        let bottom = ports.base_level(near).wrapping_add(lower).wrapping_sub(limits.limit);
        if top < level && scene != 24 {
            flags |= StepFlags::ABOVE;
        }
        if bottom > level && request.bank == 0 && scene != 24 {
            flags |= StepFlags::BELOW;
        }
    }
    if placement.level < limits.target {
        flags |= StepFlags::UNDER_TARGET;
    }
    flags
}

/// The step and level a measurement takes within its limits (0xf72ea4).
pub fn choose(ports: &mut impl StepPorts, inputs: &StepInputs) -> ChosenStep {
    let request = &inputs.request;
    let scene = request.selection.scene;
    let mut limits = exposure_limits::limits(ports, request);
    let profile = if scene == 24 { limits.far_step.max(limits.near_step) } else { request.profile };
    let (start, level, rise) = if request.selection.unmeasured {
        let reach = limits.limit.wrapping_add(settings::level_step(ports, 3));
        let opening = opening(ports, inputs, reach);
        limits.margin = limits.margin.wrapping_sub(opening.drop).max(0);
        limits.limit = limits.limit.wrapping_sub(opening.drop).max(0);
        (opening.start, inputs.placed_level.wrapping_add(opening.rise), opening.rise)
    } else {
        (inputs.level, inputs.placed_level, 0)
    };
    let mut placement = match scene {
        24 | 47 | 56 => pinned_step(ports, inputs, &limits, profile, inputs.mark.wrapping_sub(inputs.level).wrapping_add(start)),
        45 | 55 => midpoint_step(ports, inputs, &limits, level),
        46 | 54 => current_step(ports, inputs, &limits, profile, level),
        _ => threshold_step(ports, inputs, &limits, level, start),
    };
    let walked = if matches!(scene, 44 | 57) {
        let target = level.wrapping_add(placement.correction);
        walk(ports, inputs, &limits, &mut placement, target)
    } else {
        0
    };
    let flags = range_flags(ports, inputs, &limits, &placement, level) | walked;
    let correction = placement.correction.wrapping_add(rise);
    let mut above = flags & StepFlags::ABOVE != 0;
    if matches!(scene, 47 | 56) {
        let offset = if limits.floor == 0 {
            0
        } else {
            correction.wrapping_add(ports.level_offset(request.capture_profile)).wrapping_add(ports.row_level(request.cell))
        };
        above |= ports.excess(inputs.mark, placement.level, placement.step, offset) > 0;
    }
    ChosenStep {
        profile,
        step: placement.step,
        level: placement.level,
        cap: placement.cap,
        target: limits.target,
        correction,
        floor: limits.floor,
        start,
        compensated: ports.compensated_level(placement.step, request.reference),
        above,
        flags: StepFlags(flags),
    }
}
