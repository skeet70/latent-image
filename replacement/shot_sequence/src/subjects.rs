#![forbid(unsafe_code)]

//! Subject candidates for a half press: the lists the subject services fill, the subjects common
//! to them, and what the bound record publishes of them.

use crate::boundary::{Adjustment, LensReference, Platform, Publication, PublishedList, Records, Setting, SummaryWord};
use crate::ports::HalfPressMode;

pub const CAPACITY: usize = 10;
pub const DETAIL_BYTES: usize = 100;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SubjectId(pub u32);

/// One candidate as a subject service reports it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Subject {
    pub id: SubjectId,
    pub detail: [u8; DETAIL_BYTES],
}

impl Default for Subject {
    fn default() -> Self { Self { id: SubjectId(0), detail: [0; DETAIL_BYTES] } }
}

/// A candidate list. Entries past `count` keep what was last written there and are published.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SubjectList {
    pub count: u32,
    pub entries: [Subject; CAPACITY],
}

impl SubjectList {
    fn listed(&self) -> &[Subject] { &self.entries[..(self.count as usize).min(CAPACITY)] }

    fn contains(&self, id: SubjectId) -> bool { self.listed().iter().any(|subject| subject.id == id) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SubjectQuery {
    pub lens: u32,
    pub first: u32,
    pub second: u32,
    pub area_layout: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Selection { pub kind: u32, pub limit: u32 }

/// Words the first detection reports beside its list; the exposure takes the first ten.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Summary(pub [u32; 15]);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PrimaryDetection {
    pub found: bool,
    pub source: u32,
    pub list: SubjectList,
    pub summary: Summary,
}

/// The subject services, outside saclac.
pub trait SubjectServices {
    fn lens_distance(&mut self, lens_reference: LensReference) -> u32;
    fn primary_detect(&mut self, limit: u32, query: &SubjectQuery) -> PrimaryDetection;
    fn primary_select(&mut self, limit: u32, query: &SubjectQuery) -> SubjectList;
    fn primary_regions(&mut self, query: &SubjectQuery) -> SubjectList;
    fn secondary_detect(&mut self, selection: Selection, query: &SubjectQuery, list: &mut SubjectList, summary: &mut Summary);
    fn secondary_select(&mut self, selection: Selection, query: &SubjectQuery, list: &mut SubjectList);
    fn secondary_regions(&mut self, query: &SubjectQuery, list: &mut SubjectList);
    fn refine(&mut self, query: &SubjectQuery, list: &mut SubjectList);
    /// Starts a subject selection and holds task 43 until the service answers; false when the
    /// service refused to start.
    fn select(&mut self, kind: u32, option: u32) -> bool;
    fn close_selection(&mut self);
}

/// The candidate lists of the shot context; the capture alone fills the fourth.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Candidates {
    pub lists: [SubjectList; 4],
    pub tracked: SubjectList,
    pub summary: Summary,
    pub source: u32,
    pub found: bool,
}

pub trait Boundary: Records + Platform + SubjectServices {}
impl<T: Records + Platform + SubjectServices> Boundary for T {}

impl Candidates {
    /// Clears what the record publishes of subjects and, when `refresh`, asks the subject source
    /// the record names for new candidates.
    pub fn reset(&mut self, refresh: bool, lens_reference: LensReference, boundary: &mut impl Boundary) {
        let empty = Subject::default();
        self.found = false;
        self.source = 1;
        for index in 0..CAPACITY {
            boundary.publish(Publication::Subject { list: PublishedList::Selected, index, subject: &empty });
            boundary.publish(Publication::SubjectFlag { index, value: 0 });
        }
        boundary.publish(Publication::SubjectCount { list: PublishedList::Selected, count: 0 });
        for word in [SummaryWord::First, SummaryWord::Second, SummaryWord::Third, SummaryWord::Count] {
            boundary.publish(Publication::SubjectSummary { word, value: 0 });
        }
        boundary.publish(Publication::SelectedSubject(&empty));
        boundary.publish(Publication::TrackedMarker(0));
        if !refresh { return self.clear(); }
        if boundary.setting(Setting::PrimarySubjects) != 0 {
            self.primary(lens_reference, boundary);
        } else if boundary.setting(Setting::SecondarySubjects) != 0 {
            self.secondary(lens_reference, boundary);
        } else {
            self.clear();
        }
    }

    fn clear(&mut self) {
        for list in &mut self.lists { list.count = 0; }
        self.tracked.count = 0;
    }

    fn query(lens_reference: LensReference, boundary: &mut impl Boundary) -> SubjectQuery {
        SubjectQuery {
            lens: boundary.lens_distance(lens_reference),
            first: boundary.setting(Setting::SubjectQueryA),
            second: boundary.setting(Setting::SubjectQueryB),
            area_layout: boundary.setting(Setting::AreaLayout),
        }
    }

    fn primary(&mut self, lens_reference: LensReference, boundary: &mut impl Boundary) {
        let query = Self::query(lens_reference, boundary);
        let detection = boundary.primary_detect(24, &query);
        self.summary = detection.summary;
        self.source = detection.source;
        if !detection.found { return self.clear(); }
        self.lists[0] = detection.list;
        self.found = true;
        self.lists[1] = boundary.primary_select(30, &query);
        self.lists[2] = boundary.primary_regions(&query);
        boundary.refine(&query, &mut self.lists[2]);
        self.tracked.count = 0;
        self.publish_tracked(boundary);
        self.publish_selected(boundary);
    }

    fn secondary(&mut self, lens_reference: LensReference, boundary: &mut impl Boundary) {
        let query = Self::query(lens_reference, boundary);
        boundary.secondary_detect(Selection { kind: 0, limit: 24 }, &query, &mut self.lists[0], &mut self.summary);
        boundary.secondary_select(Selection { kind: 1, limit: 30 }, &query, &mut self.lists[1]);
        boundary.secondary_regions(&query, &mut self.lists[2]);
        boundary.refine(&query, &mut self.lists[2]);
        let kind = if boundary.adjustment(Adjustment::TrackedSubjectsWide) == 0 { 0 } else { 7 };
        boundary.secondary_select(Selection { kind, limit: 8 }, &query, &mut self.tracked);
        self.publish_tracked(boundary);
        self.keep_common(3);
        self.publish_selected(boundary);
    }

    fn publish_tracked(&self, boundary: &mut impl Boundary) {
        let time = boundary.tick();
        boundary.publish(Publication::SubjectCount { list: PublishedList::Tracked, count: self.tracked.count });
        boundary.publish(Publication::SubjectTime(time));
        for (index, subject) in self.tracked.entries.iter().enumerate() {
            boundary.publish(Publication::Subject { list: PublishedList::Tracked, index, subject });
        }
    }

    fn publish_selected(&self, boundary: &mut impl Boundary) {
        let selected = &self.lists[2];
        boundary.publish(Publication::SubjectCount { list: PublishedList::Selected, count: selected.count });
        for (index, subject) in selected.entries.iter().enumerate() {
            boundary.publish(Publication::Subject { list: PublishedList::Selected, index, subject });
        }
    }

    /// Keeps, in each of the first `lists` lists, the subjects present in all of them, in the order
    /// of the first list, each list keeping its own entry. Fewer than two lists keep nothing.
    pub fn keep_common(&mut self, lists: usize) {
        let lists = &mut self.lists[..lists];
        let mut common = [SubjectId::default(); CAPACITY];
        let mut count = 0;
        if let Some((first, others)) = lists.split_first() {
            if !others.is_empty() {
                for subject in first.listed() {
                    if others.iter().all(|list| list.contains(subject.id)) {
                        common[count] = subject.id;
                        count += 1;
                    }
                }
            }
        }
        for list in lists.iter_mut() {
            let mut kept = SubjectList::default();
            for id in &common[..count] {
                if let Some(subject) = list.listed().iter().find(|subject| subject.id == *id) {
                    kept.entries[kept.count as usize] = *subject;
                    kept.count += 1;
                }
            }
            *list = kept;
        }
    }
}

/// Modes 1..=4, 6 and 7 do not wait for a subject selection.
pub fn await_selection(mode: HalfPressMode, boundary: &mut impl Boundary) {
    if mode.0 <= 7 && (0b1101_1110 >> mode.0) & 1 != 0 { return; }
    if boundary.setting(Setting::SecondarySubjects) == 1
        && (boundary.setting(Setting::SelectionStyle) != 3 || boundary.setting(Setting::SelectionConfirmed) != 1)
    {
        return;
    }
    if boundary.setting(Setting::ContextMode) == 1 { return; }
    let kind = boundary.setting(Setting::SelectionKind);
    let option = boundary.setting(Setting::SelectionOption);
    if boundary.select(kind, option) { boundary.close_selection(); }
}
