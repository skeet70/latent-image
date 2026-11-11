use super::*;
use super::physical_fixture::*;

#[test]
fn every_control_operation_accepts_the_actual_command_packet() {
    for operation in (0x7202..=0x7250).chain(0x7256..=0x725c) {
        let request = packet(operation, 3, &[0x3fc0]);
        let (mut runtime, _) = setup();
        runtime.submit(request, None).unwrap();
        if operation == 0x721b || operation == 0x721c { assert_eq!(runtime.pop_publication(), None); }
        else { assert!(matches!(runtime.pop_publication(), Some(Publication::Command { callback: CallbackId(3), status: Status::Stopped, .. }))); }
    }
}
#[test]
fn capture_setup_is_not_initialization_and_cancellation_publishes_once() {
    let (mut runtime, physical) = setup();
    initialize(&mut runtime);
    runtime.submit(packet(0x723f, 7, &[0x8002_0600, 0x4940]), None).unwrap();
    assert_eq!(runtime.state(), LensServiceState::Running);
    assert!(physical.requested.get());
    assert_eq!(runtime.pop_publication(), None);
    runtime.cancel().unwrap();
    assert!(matches!(runtime.pop_publication(), Some(Publication::Command { callback: CallbackId(7), status: Status::Stopped, .. })));
    assert_eq!(runtime.pop_publication(), None);
    assert_eq!(runtime.state(), LensServiceState::Stopped);
    assert!(!physical.acquired.get());
}
#[test]
fn command_completion_requires_real_wire_acknowledgement() {
    let (mut runtime, physical) = setup();
    initialize(&mut runtime);
    runtime.submit(packet(0x7202, 9, &[0x40]), None).unwrap();
    assert_eq!(transmitted(&physical, 4), [0, 0, 0, 0x42]);
    received(&mut runtime, &physical, &[0, 0, 0, 0]);
    expire(&mut runtime, &physical);
    received(&mut runtime, &physical, &[0x08, 0, 0x80, 0x54]);
    assert_eq!(runtime.pop_publication(), None);
    expire(&mut runtime, &physical);
    received(&mut runtime, &physical, &[0, 0, 0, 0x42]);
    expire(&mut runtime, &physical);
    received(&mut runtime, &physical, &[0x08, 0, 0x80, 0x12]);
    assert!(matches!(runtime.pop_publication(), Some(Publication::Command { callback: CallbackId(9), status: Status::Complete, data: ReplyData::StatusOnly, .. })));
    assert_eq!(runtime.pop_publication(), None);
}
#[test]
fn stopped_pages_publish_native_error_status_without_driver_activity() {
    let (mut runtime, physical) = setup();
    runtime.submit(page(0x7254, 3, true), None).unwrap();
    assert_eq!(runtime.pop_publication(), Some(Publication::Read { callback: CallbackId(3), status: Status::InvalidReadChecksum, page: PageId::new(0) }));
    runtime.submit(page(0x7252, 4, true), None).unwrap();
    assert_eq!(runtime.pop_publication(), Some(Publication::Write { callback: CallbackId(4), status: Status::InvalidWriteMarker, next: PageId::new(0) }));
    assert!(!physical.acquired.get());
    assert!(physical.words.borrow().is_empty());
}
#[test]
fn read_checksum_and_acceptance_are_owned_by_the_queue_consumer() {
    let (mut runtime, physical) = setup();
    initialize(&mut runtime);
    runtime.submit(page(0x7254, 12, true), None).unwrap();
    received(&mut runtime, &physical, &[0x12, 0x34, 1, 2, 0x49]);
    assert_eq!(runtime.pop_publication(), Some(Publication::Read { callback: CallbackId(12), status: Status::Complete, page: PageId::new(0x1234) }));
    assert_eq!(&*runtime.read_data(), &[1, 2, 0x49]);
    assert!(matches!(runtime.submit(page(0x7254, 13, true), None), Err(RuntimeError::PendingPage)));
    runtime.complete_page(PageDisposition::Accept).unwrap();
    assert!(physical.timer.get().is_some());
    assert!(matches!(runtime.complete_page(PageDisposition::Accept), Err(RuntimeError::LateCompletion)));
}
#[test]
fn invalid_read_checksum_is_published_without_synthetic_success() {
    let (mut runtime, physical) = setup();
    initialize(&mut runtime);
    runtime.submit(page(0x7254, 12, true), None).unwrap();
    received(&mut runtime, &physical, &[0x12, 0x34, 1, 2, 0]);
    assert_eq!(runtime.pop_publication(), Some(Publication::Read { callback: CallbackId(12), status: Status::InvalidReadChecksum, page: PageId::new(0x1234) }));
    runtime.complete_page(PageDisposition::Stop).unwrap();
    assert_eq!(runtime.state(), LensServiceState::Stopped);
    assert!(matches!(runtime.pop_publication(), Some(Publication::Observer { callback: CallbackId(2), status: Status::Stopped, .. })));
}
#[test]
fn write_routes_owned_bytes_to_dma_and_returns_the_driver_next_index() {
    let (mut runtime, physical) = setup();
    initialize(&mut runtime);
    runtime.submit(page(0x7252, 21, true), Some(&[0xab, 0xcd])).unwrap();
    assert_eq!(transmitted(&physical, 5), [0x12, 0x34, 0xab, 0xcd, 0xbe]);
    received(&mut runtime, &physical, &[0x56, 0x78, 0, 0, 0x5a]);
    assert_eq!(runtime.pop_publication(), Some(Publication::Write { callback: CallbackId(21), status: Status::Complete, next: PageId::new(0x5678) }));
    runtime.complete_page(PageDisposition::Reject).unwrap();
    assert!(physical.timer.get().is_some());
}
#[test]
fn cancelled_wire_completion_cannot_complete_a_new_request() {
    let (mut runtime, physical) = setup();
    initialize(&mut runtime);
    runtime.submit(packet(0x7202, 9, &[0x40]), None).unwrap();
    let old = physical.wire.get().unwrap();
    runtime.cancel().unwrap();
    runtime.pop_publication();
    initialize(&mut runtime);
    runtime.submit(packet(0x7202, 10, &[0x40]), None).unwrap();
    assert!(matches!(runtime.wire_completed(old), Err(RuntimeError::Transport(xm1_lens_transport::Error::Backend(WireError::StaleToken)))));
    assert_eq!(runtime.pop_publication(), None);
    runtime.cancel().unwrap();
    assert!(matches!(runtime.pop_publication(), Some(Publication::Command { callback: CallbackId(10), status: Status::Stopped, .. })));
}
#[test]
fn denied_physical_resources_leave_the_actor_stopped() {
    let (mut runtime, physical) = setup();
    physical.deny_acquire.set(true);
    assert!(matches!(runtime.submit(QueueRequest::Initialize { callback: CallbackId(1), observer: CallbackId(2) }, None), Err(RuntimeError::Transport(xm1_lens_transport::Error::Backend(WireError::Platform(PhysicalError::Denied))))));
    assert_eq!(runtime.state(), LensServiceState::Stopped);
    assert_eq!(runtime.pop_publication(), None);
}
#[test]
fn invalid_boundary_packets_are_rejected_before_actor_dispatch() {
    assert_eq!(QueueRequest::parse(RequestId(0x723f), &[0; 47]), Err(packets::PacketError::Length));
    let mut bytes = [0; 48]; bytes[44] = 11;
    assert_eq!(QueueRequest::parse(RequestId(0x7202), &bytes), Err(packets::PacketError::CommandCount));
    assert_eq!(QueueRequest::parse(RequestId(0x7251), &[0; 8]), Err(packets::PacketError::Operation));
}

#[test]
fn callback_identity_is_not_reused_after_stop_and_reinitialize() {
    let (mut runtime, _) = setup();
    let first = runtime.allocate_callback().unwrap();
    initialize(&mut runtime);
    runtime.cancel().unwrap();
    initialize(&mut runtime);
    let second = runtime.allocate_callback().unwrap();
    assert_ne!(first, second);
    assert_ne!(first, CallbackId(0));
    assert_ne!(second, CallbackId(0));
}

#[test]
fn reinitialization_discards_the_previous_lens_timing_selection() {
    let group = |value| configuration::RawTimingGroup([value; 5]);
    let (mut runtime, _) = setup_from_configuration(configuration::LensSourceConfiguration {
        initial: group(1), baseline: group(2), primary: group(3), secondary: group(4),
        recovery: configuration::RecoveryConfiguration { delay: 1, recovery_limit: 1, busy_limit: 1, diagnostic: 0 },
        supported: configuration::SupportedCommands([0xff; 64]), extended_supported: configuration::SupportedCommands([0xff; 64]),
    });
    initialize(&mut runtime);
    runtime.finish_configuration(configuration::LensModelIdentifier(0)).unwrap();
    assert!(runtime.secondary_timing().is_some());
    runtime.cancel().unwrap();
    initialize(&mut runtime);
    assert_eq!(runtime.secondary_timing(), None);
}

#[test]
fn structured_replies_preserve_the_native_signed_and_bit_fields() {
    let operation = |value| ControlOperation::parse(RequestId(value)).unwrap();
    assert_eq!(replies::decode(operation(0x720f), 0xffff_0000), Some(ReplyData::FlaggedSigned { flag: true, value: -1 }));
    assert_eq!(replies::decode(operation(0x720e), 0xffff_0000), Some(ReplyData::FlaggedUnsigned { flag: true, value: replies::ReplyHalfword(0x3fff) }));
    assert_eq!(replies::decode(operation(0x723a), 0xffff_0000), Some(ReplyData::FlaggedUnsigned { flag: true, value: replies::ReplyHalfword(0x7fff) }));
    assert_eq!(replies::decode(operation(0x722e), 0xabcd_0000), Some(ReplyData::Channel { pages: 0x3cd, format: 5, present: true }));
    assert_eq!(replies::decode(operation(0x723b), 0xface_0000), Some(ReplyData::Halfword(replies::ReplyHalfword(0xface))));
    assert_eq!(replies::decode(operation(0x7244), 0xffff_0000), Some(ReplyData::StatusOnly));
    assert_eq!(replies::decode(operation(0x721b), 0xffff_0000), None);
}
