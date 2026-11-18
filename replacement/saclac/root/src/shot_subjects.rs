#![forbid(unsafe_code)]

use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::{Argument::{Buffer, Word}, Machine};
use xm1_shot_sequence::{boundary::LensReference, subjects::{PrimaryDetection, Selection, SubjectList, SubjectQuery, SubjectServices, Summary}, stock as shot};
use crate::{peer_services::SubjectSelection, shot_services::Libraries};

const QUERY: usize = 0;
const PRIMARY: usize = 16;
const PRIMARY_LIST: usize = PRIMARY + 12;
const SUMMARY: usize = PRIMARY_LIST + shot::SUBJECT_LIST_BYTES;
const LIST: usize = SUMMARY + 15 * 4;
const BYTES: usize = LIST + shot::SUBJECT_LIST_BYTES;

/// Primary's shared reply survives its detect/select/regions calls. Secondary's in/out list
/// has separate staging, while both sources retain the same fifteen-word summary.
/// The borrow serializes native calls; the workspace stays allocated between entries.
pub struct Workspace { bytes: [u8; BYTES] }
impl Workspace {
    pub const fn new() -> Self { Self { bytes: [0; BYTES] } }
    fn word(&self, at: usize) -> u32 { u32::from_le_bytes(self.bytes[at..at + 4].try_into().expect("subject word")) }
    fn set_word(&mut self, at: usize, value: u32) { self.bytes[at..at + 4].copy_from_slice(&value.to_le_bytes()); }
    fn query(&mut self, query: &SubjectQuery) {
        for (index, value) in [query.lens, query.first, query.second, query.area_layout].into_iter().enumerate() { self.set_word(QUERY + index * 4, value); }
    }
    fn list(&self, at: usize) -> SubjectList {
        shot::subject_list(self.bytes[at..at + shot::SUBJECT_LIST_BYTES].try_into().expect("subject list"))
    }
    fn stage_list(&mut self, list: &SubjectList) {
        let out: &mut [u8; shot::SUBJECT_LIST_BYTES] = (&mut self.bytes[LIST..LIST + shot::SUBJECT_LIST_BYTES]).try_into().expect("subject list staging");
        *out = shot::subject_list_bytes(list);
    }
    fn summary(&self) -> Summary { Summary(core::array::from_fn(|index| self.word(SUMMARY + index * 4))) }
    fn stage_summary(&mut self, summary: &Summary) { for (index, &value) in summary.0.iter().enumerate() { self.set_word(SUMMARY + index * 4, value); } }
}
impl Default for Workspace { fn default() -> Self { Self::new() } }

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q: SubjectSelection> SubjectServices for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn lens_distance(&mut self, _lens_reference: LensReference) -> u32 {
        // f51b50/f4f16c reads the record's chosen word; its second argument is not consumed.
        self.services.borrow().records.chosen_word(self.record).unwrap_or(0)
    }
    fn primary_detect(&mut self, limit: u32, query: &SubjectQuery) -> PrimaryDetection {
        let machine = self.services.borrow().machine.clone();
        let mut workspace = self.subjects.borrow_mut();
        workspace.query(query);
        machine.enter_with(0x10510e8, &[Word(limit), Buffer(QUERY), Buffer(PRIMARY), Buffer(SUMMARY)], &mut workspace.bytes);
        PrimaryDetection { found: workspace.word(PRIMARY) != 0, source: workspace.word(PRIMARY + 4), list: workspace.list(PRIMARY_LIST), summary: workspace.summary() }
    }
    fn primary_select(&mut self, limit: u32, query: &SubjectQuery) -> SubjectList {
        let machine = self.services.borrow().machine.clone();
        let mut workspace = self.subjects.borrow_mut();
        workspace.query(query);
        machine.enter_with(0x10512cc, &[Word(limit), Buffer(QUERY), Buffer(PRIMARY), Word(0)], &mut workspace.bytes);
        workspace.list(PRIMARY_LIST)
    }
    fn primary_regions(&mut self, query: &SubjectQuery) -> SubjectList {
        let machine = self.services.borrow().machine.clone();
        let mut workspace = self.subjects.borrow_mut();
        workspace.query(query);
        machine.enter_with(0x10517a0, &[Buffer(QUERY), Buffer(PRIMARY)], &mut workspace.bytes);
        workspace.list(PRIMARY_LIST)
    }
    fn secondary_detect(&mut self, selection: Selection, query: &SubjectQuery, list: &mut SubjectList, summary: &mut Summary) {
        let machine = self.services.borrow().machine.clone();
        let mut workspace = self.subjects.borrow_mut();
        workspace.query(query); workspace.stage_list(list); workspace.stage_summary(summary);
        machine.enter_with(0xeedf48, &[Word(selection.kind), Word(selection.limit), Buffer(QUERY), Buffer(LIST), Buffer(SUMMARY)], &mut workspace.bytes);
        *list = workspace.list(LIST); *summary = workspace.summary();
    }
    fn secondary_select(&mut self, selection: Selection, query: &SubjectQuery, list: &mut SubjectList) {
        let machine = self.services.borrow().machine.clone();
        let mut workspace = self.subjects.borrow_mut();
        workspace.query(query); workspace.stage_list(list);
        machine.enter_with(0xeee214, &[Word(selection.kind), Word(selection.limit), Buffer(QUERY), Buffer(LIST), Word(0)], &mut workspace.bytes);
        *list = workspace.list(LIST);
    }
    fn secondary_regions(&mut self, query: &SubjectQuery, list: &mut SubjectList) {
        let machine = self.services.borrow().machine.clone();
        let mut workspace = self.subjects.borrow_mut();
        workspace.query(query); workspace.stage_list(list);
        machine.enter_with(0xeedc84, &[Buffer(QUERY), Buffer(LIST)], &mut workspace.bytes);
        *list = workspace.list(LIST);
    }
    fn refine(&mut self, query: &SubjectQuery, list: &mut SubjectList) {
        let machine = self.services.borrow().machine.clone();
        let mut workspace = self.subjects.borrow_mut();
        workspace.query(query); workspace.stage_list(list);
        machine.enter_with(0xeed3d8, &[Buffer(QUERY), Buffer(LIST)], &mut workspace.bytes);
        *list = workspace.list(LIST);
    }
    fn select(&mut self, kind: u32, option: u32) -> bool {
        match self.peers.select_subjects(kind, option) { Ok(started) => started, Err(_) => panic!("subject selection rendezvous failed") }
    }
    fn close_selection(&mut self) {
        let machine = self.services.borrow().machine.clone();
        machine.enter(crate::stock::END_SUBJECT_SELECTION, &[]);
    }
}
