use super::{Action, Candidate, Controller, Failure, Phase, Point, Position, Score};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction { Increasing, Decreasing }
impl Direction {
    fn reversed(self) -> Self {
        match self { Self::Increasing => Self::Decreasing, Self::Decreasing => Self::Increasing }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchStep(u16);
impl SearchStep {
    pub fn new(value: u16) -> Option<Self> { (value > 0 && value <= i16::MAX as u16).then_some(Self(value)) }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvidenceCount(u8);
impl EvidenceCount {
    pub fn new(value: u8) -> Option<Self> { (value > 0 && value < u8::MAX).then_some(Self(value)) }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Percent(u8);
impl Percent {
    pub fn new(value: u8) -> Option<Self> { (value <= 100).then_some(Self(value)) }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructuralFit { Parabolic, LinearSides }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StructuralParameters {
    direction_step: SearchStep,
    peak_step: SearchStep,
    direction_evidence: EvidenceCount,
    decline_evidence: EvidenceCount,
    relative_drop: Percent,
    absolute_drop: Score,
    initial_direction: Direction,
    fit: StructuralFit,
}
impl StructuralParameters {
    pub fn new(direction_step: SearchStep, peak_step: SearchStep, direction_evidence: EvidenceCount,
        decline_evidence: EvidenceCount, relative_drop: Percent, absolute_drop: Score,
        initial_direction: Direction, fit: StructuralFit) -> Self {
        Self { direction_step, peak_step, direction_evidence, decline_evidence,
            relative_drop, absolute_drop, initial_direction, fit }
    }
    // Camera values for these lens and configuration lookups have not been recovered.
    pub const fn synthetic() -> Self {
        Self {
            // 0x00f41c48: resolved 0x00faab3c step, with key 0x8778 scaling disabled.
            direction_step: SearchStep(50),
            // 0x00f3ec50: resolved lens-dependent peak movement step.
            peak_step: SearchStep(50),
            // 0x00f3c9f4: key 0x8811 threshold.
            direction_evidence: EvidenceCount(1),
            // 0x00fa936c: key 0x8831 threshold in the param_5 != 1 branch.
            decline_evidence: EvidenceCount(2),
            // 0x00f3de28: fixed key 0x8826 branch, excluding statistic-dependent blending.
            relative_drop: Percent(1),
            // 0x00fa936c: key 0x8823 times decoded literal 10.
            absolute_drop: Score(100),
            // 0x00f3c8d8: key 0x880e direction bit; initial direction remains an input.
            initial_direction: Direction::Increasing,
            // 0x00f8e9e4 key 0x927a == 0; 0x00f8deb0 curvature gate assumed open.
            fit: StructuralFit::Parabolic,
        }
    }
}

// 0x00f8deb0 and 0x00f8dd3c round a truncated tenths quotient away from zero above remainder 4.
fn rounded_ratio(numerator: i64, denominator: i64) -> i64 {
    let tenths = numerator * 10 / denominator;
    (tenths + if tenths.abs() % 10 > 4 { tenths.signum() * 10 } else { 0 }) / 10
}

fn fit_three(before: Point, center: Point, after: Point, fit: StructuralFit) -> Position {
    let (x, y, z) = (i64::from(before.position.0), i64::from(center.position.0), i64::from(after.position.0));
    let (a, b, c) = (i64::from(before.score.0), i64::from(center.score.0), i64::from(after.score.0));
    // Ordering checks: 0x00f8e9e4.
    if !((x < y && y < z) || (x > y && y > z)) || b < a || b < c { return center.position; }
    // 0x00f8deb0 uses twice the position-weighted statistic differences.
    let denominator = 2 * (x * (b - c) + y * (c - a) + z * (a - b));
    let target = if fit == StructuralFit::Parabolic && denominator != 0 {
        rounded_ratio(x * x * (b - c) + y * y * (c - a) + z * z * (a - b), denominator)
    } else {
        // 0x00f8dd3c uses the selected side's statistic difference for its fallback denominator.
        let rise = b - a;
        let fall = b - c;
        let numerator = (z - y).abs() * rise - (y - x).abs() * fall;
        let denominator = 2 * if numerator > 0 { rise } else { fall };
        let offset = if denominator == 0 { 0 } else { rounded_ratio(numerator, denominator) };
        y + if z < x { -offset } else { offset }
    };
    // Host position-domain guard; stock machine overflow and narrowing are not emulated.
    Position(target.clamp(x.min(z), x.max(z)) as u16)
}

impl Controller {
    fn structural_parameters(&self) -> StructuralParameters {
        let Candidate::StructuralReference(parameters) = self.candidate else { unreachable!() };
        parameters
    }
    fn structural_move(&mut self, position: Position, direction: Direction, step: SearchStep) -> Action {
        let target = match direction {
            Direction::Increasing => Position(position.0.saturating_add(step.0).min(self.config.high.0)),
            Direction::Decreasing => Position(position.0.saturating_sub(step.0).max(self.config.low.0)),
        };
        self.request(target)
    }
    pub(super) fn structural_start(&mut self, position: Position) -> Action {
        let direction = self.structural_parameters().initial_direction;
        self.phase = Phase::StructuralDirection { direction, evidence: 0 };
        self.request(position)
    }
    pub(super) fn structural_direction(&mut self, point: Point, mut direction: Direction, mut evidence: u8) -> Action {
        let parameters = self.structural_parameters();
        if self.count > 1 {
            let previous = self.points[usize::from(self.count - 2)];
            if point.score.0 > previous.score.0 { evidence = evidence.saturating_add(1); }
            if point.score.0 < previous.score.0 { direction = direction.reversed(); evidence = 0; }
        }
        // 0x00f3d554 forces inward movement at either configured limit.
        if point.position == self.config.low { direction = Direction::Increasing; }
        if point.position == self.config.high { direction = Direction::Decreasing; }
        if evidence >= parameters.direction_evidence.0 {
            self.phase = Phase::StructuralPeak { direction, start: self.count.saturating_sub(2) };
            return self.structural_move(point.position, direction, parameters.peak_step);
        }
        // Host flat-curve guard after both limits have been observed.
        if self.measured(self.config.low) && self.measured(self.config.high) && !self.contrast_span() {
            return self.terminal(Action::Fail(Failure::FlatCurve));
        }
        self.phase = Phase::StructuralDirection { direction, evidence };
        self.structural_move(point.position, direction, parameters.direction_step)
    }
    pub(super) fn structural_peak(&mut self, point: Point, direction: Direction, start: u16) -> Action {
        let parameters = self.structural_parameters();
        let points = &self.points[usize::from(start)..usize::from(self.count)];
        let mut declines = 0;
        for pair in points.windows(2).rev() {
            if pair[1].score.0 >= pair[0].score.0 { break; }
            declines += 1;
        }
        let drop_confirmed = if declines == 0 { false } else {
            let peak = points[points.len() - 1 - usize::from(declines)].score.0;
            // 0x00fa936c: consecutive declines OR peak-relative plus absolute drop.
            declines >= parameters.decline_evidence.0
                || peak - point.score.0 >= peak * u32::from(parameters.relative_drop.0) / 100 + parameters.absolute_drop.0
        };
        if drop_confirmed || point.position == self.config.low || point.position == self.config.high {
            if !self.contrast_span() { return self.terminal(Action::Fail(Failure::FlatCurve)); }
            let mut best = 0;
            for index in 1..points.len() {
                if points[index].score.0 > points[best].score.0 { best = index; }
            }
            // 0x00f3e468/0x00f3e63c bind the preceding, maximum and following observations.
            let target = if best > 0 && best + 1 < points.len() {
                fit_three(points[best - 1], points[best], points[best + 1], parameters.fit)
            } else { points[best].position };
            self.phase = Phase::StructuralTarget;
            return self.request(target);
        }
        self.structural_move(point.position, direction, parameters.peak_step)
    }
}
