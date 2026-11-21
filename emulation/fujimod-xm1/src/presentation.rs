//! X-M1 1.50 VGLIB invocation completion after its event wait and callbacks.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Site { Enter, AfterWait, Finish }
impl Site {
    pub(crate) const ALL: [Self; 3] = [Self::Enter, Self::AfterWait, Self::Finish];
    /// The stock X-M1 1.50 address.
    pub(crate) const fn address(self) -> u32 {
        match self { Self::Enter => 0x006d_7920, Self::AfterWait => 0x006d_7da4, Self::Finish => 0x006d_7df4 }
    }
    /// The stock X-M1 1.50 A32 word.
    pub(crate) const fn word(self) -> u32 {
        match self { Self::Enter => 0xe92d_49f0, Self::AfterWait => 0xe284_4f46, Self::Finish => 0xe1a0_0004 }
    }
    fn at(pc: u32) -> Option<Self> { Self::ALL.into_iter().find(|site| site.address() == pc) }
}

/// A 32-bit value written as `0x` and one to eight hex digits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct HexWord(pub u32);
impl TryFrom<String> for HexWord {
    type Error = String;
    fn try_from(text: String) -> Result<Self, String> {
        text.strip_prefix("0x")
            .filter(|digits| (1..=8).contains(&digits.len()) && digits.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .and_then(|digits| u32::from_str_radix(digits, 16).ok())
            .map(Self)
            .ok_or_else(|| format!("{text:?} is not a 0x-prefixed 32-bit hex word"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedInstruction { pub pc: HexWord, pub word: HexWord }

/// `mov r4, r0`, right after the owned event wait returns.
const OWNED_AFTER_WAIT: u32 = 0xe1a0_4000;
/// `mov r0, r4`, after the owned unlock.
const OWNED_FINISH: u32 = 0xe1a0_0004;
/// `pop {r4, pc}`, the owned hand-off call's return address.
pub const OWNED_HAND_OFF_RETURN: u32 = 0xe8bd_8010;
/// `strb r2, [r3, #0x160]`, the stock word at `0x006d7d44`.
pub const OWNED_FRONT_STORE: u32 = 0xe5c3_2160;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnedSitesFields {
    published_enter: HexWord, after_wait: OwnedInstruction, finish: OwnedInstruction,
    hand_off_return: OwnedInstruction, front_store: OwnedInstruction,
}

/// The render-frame package's branch published over stock Enter, its owned AfterWait and Finish instructions, and
/// its owned stage-0 publication sites.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "OwnedSitesFields")]
pub struct OwnedSites { published_enter: u32, after_wait: u32, finish: u32, publication: OwnedPublication }

/// The owned counterparts of the stock stage-0 hand-off callers `0x006d7b3c`/`0x006d7c24`/`0x006d7d10` (the
/// hand-off's return address) and of the front-byte store `0x006d7d44` (r2 = the incremented byte).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnedPublication { pub hand_off_return: u32, pub front_store: u32 }

impl TryFrom<OwnedSitesFields> for OwnedSites {
    type Error = &'static str;
    fn try_from(fields: OwnedSitesFields) -> Result<Self, &'static str> {
        let OwnedSitesFields { published_enter: HexWord(published_enter), after_wait, finish, hand_off_return, front_store } = fields;
        if published_enter >> 24 != 0xea { return Err("published presentation entry is not an A32 B"); }
        if after_wait.word.0 != OWNED_AFTER_WAIT { return Err("owned presentation wait is not mov r4, r0"); }
        if finish.word.0 != OWNED_FINISH { return Err("owned presentation finish is not mov r0, r4"); }
        if hand_off_return.word.0 != OWNED_HAND_OFF_RETURN { return Err("owned hand-off return is not pop {r4, pc}"); }
        if front_store.word.0 != OWNED_FRONT_STORE { return Err("owned front store is not strb r2, [r3, #0x160]"); }
        let pcs = [after_wait.pc.0, finish.pc.0, hand_off_return.pc.0, front_store.pc.0];
        if pcs.iter().any(|pc| pc % 4 != 0) { return Err("owned presentation site is not word-aligned"); }
        let repeated = pcs.iter().enumerate().any(|(index, pc)| pcs[..index].contains(pc));
        if repeated || pcs.iter().any(|pc| Site::at(*pc).is_some()) {
            return Err("owned presentation sites overlap");
        }
        let publication = OwnedPublication { hand_off_return: hand_off_return.pc.0, front_store: front_store.pc.0 };
        Ok(Self { published_enter, after_wait: after_wait.pc.0, finish: finish.pc.0, publication })
    }
}
impl OwnedSites {
    fn owned(self) -> [(Site, u32, u32); 2] {
        [(Site::AfterWait, self.after_wait, OWNED_AFTER_WAIT), (Site::Finish, self.finish, OWNED_FINISH)]
    }
    pub fn publication(self) -> OwnedPublication { self.publication }
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
    sites: Option<OwnedSites>,
    invocations: [Option<Invocation>; 16],
    sequence: FrameSequence,
    latest: Option<Target>,
    selected: Option<Target>,
    established: bool,
    last: [Option<(Site, CoreInstructions)>; 2],
    failure: Option<&'static str>,
}
impl Tracker {
    pub(crate) fn new(sites: Option<OwnedSites>) -> Self { Self { sites, ..Self::default() } }
    pub(crate) fn invalidate(&mut self) {
        *self = Self { sites: self.sites, sequence: FrameSequence(self.sequence.0), ..Self::default() };
    }
    pub(crate) fn addresses(&self) -> impl Iterator<Item = u32> {
        Site::ALL.map(Site::address).into_iter()
            .chain(self.sites.into_iter().flat_map(|sites| sites.owned().map(|(_, pc, _)| pc)))
    }
    /// The stage `pc` reports, or `None` for an address outside `addresses`.
    pub(crate) fn site(&self, pc: u32, word: u32) -> Result<Option<Site>, &'static str> {
        let (site, admitted) = if let Some(site) = Site::at(pc) {
            (site, word == site.word() || (site == Site::Enter && self.sites.is_some_and(|sites| sites.published_enter == word)))
        } else if let Some((site, _, expected)) = self.sites.and_then(|sites| sites.owned().into_iter().find(|(_, owned, _)| *owned == pc)) {
            (site, word == expected)
        } else {
            return Ok(None);
        };
        if admitted { Ok(Some(site)) } else { Err("presentation instruction differs from its X-M1 1.50 or render-frame source") }
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

    const PUBLISHED: u32 = 0xea01_2345;
    const OWNED_WAIT_PC: u32 = 0x0400_1000;
    const OWNED_FINISH_PC: u32 = 0x0400_1080;
    const STOCK_ENTER: u32 = 0xe92d_49f0;
    const STOCK_WAIT: u32 = 0xe284_4f46;
    const MOV_R4_R0: u32 = 0xe1a0_4000;
    const MOV_R0_R4: u32 = 0xe1a0_0004;

    fn configuration(published: &str, wait: (&str, &str), finish: (&str, &str)) -> serde_json::Value {
        serde_json::json!({"published_enter": published, "after_wait": {"pc": wait.0, "word": wait.1},
            "finish": {"pc": finish.0, "word": finish.1},
            "hand_off_return": {"pc": "0x04001100", "word": "0xe8bd8010"},
            "front_store": {"pc": "0x04001140", "word": "0xe5c32160"}})
    }
    fn with_site(mut value: serde_json::Value, name: &str, site: Option<(&str, &str)>) -> serde_json::Value {
        let fields = value.as_object_mut().unwrap();
        match site {
            Some((pc, word)) => { fields.insert(name.into(), serde_json::json!({"pc": pc, "word": word})); }
            None => { fields.remove(name); }
        }
        value
    }
    fn owned_tracker() -> Tracker {
        let value = configuration("0xea012345", ("0x04001000", "0xe1a04000"), ("0x4001080", "0xe1a00004"));
        Tracker::new(Some(serde_json::from_value(value).unwrap()))
    }
    fn step(tracker: &mut Tracker, pc: u32, word: u32, fp: u32, count: u64, result: u32) -> Result<(), &'static str> {
        let site = tracker.site(pc, word)?.ok_or("address is not a presentation site")?;
        tracker.observe(site, Core::Primary, FramePointer(fp), CoreInstructions(count), result);
        Ok(())
    }
    #[test]
    fn an_owned_frame_completes_through_the_published_entry_and_owned_sites() {
        let mut tracker = owned_tracker();
        step(&mut tracker, 0x006d_7920, PUBLISHED, 0x1000, 10, 0).unwrap();
        let token = tracker.select_target().unwrap().unwrap();
        step(&mut tracker, OWNED_WAIT_PC, MOV_R4_R0, 0x1000, 20, 0).unwrap();
        assert!(tracker.acknowledgement(token).unwrap().is_none());
        step(&mut tracker, OWNED_FINISH_PC, MOV_R0_R4, 0x1000, 30, 0).unwrap();
        let proof = serde_json::to_value(tracker.acknowledgement(token).unwrap().unwrap()).unwrap();
        assert_eq!(proof, serde_json::json!({"status":"acknowledged","selected_frame":1,"completed_frame":1}));
    }
    #[test]
    fn a_failed_owned_wait_refuses_the_frame() {
        let mut tracker = owned_tracker();
        step(&mut tracker, 0x006d_7920, PUBLISHED, 0x1000, 10, 0).unwrap();
        let token = tracker.select_target().unwrap().unwrap();
        step(&mut tracker, OWNED_WAIT_PC, MOV_R4_R0, 0x1000, 20, (-50i32) as u32).unwrap();
        step(&mut tracker, OWNED_FINISH_PC, MOV_R0_R4, 0x1000, 30, 0).unwrap();
        assert!(tracker.acknowledgement(token).is_err());
    }
    #[test]
    fn stock_fallback_frames_complete_beside_owned_frames_and_survive_invalidation() {
        let mut tracker = owned_tracker();
        let mut addresses: Vec<u32> = tracker.addresses().collect();
        addresses.sort_unstable();
        assert_eq!(addresses, [0x006d_7920, 0x006d_7da4, 0x006d_7df4, OWNED_WAIT_PC, OWNED_FINISH_PC]);
        step(&mut tracker, 0x006d_7920, STOCK_ENTER, 0x1000, 10, 0).unwrap();
        let stock = tracker.select_target().unwrap().unwrap();
        step(&mut tracker, 0x006d_7da4, STOCK_WAIT, 0x1000, 20, 0).unwrap();
        step(&mut tracker, 0x006d_7df4, MOV_R0_R4, 0x1000, 30, 0).unwrap();
        assert!(tracker.acknowledgement(stock).unwrap().is_some());
        tracker.invalidate();
        step(&mut tracker, 0x006d_7920, PUBLISHED, 0x2000, 40, 0).unwrap();
        step(&mut tracker, OWNED_WAIT_PC, MOV_R4_R0, 0x2000, 50, 0).unwrap();
        step(&mut tracker, OWNED_FINISH_PC, MOV_R0_R4, 0x2000, 60, 0).unwrap();
        step(&mut tracker, 0x006d_7920, STOCK_ENTER, 0x3000, 70, 0).unwrap();
        let fallback = tracker.select_target().unwrap().unwrap();
        assert_ne!(fallback, stock);
        step(&mut tracker, 0x006d_7da4, STOCK_WAIT, 0x3000, 80, 0).unwrap();
        step(&mut tracker, 0x006d_7df4, MOV_R0_R4, 0x3000, 90, 0).unwrap();
        assert!(tracker.acknowledgement(fallback).unwrap().is_some());
    }
    #[test]
    fn the_published_word_needs_configuration_and_owned_sites_need_their_words() {
        let unconfigured = Tracker::default();
        assert!(unconfigured.site(0x006d_7920, PUBLISHED).is_err());
        assert_eq!(unconfigured.site(OWNED_WAIT_PC, MOV_R4_R0), Ok(None));
        assert_eq!(unconfigured.addresses().count(), 3);
        let configured = owned_tracker();
        assert!(configured.site(0x006d_7920, 0xea00_0000).is_err());
        assert!(configured.site(OWNED_WAIT_PC, STOCK_WAIT).is_err());
        assert!(configured.site(OWNED_FINISH_PC, MOV_R4_R0).is_err());
        assert!(configured.site(0x006d_7da4, MOV_R4_R0).is_err());
    }
    #[test]
    fn the_owned_publication_sites_are_the_configured_addresses() {
        let sites: OwnedSites = serde_json::from_value(configuration("0xea012345", ("0x04001000", "0xe1a04000"), ("0x04001080", "0xe1a00004"))).unwrap();
        assert_eq!(sites.publication(), OwnedPublication { hand_off_return: 0x0400_1100, front_store: 0x0400_1140 });
    }
    #[test]
    fn malformed_owned_site_configurations_are_refused() {
        let wait = ("0x04001000", "0xe1a04000");
        let finish = ("0x04001080", "0xe1a00004");
        let valid = configuration("0xea012345", wait, finish);
        for value in [
            configuration("0xe92d49f0", wait, finish),
            configuration("ea012345", wait, finish),
            configuration("0x1ea012345", wait, finish),
            configuration("0xea012345", ("0x04001000", "0xe1a00004"), finish),
            configuration("0xea012345", wait, ("0x04001080", "0xe1a04000")),
            configuration("0xea012345", ("0x04001002", "0xe1a04000"), finish),
            configuration("0xea012345", wait, ("0x04001000", "0xe1a00004")),
            configuration("0xea012345", ("0x006d7da4", "0xe1a04000"), finish),
            configuration("0xea012345", wait, ("0x006d7920", "0xe1a00004")),
            with_site(valid.clone(), "finish", None),
            with_site(valid.clone(), "hand_off_return", None),
            with_site(valid.clone(), "front_store", None),
            with_site(valid.clone(), "hand_off_return", Some(("0x04001100", "0xe8bd8000"))),
            with_site(valid.clone(), "front_store", Some(("0x04001140", "0xe5c32161"))),
            with_site(valid.clone(), "front_store", Some(("0x04001100", "0xe5c32160"))),
            with_site(valid.clone(), "front_store", Some(("0x04001142", "0xe5c32160"))),
            with_site(valid.clone(), "hand_off_return", Some(("0x006d7df4", "0xe8bd8010"))),
            with_site(valid.clone(), "extra", Some(("0x04001200", "0xe1a00000"))),
        ] {
            assert!(serde_json::from_value::<OwnedSites>(value.clone()).is_err(), "{value}");
        }
        assert!(serde_json::from_value::<OwnedSites>(valid).is_ok());
    }
}
