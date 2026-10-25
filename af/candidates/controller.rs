#![no_std]

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position(u16);
impl Position {
    pub const fn new(value: u16) -> Self { Self(value) }
    pub const fn units(self) -> u16 { self.0 }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Score(u32);
impl Score {
    pub fn new(value: u32) -> Option<Self> { (value <= 1_000_000).then_some(Self(value)) }
    pub const fn units(self) -> u32 { self.0 }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestId(u16);
impl RequestId { pub const fn units(self) -> u16 { self.0 } }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Candidate { CoarseFine, AdaptivePeak }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure { Budget, FlatCurve, InvalidObservation, Cancelled }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Observe { request: RequestId, position: Position },
    Wait,
    Finish(Position),
    Fail(Failure),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Start,
    Missing,
    Cancel,
    Sample { request: RequestId, position: Position, score: Score, fresh: bool, settled: bool },
}
#[derive(Clone, Copy, Debug)]
pub struct Config {
    low: Position,
    high: Position,
    coarse: u16,
    fine: u16,
    budget: u16,
    min_span: Score,
}
impl Config {
    pub fn new(low: Position, high: Position, coarse: u16, fine: u16, budget: u16, min_span: Score) -> Option<Self> {
        (low < high && fine > 0 && fine <= coarse && coarse <= high.0 - low.0
            && (3..=64).contains(&budget) && min_span.0 > 0)
            .then_some(Self { low, high, coarse, fine, budget, min_span })
    }
}
#[derive(Clone, Copy, Debug)]
struct Point { position: Position, score: Score }
const EMPTY: Point = Point { position: Position(0), score: Score(0) };
#[derive(Clone, Copy, Debug)]
enum Phase { Idle, Coarse, Refine { low: Position, high: Position }, Verify, Terminal(Action) }

#[derive(Debug)]
pub struct Controller {
    config: Config,
    candidate: Candidate,
    phase: Phase,
    points: [Point; 64],
    count: u16,
    pending: Option<(RequestId, Position)>,
    calls: u16,
}
impl Controller {
    pub const fn new(config: Config, candidate: Candidate) -> Self {
        Self { config, candidate, phase: Phase::Idle, points: [EMPTY; 64], count: 0, pending: None, calls: 0 }
    }
    pub fn samples(&self) -> u16 { self.count }
    fn terminal(&mut self, action: Action) -> Action {
        self.phase = Phase::Terminal(action);
        self.pending = None;
        action
    }
    fn request(&mut self, position: Position) -> Action {
        if self.count >= self.config.budget { return self.terminal(Action::Fail(Failure::Budget)); }
        let request = RequestId(self.count + 1);
        self.pending = Some((request, position));
        Action::Observe { request, position }
    }
    fn best(&self) -> Point {
        let mut best = self.points[0];
        for point in &self.points[1..usize::from(self.count)] {
            if point.score.0 > best.score.0 { best = *point; }
        }
        best
    }
    fn measured(&self, position: Position) -> bool {
        self.points[..usize::from(self.count)].iter().any(|p| p.position == position)
    }
    fn verify(&mut self) -> Action {
        self.phase = Phase::Verify;
        self.request(self.best().position)
    }
    fn adaptive(&mut self, low: Position, high: Position) -> Action {
        let best = self.best();
        let points = &self.points[..usize::from(self.count)];
        let left = points.iter().filter(|p| p.position < best.position).max_by_key(|p| p.position);
        let right = points.iter().filter(|p| p.position > best.position).min_by_key(|p| p.position);
        if let (Some(left), Some(right)) = (left, right) {
            let dl = i64::from(best.position.0 - left.position.0);
            let dr = i64::from(right.position.0 - best.position.0);
            let rise = i64::from(best.score.0) - i64::from(left.score.0);
            let fall = i64::from(right.score.0) - i64::from(best.score.0);
            let curvature = fall * dl - rise * dr;
            if curvature < 0 {
                // Position <= 65535 and score <= 1000000 bound products below 2^54.
                let offset = -(fall * dl * dl + rise * dr * dr) / (2 * curvature);
                let fitted = (i64::from(best.position.0) + offset)
                    .clamp(i64::from(left.position.0), i64::from(right.position.0));
                let target = Position(fitted as u16);
                if !self.measured(target) { return self.request(target); }
            }
        }
        let lo = left.map_or(low, |p| p.position);
        let hi = right.map_or(high, |p| p.position);
        let target = if best.position.0 - lo.0 >= hi.0 - best.position.0 {
            Position(lo.0 + (best.position.0 - lo.0) / 2)
        } else {
            Position(best.position.0 + (hi.0 - best.position.0) / 2)
        };
        if (best.position.0 - lo.0 <= self.config.fine && hi.0 - best.position.0 <= self.config.fine)
            || self.measured(target) { self.verify() } else { self.request(target) }
    }
    pub fn step(&mut self, event: Event) -> Action {
        if let Phase::Terminal(action) = self.phase { return action; }
        if matches!(event, Event::Cancel) { return self.terminal(Action::Fail(Failure::Cancelled)); }
        self.calls += 1;
        if self.calls > 256 { return self.terminal(Action::Fail(Failure::Budget)); }
        if matches!(self.phase, Phase::Idle) {
            return if matches!(event, Event::Start) {
                self.phase = Phase::Coarse;
                self.request(self.config.low)
            } else { self.terminal(Action::Fail(Failure::InvalidObservation)) };
        }
        let Event::Sample { request, position, score, fresh, settled } = event else { return Action::Wait; };
        if !fresh || !settled { return Action::Wait; }
        if self.pending != Some((request, position)) { return self.terminal(Action::Fail(Failure::InvalidObservation)); }
        self.pending = None;
        self.points[usize::from(self.count)] = Point { position, score };
        self.count += 1;
        match self.phase {
            Phase::Coarse if position < self.config.high => {
                self.request(Position(position.0.saturating_add(self.config.coarse).min(self.config.high.0)))
            }
            Phase::Coarse => {
                let best = self.best();
                let min = self.points[..usize::from(self.count)].iter().map(|p| p.score.0).min().unwrap_or(0);
                if best.score.0 - min < self.config.min_span.0 {
                    return self.terminal(Action::Fail(Failure::FlatCurve));
                }
                let low = Position(best.position.0.saturating_sub(self.config.coarse).max(self.config.low.0));
                let high = Position(best.position.0.saturating_add(self.config.coarse).min(self.config.high.0));
                self.phase = Phase::Refine { low, high };
                self.refine(low, high)
            }
            Phase::Refine { low, high } => self.refine(low, high),
            Phase::Verify => {
                let previous = self.points[..usize::from(self.count - 1)].iter()
                    .filter(|p| p.position == position).map(|p| p.score.0).max().unwrap_or(0);
                if score.0.abs_diff(previous) > self.config.min_span.0 {
                    self.terminal(Action::Fail(Failure::InvalidObservation))
                } else { self.terminal(Action::Finish(position)) }
            }
            _ => self.terminal(Action::Fail(Failure::InvalidObservation)),
        }
    }
    fn refine(&mut self, low: Position, high: Position) -> Action {
        if self.candidate == Candidate::AdaptivePeak { return self.adaptive(low, high); }
        let mut target = low;
        loop {
            if !self.measured(target) { return self.request(target); }
            if target == high { return self.verify(); }
            target = Position(target.0.saturating_add(self.config.fine).min(high.0));
        }
    }
}
