use crate::layers::{Block, Field};
use crate::s1_fake::{record, Fake, Seen, DESCRIPTOR};
use crate::s1_statistics::Request;
use crate::s1_statistics_profile::{request, take, Taken};
use std::vec;
use xm1_saclac_interface::{Completion as C, ImgDcOperation, StatisticsOperation as Op};

fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) }

fn taken(fake: &mut Fake) -> Taken {
    let taken = take(record(), fake);
    fake.take_seen();
    taken
}

#[test]
fn the_imaging_job_carries_the_snapshot_with_its_step_and_classes_adjusted() {
    let mut fake = Fake::with(&[
        (Field::Selection, 1),
        (Field::SceneKind, 0x08),
        (Field::Step, 12),
        (Field::Level, 0xfffe),
        (Field::LensIndex, 0x98),
        (Field::StopMode, 3),
        (Field::ViewActive, 2),
        (Field::FormatRatio, 0x0280_01e0),
    ]);
    fake.blocks.push((Block::Classes, vec![9, 4]));
    fake.blocks.push((Block::AuxiliaryWindow, vec![1, 2, 0x20, 0x24]));
    let taken = taken(&mut fake);
    assert!(taken.view_active());
    request(Request::BeginImaging, &taken, record(), &mut fake);
    let seen = fake.take_seen();
    let [Seen::Imaging(operation, bytes, completions)] = seen.as_slice() else { panic!("{seen:?}") };
    assert_eq!((*operation, bytes.len(), completions.as_slice()), (ImgDcOperation::Request42, 136, &[C::Image57, C::Image58, C::Image59][..]));
    assert_eq!(&bytes[0..2], &[0xfe, 0xff]);
    assert_eq!((word(bytes, 8), word(bytes, 16), word(bytes, 20)), (0x98, 1, 20), "a step of 12 becomes 20 without the view byte");
    assert_eq!(word(bytes, 28), 0xc003);
    assert_eq!((word(bytes, 44), word(bytes, 56)), (DESCRIPTOR, DESCRIPTOR), "the descriptor's lens argument is unused (0xf4f16c, 0x752714)");
    assert_eq!((word(bytes, 68), word(bytes, 72), word(bytes, 84)), (0x20, 0x24, 2));
    assert_eq!((word(bytes, 96), word(bytes, 100), word(bytes, 104), word(bytes, 120)), (6, 4, 0x0280_01e0, 0x08));
    assert_eq!(&bytes[124..128], &0x00f2_27c0u32.to_le_bytes());
}

#[test]
fn the_snapshot_marks_the_results_and_resets_the_lens_target_and_release() {
    let mut fake = Fake::default();
    fake.blocks.push((Block::ReleaseRange, vec![0, 0, 0, 0, 0x0007, 0x10, 0, 0x20, 0, 0, 0, 0]));
    fake.blocks.push((Block::Release, (0..33).collect()));
    take(record(), &mut fake);
    let seen = fake.take_seen();
    assert_eq!(seen[0], Seen::Write(Field::ResultsTaken, 0));
    assert_eq!(seen[1], Seen::Block(Block::LensTarget, vec![0x0037_0007]));
    let Seen::Block(Block::Release, words) = &seen[2] else { panic!("{seen:?}") };
    assert_eq!((words[0], words[1], words[2], words[24], words[32]), (0, 0, 2, 0, 32));

    fake.blocks.push((Block::Options, vec![0, 2, 0, 0, 0, 0, 0]));
    take(record(), &mut fake);
    assert_eq!(fake.take_seen()[1], Seen::Block(Block::LensTarget, vec![0x0007]), "more than one option leaves no steps");
}

#[test]
fn an_unselected_s1_start_is_0x4004_with_the_live_header_and_the_view_flag() {
    let mut fake = Fake::with(&[(Field::ViewActive, 1), (Field::FormatMode, 3), (Field::CaptureOptionFirst, 1)]);
    fake.live = vec![(Field::StartHeader, 0x51), (Field::CaptureWindowMode, 1)];
    let taken = taken(&mut fake);
    request(Request::Start, &taken, record(), &mut fake);
    let (operation, bytes, completions) = fake.statistics();
    assert_eq!((operation, bytes.len(), completions), (Op::Request04, 208, vec![C::Statistics7b, C::Statistics7f]));
    assert_eq!(word(&bytes, 12), 1, "the S1 setup tail");
    assert_eq!((word(&bytes, 16), word(&bytes, 20), word(&bytes, 32), word(&bytes, 36)), (240, 640, 5, 0));
    assert_eq!((word(&bytes, 104), &bytes[108..114]), (0x54, &[11, 0, 12, 0, 13, 0][..]));
    assert_eq!((word(&bytes, 144), word(&bytes, 172), word(&bytes, 176)), (0, 0x101, 0x201));
    assert_eq!((word(&bytes, 184), word(&bytes, 188), word(&bytes, 192), word(&bytes, 196)), (1, 1, 0x016b_5b30, 0x00f2_2730));
    assert_eq!((word(&bytes, 200), word(&bytes, 204)), (0x51, 0x00f2_27f8));
}

#[test]
fn a_selected_s1_start_is_0x4006_with_the_capture_window_and_format_mode() {
    let mut fake = Fake::with(&[(Field::Selection, 1), (Field::FormatMode, 3), (Field::ViewActive, 1)]);
    let taken = taken(&mut fake);
    request(Request::Start, &taken, record(), &mut fake);
    let (operation, bytes, _) = fake.statistics();
    assert_eq!((operation, bytes.len()), (Op::Request06, 204));
    assert_eq!((word(&bytes, 16), word(&bytes, 20), word(&bytes, 36)), (0x103, 0x203, 3));
    assert_eq!((word(&bytes, 196), word(&bytes, 200)), (1, 0x00f2_27f8));
}

#[test]
fn the_retune_reads_the_capture_block_when_sent_and_leaves_two_option_bytes_clear() {
    let mut fake = Fake::with(&[
        (Field::StatisticsRetune, 0x77),
        (Field::CaptureOptionFirst, 1),
        (Field::SelectionCallbackArmed, 1),
        (Field::IntervalTracking, 1),
        (Field::CaptureOptionFourth, 1),
        (Field::CaptureOptionFifth, 1),
    ]);
    fake.live = vec![(Field::SpanMode, 1), (Field::SpanSet, 1)];
    fake.blocks.push((Block::Spans, (0..26).map(|index| 0x900 + index).collect()));
    let taken = taken(&mut fake);
    request(Request::Retune, &taken, record(), &mut fake);
    let (operation, bytes, completions) = fake.statistics();
    assert_eq!((operation, completions), (Op::Request2c, vec![C::Statistics80]));
    assert_eq!(word(&bytes, 16), 0x77);
    assert_eq!(&bytes[29..34], &[1, 0, 1, 1, 0]);
    assert_eq!((word(&bytes, 36), word(&bytes, 40)), (0x904, 0x905));
    assert_eq!(word(&bytes, 56), 0x00f2_2708);
}

#[test]
fn stops_selection_output_and_the_lens_option_go_where_the_s1_configuration_sends_them() {
    let mut fake = Fake::with(&[(Field::ViewActive, 1)]);
    fake.live = vec![(Field::OutputFirst, 0x41), (Field::OutputFourth, 0x44)];
    let taken = taken(&mut fake);
    request(Request::Stop { pause: true }, &taken, record(), &mut fake);
    assert_eq!(fake.statistics(), (Op::Request07, vec![1, 0, 0, 0, 1, 0, 0, 0, 0xdc, 0x27, 0xf2, 0], vec![C::Statistics7c]));
    request(Request::Selection, &taken, record(), &mut fake);
    let (operation, bytes, _) = fake.statistics();
    assert_eq!((operation, word(&bytes, 0), word(&bytes, 4), word(&bytes, 16)), (Op::Request2a, 1, 0x016b_5b30, 1));
    request(Request::Output, &taken, record(), &mut fake);
    let (operation, bytes, completions) = fake.statistics();
    assert_eq!((operation, word(&bytes, 0), word(&bytes, 12), completions), (Op::Request13, 0x41, 0x44, vec![C::Statistics75]));
    request(Request::EndImaging, &taken, record(), &mut fake);
    request(Request::LensOption, &taken, record(), &mut fake);
    assert_eq!(fake.take_seen(), [Seen::Imaging(ImgDcOperation::Request43, vec![0xcc, 0x26, 0xf2, 0], vec![]), Seen::LensOption]);
}
