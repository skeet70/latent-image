use xm1_kernel::fake::Fake as KernelFake;
use xm1_kernel::{Kernel, PostError, TaskId};
use xm1_saclac_controllers::children::{ChildEvent, ChildOutcome, LensControl};
use xm1_saclac_interface::stock::{completion, parameters, record_handle_word};
use xm1_saclac_interface::{Command, CommandKind, Completed, Message, RecordHandle};
use xm1_saclac_lens_drive::children::LensChildren;
use xm1_saclac_profile_records::Bytes;

use crate::fake::{open, peers, posts, root_with, saclac_kernel, Entries, Fake, Peers, Root};

const CLIENT: u32 = 0x00c0_ffee;
const PENDING: u32 = 0x169_cf24;
const LENS_DRIVE_BLOCK: u32 = 0x169_d1c8;
const CHANNEL_COUNT: u32 = 0x3e_0f80;
const LIVE_RESULT: u32 = 0x95_4dbc;
const PROBES: [u32; 3] = [0x76_d9a8, 0x76_da38, 0x76_dba4];
const LENS_DC: u8 = 50;

fn command(kind: CommandKind, record: RecordHandle) -> ChildEvent<'static, LensControl> {
    ChildEvent::Command { command: Command { kind, parameters: parameters(&[]) }, record }
}

fn done(code: u8, bytes: &[u8], record: RecordHandle) -> ChildEvent<'_, LensControl> {
    ChildEvent::Completed { completed: Completed { completion: completion(code).unwrap(), parameters: parameters(bytes) }, record }
}

fn words(values: &[u32]) -> Vec<u8> { values.iter().flat_map(|value| value.to_le_bytes()).collect() }

/// A record with no grants, a lens-drive command waiting with its client and words 11..14,
/// four configured channels and the probe at 0x76d9a8 answering `first_probe` in turn;
/// `offered` is the shared block's offered-measurement word.
fn driving_with<'c, 'k>(peers: Peers<'c, 'k>, offered: u8, first_probe: &[u32]) -> (Fake, Root<Peers<'c, 'k>>, RecordHandle) {
    let mut bytes = Bytes::default();
    let record = open(&mut bytes, &[]);
    bytes.shared[0x13c] = offered;
    let (fake, root) = root_with(peers, bytes, Entries::default());
    for (index, word) in [CLIENT, 11, 0x1234, 13, 14].into_iter().enumerate() {
        fake.set_word(LENS_DRIVE_BLOCK + 4 * index as u32, word);
    }
    fake.set_byte(PENDING + 0x0e, 1);
    fake.adjustment(CHANNEL_COUNT, 4);
    for &answer in first_probe {
        fake.answer(PROBES[0], answer);
    }
    (fake, root, record)
}

/// 0x76d9a8 answers forced to the drive's check, which then posts, and to the reply.
fn driving<'c, 'k>(peers: Peers<'c, 'k>) -> (Fake, Root<Peers<'c, 'k>>, RecordHandle) { driving_with(peers, 0, &[2, 2]) }

/// 0x75a7c4(record, 63, 3, 0x81): slot and serial kept, owner dropped, writable, every group.
fn slot_reference(record: RecordHandle) -> u32 { record_handle_word(record) & 0xffff | 1 << 17 | 63 << 18 | 0x81 << 24 }

#[test]
fn a_live_drive_posts_to_lensdc_and_answers_the_client_with_the_recorded_result() {
    let kernel = saclac_kernel();
    let cx = kernel.task().unwrap();
    let (fake, mut root, record) = driving(peers(&cx));
    fake.set_word(LIVE_RESULT, 2);
    let mut children = LensChildren::new();
    assert_eq!(children.live(command(CommandKind::LensDrive, record), &mut root), ChildOutcome::Pending);
    assert_eq!(posts(&kernel), [(LENS_DC, 0x4207, words(&[0xf1_4be4, 11, 0x1234, 13, 14, slot_reference(record)]))]);
    assert_eq!(fake.take_entered(), [(PROBES[0], vec![])]);
    assert_eq!(fake.byte(PENDING + 0x0e), 1);

    assert_eq!(children.live(done(0x34, &[], record), &mut root), ChildOutcome::Complete);
    assert_eq!(fake.take_entered(), [
        (PROBES[0], vec![]),
        (PROBES[1], vec![]),
        (PROBES[2], vec![]),
        (CLIENT, vec![0, 2, 0x1234, 1, 0b010]),
    ]);
    assert_eq!(fake.byte(PENDING + 0x0e), 0);
    assert_eq!(posts(&kernel).len(), 1);
}

#[test]
fn a_drive_result_outside_the_configured_channels_hides_the_clients_word() {
    let kernel = saclac_kernel();
    let cx = kernel.task().unwrap();
    let (fake, mut root, record) = driving(peers(&cx));
    fake.set_word(LIVE_RESULT, 4);
    let mut children = LensChildren::new();
    children.live(command(CommandKind::LensDrive, record), &mut root);
    children.live(done(0x34, &[], record), &mut root);
    assert_eq!(fake.take_entered().last(), Some(&(CLIENT, vec![0, 4, 0x80_0000, 1, 0b010])));
}

#[test]
fn a_drive_the_kernel_refuses_faults_the_child_and_ends_the_command() {
    let kernel = Kernel::new(KernelFake::with_task(TaskId::new(43).unwrap()).post_result(Err(PostError::NoBlock)));
    let cx = kernel.task().unwrap();
    let (fake, mut root, record) = driving(peers(&cx));
    let mut children = LensChildren::new();
    assert_eq!(children.live(command(CommandKind::LensDrive, record), &mut root), ChildOutcome::Fault);
    assert_eq!(posts(&kernel).len(), 1);
    assert_eq!(fake.take_entered().last(), Some(&(CLIENT, vec![-98i32 as u32, 0, 0x1234, 1, 0b010])));
    assert_eq!(fake.byte(PENDING + 0x0e), 0);
}

#[test]
fn lens_side_adjustment_preserves_signed_wrap_and_reads_configuration_changes() {
    use xm1_saclac_calc::adjust::motion::ports::LensTables;
    let (fake, mut root) = crate::fake::root(Bytes::default(), Entries::default());
    fake.set_word(0x96ca50, 0xa0000000);
    fake.adjustment(0x3e0240, 1);
    assert_eq!(root.side_adjusted(i16::MAX, 1, 0), i16::MIN);
    assert_eq!(root.side_adjusted(i16::MIN, 0, 1), i16::MAX);
    assert_eq!(root.side_adjusted(-123, 1, 1), -123);
    fake.adjustment(0x3e0240, 0);
    fake.adjustment(0x3e0241, 1);
    assert_eq!(root.side_adjusted(-123, 1, 0), 133);
    assert_eq!(root.side_adjusted(-123, 0, 1), -379);
    assert_eq!(root.side_adjusted(-123, 3, 0), -123);
}

#[test]
fn a_drive_is_not_posted_while_lens_control_reports_its_first_probe() {
    let kernel = saclac_kernel();
    let cx = kernel.task().unwrap();
    let (fake, mut root, record) = driving_with(peers(&cx), 0, &[1, 2]);
    let mut children = LensChildren::new();
    assert_eq!(children.live(command(CommandKind::LensDrive, record), &mut root), ChildOutcome::Fault);
    assert_eq!(posts(&kernel), []);
    assert_eq!(fake.take_entered().last(), Some(&(CLIENT, vec![-98i32 as u32, 0, 0x1234, 1, 0b010])));
    assert_eq!(fake.byte(PENDING + 0x0e), 0);
}

#[test]
fn a_corrected_drive_sends_word_four_as_one_and_answers_with_word_one() {
    let kernel = saclac_kernel();
    let cx = kernel.task().unwrap();
    let (fake, mut root, record) = driving_with(peers(&cx), 0b10, &[2, 2]);
    fake.adjustment(0x88ef, 0b1000);
    fake.set_word(LIVE_RESULT, 3);
    let mut children = LensChildren::new();
    children.live(command(CommandKind::LensDrive, record), &mut root);
    assert_eq!(posts(&kernel), [(LENS_DC, 0x4207, words(&[0xf1_4be4, 11, 0x1234, 13, 1, slot_reference(record)]))]);
    children.live(done(0x34, &[], record), &mut root);
    assert_eq!(fake.take_entered().last(), Some(&(CLIENT, vec![0, 11, 0x80_0000, 0, 0b010])));
}

#[test]
fn an_idle_cancel_answers_its_own_client_with_the_status_alone() {
    let kernel = saclac_kernel();
    let cx = kernel.task().unwrap();
    let (fake, mut root, record) = driving(peers(&cx));
    fake.set_word(0x169_d1dc, 0x00ca_ce11);
    fake.set_byte(PENDING + 0x0f, 1);
    let mut children = LensChildren::new();
    assert_eq!(children.live(command(CommandKind::Cancel, record), &mut root), ChildOutcome::Complete);
    assert_eq!(fake.take_entered(), [(0x00ca_ce11, vec![21])]);
    assert_eq!(fake.byte(PENDING + 0x0f), 0);
    assert_eq!(posts(&kernel), []);
}

#[test]
fn a_step_posts_the_halfword_and_answers_notification_1b() {
    let kernel = saclac_kernel();
    let cx = kernel.task().unwrap();
    let (fake, mut root, record) = driving_with(peers(&cx), 0, &[2]);
    fake.set_word(0x169_d28c, 0x0057_e900);
    fake.set_word(0x169_d290, 0xfffd);
    fake.set_byte(PENDING + 0x1b, 1);
    let mut children = LensChildren::new();
    let step = ChildEvent::Control { control: LensControl::Step, parameters: parameters(&[]), record };
    assert_eq!(children.live(step, &mut root), ChildOutcome::Pending);
    assert_eq!(posts(&kernel), [(LENS_DC, 0x420b, words(&[0xf1_4bc8, 0xfffd, 0, 1, 0, 0, 4]))]);
    assert_eq!(fake.take_entered(), []);
    assert_eq!(children.live(done(0x35, &[], record), &mut root), ChildOutcome::Complete);
    assert_eq!(fake.take_entered().last(), Some(&(0x0057_e900, vec![0, 0b010])));
    assert_eq!(fake.byte(PENDING + 0x1b), 0);
}

#[test]
fn a_bring_up_applies_the_resource_index_before_answering_with_the_completion_words() {
    let kernel = saclac_kernel();
    let cx = kernel.task().unwrap();
    let (fake, mut root, record) = driving(peers(&cx));
    for (index, word) in [0x00b0_0001, 1, 2, 3, 4, 5].into_iter().enumerate() {
        fake.set_word(0x169_d038 + 4 * index as u32, word);
    }
    fake.answer(0x76_d9c8, 7);
    let mut children = LensChildren::new();
    let bring_up = Message::Command(Command { kind: CommandKind::LensBringUp, parameters: parameters(&[]) });
    assert_eq!(children.bring_up(bring_up, record, &mut root), ChildOutcome::Pending);
    assert_eq!(posts(&kernel), [(LENS_DC, 0x4202, words(&[0xf1_50f0, 1, 2, 3, record_handle_word(record), 4, 5]))]);
    assert_eq!(fake.take_entered(), []);
    let finished = words(&[0, 9, 8, 6]);
    let completed = Message::Completed(Completed { completion: completion(0x2f).unwrap(), parameters: parameters(&finished) });
    assert_eq!(children.bring_up(completed, record, &mut root), ChildOutcome::Complete);
    assert_eq!(fake.take_entered(), [(0x76_d9c8, vec![]), (0x74_0b18, vec![7]), (0x00b0_0001, vec![0, 9, 8, 6])]);
}

#[test]
fn a_lens_stop_answers_its_client_with_the_status_and_the_stopped_value() {
    let kernel = saclac_kernel();
    let cx = kernel.task().unwrap();
    let (fake, mut root, record) = driving(peers(&cx));
    fake.set_word(0x169_d050, 0x00b0_0005);
    let mut children = LensChildren::new();
    let stop = Message::Command(Command { kind: CommandKind::LensStop, parameters: parameters(&[]) });
    assert_eq!(children.bring_up(stop, record, &mut root), ChildOutcome::Pending);
    fake.take_entered();
    let finished = words(&[0, 9]);
    let completed = Message::Completed(Completed { completion: completion(0x30).unwrap(), parameters: parameters(&finished) });
    assert_eq!(children.bring_up(completed, record, &mut root), ChildOutcome::Complete);
    assert_eq!(fake.take_entered(), [(0x00b0_0005, vec![0, 9])]);
}
