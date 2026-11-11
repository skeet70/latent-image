use super::*;
use crate::{PROFILE_FIRST_PHYSICAL_BLOCK, PROFILE_PHYSICAL_BLOCK_COUNT};

use std::vec;
use std::vec::Vec;

fn erased(blocks: usize) -> (Vec<u8>, Vec<[u8; 16]>, Vec<BlockFailures>) {
    (
        vec![0xff; blocks * BLOCK_BYTES as usize],
        vec![[0xff; 16]; blocks * PAGES_PER_BLOCK as usize],
        vec![BlockFailures::default(); blocks],
    )
}

fn adopted<'image>(
    data: &'image mut [u8],
    spare: &'image mut [[u8; 16]],
    failures: &'image mut [BlockFailures],
    base: PhysicalBlock,
    provenance: ImageProvenance,
    configuration: HostConfiguration,
) -> MemoryNand<'image> {
    MemoryNand::adopt(base, data, spare, failures, provenance, configuration).expect("valid image")
}

fn page(fill: u8) -> [u8; PAGE_BYTES] {
    [fill; PAGE_BYTES]
}

fn audio_bytes() -> Vec<u8> {
    (0..33 * BLOCK_BYTES as usize).map(|index| (index % 251) as u8).collect()
}

#[test]
fn audio_profile_write_41_count_33_round_trips_under_both_configurations() {
    for configuration in [HostConfiguration::G1StrictRejectBeforeMutation, HostConfiguration::LegacyAndCommitFailedStatus] {
        let (mut data, mut spare, mut failures) = erased(PROFILE_PHYSICAL_BLOCK_COUNT);
        let nand = adopted(&mut data, &mut spare, &mut failures, PROFILE_FIRST_PHYSICAL_BLOCK, ImageProvenance::Synthetic, configuration);
        let mut profile = MemoryProfile::mount(nand, Timeout::new(1000)).expect("mount");
        let bytes = audio_bytes();
        let first = LogicalBlock::new(41).expect("logical block");
        profile.write_blocks(first, &bytes, Verification::ReadBack, Timeout::new(1000)).expect("write");
        let mut observed = vec![0u8; bytes.len()];
        profile.read_blocks(first, &mut observed, Timeout::new(1000)).expect("read");
        assert_eq!(observed, bytes);
        let media = profile.into_media();
        assert_eq!(media.observation().configuration, configuration);
        assert_eq!(media.observation().provenance, ImageProvenance::Synthetic);
        assert!(media.observation().first_failure.is_none());
        assert_eq!(media.observation().failed.get(), 0);

        let mut profile = MemoryProfile::mount(media, Timeout::new(1000)).expect("remount");
        let mut observed = vec![0u8; bytes.len()];
        profile.read_blocks(first, &mut observed, Timeout::new(1000)).expect("remount read");
        assert_eq!(observed, bytes);
        let mut neighbour = vec![0u8; BLOCK_BYTES as usize];
        profile.read_blocks(LogicalBlock::new(40).expect("block 40"), &mut neighbour, Timeout::new(1000)).expect("block 40");
        assert_eq!(neighbour, vec![0xff; BLOCK_BYTES as usize]);
        profile.read_blocks(LogicalBlock::new(74).expect("block 74"), &mut neighbour, Timeout::new(1000)).expect("block 74");
        assert_eq!(neighbour, vec![0xff; BLOCK_BYTES as usize]);
    }
}

#[test]
fn strict_policy_rejects_zero_to_one_program_before_mutation() {
    let (mut data, mut spare, mut failures) = erased(1);
    let mut nand = adopted(&mut data, &mut spare, &mut failures, PhysicalBlock::new(0).unwrap(), ImageProvenance::Synthetic, HostConfiguration::G1StrictRejectBeforeMutation);
    nand.program_page(PhysicalPage::new(0).unwrap(), &page(0x0f), &[0xff; 16]).expect("first program");
    assert_eq!(nand.program_page(PhysicalPage::new(0).unwrap(), &page(0xf0), &[0xff; 16]), Err(Error::ProgramFailed));
    assert_eq!(&nand.bytes()[..PAGE_BYTES], &page(0x0f)[..]);
    assert_eq!(nand.spare_records()[0], [0xff; 16]);
    assert_eq!(nand.observation().failed.get(), 1);
    assert_eq!(nand.observation().first_failure.expect("failure recorded").operation, Operation::ProgramPage(PhysicalPage::new(0).unwrap()));
}

#[test]
fn legacy_policy_commits_cleared_bits_and_reports_program_failed() {
    let (mut data, mut spare, mut failures) = erased(1);
    let mut nand = adopted(&mut data, &mut spare, &mut failures, PhysicalBlock::new(0).unwrap(), ImageProvenance::Synthetic, HostConfiguration::LegacyAndCommitFailedStatus);
    nand.program_page(PhysicalPage::new(0).unwrap(), &page(0x0f), &[0xff; 16]).expect("first program");
    assert_eq!(nand.program_page(PhysicalPage::new(0).unwrap(), &page(0xf0), &[0xff; 16]), Err(Error::ProgramFailed));
    assert_eq!(&nand.bytes()[..PAGE_BYTES], &page(0x00)[..]);
    assert_eq!(nand.observation().failed.get(), 1);
    assert_eq!(nand.observation().processed_data.get(), 2 * PAGE_BYTES as u64);
}

#[test]
fn mark_bad_on_an_erased_block_marks_every_page_under_both_configurations() {
    for configuration in [HostConfiguration::G1StrictRejectBeforeMutation, HostConfiguration::LegacyAndCommitFailedStatus] {
        let (mut data, mut spare, mut failures) = erased(1);
        let mut nand = adopted(&mut data, &mut spare, &mut failures, PhysicalBlock::new(0).unwrap(), ImageProvenance::Synthetic, configuration);
        nand.mark_bad(PhysicalBlock::new(0).unwrap(), Timeout::new(1000)).expect("mark bad");
        assert_eq!(&nand.bytes()[..], &vec![0xff; BLOCK_BYTES as usize][..]);
        for record in nand.spare_records() {
            assert_eq!(&record[..5], &[0xff; 5]);
            assert_eq!(record[5], 0xe0);
        }
        assert_eq!(nand.observation().failed.get(), 0);
        assert_eq!(nand.observation().completed.get(), u64::from(PAGES_PER_BLOCK));
    }
}

#[test]
fn mark_bad_on_a_programmed_block_reports_the_policy_outcome() {
    let mut outcomes = Vec::new();
    for configuration in [HostConfiguration::G1StrictRejectBeforeMutation, HostConfiguration::LegacyAndCommitFailedStatus] {
        let (mut data, mut spare, mut failures) = erased(1);
        let mut nand = adopted(&mut data, &mut spare, &mut failures, PhysicalBlock::new(0).unwrap(), ImageProvenance::Synthetic, configuration);
        nand.program_page(PhysicalPage::new(0).unwrap(), &page(0x00), &[0xff; 16]).expect("program");
        nand.mark_bad(PhysicalBlock::new(0).unwrap(), Timeout::new(1000)).expect("mark bad returns the pinned caller status");
        assert_eq!(&nand.bytes()[..PAGE_BYTES], &page(0x00)[..]);
        assert_eq!(nand.spare_records()[0][5], if configuration == HostConfiguration::G1StrictRejectBeforeMutation { 0xff } else { 0xe0 });
        for record in &nand.spare_records()[1..] {
            assert_eq!(record[5], 0xe0);
        }
        assert_eq!(nand.observation().failed.get(), 1);
        assert_eq!(nand.observation().completed.get(), u64::from(PAGES_PER_BLOCK));
        outcomes.push((configuration, nand.spare_records()[0][5]));
    }
    assert_ne!(outcomes[0].1, outcomes[1].1, "the two laws leave different page-0 markers");
}

#[test]
fn strict_policy_also_rejects_a_tag_spare_rewrite_before_mutation() {
    let (mut data, mut spare, mut failures) = erased(1);
    let mut nand = adopted(&mut data, &mut spare, &mut failures, PhysicalBlock::new(0).unwrap(), ImageProvenance::Synthetic, HostConfiguration::G1StrictRejectBeforeMutation);
    let tag = [0x10, 0x01, 0x10, 0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
    nand.program_page(PhysicalPage::new(0).unwrap(), &page(0xff), &tag).expect("program tag page");
    nand.mark_bad(PhysicalBlock::new(0).unwrap(), Timeout::new(1000)).expect("mark bad");
    assert_eq!(nand.spare_records()[0], tag);
    assert_eq!(nand.observation().failed.get(), 1);
}

#[test]
fn cancellation_preserves_the_completed_prefix_and_is_accounted_once() {
    let (mut data, mut spare, mut failures) = erased(1);
    let mut nand = adopted(&mut data, &mut spare, &mut failures, PhysicalBlock::new(0).unwrap(), ImageProvenance::Synthetic, HostConfiguration::LegacyAndCommitFailedStatus);
    let mut output = [0xaa; PAGE_BYTES];
    let mut operation = nand.begin_read_page(PhysicalPage::new(0).unwrap(), &mut output).expect("begin");
    let record = operation.advance(StepBytes::SECTOR).expect("first slice");
    assert_eq!(record.outcome, OperationOutcome::Pending);
    assert_eq!(record.progress.data.get(), 512);
    let cancelled = operation.cancel();
    assert_eq!(cancelled.outcome, OperationOutcome::Cancelled);
    assert_eq!(nand.observation().cancelled.get(), 1);
    assert_eq!(&output[..512], &[0xff; 512][..]);
    assert_eq!(&output[512..], &[0xaa; PAGE_BYTES - 512][..]);

    let mut dropped = nand.begin_read_page(PhysicalPage::new(1).unwrap(), &mut output).expect("begin");
    dropped.advance(StepBytes::SECTOR).expect("slice");
    drop(dropped);
    assert_eq!(nand.observation().cancelled.get(), 2);
}

#[test]
fn cancelled_program_and_erase_keep_the_written_prefix() {
    let (mut data, mut spare, mut failures) = erased(1);
    let mut nand = adopted(&mut data, &mut spare, &mut failures, PhysicalBlock::new(0).unwrap(), ImageProvenance::Synthetic, HostConfiguration::G1StrictRejectBeforeMutation);
    let data = page(0x00);
    let spare = [0xff; 16];
    let mut operation = nand.begin_program_page(PhysicalPage::new(0).unwrap(), &data, &spare).expect("begin program");
    operation.advance(StepBytes::SECTOR).expect("slice");
    assert_eq!(operation.cancel().outcome, OperationOutcome::Cancelled);
    assert_eq!(&nand.bytes()[..512], &[0x00; 512][..]);
    assert_eq!(&nand.bytes()[512..PAGE_BYTES], &[0xff; PAGE_BYTES - 512][..]);
    assert_eq!(nand.observation().cancelled.get(), 1);

    let mut operation = nand.begin_erase_block(PhysicalBlock::new(0).unwrap()).expect("begin erase");
    operation.advance(StepBytes::SECTOR).expect("slice");
    assert_eq!(operation.cancel().outcome, OperationOutcome::Cancelled);
    assert_eq!(&nand.bytes()[..512], &[0xff; 512][..]);
    assert_eq!(&nand.bytes()[512..1024], &[0xff; 512][..]);
    assert_eq!(&nand.bytes()[1024..], &[0xff; BLOCK_BYTES as usize - 1024][..]);
    assert_eq!(nand.observation().cancelled.get(), 2);
}

#[test]
fn tagged_read_fault_reports_the_transferred_prefix() {
    let (mut data, mut spare, mut failures) = erased(1);
    failures[0].tagged_read = FailureCount::new(1);
    let mut nand = adopted(&mut data, &mut spare, &mut failures, PhysicalBlock::new(0).unwrap(), ImageProvenance::Synthetic, HostConfiguration::G1StrictRejectBeforeMutation);
    let mut output = [0xaa; PAGE_BYTES];
    let record = nand.begin_read_page(PhysicalPage::new(0).unwrap(), &mut output).expect("begin").finish().expect("finish");
    assert_eq!(record.outcome, OperationOutcome::TaggedFault(WorkBytes(512)));
    assert_eq!(&output[..512], &[0xff; 512][..]);
    assert_eq!(&output[512..], &[0xaa; PAGE_BYTES - 512][..]);
    nand.read_page(PhysicalPage::new(0).unwrap(), &mut output).expect("the injected tagged fault is consumed");
}

#[test]
fn injected_program_failure_exposes_the_synthetic_prefix_law() {
    let (mut data, mut spare, mut failures) = erased(1);
    failures[0].program = FailureCount::new(1);
    let mut nand = adopted(&mut data, &mut spare, &mut failures, PhysicalBlock::new(0).unwrap(), ImageProvenance::Synthetic, HostConfiguration::G1StrictRejectBeforeMutation);
    assert_eq!(nand.program_page(PhysicalPage::new(2).unwrap(), &page(0x00), &[0xff; 16]), Err(Error::ProgramFailed));
    assert_eq!(nand.observation().processed_data.get(), 1024);
    assert_eq!(&nand.bytes()[2 * PAGE_BYTES..2 * PAGE_BYTES + 1024], &vec![0x00; 1024][..]);
    assert_eq!(&nand.bytes()[2 * PAGE_BYTES + 1024..3 * PAGE_BYTES], &vec![0xff; PAGE_BYTES - 1024][..]);
    assert_eq!(nand.observation().first_failure.expect("failure").outcome, OperationOutcome::Failed(Error::ProgramFailed));
    nand.program_page(PhysicalPage::new(2).unwrap(), &page(0x00), &[0xff; 16]).expect("the injected failure is consumed");
}

#[test]
fn injected_read_and_erase_failures_do_not_mutate_a_programmed_page() {
    let (mut data, mut spare, mut failures) = erased(1);
    let mut nand = adopted(&mut data, &mut spare, &mut failures, PhysicalBlock::new(0).unwrap(), ImageProvenance::Synthetic, HostConfiguration::G1StrictRejectBeforeMutation);
    nand.program_page(PhysicalPage::new(0).unwrap(), &page(0x00), &[0xff; 16]).expect("program");
    nand.failures_mut()[0].read = FailureCount::new(1);
    nand.failures_mut()[0].erase = FailureCount::new(1);
    let mut output = [0; PAGE_BYTES];
    assert_eq!(nand.read_page(PhysicalPage::new(0).unwrap(), &mut output), Err(Error::ReadFault));
    assert_eq!(nand.observation().processed_data.get(), PAGE_BYTES as u64);
    assert_eq!(nand.erase_block(PhysicalBlock::new(0).unwrap()), Err(Error::ProgramFailed));
    assert_eq!(&nand.bytes()[..PAGE_BYTES], &page(0x00)[..]);
    assert_eq!(&nand.bytes()[PAGE_BYTES..], &vec![0xff; BLOCK_BYTES as usize - PAGE_BYTES][..]);
    assert_eq!(nand.spare_records()[0], [0xff; 16]);
    assert_eq!(nand.observation().failed.get(), 2);
    nand.read_page(PhysicalPage::new(0).unwrap(), &mut output).expect("the injected read failure is consumed");
}

#[test]
fn successful_erase_clears_data_and_every_spare_record() {
    let (mut data, mut spare, mut failures) = erased(1);
    let mut nand = adopted(&mut data, &mut spare, &mut failures, PhysicalBlock::new(0).unwrap(), ImageProvenance::Synthetic, HostConfiguration::G1StrictRejectBeforeMutation);
    let mut tag = [0xff; 16];
    tag[5] = 0x00;
    nand.program_page(PhysicalPage::new(0).unwrap(), &page(0x00), &tag).expect("program");
    nand.erase_block(PhysicalBlock::new(0).unwrap()).expect("erase");
    assert_eq!(&nand.bytes()[..], &vec![0xff; BLOCK_BYTES as usize][..]);
    assert!(nand.spare_records().iter().all(|record| record == &[0xff; 16]));
    assert_eq!(nand.observation().completed.get(), 2);
}

#[test]
fn erase_timeout_stops_after_the_mutated_prefix() {
    let (mut data, mut spare, mut failures) = erased(1);
    let mut nand = adopted(&mut data, &mut spare, &mut failures, PhysicalBlock::new(0).unwrap(), ImageProvenance::Synthetic, HostConfiguration::G1StrictRejectBeforeMutation);
    nand.program_page(PhysicalPage::new(0).unwrap(), &page(0x00), &[0xff; 16]).expect("program");
    let record = nand.begin_erase_block(PhysicalBlock::new(0).unwrap()).expect("begin").finish_with_timeout(Timeout::new(2));
    assert!(matches!(record, Err(Error::Timeout)));
    assert_eq!(&nand.bytes()[..1024], &[0xff; 1024][..]);
    assert_eq!(&nand.bytes()[1024..PAGE_BYTES], &[0x00; PAGE_BYTES - 1024][..]);
    assert_eq!(nand.observation().failed.get(), 1);
}

#[test]
fn journal_records_each_terminal_operation_and_refuses_overflow() {
    let (mut data, mut spare, mut failures) = erased(1);
    let mut nand = adopted(&mut data, &mut spare, &mut failures, PhysicalBlock::new(0).unwrap(), ImageProvenance::Synthetic, HostConfiguration::LegacyAndCommitFailedStatus);
    let mut records = [None; 2];
    let mut second_records = [None; 2];
    nand.attach_journal(&mut records).expect("journal");
    let mut output = [0; PAGE_BYTES];
    nand.read_page(PhysicalPage::new(0).unwrap(), &mut output).expect("read");
    nand.program_page(PhysicalPage::new(0).unwrap(), &page(0x00), &[0xff; 16]).expect("program");
    let journal = nand.journal().expect("journal retained");
    assert_eq!(journal.len(), 2);
    assert_eq!(journal[0].expect("first record").outcome, OperationOutcome::Complete);
    assert_eq!(journal[1].expect("second record").outcome, OperationOutcome::Complete);
    assert_eq!(nand.read_page(PhysicalPage::new(1).unwrap(), &mut output), Err(Error::Image(ImageError::JournalFull)));
    assert_eq!(nand.attach_journal(&mut second_records), Err(Error::Image(ImageError::AlreadyStarted)));
}

#[test]
fn provenance_and_law_identity_are_observation_fields() {
    let digest = ImageDigest([7; 32]);
    let (mut data, mut spare, mut failures) = erased(1);
    let nand = adopted(&mut data, &mut spare, &mut failures, PhysicalBlock::new(0).unwrap(), ImageProvenance::Captured(digest), HostConfiguration::LegacyAndCommitFailedStatus);
    let observation = nand.observation();
    assert_eq!(observation.provenance, ImageProvenance::Captured(digest));
    assert_eq!(observation.configuration, HostConfiguration::LegacyAndCommitFailedStatus);
    assert_eq!(nand.laws().configuration_provenance, LawProvenance::FujiModControllerModelSynthetic);
    assert_eq!(nand.laws().configuration_provenance.label(), "fujimod-controller-model-synthetic");
    assert_eq!(nand.laws().configuration.label(), "legacy-and-commit-failed-status");
    assert_eq!(nand.laws().generation.label(), "shared-profile-endpoint-v1");
    assert_eq!(nand.laws().timing.label(), "synthetic-512-bytes-per-request-tick");
    assert_eq!(nand.laws().injected_failure.label(), "synthetic-program-prefix-1024-erase-before-mutation");
    assert_eq!(nand.laws().cancellation.label(), "preserve-completed-prefix");
}

#[test]
fn image_layout_is_validated_before_any_mutation() {
    let (mut data, mut spare, mut failures) = erased(2);
    assert!(matches!(
        MemoryNand::adopt(PhysicalBlock::new(0).unwrap(), &mut data[..BLOCK_BYTES as usize - 1], &mut spare, &mut failures, ImageProvenance::Synthetic, HostConfiguration::G1StrictRejectBeforeMutation),
        Err(Error::Image(ImageError::Layout))
    ));
    assert!(matches!(
        MemoryNand::adopt(PhysicalBlock::new(0).unwrap(), &mut data, &mut spare[..1], &mut failures, ImageProvenance::Synthetic, HostConfiguration::G1StrictRejectBeforeMutation),
        Err(Error::Image(ImageError::Layout))
    ));
    assert!(matches!(
        MemoryNand::adopt(PhysicalBlock::new(0).unwrap(), &mut data, &mut spare, &mut failures[..1], ImageProvenance::Synthetic, HostConfiguration::G1StrictRejectBeforeMutation),
        Err(Error::Image(ImageError::Layout))
    ));
    assert!(matches!(
        MemoryNand::adopt(PhysicalBlock::new(1023).unwrap(), &mut data, &mut spare, &mut failures, ImageProvenance::Synthetic, HostConfiguration::G1StrictRejectBeforeMutation),
        Err(Error::Image(ImageError::Layout))
    ));
}

#[test]
fn masked_write_bytes_preserves_zero_bits_and_reads_back_the_cleared_page() {
    let (mut data, mut spare, mut failures) = erased(PROFILE_PHYSICAL_BLOCK_COUNT);
    let nand = adopted(&mut data, &mut spare, &mut failures, PROFILE_FIRST_PHYSICAL_BLOCK, ImageProvenance::Synthetic, HostConfiguration::G1StrictRejectBeforeMutation);
    let mut profile = MemoryProfile::mount(nand, Timeout::new(1000)).expect("mount");
    let first = LogicalBlock::new(3).expect("logical block");
    profile.write_bytes(first, ByteOffset::ZERO, &[0x0f, 0xf0, 0xaa], Verification::ReadBack, Timeout::new(1000)).expect("first write");
    profile.write_bytes(first, ByteOffset::ZERO, &[0xff, 0x00], Verification::ReadBack, Timeout::new(1000)).expect("masked write");
    let mut observed = [0u8; 3];
    profile.read_bytes(first, ByteOffset::ZERO, &mut observed, Timeout::new(1000)).expect("read");
    assert_eq!(observed, [0x0f, 0x00, 0xaa]);
}

#[test]
fn profile_rewrite_erases_the_previous_block_under_both_laws() {
    for configuration in [HostConfiguration::G1StrictRejectBeforeMutation, HostConfiguration::LegacyAndCommitFailedStatus] {
        let (mut data, mut spare, mut failures) = erased(PROFILE_PHYSICAL_BLOCK_COUNT);
        let nand = adopted(&mut data, &mut spare, &mut failures, PROFILE_FIRST_PHYSICAL_BLOCK, ImageProvenance::Synthetic, configuration);
        let mut profile = MemoryProfile::mount(nand, Timeout::new(1000)).expect("mount");
        let first = LogicalBlock::new(5).expect("logical block");
        profile.write_blocks(first, &vec![0x00; BLOCK_BYTES as usize], Verification::Disabled, Timeout::new(1000)).expect("first write");
        profile.write_blocks(first, &vec![0x55; BLOCK_BYTES as usize], Verification::Disabled, Timeout::new(1000)).expect("rewrite");
        let mut observed = vec![0u8; BLOCK_BYTES as usize];
        profile.read_blocks(first, &mut observed, Timeout::new(1000)).expect("read");
        assert_eq!(observed, vec![0x55; BLOCK_BYTES as usize]);
        assert!(profile.media().observation().first_failure.is_none());
    }
}

#[test]
fn read_with_spare_keeps_the_declared_stride() {
    let (mut data, mut spare, mut failures) = erased(1);
    let mut nand = adopted(&mut data, &mut spare, &mut failures, PhysicalBlock::new(0).unwrap(), ImageProvenance::Synthetic, HostConfiguration::G1StrictRejectBeforeMutation);
    let mut spare_bytes = [0xff; 16];
    spare_bytes[5] = 0x00;
    nand.program_page(PhysicalPage::new(0).unwrap(), &page(0x5a), &spare_bytes).expect("program");
    let mut output = vec![0xaa; 64 * (PAGE_BYTES + 64)];
    nand.read_with_spare(PhysicalBlock::new(0).unwrap(), PhysicalBlockCount::new(1).unwrap(), SpareLayout::Interleaved, &mut output).expect("read");
    assert_eq!(&output[..PAGE_BYTES], &page(0x5a)[..]);
    assert_eq!(&output[PAGE_BYTES..PAGE_BYTES + 16], &spare_bytes[..]);
    assert_eq!(&output[PAGE_BYTES + 16..PAGE_BYTES + 64], &[0xaa; 48][..]);
}
