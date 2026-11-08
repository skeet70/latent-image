//! X-M1 1.50 VGLIB invocation completion after its event wait and callbacks.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Site { Enter, AfterWait, Finish }
impl Site {
    pub(crate) const ALL: [Self; 3] = [Self::Enter, Self::AfterWait, Self::Finish];
    pub(crate) const fn address(self) -> u32 {
        match self { Self::Enter => 0x006d_7920, Self::AfterWait => 0x006d_7da4, Self::Finish => 0x006d_7df4 }
    }
    pub(crate) const fn word(self) -> u32 {
        match self { Self::Enter => 0xe92d_49f0, Self::AfterWait => 0xe284_4f46, Self::Finish => 0xe1a0_0004 }
    }
    pub(crate) fn at(pc: u32) -> Option<Self> { Self::ALL.into_iter().find(|site| site.address() == pc) }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Core { Primary, Secondary }
impl Core {
    pub(crate) fn from_index(core: usize) -> Result<Self, &'static str> {
        match core { 0 => Ok(Self::Primary), 1 => Ok(Self::Secondary), _ => Err("presentation core exceeds two cores") }
    }
    fn index(self) -> usize { match self { Self::Primary => 0, Self::Secondary => 1 } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FramePointer(pub(crate) u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CoreInstructions(pub(crate) u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Owner { core: Core, frame: FramePointer }
#[derive(Clone, Copy)]
enum Stage { Entered, WaitSucceeded, WaitFailed }
#[derive(Clone, Copy)]
struct Invocation { owner: Owner, token: FrameToken, stage: Stage }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct FrameToken(std::num::NonZeroU64);
#[derive(Default)]
struct FrameSequence(u64);
#[derive(Clone, Copy)]
struct Target { token: FrameToken, outcome: Option<Result<(), &'static str>> }
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum CompletionStatus { Acknowledged }
#[derive(Clone, Copy, Serialize)]
pub struct Acknowledgement {
    status: CompletionStatus,
    selected_frame: FrameToken,
    completed_frame: FrameToken,
}

#[derive(Default)]
pub(crate) struct Tracker {
    invocations: [Option<Invocation>; 16],
    sequence: FrameSequence,
    latest: Option<Target>,
    selected: Option<Target>,
    established: bool,
    last: [Option<(Site, CoreInstructions)>; 2],
    failure: Option<&'static str>,
}
impl Tracker {
    pub(crate) fn invalidate(&mut self) {
        *self = Self { sequence: FrameSequence(self.sequence.0), ..Self::default() };
    }
    fn add(&mut self, owner: Owner, stage: Stage) -> Result<usize, &'static str> {
        let index = self.invocations.iter().position(Option::is_none).ok_or("presentation invocation bound exceeded")?;
        self.sequence.0 = self.sequence.0.checked_add(1).ok_or("presentation frame token overflow")?;
        let token = FrameToken(std::num::NonZeroU64::new(self.sequence.0).ok_or("presentation frame token is zero")?);
        self.invocations[index] = Some(Invocation { owner, token, stage });
        self.latest = Some(Target { token, outcome: None });
        Ok(index)
    }
    pub(crate) fn observe(&mut self, site: Site, core: Core, frame: FramePointer, instructions: CoreInstructions, result: u32) {
        if self.failure.is_some() || self.last[core.index()] == Some((site, instructions)) { return; }
        self.last[core.index()] = Some((site, instructions));
        if let Err(failure) = self.record(site, Owner { core, frame }, result) { self.failure = Some(failure); }
    }
    fn record(&mut self, site: Site, owner: Owner, result: u32) -> Result<(), &'static str> {
        let existing = self.invocations.iter().position(|entry| entry.is_some_and(|entry| entry.owner == owner));
        match site {
            Site::Enter => {
                if existing.is_some() { return Err("presentation invocation entered twice"); }
                self.add(owner, Stage::Entered)?;
            }
            Site::AfterWait => {
                let stage = if result == 0 { Stage::WaitSucceeded } else { Stage::WaitFailed };
                let index = match existing {
                    Some(index) => index,
                    None if !self.established && self.invocations.iter().all(Option::is_none) => self.add(owner, stage)?,
                    None => return Err("presentation wait has no matching invocation"),
                };
                let invocation = self.invocations[index].as_mut().ok_or("presentation invocation disappeared")?;
                invocation.stage = stage;
            }
            Site::Finish => {
                let Some(index) = existing else {
                    if !self.established && self.invocations.iter().all(Option::is_none) { return Ok(()); }
                    return Err("presentation finish has no matching invocation");
                };
                let invocation = self.invocations[index].take().ok_or("presentation invocation disappeared")?;
                let outcome = if result != 0 { Err("presentation handler did not succeed") }
                    else if !matches!(invocation.stage, Stage::WaitSucceeded) { Err("presentation finish lacks a successful event wait") }
                    else { Ok(()) };
                for target in [&mut self.latest, &mut self.selected].into_iter().flatten() {
                    if target.token == invocation.token { target.outcome = Some(outcome); }
                }
                self.established |= outcome.is_ok();
            }
        }
        Ok(())
    }
    pub(crate) fn select_target(&mut self) -> Result<Option<FrameToken>, &'static str> {
        if let Some(failure) = self.failure { return Err(failure); }
        self.selected = self.latest;
        Ok(self.selected.map(|target| target.token))
    }
    pub(crate) fn acknowledgement(&self, token: FrameToken) -> Result<Option<Acknowledgement>, &'static str> {
        if let Some(failure) = self.failure { return Err(failure); }
        let selected = self.selected.filter(|selected| selected.token == token).ok_or("presentation target is not selected")?;
        match selected.outcome {
            None => Ok(None),
            Some(Err(failure)) => Err(failure),
            Some(Ok(())) => Ok(Some(Acknowledgement { status: CompletionStatus::Acknowledged, selected_frame: token, completed_frame: token })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(tracker: &mut Tracker, site: Site, core: Core, fp: u32, count: u64, result: u32) {
        tracker.observe(site, core, FramePointer(fp), CoreInstructions(count), result);
    }
    fn finish(tracker: &mut Tracker, fp: u32) {
        event(tracker, Site::AfterWait, Core::Primary, fp, 20, 0);
        event(tracker, Site::Finish, Core::Primary, fp, 30, 0);
    }
    #[test]
    fn a_selected_entered_frame_waits_for_its_successful_wait_and_callbacks() {
        let mut tracker = Tracker::default();
        assert!(tracker.select_target().unwrap().is_none());
        event(&mut tracker, Site::Enter, Core::Primary, 0x1000, 10, 0);
        let token = tracker.select_target().unwrap().unwrap();
        assert!(tracker.acknowledgement(token).unwrap().is_none());
        event(&mut tracker, Site::AfterWait, Core::Primary, 0x1000, 20, 0);
        assert!(tracker.acknowledgement(token).unwrap().is_none());
        event(&mut tracker, Site::Finish, Core::Primary, 0x1000, 30, 0);
        let proof = serde_json::to_value(tracker.acknowledgement(token).unwrap().unwrap()).unwrap();
        assert_eq!(proof, serde_json::json!({"status":"acknowledged","selected_frame":1,"completed_frame":1}));
    }
    #[test]
    fn a_failed_wait_cannot_be_replaced_by_a_successful_handler_tail() {
        let mut tracker = Tracker::default();
        event(&mut tracker, Site::Enter, Core::Primary, 0x1000, 10, 0);
        let token = tracker.select_target().unwrap().unwrap();
        event(&mut tracker, Site::AfterWait, Core::Primary, 0x1000, 20, (-50i32) as u32);
        event(&mut tracker, Site::Finish, Core::Primary, 0x1000, 30, 0);
        assert!(tracker.acknowledgement(token).is_err());
    }
    #[test]
    fn another_frame_or_core_cannot_complete_the_pending_invocation() {
        for (core, fp) in [(Core::Secondary, 0x1000), (Core::Primary, 0x1004)] {
            let mut tracker = Tracker::default();
            event(&mut tracker, Site::Enter, Core::Primary, 0x1000, 10, 0);
            let token = tracker.select_target().unwrap().unwrap();
            event(&mut tracker, Site::AfterWait, core, fp, 20, 0);
            event(&mut tracker, Site::Finish, core, fp, 30, 0);
            assert!(tracker.acknowledgement(token).is_err());
        }
    }
    #[test]
    fn initially_unknown_state_recovers_only_from_an_observed_wait_and_tail() {
        let mut tracker = Tracker::default();
        event(&mut tracker, Site::Finish, Core::Primary, 0x1000, 10, 0);
        assert!(tracker.select_target().unwrap().is_none());
        finish(&mut tracker, 0x1000);
        let token = tracker.select_target().unwrap().unwrap();
        assert!(tracker.acknowledgement(token).unwrap().is_some());
        tracker.invalidate();
        assert!(tracker.select_target().unwrap().is_none());
        assert!(tracker.acknowledgement(token).is_err());
        finish(&mut tracker, 0x1000);
        let next = tracker.select_target().unwrap().unwrap();
        assert_ne!(next, token);
        assert!(tracker.acknowledgement(next).unwrap().is_some());
        assert!(tracker.acknowledgement(token).is_err());
    }
    #[test]
    fn retrying_a_pending_instruction_does_not_duplicate_the_frame() {
        let mut tracker = Tracker::default();
        event(&mut tracker, Site::Enter, Core::Primary, 0x1000, 10, 0);
        event(&mut tracker, Site::Enter, Core::Primary, 0x1000, 10, 0);
        finish(&mut tracker, 0x1000);
        let token = tracker.select_target().unwrap().unwrap();
        let proof = serde_json::to_value(tracker.acknowledgement(token).unwrap().unwrap()).unwrap();
        assert_eq!(proof["selected_frame"], 1);
    }
    #[test]
    fn later_frames_neither_extend_nor_replace_the_selected_target() {
        for result in [0, (-50i32) as u32] {
            let mut tracker = Tracker::default();
            event(&mut tracker, Site::Enter, Core::Primary, 0x1000, 10, 0);
            let token = tracker.select_target().unwrap().unwrap();
            finish(&mut tracker, 0x1000);
            event(&mut tracker, Site::Enter, Core::Primary, 0x2000, 40, 0);
            assert!(tracker.acknowledgement(token).unwrap().is_some());
            event(&mut tracker, Site::AfterWait, Core::Primary, 0x2000, 50, result);
            event(&mut tracker, Site::Finish, Core::Primary, 0x2000, 60, 0);
            assert!(tracker.acknowledgement(token).unwrap().is_some());
            let next = tracker.select_target().unwrap().unwrap();
            assert_ne!(next, token);
            assert_eq!(tracker.acknowledgement(next).is_ok(), result == 0);
        }
    }
}
