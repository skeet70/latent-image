use crate::children::{AcquisitionControl, ChildEvent, SearchControl};
use crate::layers::{Block, Change, Field, LensMove, LensProbe, LensSlot, RecordQuery, RemapRequest, ResultIndex, Restarting};
use crate::live_profile::LiveProfile;
use crate::live_view::PreparationOverride;
use crate::replies::ReplyCommand as R;
use crate::root::Outcome;
use crate::s1::StatisticsEvent;
use crate::s1_fake::{
    record, Child, Fake, Seen, CHANNEL, DESCRIPTOR, LENS_POSITION, LENS_VALUE, MODE_INDEX, REMAPPED, STEP, TICKS,
};
use crate::s1_statistics::{Apply, AutofocusChange, Condition, Port, Reply, S1Statistics};
use crate::s1_statistics_leaf::{Leaf, S1Profile};
use crate::stock;
use std::{vec, vec::Vec};
use xm1_saclac_interface::{stock as interface, CommandKind as K, Completed, Completion as C, ImgDcOperation};

struct Rig {
    fake: Fake,
    profile: S1Profile,
    live: LiveProfile,
    preparation: PreparationOverride,
}

impl Rig {
    fn new(fake: Fake) -> Self { Self { fake, profile: S1Profile::new(), live: LiveProfile::new(), preparation: PreparationOverride::Set } }

    fn leaf(&mut self) -> Leaf<'_, Fake> {
        Leaf { layers: &mut self.fake, profile: &mut self.profile, live: &mut self.live, preparation: &mut self.preparation }
    }

    fn seen(&mut self) -> Vec<Seen> { self.fake.take_seen() }
}

const BASE: u32 = 0x50;

fn autofocus(fake: &mut Fake) { fake.parameters.push((K::Autofocus, (0..39).map(|index| if index == 38 { BASE } else { 0 }).collect())); }

fn with_blocks(mut fake: Fake, blocks: &[(Block, &[u32])]) -> Fake {
    fake.blocks.extend(blocks.iter().map(|(block, words)| (*block, words.to_vec())));
    fake
}

#[test]
fn the_begin_brings_up_both_children_the_record_mode_and_the_s1_imaging_job() {
    let mut rig = Rig::new(Fake { record_mode_available: true, ..Fake::default() });
    let mut statistics = S1Statistics::new();
    assert_eq!(statistics.send(StatisticsEvent::Begin, record(), &mut rig.leaf()), Outcome::Pending);
    let seen = rig.seen();
    assert_eq!(
        seen[..3],
        [Seen::Child(Child::Acquisition("initialize")), Seen::Child(Child::Search("initialize")), Seen::RecordModeBegun],
    );
    let Some(Seen::Imaging(ImgDcOperation::Request42, _, completions)) = seen.last() else { panic!("{seen:?}") };
    assert_eq!(completions, &[C::Image57, C::Image58, C::Image59]);
}

#[test]
fn children_take_their_controls_and_the_completions_forwarded_to_them() {
    let mut rig = Rig::new(Fake::default());
    let parameters = interface::parameters(&[]);
    rig.leaf().acquisition(ChildEvent::Control { control: AcquisitionControl::Stop, parameters, record: record() });
    rig.leaf().search(ChildEvent::Control { control: SearchControl::Conclude, parameters, record: record() });
    let completed = Completed { completion: C::Calculation9d, parameters };
    rig.leaf().search(ChildEvent::Completed { completed, record: record() });
    assert_eq!(
        rig.seen(),
        [Seen::Child(Child::Acquisition("stop")), Seen::Child(Child::Search("conclude")), Seen::Child(Child::Completed(C::Calculation9d))],
    );
}

#[test]
fn the_record_mode_is_held_and_released_only_while_its_channel_is_available() {
    let mut rig = Rig::new(Fake::default());
    rig.leaf().record_mode(true);
    rig.leaf().record_mode(false);
    assert!(rig.seen().is_empty());
    rig.fake.record_mode_available = true;
    rig.leaf().record_mode(true);
    rig.leaf().record_mode(false);
    assert_eq!(rig.seen(), [Seen::RecordModeBegun, Seen::RecordModeFinished]);
}

#[test]
fn profile_replies_carry_the_descriptor_the_lens_value_and_the_probe_flags() {
    let mut rig = Rig::new(Fake { overridden: vec![LensProbe::First, LensProbe::Third], ..Fake::with(&[(Field::Descriptor, 0x44)]) });
    rig.leaf().reply(Reply::Command02, record());
    rig.fake.overridden = vec![LensProbe::Second];
    rig.leaf().reply(Reply::Command03, record());
    rig.leaf().reply(Reply::Command1d, record());
    rig.leaf().reply(Reply::Autofocus, record());
    assert_eq!(
        rig.seen(),
        [
            Seen::Reply(R::Command02, 0, vec![0x44, LENS_VALUE, 0, 2]),
            Seen::Reply(R::Command03, 0, vec![0x44, LENS_VALUE, 0, 1]),
            Seen::Reply(R::Command1d, 0, vec![]),
            Seen::LensReply,
        ],
    );
}

fn command17(fake: &mut Fake, mask: u32, values: u32) {
    fake.parameters.push((K::Command17, vec![0xc17, mask, values, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99]));
}

const FOCUS_BLOCKS: [(Block, &[u32]); 4] = [
    (Block::Release, &[0x40; 33]),
    (Block::FocusConfirmation, &[0x80; 8]),
    (Block::MeasuredWindow, &[0x50; 5]),
    (Block::FocusHistory, &[0x20; 20]),
];

fn cleared(block: Block, length: usize, index: usize, fill: u32) -> Seen {
    let mut words = vec![fill; length];
    words[index] = 0;
    Seen::Block(block, words)
}

#[test]
fn command17_sets_its_preparation_bits_applies_the_live_selection_and_clears_the_unselected_focus_blocks() {
    let mut fake = with_blocks(Fake::default(), &FOCUS_BLOCKS);
    command17(&mut fake, 0b1101, 0b0001);
    let mut rig = Rig::new(fake);
    rig.leaf().apply(Apply::Command17, record());
    assert_eq!(
        rig.seen(),
        [
            Seen::Write(Field::PreparationFirst, 1),
            Seen::Write(Field::PreparationSecond, 1),
            Seen::Write(Field::PendingOutput, 0),
            Seen::Write(Field::Command17First, 0x33),
            Seen::Write(Field::Command17Third, 0),
            Seen::Write(Field::Command17Fourth, 0),
            Seen::Write(Field::Command17Fifth, 0x88),
            Seen::Write(Field::Command17Sixth, 0x99),
            Seen::Write(Field::Command17Selection, 1),
            cleared(Block::FocusConfirmation, 8, 0, 0x80),
            cleared(Block::MeasuredWindow, 5, 3, 0x50),
            Seen::Reply(R::Command17, 0, vec![]),
        ],
    );
    assert_ne!(rig.live, LiveProfile::new(), "the live view keeps the selection it was handed");
}

#[test]
fn a_refused_command17_is_answered_with_minus_96_then_applied_and_answered_again() {
    let mut fake = with_blocks(Fake { refuses_selection: true, ..Fake::default() }, &FOCUS_BLOCKS);
    command17(&mut fake, 0, 0);
    let mut rig = Rig::new(fake);
    rig.leaf().apply(Apply::Command17, record());
    let seen = rig.seen();
    assert_eq!(seen[0], Seen::Reply(R::Command17, -96, vec![]));
    assert_eq!(
        seen[seen.len() - 5..],
        [
            cleared(Block::Release, 33, 1, 0x40),
            cleared(Block::FocusConfirmation, 8, 0, 0x80),
            cleared(Block::MeasuredWindow, 5, 3, 0x50),
            cleared(Block::FocusHistory, 20, 0, 0x20),
            Seen::Reply(R::Command17, 0, vec![]),
        ],
    );
}

#[test]
fn prepare_applies_its_enables_sets_the_shared_override_and_answers_the_flags() {
    let mut fake = Fake::default();
    fake.parameters.push((K::Prepare, vec![0xc1a, 1, 1, 0x33, 0x44]));
    let mut rig = Rig::new(fake);
    rig.leaf().apply(Apply::Prepare, record());
    assert_eq!(
        rig.seen(),
        [
            Seen::Write(Field::SelectionEnables, 1),
            Seen::Write(Field::SelectionFlags, 1),
            Seen::Write(Field::PrepareFirst, 0x33),
            Seen::Write(Field::PrepareSecond, 0x44),
            Seen::Reply(R::Prepare, 0, vec![1]),
        ],
    );
    assert_eq!(rig.preparation, PreparationOverride::Pending);
    rig.fake.parameters = vec![(K::Prepare, vec![0xc1a, 1, 0, 0, 0])];
    rig.leaf().apply(Apply::Prepare, record());
    assert_eq!(rig.seen().last(), Some(&Seen::Reply(R::Prepare, 0, vec![0])));
    assert_eq!(rig.preparation, PreparationOverride::None);
}

#[test]
fn command1d_keeps_its_two_words() {
    let mut fake = Fake::default();
    fake.parameters.push((K::Command1d, vec![0xc1d, 0x11, 0x22]));
    let mut rig = Rig::new(fake);
    rig.leaf().apply(Apply::Command1d, record());
    assert_eq!(rig.seen(), [Seen::Write(Field::Command1dFirst, 0x11), Seen::Write(Field::Command1dSecond, 0x22)]);
}

#[test]
fn the_resume_clears_the_last_option_and_puts_the_lens_target_at_the_lens() {
    let fake = with_blocks(Fake::default(), &[(Block::Options, &[1, 2, 3, 4, 5, 6, 7]), (Block::LensTarget, &[0x0005_0009])]);
    let mut rig = Rig::new(fake);
    rig.leaf().apply(Apply::Resume, record());
    assert_eq!(
        rig.seen(),
        [Seen::Block(Block::Options, vec![1, 2, 3, 4, 5, 6, 0]), Seen::Block(Block::LensTarget, vec![0x0005_0000 | LENS_POSITION])],
    );
}

fn result_records() -> Vec<u8> {
    let mut bytes = 3u32.to_le_bytes().to_vec();
    bytes.extend((0..1040).map(|index| (index / 4 % 251) as u8));
    bytes
}

#[test]
fn the_results_land_in_the_profile_and_reach_its_callback_only_while_a_selection_is_pending() {
    let fields = [
        (Field::ResultsCallback, 0xf00d),
        (Field::SelectionPending, 1),
        (Field::DescriptorFirst, 0xa),
        (Field::DescriptorSecond, 0xb),
        (Field::FormatRatio, 0x0003_0004),
    ];
    let mut rig = Rig::new(Fake { extended: true, records: result_records(), ..Fake::with(&fields) });
    rig.leaf().apply(Apply::Results, record());
    let seen = rig.seen();
    let query = RecordQuery { bank: 7, mode: 8, descriptor: DESCRIPTOR, pair: [0xa, 0xb], ratio: 0x0003_0004 };
    assert_eq!(seen[..4], [Seen::Write(Field::ResultsTaken, 1), Seen::Query(query), Seen::Write(Field::ResultsCount, 3), Seen::Write(Field::ResultsTime, TICKS)]);
    let records: Vec<Seen> = (0..10u8)
        .map(|index| {
            let words = (0..26).map(|offset| u32::from_le_bytes([((index as u32 * 26 + offset) % 251) as u8; 4])).collect();
            Seen::Block(Block::ResultRecord(ResultIndex::new(index).unwrap()), words)
        })
        .collect();
    assert_eq!(seen[4..14], records[..]);
    assert_eq!(seen[14..], [Seen::Delivered(0xf00d)]);

    rig.fake.extended = false;
    rig.fake.fields.push((Field::SelectionPending, 0));
    rig.leaf().apply(Apply::Results, record());
    let seen = rig.seen();
    assert_eq!(seen[1], Seen::Query(RecordQuery { bank: 0, ..query }));
    assert!(!seen.iter().any(|seen| matches!(seen, Seen::Delivered(_))));
}

#[test]
fn the_output_block_is_needed_for_a_measuring_view_mode_or_an_output_word() {
    for (mode, output, needed) in [(0, 0, false), (3, 0, false), (1, 0, true), (2, 0, true), (0, 5, true), (3, 5, true)] {
        let mut rig = Rig::new(Fake::with(&[(Field::ViewMode, mode), (Field::OutputFirst, output)]));
        assert_eq!(rig.leaf().condition(Condition::OutputNeeded, record()), needed, "mode {mode}, output {output}");
    }
    let mut rig = Rig::new(Fake::default());
    assert!(!rig.leaf().condition(Condition::RestartRequired, record()));
    rig.fake.changes.push(Change::Restarting(Restarting::Fourth));
    assert!(rig.leaf().condition(Condition::RestartRequired, record()));
}

fn lens_fake(fields: &[(Field, u32)], changes: &[Change], blocks: &[(Block, &[u32])]) -> Rig {
    let mut fake = with_blocks(Fake { changes: changes.to_vec(), ..Fake::with(fields) }, blocks);
    autofocus(&mut fake);
    Rig::new(fake)
}

#[test]
fn a_selection_keeps_the_measurement_and_restarts_the_next_pair_from_the_autofocus_base() {
    let tracked = [(Field::Selection, 1), (Field::SelectionTracked, 1)];
    let pairs: [(Block, &[u32]); 2] = [(Block::Lens(LensSlot::Next), &[7, 9]), (Block::Lens(LensSlot::Reported), &[1, 9])];
    let mut rig = lens_fake(&tracked, &[Change::Retune, Change::LensRange], &pairs);
    assert_eq!(rig.leaf().autofocus(record()), AutofocusChange::None);
    assert_eq!(rig.seen(), [Seen::Block(Block::Lens(LensSlot::Next), vec![BASE, 9])]);

    let moved: [(Block, &[u32]); 2] = [(Block::Lens(LensSlot::Next), &[7, 9]), (Block::Lens(LensSlot::Reported), &[1, 8])];
    let mut rig = lens_fake(&tracked, &[], &moved);
    assert_eq!(rig.leaf().autofocus(record()), AutofocusChange::None);
    assert_eq!(rig.seen().last(), Some(&Seen::SelectionChanged), "the reported pair maps elsewhere");

    let mut rig = lens_fake(&[(Field::Selection, 1)], &[], &pairs);
    rig.leaf().autofocus(record());
    assert_eq!(rig.seen().last(), Some(&Seen::SelectionChanged), "the selection record tracks nothing");
}

#[test]
fn a_capture_change_retunes() {
    let pairs: [(Block, &[u32]); 2] = [(Block::Lens(LensSlot::Next), &[7, 9]), (Block::Lens(LensSlot::Reported), &[1, 9])];
    let mut rig = lens_fake(&[(Field::SelectionTracked, 1)], &[Change::Retune, Change::LensRange], &pairs);
    assert_eq!(rig.leaf().autofocus(record()), AutofocusChange::Retune);
    assert_eq!(rig.seen(), [Seen::Block(Block::Lens(LensSlot::Next), vec![BASE, 9])]);
}

#[test]
fn a_lens_range_change_takes_the_pair_the_range_gives() {
    let current: [(Block, &[u32]); 1] = [(Block::Lens(LensSlot::Current), &[1, 2])];
    let mut rig = lens_fake(&[(Field::LensRange, 0x10)], &[Change::LensRange], &current);
    assert_eq!(rig.leaf().autofocus(record()), AutofocusChange::Lens);
    let pair = vec![BASE, BASE + 0x10 + BASE];
    assert_eq!(
        rig.seen(),
        [
            Seen::Block(Block::Lens(LensSlot::Current), pair.clone()),
            Seen::Block(Block::Lens(LensSlot::Next), pair),
            Seen::SelectionChanged,
        ],
    );

    let range = 1 + 2 + BASE;
    let held = BASE + range + BASE;
    let unchanged: [(Block, &[u32]); 2] = [(Block::Lens(LensSlot::Current), &[1, 2]), (Block::Lens(LensSlot::Next), &[0, held])];
    let mut rig = lens_fake(&[(Field::LensRange, range), (Field::SelectionTracked, 1)], &[Change::LensRange], &unchanged);
    assert_eq!(rig.leaf().autofocus(record()), AutofocusChange::Lens, "a range change asks for the lens even when the pair holds");
    assert_eq!(rig.seen(), [Seen::Block(Block::Lens(LensSlot::Next), vec![BASE, held])]);
}

#[test]
fn the_bounds_notice_reports_the_curve_positions_the_target_steps_reach() {
    let target: [(Block, &[u32]); 2] = [(Block::LensTarget, &[0x0004_0000]), (Block::Lens(LensSlot::Current), &[0, 0])];
    let bounds = |setup: fn(&mut Fake)| {
        let mut rig = lens_fake(&[(Field::LensRange, 0x10)], &[Change::LensRange], &target);
        setup(&mut rig.fake);
        rig.leaf().autofocus(record());
        rig.seen().into_iter().find(|seen| matches!(seen, Seen::Bounds(..)))
    };
    assert_eq!(bounds(|_| {}), Some(Seen::Bounds(1, [4, BASE, 0x10])));
    assert_eq!(bounds(|fake| fake.interpolates = true), Some(Seen::Bounds(1, [4, CHANNEL.0, STEP])));
    assert_eq!(bounds(|fake| fake.fields.push((Field::Command1dRestarts, 1))), Some(Seen::Bounds(1, [4, CHANNEL.0, STEP])));
    assert_eq!(bounds(|fake| {
        fake.interpolates = true;
        fake.table_disabled = true;
    }), Some(Seen::Bounds(1, [u32::MAX; 3])));
    assert_eq!(bounds(|fake| fake.overridden.push(LensProbe::Second)), Some(Seen::Bounds(1, [u32::MAX; 3])));
    assert_eq!(bounds(|fake| fake.overridden.push(LensProbe::First)), Some(Seen::Bounds(1, [u32::MAX; 3])));
    assert_eq!(bounds(|fake| fake.blocks.push((Block::LensTarget, vec![0x0000_0004]))), None, "a target without steps notices nothing");
}

#[test]
fn a_lens_window_change_takes_the_searched_pair_when_the_search_moved_it() {
    let next: [(Block, &[u32]); 2] = [(Block::Lens(LensSlot::Next), &[3, 4]), (Block::Lens(LensSlot::Reported), &[0, 4])];
    let mut rig = lens_fake(&[(Field::SelectionTracked, 1)], &[Change::LensWindow], &next);
    rig.fake.searched = 11;
    assert_eq!(rig.leaf().autofocus(record()), AutofocusChange::Lens);
    assert_eq!(
        rig.seen(),
        [
            Seen::Block(Block::Lens(LensSlot::Current), vec![BASE, 11]),
            Seen::Block(Block::Lens(LensSlot::Next), vec![BASE, 11]),
            Seen::SelectionChanged,
        ],
    );

    let mut rig = lens_fake(&[(Field::SelectionTracked, 1)], &[Change::LensValue], &next);
    rig.fake.searched = 3 + 4 + 3;
    assert_eq!(rig.leaf().autofocus(record()), AutofocusChange::None);
    assert_eq!(rig.seen(), [Seen::SelectionChanged, Seen::Block(Block::Lens(LensSlot::Next), vec![BASE, 4])]);
}

fn move_fake(fields: &[(Field, u32)], release: u32) -> Rig {
    let mut release_range = vec![0; 12];
    release_range[8] = release;
    let blocks: [(Block, &[u32]); 3] =
        [(Block::LensTarget, &[0x0009_0150]), (Block::Lens(LensSlot::Next), &[0, 0x77]), (Block::ReleaseRange, &release_range)];
    Rig::new(with_blocks(Fake::with(fields), &blocks))
}

fn lens_move(steps: i16, settle: u32) -> Seen {
    Seen::Move(LensMove { callback: stock::S1_LENS_MOVED, steps, mode_index: MODE_INDEX, words: [1, 0, settle, 4] })
}

#[test]
fn the_lens_moves_by_its_target_less_its_position() {
    let mut rig = move_fake(&[], 0);
    assert!(!rig.leaf().move_lens(record()));
    assert_eq!(rig.seen(), [lens_move(0x50, 11)]);
    let mut rig = move_fake(&[(Field::Command1dRestarts, 2)], 0);
    assert!(!rig.leaf().move_lens(record()));
    assert_eq!(rig.seen(), [lens_move(0x50, 0)]);
}

#[test]
fn the_unselected_mode_without_a_lens_designation_needs_no_move() {
    let mut rig = move_fake(&[], 0);
    rig.fake.unselected = true;
    assert!(rig.leaf().move_lens(record()));
    assert!(rig.seen().is_empty());
    rig.fake.fields.push((Field::LensDesignation, 1));
    assert!(!rig.leaf().move_lens(record()));
    assert_eq!(rig.seen(), [lens_move(0x50, 11)]);
}

#[test]
fn the_release_range_remaps_the_target_before_the_move() {
    let mut rig = move_fake(&[(Field::LensReference, 0x720), (Field::LensConversion, 1)], 1);
    assert!(!rig.leaf().move_lens(record()));
    let remap = RemapRequest { position: 0x150, reference: 0x720, target: 0x77, convert: true, mode_index: MODE_INDEX };
    assert_eq!(rig.seen(), [Seen::Remap(remap), lens_move((u32::from(REMAPPED)).wrapping_sub(LENS_POSITION) as i16, 11)]);
}

#[test]
fn live_correction_rebases_an_equal_target_without_starting_a_move() {
    let mut fake = with_blocks(Fake::default(), &[
        (Block::Lens(LensSlot::Current), &[2, 10]),
        (Block::Lens(LensSlot::Next), &[99, 15]),
    ]);
    assert!(crate::s1_autofocus::correct_drive(record(), 3, interface::StockCallbackWord(0xca11), &mut fake));
    assert_eq!(fake.block_words(Block::Lens(LensSlot::Next)).unwrap(), &[3, 15]);
    let seen = fake.take_seen();
    assert_eq!(seen.iter().filter(|seen| **seen == Seen::SelectionChanged).count(), 1);
    assert!(!seen.iter().any(|seen| matches!(seen, Seen::Move(_))));
}

#[test]
fn live_correction_keeps_the_new_pair_but_does_not_move_an_undesignated_unselected_lens() {
    let mut fake = with_blocks(Fake { unselected: true, ..Fake::default() }, &[
        (Block::Lens(LensSlot::Current), &[2, 10]),
        (Block::Lens(LensSlot::Next), &[99, 11]),
    ]);
    assert!(crate::s1_autofocus::correct_drive(record(), 3, interface::StockCallbackWord(0xca11), &mut fake));
    assert_eq!(fake.block_words(Block::Lens(LensSlot::Next)).unwrap(), &[3, 15]);
    assert!(!fake.take_seen().iter().any(|seen| matches!(seen, Seen::Move(_) | Seen::Remap(_))));
}

#[test]
fn live_correction_remaps_the_signed_held_position_against_the_current_resource_before_moving() {
    for restarts in [0, 1] {
        let mut release = [0; 12];
        release[8] = 1;
        let mut fake = with_blocks(Fake::with(&[
            (Field::LensReference, 0x999),
            (Field::LensConversion, 1),
            (Field::Command1dRestarts, restarts),
        ]), &[
            (Block::Lens(LensSlot::Current), &[2, 10]),
            (Block::Lens(LensSlot::Next), &[99, 11]),
            (Block::LensTarget, &[0xabcd_ff80]),
            (Block::ReleaseRange, &release),
        ]);
        let callback = interface::StockCallbackWord(0xca11);
        assert!(!crate::s1_autofocus::correct_drive(record(), 3, callback, &mut fake));
        let seen = fake.take_seen();
        assert!(seen.contains(&Seen::Remap(RemapRequest { position: -128, reference: 3, target: 15, convert: true, mode_index: MODE_INDEX })));
        assert!(seen.contains(&Seen::Move(LensMove { callback, steps: (u32::from(REMAPPED) - LENS_POSITION) as i16, mode_index: MODE_INDEX, words: [1, 0, if restarts == 0 { 11 } else { 0 }, 4] })));
        assert_eq!(fake.block_words(Block::Lens(LensSlot::Next)).unwrap(), &[3, 15]);
    }
}
