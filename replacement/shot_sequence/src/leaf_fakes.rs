use crate::boundary::*;
use crate::preparation::FrameHeader;
use crate::subjects::{PrimaryDetection, Selection, Subject, SubjectList, SubjectQuery, SubjectServices, Summary};
use crate::summary::{AreaStatistics, FrameSummary};
use std::collections::BTreeMap;
use std::vec::Vec;

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Trace(Trace),
    Hold(bool),
    SelectFirstMode,
    StoreLensSnapshot(LensSnapshot),
    StoreFocusResult(FocusResult),
    StoreFocusConfirmation(FocusConfirmation),
    StoreFocusHistory(FocusHistory),
    ClearExposureRecord,
    Published(Published),
    LoadLensCapabilities,
    LensTarget { target: ClientRef, point: LensPoint, step: u16, distance: u32 },
    FrameProgressCleared(ClientRef),
    Announced(Frame, FrameHeader),
    Summary(Frame, FrameSummary),
    Dump(Dump, Frame, u32),
    DumpCalculation,
    ResetImageProgress,
    Plan { area_count: Option<ClientRef>, live: bool },
    Detect(&'static str, u32),
    Select(u32, u32),
    CloseSelection,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Published {
    Subject { list: PublishedList, index: usize, subject: Subject },
    SubjectCount { list: PublishedList, count: u32 },
    SubjectFlag { index: usize, value: u16 },
    SubjectSummary { word: SummaryWord, value: u32 },
    SelectedSubject(Subject),
    SubjectTime(u32),
    TrackedMarker(u32),
    AreaSelection(u32),
    Area { index: usize, word: u32 },
    LensTargetArea(ClientRef),
    CaptureFrame(Option<Frame>),
    CaptureLink(u32),
}

impl From<Publication<'_>> for Published {
    fn from(publication: Publication<'_>) -> Self {
        match publication {
            Publication::Subject { list, index, subject } => Self::Subject { list, index, subject: *subject },
            Publication::SubjectCount { list, count } => Self::SubjectCount { list, count },
            Publication::SubjectFlag { index, value } => Self::SubjectFlag { index, value },
            Publication::SubjectSummary { word, value } => Self::SubjectSummary { word, value },
            Publication::SelectedSubject(subject) => Self::SelectedSubject(*subject),
            Publication::SubjectTime(time) => Self::SubjectTime(time),
            Publication::TrackedMarker(marker) => Self::TrackedMarker(marker),
            Publication::AreaSelection(word) => Self::AreaSelection(word),
            Publication::Area { index, word } => Self::Area { index, word },
            Publication::LensTargetArea(target) => Self::LensTargetArea(target),
            Publication::CaptureFrame(frame) => Self::CaptureFrame(frame),
            Publication::CaptureLink(link) => Self::CaptureLink(link),
        }
    }
}

pub struct Fake {
    pub events: Vec<Event>,
    pub settings: BTreeMap<SettingKey, u32>,
    pub lens: BTreeMap<ReadingKey, u32>,
    pub change: u32,
    pub designated: Option<LensPoint>,
    pub snapshot: LensSnapshot,
    pub result: FocusResult,
    pub confirmation: FocusConfirmation,
    pub history: FocusHistory,
    pub statistics: AreaStatistics,
    pub area_words: Vec<u32>,
    pub frame: Option<Frame>,
    pub adjustments: BTreeMap<AdjustmentKey, u8>,
    pub calculation_mode: u32,
    pub level: u8,
    pub tick: u32,
    pub detection: PrimaryDetection,
    pub lists: Vec<SubjectList>,
    pub selection_starts: bool,
    pub exposure: crate::exposure_fakes::ExposureWorld,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SettingKey(u8);
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ReadingKey(u8);
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AdjustmentKey(u8);

pub fn setting_key(setting: Setting) -> SettingKey { SettingKey(setting as u8) }
pub fn reading_key(reading: LensReading) -> ReadingKey { ReadingKey(reading as u8) }
pub fn adjustment_key(adjustment: Adjustment) -> AdjustmentKey { AdjustmentKey(adjustment as u8) }

impl Fake {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            settings: BTreeMap::new(),
            lens: BTreeMap::new(),
            change: 0,
            designated: None,
            snapshot: LensSnapshot::default(),
            result: FocusResult { reference: LensReference(0), valid: 1, kept: 1, refined: 1, extent: 0, bytes: [0; 132] },
            confirmation: FocusConfirmation { active: 0, reference: LensReference(0), bytes: [0; 32] },
            history: FocusHistory { valid: 1, bytes: [0; 80] },
            statistics: AreaStatistics::default(),
            area_words: Vec::new(),
            frame: None,
            adjustments: BTreeMap::new(),
            calculation_mode: 0,
            level: 0,
            tick: 0,
            detection: PrimaryDetection::default(),
            lists: Vec::new(),
            selection_starts: true,
            exposure: crate::exposure_fakes::ExposureWorld::default(),
        }
    }

    pub fn set(&mut self, setting: Setting, value: u32) { self.settings.insert(setting_key(setting), value); }
    pub fn set_lens(&mut self, reading: LensReading, value: u32) { self.lens.insert(reading_key(reading), value); }
    pub fn set_adjustment(&mut self, adjustment: Adjustment, value: u8) { self.adjustments.insert(adjustment_key(adjustment), value); }

    pub fn published(&self) -> Vec<Published> {
        self.events.iter().filter_map(|event| match event { Event::Published(published) => Some(published.clone()), _ => None }).collect()
    }

    pub fn has(&self, wanted: &Event) -> bool { self.events.contains(wanted) }

    fn next_list(&mut self) -> SubjectList { if self.lists.is_empty() { SubjectList::default() } else { self.lists.remove(0) } }
}

impl Records for Fake {
    fn setting(&mut self, setting: Setting) -> u32 { self.settings.get(&setting_key(setting)).copied().unwrap_or(0) }
    fn lens_snapshot(&mut self) -> LensSnapshot { self.snapshot }
    fn store_lens_snapshot(&mut self, snapshot: &LensSnapshot) {
        self.snapshot = *snapshot;
        self.events.push(Event::StoreLensSnapshot(*snapshot));
    }
    fn focus_result(&mut self) -> FocusResult { self.result }
    fn store_focus_result(&mut self, result: &FocusResult) {
        self.result = *result;
        self.events.push(Event::StoreFocusResult(*result));
    }
    fn focus_confirmation(&mut self) -> FocusConfirmation { self.confirmation }
    fn store_focus_confirmation(&mut self, confirmation: &FocusConfirmation) {
        self.confirmation = *confirmation;
        self.events.push(Event::StoreFocusConfirmation(*confirmation));
    }
    fn focus_history(&mut self) -> FocusHistory { self.history }
    fn store_focus_history(&mut self, history: &FocusHistory) {
        self.history = *history;
        self.events.push(Event::StoreFocusHistory(*history));
    }
    fn clear_exposure_record(&mut self) { self.events.push(Event::ClearExposureRecord); }
    fn area_statistics(&mut self) -> AreaStatistics { self.statistics }
    fn publish(&mut self, publication: Publication<'_>) { self.events.push(Event::Published(publication.into())); }
    fn select_first_mode(&mut self) { self.events.push(Event::SelectFirstMode); }
    fn load_lens_capabilities(&mut self) { self.events.push(Event::LoadLensCapabilities); }
}

impl Lens for Fake {
    fn read(&mut self, reading: LensReading) -> u32 { self.lens.get(&reading_key(reading)).copied().unwrap_or(0) }
    fn take_change(&mut self) -> u32 { core::mem::take(&mut self.change) }
    fn designated_point(&mut self) -> Option<LensPoint> { self.designated }
    fn target_distance(&mut self, point: LensPoint, control: u32) -> u32 { point.state + control }
    fn target_step(&mut self, point: LensPoint) -> u16 { point.kind as u16 + 1 }
}

impl Planner for Fake {
    fn plan_selection(&mut self, area_count: Option<ClientRef>, live: bool) -> SelectionPlan {
        self.events.push(Event::Plan { area_count, live });
        SelectionPlan::default()
    }
}

impl Client for Fake {
    fn area_count(&mut self, _area_count: ClientRef) -> u32 { self.area_words.len() as u32 }
    fn area(&mut self, _areas: ClientRef, index: usize) -> u32 { self.area_words[index] }
    fn write_lens_target(&mut self, target: ClientRef, point: LensPoint, step: u16, distance: u32) {
        self.events.push(Event::LensTarget { target, point, step, distance });
    }
    fn frame(&mut self, _slot: ClientRef) -> Option<Frame> { self.frame }
    fn clear_frame_progress(&mut self, slot: ClientRef) { self.events.push(Event::FrameProgressCleared(slot)); }
    fn announce_frame(&mut self, frame: Frame, header: FrameHeader) { self.events.push(Event::Announced(frame, header)); }
    fn write_summary(&mut self, frame: Frame, summary: &FrameSummary) { self.events.push(Event::Summary(frame, *summary)); }
}

impl Diagnostics for Fake {
    fn trace(&mut self, trace: Trace) { self.events.push(Event::Trace(trace)); }
    fn dump(&mut self, dump: Dump, frame: Frame, argument: u32) { self.events.push(Event::Dump(dump, frame, argument)); }
    fn calculation_context_mode(&mut self) -> u32 { self.calculation_mode }
    fn dump_calculation(&mut self) { self.events.push(Event::DumpCalculation); }
}

impl Platform for Fake {
    fn hold(&mut self, _hold: Hold, held: bool) { self.events.push(Event::Hold(held)); }
    fn adjustment(&mut self, adjustment: Adjustment) -> u8 { self.adjustments.get(&adjustment_key(adjustment)).copied().unwrap_or(0) }
    fn level(&mut self) -> u8 { self.level }
    fn tick(&mut self) -> u32 { self.tick }
    fn reset_image_progress(&mut self) { self.events.push(Event::ResetImageProgress); }
}

impl SubjectServices for Fake {
    fn lens_distance(&mut self, lens_reference: LensReference) -> u32 { lens_reference.0 * 2 }
    fn primary_detect(&mut self, limit: u32, _query: &SubjectQuery) -> PrimaryDetection {
        self.events.push(Event::Detect("primary", limit));
        self.detection
    }
    fn primary_select(&mut self, limit: u32, _query: &SubjectQuery) -> SubjectList {
        self.events.push(Event::Detect("primary select", limit));
        self.next_list()
    }
    fn primary_regions(&mut self, _query: &SubjectQuery) -> SubjectList {
        self.events.push(Event::Detect("primary regions", 0));
        self.next_list()
    }
    fn secondary_detect(&mut self, selection: Selection, _query: &SubjectQuery, list: &mut SubjectList, _summary: &mut Summary) {
        self.events.push(Event::Detect("secondary", selection.limit));
        *list = self.next_list();
    }
    fn secondary_select(&mut self, selection: Selection, _query: &SubjectQuery, list: &mut SubjectList) {
        self.events.push(Event::Detect("secondary select", selection.kind));
        *list = self.next_list();
    }
    fn secondary_regions(&mut self, _query: &SubjectQuery, list: &mut SubjectList) {
        self.events.push(Event::Detect("secondary regions", 0));
        *list = self.next_list();
    }
    fn refine(&mut self, _query: &SubjectQuery, _list: &mut SubjectList) { self.events.push(Event::Detect("refine", 0)); }
    fn select(&mut self, kind: u32, option: u32) -> bool {
        self.events.push(Event::Select(kind, option));
        self.selection_starts
    }
    fn close_selection(&mut self) { self.events.push(Event::CloseSelection); }
}
