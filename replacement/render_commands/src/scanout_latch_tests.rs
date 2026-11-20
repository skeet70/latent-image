use super::{ScanoutAddress, ScanoutLatch};
use super::super::publisher::{BufferState, ContextId, FrameId, Publisher, PublisherError, SubmissionError};
use super::super::test_buffers::StaticBuffers;
use super::super::SurfaceId;

const FIRST: ScanoutAddress = ScanoutAddress::new(0x4228_c800);
const SECOND: ScanoutAddress = ScanoutAddress::new(0x423d_e000);

fn submitted(publisher: &mut Publisher) -> FrameId {
    let frame = publisher.begin().unwrap();
    publisher.publish(frame).unwrap();
    publisher.submit(frame, |_, _, _| Ok::<_, SubmissionError<()>>(())).unwrap().unwrap();
    frame
}

/// A publisher whose first buffer scans frame 1 and whose second holds submitted frame 2.
fn scanning_and_submitted(buffers: &'static StaticBuffers, context: u64) -> (Publisher, FrameId, FrameId) {
    let [first, second] = buffers.take();
    let mut publisher = Publisher::new(ContextId::new(context).unwrap(), SurfaceId::Osd, first, second).unwrap();
    let scanning = submitted(&mut publisher);
    publisher.acknowledge(scanning).unwrap();
    let pending = submitted(&mut publisher);
    (publisher, scanning, pending)
}

#[test]
fn delivery_after_the_writing_pass_acknowledges_exactly_that_frame_and_releases_older_storage() {
    static BUFFERS: StaticBuffers = StaticBuffers::new(0);
    let (mut publisher, _, pending) = scanning_and_submitted(&BUFFERS, 1);
    let mut latch = ScanoutLatch::new();
    latch.arm(pending, SECOND, FIRST);
    latch.commit_pass(ScanoutAddress::new(0x023d_e000), true);
    assert_eq!(latch.take(), None);
    latch.delivered();
    let latched = latch.take().unwrap();
    assert_eq!(latched, pending);
    publisher.acknowledge(latched).unwrap();
    assert_eq!(publisher.states(), [BufferState::Free, BufferState::Scanning(pending)]);
}

#[test]
fn a_pass_that_did_not_write_the_frame_cannot_acknowledge_it() {
    let frame = {
        static BUFFERS: StaticBuffers = StaticBuffers::new(0);
        scanning_and_submitted(&BUFFERS, 1).2
    };
    // The register still names the scanning buffer.
    let mut latch = ScanoutLatch::new();
    latch.arm(frame, SECOND, FIRST);
    latch.commit_pass(FIRST, true);
    latch.delivered();
    assert_eq!(latch.take(), None);
    // The register already named the frame's buffer when it was armed.
    let mut latch = ScanoutLatch::new();
    latch.arm(frame, SECOND, SECOND);
    latch.commit_pass(SECOND, true);
    latch.delivered();
    assert_eq!(latch.take(), None);
    // The pass wrote the frame's buffer but its one-shot was not registered.
    let mut latch = ScanoutLatch::new();
    latch.arm(frame, SECOND, FIRST);
    latch.commit_pass(SECOND, false);
    latch.delivered();
    assert_eq!(latch.take(), None);
}

#[test]
fn a_frame_latches_only_after_every_earlier_registered_one_shot_was_delivered() {
    static BUFFERS: StaticBuffers = StaticBuffers::new(0);
    let (_, _, frame) = scanning_and_submitted(&BUFFERS, 1);
    let mut latch = ScanoutLatch::new();
    latch.arm(frame, SECOND, FIRST);
    latch.commit_pass(SECOND, true);
    latch.commit_pass(SECOND, true);
    latch.delivered();
    assert_eq!(latch.take(), None);
    latch.delivered();
    assert_eq!(latch.take(), Some(frame));
    // An unassociated one-shot still outstanding refuses association.
    let mut latch = ScanoutLatch::new();
    latch.commit_pass(FIRST, true);
    latch.arm(frame, SECOND, FIRST);
    latch.commit_pass(SECOND, true);
    latch.delivered();
    latch.delivered();
    assert_eq!(latch.take(), None);
}

#[test]
fn duplicate_stale_and_other_context_deliveries_change_nothing() {
    static BUFFERS: StaticBuffers = StaticBuffers::new(0);
    static OTHER: StaticBuffers = StaticBuffers::new(0);
    let (mut publisher, _, frame) = scanning_and_submitted(&BUFFERS, 1);
    let mut latch = ScanoutLatch::new();
    latch.arm(frame, SECOND, FIRST);
    latch.commit_pass(SECOND, true);
    latch.delivered();
    let latched = latch.take().unwrap();
    latch.delivered();
    assert_eq!(latch.take(), None);
    publisher.acknowledge(latched).unwrap();
    let states = publisher.states();
    assert_eq!(publisher.acknowledge(latched), Err(PublisherError::StaleAcknowledgment));
    assert_eq!(publisher.states(), states);
    let (mut other, _, _) = scanning_and_submitted(&OTHER, 2);
    let before = other.states();
    assert_eq!(other.acknowledge(latched), Err(PublisherError::UnknownFrame));
    assert_eq!(other.states(), before);
}
