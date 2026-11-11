use super::SurfaceId;
use super::publisher::*;
use super::test_buffers::StaticBuffers;

fn context(value: u64) -> ContextId { ContextId::new(value).unwrap() }

#[test]
fn presentation_not_submission_recycles_previous_frame() {
    static BUFFERS: StaticBuffers = StaticBuffers::new(0);
    let [first, second] = BUFFERS.take();
    let mut runtime = Publisher::new(context(1), SurfaceId::Osd, first, second).unwrap();
    let a = runtime.begin().unwrap();
    runtime.drawing_pixels(a).unwrap()[0] = 17;
    runtime.publish(a).unwrap();
    runtime.submit(a, |_, _, pixels| { assert_eq!(pixels[0], 17); Ok::<_, SubmissionError<()>>(()) }).unwrap().unwrap();
    assert_eq!(runtime.drain_state(), DrainState::Pending);
    assert_eq!(runtime.acknowledge(a).unwrap(), FramePresented { surface: SurfaceId::Osd, frame: a });
    assert_eq!(runtime.drain_state(), DrainState::Drained);
    let b = runtime.begin().unwrap();
    runtime.publish(b).unwrap();
    runtime.submit(b, |_, _, _| Ok::<_, SubmissionError<()>> (())).unwrap().unwrap();
    assert_eq!(runtime.begin(), Err(PublisherError::Busy));
    runtime.acknowledge(b).unwrap();
    assert!(runtime.begin().is_ok());
    assert_eq!(runtime.acknowledge(a), Err(PublisherError::StaleAcknowledgment));
    assert_eq!(runtime.acknowledge(b), Err(PublisherError::StaleAcknowledgment));
}

#[test]
fn close_waits_for_actual_detach_and_rejects_late_context_events() {
    static BUFFERS: StaticBuffers = StaticBuffers::new(0);
    let [first, second] = BUFFERS.take();
    let mut runtime = Publisher::new(context(2), SurfaceId::Osd, first, second).unwrap();
    let a = runtime.begin().unwrap();
    runtime.publish(a).unwrap();
    runtime.submit(a, |_, _, _| Ok::<_, SubmissionError<()>> (())).unwrap().unwrap();
    assert_eq!(runtime.close(), CloseState::Pending);
    assert_eq!(runtime.cancel(a), Err(PublisherError::Busy));
    assert_eq!(runtime.begin(), Err(PublisherError::Closed));
    runtime.acknowledge(a).unwrap();
    assert_eq!(runtime.close_state(), CloseState::Pending);
    assert_eq!(runtime.drain_state(), DrainState::Drained);
    assert_eq!(runtime.acknowledge_detached(context(1)), Err(PublisherError::NotDetached));
    assert_eq!(runtime.acknowledge_detached(context(2)), Ok(CloseState::Released));
    assert_eq!(runtime.close(), CloseState::Released);
    assert_eq!(runtime.acknowledge(a), Err(PublisherError::StaleAcknowledgment));
}

#[test]
fn failed_submission_remains_cancelable_and_close_discards_unsubmitted_work() {
    static BUFFERS: StaticBuffers = StaticBuffers::new(0);
    let [first, second] = BUFFERS.take();
    let mut runtime = Publisher::new(context(3), SurfaceId::Osd, first, second).unwrap();
    let a = runtime.begin().unwrap();
    runtime.publish(a).unwrap();
    assert_eq!(runtime.submit(a, |_, _, _| Err(SubmissionError::Rejected(9))), Ok(Err(SubmissionError::Rejected(9))));
    assert_eq!(runtime.acknowledge(a), Err(PublisherError::UnknownFrame));
    runtime.cancel(a).unwrap();
    assert!(runtime.begin().is_ok());
    assert_eq!(runtime.close(), CloseState::Released);
    assert_eq!(runtime.drain_state(), DrainState::Drained);
}

#[test]
fn newest_actual_latch_retires_older_submission_without_replaying_event() {
    static BUFFERS: StaticBuffers = StaticBuffers::new(0);
    let [first, second] = BUFFERS.take();
    let mut runtime = Publisher::new(context(4), SurfaceId::Osd, first, second).unwrap();
    let a = runtime.begin().unwrap();
    runtime.publish(a).unwrap();
    let b = runtime.begin().unwrap();
    runtime.publish(b).unwrap();
    runtime.submit(a, |_, _, _| Ok::<_, SubmissionError<()>> (())).unwrap().unwrap();
    runtime.submit(b, |_, _, _| Ok::<_, SubmissionError<()>> (())).unwrap().unwrap();
    assert_eq!(runtime.acknowledge(b).unwrap().frame, b);
    assert_eq!(runtime.drain_state(), DrainState::Drained);
    assert_eq!(runtime.acknowledge(a), Err(PublisherError::StaleAcknowledgment));
    assert!(runtime.begin().is_ok());
}

#[test]
fn replaced_context_rejects_old_presented_event_with_same_sequence() {
    static BUFFERS: StaticBuffers = StaticBuffers::new(0);
    let [first, second] = BUFFERS.take();
    let mut old = Publisher::new(context(5), SurfaceId::Osd, first, second).unwrap();
    let old_frame = old.begin().unwrap();
    old.publish(old_frame).unwrap();
    assert_eq!(old.close(), CloseState::Released);
    let [first, second] = match old.release() { Ok(buffers) => buffers, Err(_) => panic!("closed unsubmitted frame retained buffers") };
    let mut current = Publisher::new(context(6), SurfaceId::Osd, first, second).unwrap();
    let current_frame = current.begin().unwrap();
    assert_eq!(current_frame.sequence(), old_frame.sequence());
    current.publish(current_frame).unwrap();
    current.submit(current_frame, |_, _, _| Ok::<_, SubmissionError<()>> (())).unwrap().unwrap();
    assert_eq!(current.acknowledge(old_frame), Err(PublisherError::UnknownFrame));
    assert_eq!(current.begin().unwrap().sequence(), 2);
    assert_eq!(current.acknowledge(current_frame).unwrap().frame, current_frame);
}

#[test]
fn queue_order_rejection_and_submission_retry_preserve_frame_contents() {
    static BUFFERS: StaticBuffers = StaticBuffers::new(0);
    let [first, second] = BUFFERS.take();
    let mut runtime = Publisher::new(context(7), SurfaceId::Osd, first, second).unwrap();
    let a = runtime.begin().unwrap();
    let b = runtime.begin().unwrap();
    runtime.drawing_pixels(a).unwrap()[0] = 42;
    assert_eq!(runtime.publish(b), Err(PublisherError::WrongState));
    runtime.publish(a).unwrap();
    runtime.publish(b).unwrap();
    assert_eq!(runtime.submit(b, |_, _, _| Ok::<_, SubmissionError<()>> (())), Err(PublisherError::WrongState));
    assert_eq!(runtime.submit(a, |_, _, pixels| {
        assert_eq!(pixels[0], 42);
        Err(SubmissionError::Rejected(3))
    }), Ok(Err(SubmissionError::Rejected(3))));
    runtime.submit(a, |_, _, pixels| {
        assert_eq!(pixels[0], 42);
        Ok::<_, SubmissionError<()>> (())
    }).unwrap().unwrap();
    assert_eq!(runtime.submit(a, |_, _, _| Ok::<_, SubmissionError<()>> (())), Err(PublisherError::Busy));
    runtime.cancel(b).unwrap();
    assert_eq!(runtime.acknowledge(a).unwrap().frame, a);
    assert!(runtime.begin().is_ok());
}

#[test]
fn submission_unwind_does_not_return_in_flight_storage() {
    extern crate std;
    static BUFFERS: StaticBuffers = StaticBuffers::new(0);
    let [first, second] = BUFFERS.take();
    let mut runtime = Publisher::new(context(8), SurfaceId::Osd, first, second).unwrap();
    let frame = runtime.begin().unwrap();
    runtime.publish(frame).unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = runtime.submit::<()>(frame, |_, _, _| panic!("transport unwound after accepting storage"));
    }));
    assert!(result.is_err());
    assert_eq!(runtime.cancel(frame), Err(PublisherError::Busy));
    assert_eq!(runtime.close(), CloseState::Pending);
    assert_eq!(runtime.acknowledge_detached(context(8)), Ok(CloseState::Released));
    assert!(runtime.release().is_ok());
}

#[test]
fn invalid_buffer_admission_returns_exclusive_storage_to_caller() {
    static BUFFERS: StaticBuffers<4> = StaticBuffers::new(23);
    let [first, second] = BUFFERS.take();
    let error = match Publisher::new(context(9), SurfaceId::Osd, first, second) {
        Err(error) => error,
        Ok(_) => panic!("four-byte buffers admitted as complete frames"),
    };
    assert_eq!(error.error(), PublisherError::BufferLength);
    let [first, second] = error.into_buffers();
    first[0] = 11;
    assert_eq!(first, &[11, 23, 23, 23]);
    assert_eq!(second, &[23, 23, 23, 23]);
}
