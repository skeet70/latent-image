use crate::boundary::{Adjustment, LensReference, PublishedList, Setting};
use crate::leaf_fakes::{Event, Fake, Published};
use crate::ports::HalfPressMode;
use crate::subjects::{await_selection, Candidates, PrimaryDetection, Subject, SubjectId, SubjectList, Summary};
use std::vec::Vec;

fn subject(id: u32, tag: u8) -> Subject { Subject { id: SubjectId(id), detail: [tag; 100] } }

fn list(subjects: &[Subject]) -> SubjectList {
    let mut list = SubjectList::default();
    for (entry, subject) in list.entries.iter_mut().zip(subjects) { *entry = *subject; }
    list.count = subjects.len() as u32;
    list
}

fn ids(list: &SubjectList) -> Vec<u32> { list.entries[..list.count as usize].iter().map(|subject| subject.id.0).collect() }

#[test]
fn common_subjects_keep_the_first_lists_order_and_each_lists_own_entry() {
    let mut candidates = Candidates::default();
    candidates.lists[0] = list(&[subject(3, 0), subject(1, 0), subject(2, 0)]);
    candidates.lists[1] = list(&[subject(2, 1), subject(3, 1), subject(9, 1)]);
    candidates.lists[2] = list(&[subject(3, 2), subject(2, 2), subject(1, 2)]);
    candidates.lists[3] = list(&[subject(5, 3)]);
    candidates.keep_common(3);
    for (index, kept) in candidates.lists[..3].iter().enumerate() {
        assert_eq!(ids(kept), [3, 2]);
        assert!(kept.entries[..2].iter().all(|subject| subject.detail[0] == index as u8));
        assert_eq!(kept.entries[2], Subject::default());
    }
    assert_eq!(ids(&candidates.lists[3]), [5]);
}

#[test]
fn a_subject_absent_from_one_list_is_dropped_everywhere() {
    let mut candidates = Candidates::default();
    for index in 0..4 { candidates.lists[index] = list(&[subject(1, 0), subject(2, 0)]); }
    candidates.lists[3] = list(&[subject(2, 0)]);
    candidates.keep_common(4);
    assert!(candidates.lists.iter().all(|kept| ids(kept) == [2]));
}

#[test]
fn a_single_list_has_no_common_subjects() {
    let mut candidates = Candidates::default();
    candidates.lists[0] = list(&[subject(1, 0)]);
    candidates.keep_common(1);
    assert_eq!(candidates.lists[0].count, 0);
}

#[test]
fn a_reset_without_refresh_clears_publications_and_counts() {
    let mut fake = Fake::new();
    let mut candidates = Candidates::default();
    candidates.lists[2] = list(&[subject(4, 4)]);
    candidates.reset(false, LensReference(1), &mut fake);
    assert_eq!(candidates.lists[2].count, 0);
    assert_eq!(candidates.lists[2].entries[0].id, SubjectId(4));
    let published = fake.published();
    assert_eq!(published.len(), 10 * 2 + 1 + 4 + 2);
    assert!(published.contains(&Published::SubjectCount { list: PublishedList::Selected, count: 0 }));
    assert!(!fake.events.iter().any(|event| matches!(event, Event::Detect(..))));
}

#[test]
fn the_primary_source_publishes_its_regions_list() {
    let mut fake = Fake::new();
    fake.set(Setting::PrimarySubjects, 1);
    fake.tick = 99;
    fake.detection = PrimaryDetection { found: true, source: 7, list: list(&[subject(1, 0)]), summary: Summary([3; 15]) };
    fake.lists = std::vec![list(&[subject(2, 0)]), list(&[subject(5, 5), subject(6, 6)])];
    let mut candidates = Candidates::default();
    candidates.reset(true, LensReference(1), &mut fake);
    assert!(candidates.found);
    assert_eq!(candidates.source, 7);
    assert_eq!(candidates.summary, Summary([3; 15]));
    assert_eq!(ids(&candidates.lists[2]), [5, 6]);
    let published = fake.published();
    assert!(published.contains(&Published::SubjectTime(99)));
    assert!(published.contains(&Published::SubjectCount { list: PublishedList::Tracked, count: 0 }));
    assert_eq!(published.last(), Some(&Published::Subject { list: PublishedList::Selected, index: 9, subject: Subject::default() }));
    assert!(published.contains(&Published::Subject { list: PublishedList::Selected, index: 1, subject: subject(6, 6) }));
}

#[test]
fn a_primary_source_without_subjects_clears_the_lists() {
    let mut fake = Fake::new();
    fake.set(Setting::PrimarySubjects, 1);
    fake.detection = PrimaryDetection { found: false, source: 2, ..PrimaryDetection::default() };
    let mut candidates = Candidates::default();
    candidates.lists[1] = list(&[subject(1, 0)]);
    candidates.reset(true, LensReference(1), &mut fake);
    assert!(!candidates.found);
    assert_eq!(candidates.source, 2);
    assert!(candidates.lists.iter().all(|kept| kept.count == 0));
    assert!(!fake.has(&Event::Detect("primary select", 30)));
}

#[test]
fn the_secondary_source_publishes_the_common_subjects() {
    for (wide, kind) in [(0, 0), (1, 7)] {
        let mut fake = Fake::new();
        fake.set(Setting::SecondarySubjects, 1);
        fake.set_adjustment(Adjustment::TrackedSubjectsWide, wide);
        fake.lists = std::vec![
            list(&[subject(1, 0), subject(2, 0)]),
            list(&[subject(2, 1)]),
            list(&[subject(2, 2), subject(1, 2)]),
            list(&[subject(8, 8)]),
        ];
        let mut candidates = Candidates::default();
        candidates.reset(true, LensReference(1), &mut fake);
        assert!(fake.has(&Event::Detect("secondary select", kind)));
        assert_eq!(ids(&candidates.lists[2]), [2]);
        assert_eq!(ids(&candidates.tracked), [8]);
        assert!(fake.published().contains(&Published::SubjectCount { list: PublishedList::Selected, count: 1 }));
    }
}

#[test]
fn only_modes_0_and_5_wait_for_a_selection() {
    for mode in 0..9 {
        let mut fake = Fake::new();
        await_selection(HalfPressMode(mode), &mut fake);
        assert_eq!(fake.has(&Event::Select(0, 0)), matches!(mode, 0 | 5 | 8), "mode {mode}");
    }
}

#[test]
fn a_selection_is_closed_only_when_it_started() {
    for starts in [true, false] {
        let mut fake = Fake::new();
        fake.selection_starts = starts;
        fake.set(Setting::SelectionKind, 2);
        fake.set(Setting::SelectionOption, 3);
        await_selection(HalfPressMode(0), &mut fake);
        assert!(fake.has(&Event::Select(2, 3)));
        assert_eq!(fake.has(&Event::CloseSelection), starts);
    }
}

#[test]
fn context_mode_1_and_an_unconfirmed_secondary_style_skip_the_wait() {
    let mut fake = Fake::new();
    fake.set(Setting::ContextMode, 1);
    await_selection(HalfPressMode(0), &mut fake);
    assert!(fake.events.is_empty());
    let mut fake = Fake::new();
    fake.set(Setting::SecondarySubjects, 1);
    fake.set(Setting::SelectionStyle, 3);
    await_selection(HalfPressMode(0), &mut fake);
    assert!(fake.events.is_empty());
    fake.set(Setting::SelectionConfirmed, 1);
    await_selection(HalfPressMode(0), &mut fake);
    assert!(fake.has(&Event::Select(0, 0)));
}
