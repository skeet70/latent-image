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
pub enum StructuralFit { Parabolic, LinearSides, LogarithmicParabolic }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DeciScale(u8);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RangeDivisor(u8);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CurvatureLimit(u8);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DirectionTuning {
    thresholds: [Score; 4],
    scales: [DeciScale; 5],
    cap: RangeDivisor,
}
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
    direction_tuning: Option<DirectionTuning>,
    curvature_limit: Option<CurvatureLimit>,
}
impl StructuralParameters {
    pub fn new(direction_step: SearchStep, peak_step: SearchStep, direction_evidence: EvidenceCount,
        decline_evidence: EvidenceCount, relative_drop: Percent, absolute_drop: Score,
        initial_direction: Direction, fit: StructuralFit) -> Self {
        Self { direction_step, peak_step, direction_evidence, decline_evidence,
            relative_drop, absolute_drop, initial_direction, fit, direction_tuning: None, curvature_limit: None }
    }
    // Run-3 hardware-observed keys use physical base 0x01acd800, uncached alias 0x41acd800.
    pub const fn run3_viltrox() -> Self {
        Self {
            // Synthetic resolved step at 0x00faab3c; the captured active step is zero.
            direction_step: SearchStep(50),
            // Synthetic resolved step at 0x00f3ec50; the captured movement plan is zero.
            peak_step: SearchStep(50),
            // Hardware-observed key 0x8811 at 0x01ad6011; 0x00f3c9f4.
            direction_evidence: EvidenceCount(3),
            // Hardware-observed key 0x8831 at 0x01ad6031; 0x00fa936c.
            decline_evidence: EvidenceCount(8),
            // Hardware-observed key 0x8826 at 0x01ad6026; fixed branch of 0x00f3de28.
            relative_drop: Percent(20),
            // Hardware-observed key 0x8823 = 5 at 0x01ad6023; 0x00fa936c multiplies by 10.
            absolute_drop: Score(50),
            // Hardware-observed key 0x880e = 0 at 0x01ad600e; bit 1 in 0x00f3c8d8.
            initial_direction: Direction::Decreasing,
            // Hardware-observed key 0x927a = 1 at 0x01ad6a7a; mode-3 branch of 0x00f8e0c0.
            fit: StructuralFit::LogarithmicParabolic,
            // Hardware-observed key 0x8778 = 1 enables the 0x00f41c48 band policy.
            direction_tuning: Some(DirectionTuning {
                // Keys 0x877f..0x8782 = 15,8,10,10; decoded factors 1,10,100,1000.
                thresholds: [Score(15), Score(80), Score(1000), Score(10000)],
                // Keys 0x8750..0x8754 at 0x01ad5f50..0x01ad5f54; M/S context assumption.
                scales: [DeciScale(2), DeciScale(6), DeciScale(30), DeciScale(40), DeciScale(50)],
                // Hardware-observed key 0x8776 at 0x01ad5f76; 0x00f41c48 range cap.
                cap: RangeDivisor(30),
            }),
            // Hardware-observed key 0x927b at 0x01ad6a7b; 0x00f8deb0 fallback gate.
            curvature_limit: Some(CurvatureLimit(10)),
        }
    }
}

// 0x00f8deb0 and 0x00f8dd3c round a truncated tenths quotient away from zero above remainder 4.
fn rounded_tenths(tenths: i64) -> i64 {
    (tenths + if tenths.abs() % 10 > 4 { tenths.signum() * 10 } else { 0 }) / 10
}
fn rounded_ratio(numerator: i64, denominator: i64) -> i64 { rounded_tenths(numerator * 10 / denominator) }

// 0x00ee97f0 quantizes the mantissa to hundredths before its rounded base-2 logarithm.
fn logarithmic_score(score: Score) -> i64 {
    if score.0 == 0 { return 0; }
    let exponent = 31 - score.0.leading_zeros();
    let ratio = u64::from(score.0) * 100 / (1_u64 << exponent);
    // Host Q31 arithmetic with 24 fractional iterations reproduces the inspected table's 100 entries.
    let mut normalized = (ratio << 31) / 100;
    let mut fraction = 0_u32;
    for bit in (0..24).rev() {
        normalized = normalized * normalized >> 31;
        if normalized >= 2_u64 << 31 { normalized >>= 1; fraction |= 1 << bit; }
    }
    i64::from(exponent * 100 + ((fraction * 100 + (1 << 23)) >> 24))
}

fn parabolic_tenths(positions: &[i64; 3], scores: &[i64; 3]) -> Option<i64> {
    let [x, y, z] = *positions;
    let [a, b, c] = *scores;
    // 0x00f8deb0 and the mode-3 branch of 0x00f8e0c0.
    let denominator = 2 * (x * (b - c) + y * (c - a) + z * (a - b));
    (denominator != 0).then(|| (x * x * (b - c) + y * y * (c - a) + z * z * (a - b)) * 10 / denominator)
}

fn fit_three(before: &Point, center: &Point, after: &Point, fit: StructuralFit,
    curvature_limit: Option<CurvatureLimit>) -> Position {
    let positions = [i64::from(before.position.0), i64::from(center.position.0), i64::from(after.position.0)];
    let scores = [i64::from(before.score.0), i64::from(center.score.0), i64::from(after.score.0)];
    let [x, y, z] = positions;
    let [a, b, c] = scores;
    // Ordering checks: 0x00f8e9e4.
    if !((x < y && y < z) || (x > y && y > z)) || b < a || b < c { return center.position; }
    let logarithmic_target = if fit == StructuralFit::LogarithmicParabolic {
        // 0x00f8a390: normalize position gaps above 500 with integer scale 50000 / max_gap.
        let gap = (y - x).abs().max((z - y).abs());
        let scale = if gap < 501 { 100 } else { 50000 / gap };
        let transformed = [logarithmic_score(before.score), logarithmic_score(center.score), logarithmic_score(after.score)];
        if scale == 0 { None } else {
            let normalized = positions.map(|position| position * scale / 100);
            parabolic_tenths(&normalized, &transformed).map(|tenths| rounded_tenths(tenths * 100 / scale))
        }
    } else { None };
    let estimate = logarithmic_target.or_else(|| {
        if fit == StructuralFit::LinearSides { return None; }
        // 0x00f8cae8 and 0x00f8deb0 gate the untransformed fallback with key 0x927b.
        let allowed = curvature_limit.is_none_or(|limit| {
            let curvature = if b == 0 { 0 } else {
                ((((b - a) / (y - x) - (c - b) / (z - y)) * 10 / (z - x))
                    * 100 * (y - x).abs() * (z - y).abs() / b).abs()
            };
            curvature < i64::from(limit.0)
        });
        if allowed { parabolic_tenths(&positions, &scores).map(rounded_tenths) } else { None }
    });
    let target = if let Some(target) = estimate { target } else {
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
    fn structural_parameters(&self) -> &StructuralParameters {
        let Candidate::StructuralReference(parameters) = &self.candidate else { unreachable!() };
        parameters
    }
    fn structural_direction_step(&self, score: Score) -> Option<SearchStep> {
        let parameters = self.structural_parameters();
        let mut step = u32::from(parameters.direction_step.0);
        if let Some(tuning) = &parameters.direction_tuning {
            let band = tuning.thresholds.iter().filter(|threshold| score.0 >= threshold.0).count();
            // 0x00f41c48 divides the band scale by 10 before applying key 0x8776's range cap.
            step = (step * u32::from(tuning.scales[band].0) / 10)
                .min(u32::from(self.config.high.0 - self.config.low.0) / u32::from(tuning.cap.0));
        }
        u16::try_from(step).ok().and_then(SearchStep::new)
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
            let peak_step = parameters.peak_step;
            self.phase = Phase::StructuralPeak { direction, start: self.count.saturating_sub(2) };
            return self.structural_move(point.position, direction, peak_step);
        }
        // Host flat-curve guard after both limits have been observed.
        if self.measured(self.config.low) && self.measured(self.config.high) && !self.contrast_span() {
            return self.terminal(Action::Fail(Failure::FlatCurve));
        }
        let Some(step) = self.structural_direction_step(point.score) else { return self.terminal(Action::Fail(Failure::Budget)); };
        self.phase = Phase::StructuralDirection { direction, evidence };
        self.structural_move(point.position, direction, step)
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
                fit_three(&points[best - 1], &points[best], &points[best + 1], parameters.fit, parameters.curvature_limit)
            } else { points[best].position };
            self.phase = Phase::StructuralTarget;
            return self.request(target);
        }
        self.structural_move(point.position, direction, parameters.peak_step)
    }
}
