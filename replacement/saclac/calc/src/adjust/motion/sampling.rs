//! Where a measurement drive takes its focus samples, and the drive replanned from the lens
//! positions a run recorded.

use crate::adjust::motion::drive::{quotient, DriveLimits};
use crate::adjust::motion::state::{AreaLayout, MotionState};
use crate::exposure::ports::{Properties, StockMemory};
use crate::stock::motion::{self as trace, MEASURED_RUN_FRAMES, TERMINATOR};

pub const PRIMARY_AREAS: usize = 7;
pub const SECONDARY_AREAS: usize = 10;

/// The lens target for each area around one sampled position.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SampleTargets {
    pub primary: [i16; PRIMARY_AREAS],
    pub secondary: [i16; SECONDARY_AREAS],
}

impl SampleTargets {
    /// The targets in the order the measurement request carries them.
    pub fn words(&self) -> [i16; PRIMARY_AREAS + SECONDARY_AREAS] {
        let mut words = [0; PRIMARY_AREAS + SECONDARY_AREAS];
        words[..PRIMARY_AREAS].copy_from_slice(&self.primary);
        words[PRIMARY_AREAS..].copy_from_slice(&self.secondary);
        words
    }
}

/// One step of the measurement's speed schedule: the frame it starts at and the drive speed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SpeedStep {
    pub frame: u8,
    pub speed: u32,
}

impl MotionState {
    /// The lens position `frame` frame periods into the run (0xfa30c0).
    pub fn position_at_frame(&self, frame: u32) -> i16 {
        let time = self.timing.frame_period.wrapping_mul(frame) as i32;
        self.drive.profile.position_at(time.wrapping_sub(self.drive.start_time))
    }

    /// How far a target sits from `reference` when its area is exposed `offset` after `time`;
    /// none while the drive stands still.
    fn correction(&self, properties: &mut impl Properties, reference: i16, time: i32, offset: i32) -> i16 {
        if !self.drive.moving {
            return 0;
        }
        self.drive.profile.midpoint_correction(properties, reference, time, offset, self.sampling.exposure)
    }

    fn target(&self, properties: &mut impl Properties, position: i16, time: i32, offset: i32) -> i16 {
        self.correction(properties, position, time, offset).wrapping_add(position)
    }

    /// The area targets around `position`, sampled as frame `frame` of the run, and their trace
    /// in the dump frame (0xfa2e58).
    pub fn targets(&self, ports: &mut (impl Properties + StockMemory), position: i16, frame: u32) -> SampleTargets {
        let time = if frame == 0 { self.drive.start_time.wrapping_neg() } else { self.drive.profile.time_to(position) as i32 };
        let mut targets = SampleTargets::default();
        for (slot, offset) in targets.primary.iter_mut().zip(self.sampling.primary()) {
            *slot = self.target(ports, position, time, *offset);
        }
        match self.drive.layout {
            AreaLayout::Listed => {
                let sampled = self.sampling.secondary();
                for (slot, offset) in targets.secondary.iter_mut().zip(sampled) {
                    *slot = self.target(ports, position, time, *offset);
                }
                let first = targets.secondary[0];
                let filled = usize::from(self.sampling.secondary_columns).min(SECONDARY_AREAS);
                for slot in targets.secondary.iter_mut().take(filled).skip(sampled.len()) {
                    *slot = first;
                }
            }
            AreaLayout::Grid | AreaLayout::GridAlternate => {
                let centre = targets.primary[usize::from(self.sampling.primary_count >> 1)];
                let areas = self.sampling.secondary_columns.wrapping_mul(self.sampling.secondary_rows);
                for slot in targets.secondary.iter_mut().take(usize::from(areas)) {
                    *slot = centre;
                }
            }
            AreaLayout::Single => {}
        }
        ports.write_u32(trace::sample_position(self.section, frame), i32::from(position) as u32);
        ports.write_u32(trace::sample_position(self.section, frame.wrapping_add(1)), TERMINATOR);
        ports.write_u32(trace::sample_time(self.section, frame), time as u32);
        ports.write_u32(trace::sample_time(self.section, frame.wrapping_add(1)), TERMINATOR);
        targets
    }

    /// Replans the drive at the speed the recorded `positions` show between the run's turning
    /// points, or at the step in force, re-anchors its start time to the first position that
    /// moved, and traces both with the schedule (0xfa30f0).
    pub fn replan_from_run(&mut self, ports: &mut (impl Properties + StockMemory + DriveLimits), earlier: &[SpeedStep], in_force: SpeedStep, positions: &[i16]) {
        let speed = match self.run_speed(ports, usize::from(in_force.frame) + 2, positions) {
            Some(measured) if measured != 0 => measured,
            _ => in_force.speed,
        };
        let planning = self.drive.profile.plan(speed, self.drive_kind, ports);
        planning.trace(self.drive.start_time, self.section, ports);
        if positions.len() > 2 {
            let last = positions.len() - 1;
            if let Some(moved) = (1..last).find(|&index| positions[index] != positions[0]) {
                let elapsed = self.drive.profile.time_to(positions[moved]);
                let frames = moved as u32 + u32::from(self.drive.first_frame);
                self.drive.start_time = self.timing.frame_period.wrapping_mul(frames).wrapping_sub(elapsed) as i32;
            }
        }
        ports.write_u32(trace::drive_planning(self.section, 0), self.drive.start_time as u32);
        let mut traced = 0;
        for step in earlier.iter().chain(core::iter::once(&in_force)) {
            ports.write_u32(trace::schedule_frame(self.section, traced), u32::from(step.frame));
            ports.write_u32(trace::schedule_speed(self.section, traced), step.speed);
            traced += 1;
        }
        ports.write_u32(trace::schedule_frame(self.section, traced), TERMINATOR);
        ports.write_u32(trace::schedule_speed(self.section, traced), TERMINATOR);
    }

    /// The speed between the frame the run starts moving after `from` and the frame it settles
    /// after its turning point, when that span is long enough.
    fn run_speed(&self, properties: &mut impl Properties, from: usize, positions: &[i16]) -> Option<u32> {
        let count = positions.len();
        let at = |index: usize| i32::from(positions[index]);
        let step = |index: usize| (at(index + 1) - at(index)).abs();
        let bend = |index: usize| (at(index + 1) - at(index)) + (at(index - 1) - at(index));
        let mut moving = from;
        if count <= moving + 1 {
            return None;
        }
        while step(moving) <= 1 {
            moving += 1;
            if moving + 1 >= count {
                return None;
            }
        }
        let mut turn = moving;
        loop {
            if turn + 2 >= count {
                return None;
            }
            if bend(turn).wrapping_mul(bend(turn + 1)) <= 0 {
                break;
            }
            turn += 1;
        }
        let mut settled = turn;
        let mut runs_out = false;
        loop {
            if settled + 1 >= count {
                settled = count - 2;
                runs_out = true;
                break;
            }
            if step(settled) <= 1 {
                break;
            }
            settled += 1;
        }
        let half = step(turn) / 2;
        let braking = (turn + 1..settled).rev().find(|&index| bend(index).wrapping_mul(bend(index + 1)) <= 0 && step(index) > half);
        let end = match braking {
            Some(index) => index + 1,
            None if runs_out => settled + 1,
            None => return None,
        };
        let span = (end - turn) as i32;
        if properties.property(MEASURED_RUN_FRAMES) as i32 > span + 1 {
            return None;
        }
        let distance = (at(end) - at(turn)).unsigned_abs();
        let duration = self.timing.frame_period.wrapping_mul(span as u32);
        Some(quotient(u64::from(distance) * 1_000_000, u64::from(duration)) as u32)
    }
}
