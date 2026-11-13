#![forbid(unsafe_code)]

use crate::scheduling::{Lane, Lanes};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Work { PrimaryAcquisition, SecondaryAcquisition, Search, Auxiliary, Image, Window, Source }
impl Work {
    pub const fn lane(self) -> Lane {
        match self {
            Self::PrimaryAcquisition | Self::SecondaryAcquisition => Lane::Acquisition,
            Self::Search => Lane::Search, Self::Auxiliary => Lane::Auxiliary,
            Self::Image => Lane::Image, Self::Window => Lane::Window, Self::Source => Lane::Source,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SamplingBand { AboveThreshold, BelowThreshold }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModeInputs {
    pub record_selected: bool,
    pub resource_selected: bool,
    pub profile_selected: bool,
    pub automatic_sampling: Option<SamplingBand>,
}

pub const fn classify_mode(inputs: ModeInputs) -> Mode {
    if inputs.record_selected { Mode::RecordSelected }
    else if inputs.resource_selected { Mode::ResourceSelected }
    else if inputs.profile_selected { Mode::ProfileSelected }
    else { match inputs.automatic_sampling {
        Some(SamplingBand::AboveThreshold) => Mode::AboveThreshold,
        Some(SamplingBand::BelowThreshold) => Mode::BelowThreshold,
        None => Mode::Default,
    } }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode { Default, RecordSelected, ResourceSelected, ProfileSelected, AboveThreshold, BelowThreshold, Paused }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase { First, Second, Third, Fourth }
impl Phase {
    fn next(self) -> Self {
        match self { Self::First => Self::Second, Self::Second => Self::Third, Self::Third => Self::Fourth, Self::Fourth => Self::First }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cadence { pub first: bool, pub second: bool, pub third: bool, pub fourth: bool }
impl Cadence {
    fn admits(self, phase: Phase) -> bool {
        match phase { Phase::First => self.first, Phase::Second => self.second, Phase::Third => self.third, Phase::Fourth => self.fourth }
    }
}

pub trait Policy { fn cadence(&mut self, work: Work, mode: Mode) -> Cadence; }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Slot { First, Second }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Plan {
    pub first: Option<Work>,
    pub second: Option<Work>,
    pub primary_considered: bool,
    pub cycle_complete: bool,
}

pub struct Planner {
    first_cursor: usize,
    second_cursor: usize,
    phase: Phase,
    mode: Mode,
    first_running: Option<Work>,
    second_running: Option<Work>,
}

impl Default for Planner { fn default() -> Self { Self::new() } }

impl Planner {
    pub const fn new() -> Self {
        Self { first_cursor: 0, second_cursor: 0, phase: Phase::First, mode: Mode::Default, first_running: None, second_running: None }
    }

    pub fn next(&mut self, mode: Mode, enabled: Lanes, policy: &mut impl Policy) -> Plan {
        if self.first_cursor == 0 && self.second_cursor == 0 { self.mode = mode; }
        let mut plan = Plan { first: None, second: None, primary_considered: false, cycle_complete: false };
        if self.mode == Mode::Paused { return plan; }
        if self.first_running.is_none() {
            plan.first = Self::select(&[Work::Window, Work::Search, Work::Image, Work::Source], &mut self.first_cursor, self.phase, self.mode, enabled, policy, &mut plan.primary_considered);
        }
        if self.second_running.is_none() {
            plan.second = Self::select(&[Work::PrimaryAcquisition, Work::SecondaryAcquisition, Work::Auxiliary], &mut self.second_cursor, self.phase, self.mode, enabled, policy, &mut plan.primary_considered);
        }
        if plan.first.is_none() && plan.second.is_none() && self.first_running.is_none() && self.second_running.is_none() {
            self.first_cursor = 0;
            self.second_cursor = 0;
            self.phase = self.phase.next();
            plan.cycle_complete = true;
        }
        plan
    }

    pub fn submitted(&mut self, slot: Slot, work: Work) {
        match slot { Slot::First => self.first_running = Some(work), Slot::Second => self.second_running = Some(work) }
    }

    pub fn completed(&mut self, work: Work) {
        if self.first_running == Some(work) { self.first_running = None; }
        if self.second_running == Some(work) { self.second_running = None; }
    }

    pub fn reset_cycle(&mut self) { self.first_cursor = 0; self.second_cursor = 0; self.phase = Phase::First; }

    fn select(order: &[Work], cursor: &mut usize, phase: Phase, mode: Mode, enabled: Lanes, policy: &mut impl Policy, primary_considered: &mut bool) -> Option<Work> {
        while let Some(&work) = order.get(*cursor) {
            *cursor += 1;
            let acquisition = enabled.contains(Lane::Acquisition) || enabled.contains(Lane::Window);
            let admitted = match work {
                Work::Window => acquisition,
                Work::PrimaryAcquisition => { *primary_considered = true; acquisition && policy.cadence(work, mode).admits(phase) }
                Work::SecondaryAcquisition => acquisition && phase == Phase::Fourth,
                _ => enabled.contains(work.lane()) && policy.cadence(work, mode).admits(phase),
            };
            if admitted { return Some(work); }
        }
        None
    }
}
