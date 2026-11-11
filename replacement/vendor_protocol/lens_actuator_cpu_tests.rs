use super::*;
use crate::lens_queue_runtime::physical_fixture;
use crate::lens_queue_runtime::replies::ReplyHalfword;
use std::vec::Vec;

fn words(request: ActuatorRequest) -> Vec<u32> {
    request.packet(CallbackId(77)).unwrap().words().iter().map(|word| word.raw()).collect()
}
fn options(mode: u8) -> FocusOptions { FocusOptions { first: true, second: false, third: true, mode: Selector(mode), speed: Selector(3) } }

#[test]
fn rate_and_percentage_use_distinct_wire_commands_and_preserve_full_target() {
    assert_eq!(words(ActuatorRequest::PositionRate { position: LensPosition(0xabcd), rate: LensRate(0x2345), flags: MotionFlags { first: true, second: false, third: true } }), [0xabcd5440, 0x1a2d5480]);
    assert_eq!(words(ActuatorRequest::PositionPercentage { percentage: LensPercentage(255), position: LensPosition(0xabcd) }), [0x00ff54c0, 0xabcd5500]);
}

#[test]
fn all_three_rate_flags_are_independent_and_rate_is_thirteen_bits() {
    for bits in 0..8 {
        let packet = words(ActuatorRequest::PositionRate { position: LensPosition(0), rate: LensRate(0xffff), flags: MotionFlags { first: bits & 1 != 0, second: bits & 2 != 0, third: bits & 4 != 0 } });
        let expected = [0xfff85480, 0xfff95480, 0xfffa5480, 0xfffb5480, 0xfffc5480, 0xfffd5480, 0xfffe5480, 0xffff5480];
        assert_eq!(packet[1], expected[bits]);
    }
}

#[test]
fn focus_encoding_changes_at_decimal_firmware_two_hundred() {
    assert_eq!(words(ActuatorRequest::Focus { drive: LensRate(0x9234), target: LensPosition(0xcafe), options: options(2), firmware: LensFirmwareVersion(199) }), [0x12341500, 0xb1011540, 0xcafe1580]);
    assert_eq!(words(ActuatorRequest::Focus { drive: LensRate(0x9234), target: LensPosition(0xcafe), options: options(2), firmware: LensFirmwareVersion(200) }), [0x12341500, 0x31211540, 0xcafe1580]);
    assert_eq!(words(ActuatorRequest::FocusMaximum { target: LensPosition(0xcafe), options: options(255), firmware: LensFirmwareVersion(199) }), [0x7fff1500, 0x31011540, 0xcafe1580]);
    assert_eq!(words(ActuatorRequest::FocusMaximum { target: LensPosition(0xcafe), options: options(255), firmware: LensFirmwareVersion(200) }), [0x7fff1500, 0x31f11540, 0xcafe1580]);
}

#[test]
fn iris_and_query_masks_are_command_specific() {
    assert_eq!(words(ActuatorRequest::IrisMove { position: LensPosition(0xffff), rate: LensRate(0xffff), options: IrisOptions { first: true, second: false, third: true, fourth: false } }), [0x7fff1840, 0xcfff1880]);
    assert_eq!(words(ActuatorRequest::QuerySigned { selector: Selector(0xfe), first: true, second: false }), [0xfe100840]);
    assert_eq!(words(ActuatorRequest::QueryUnsigned { selector: Selector(0xfe), first: false, second: true }), [0x0e010880]);
    assert_eq!(words(ActuatorRequest::QueryBytes { selector: Selector(0xfe), first: true, second: true }), [0x0e110900]);
    assert_eq!(words(ActuatorRequest::AuxiliaryFlags { first: true, second: false }), [0x01001e00]);
    assert_eq!(words(ActuatorRequest::Control { enabled: true, selector: Selector(0xff) }), [0x807f2800]);
}

#[test]
fn simple_operations_publish_their_expected_command() {
    for (request, operation, expected) in [
        (ActuatorRequest::Initialize { first: false, second: true }, 0x7218, 0x00021000),
        (ActuatorRequest::Home, 0x721a, 0x1400),
        (ActuatorRequest::FocusPosition(LensPosition(0xffff)), 0x721e, 0x7fff1500),
        (ActuatorRequest::FocusEnabled(true), 0x721f, 0x00011600),
        (ActuatorRequest::Iris, 0x7221, 0x1800),
        (ActuatorRequest::AuxiliaryHome, 0x7224, 0x1c00),
        (ActuatorRequest::AuxiliaryPosition(LensPosition(0xffff)), 0x7226, 0xffff1d40),
        (ActuatorRequest::QueryFlags, 0x7212, 0x9c0),
        (ActuatorRequest::QueryPosition, 0x723a, 0x4800),
        (ActuatorRequest::PositionEnabled(true), 0x7240, 0x00015400),
        (ActuatorRequest::PercentageEnabled(true), 0x7246, 0x00015800),
        (ActuatorRequest::AuxiliaryMode(Selector(0xff)), 0x722d, 0x000f2a40),
    ] {
        let packet = request.packet(CallbackId(77)).unwrap();
        assert_eq!(packet.operation.request_id(), RequestId(operation));
        assert_eq!(packet.callback, CallbackId(77));
        assert_eq!(words(request), [expected]);
    }
}

#[test]
fn stopped_queue_result_reaches_current_registered_owner_without_success() {
    let (mut queue, _) = physical_fixture::setup();
    let mut cpu = LensActuatorCpu::new();
    cpu.home(&mut queue, Some(OwnerToken(1))).unwrap();
    let previous = queue.pop_publication().unwrap();
    cpu.home(&mut queue, Some(OwnerToken(2))).unwrap();
    let completion = cpu.publication(previous).unwrap();
    assert_eq!(completion.owner, OwnerToken(2));
    assert_eq!(completion.operation, ActuatorOperation::Home);
    assert!(completion.failed);
    assert_eq!(completion.status, Status::Stopped);
    cpu.home(&mut queue, None).unwrap();
    assert_eq!(cpu.publication(previous), None);
}

#[test]
fn reset_and_cancel_reject_old_lifetimes_but_not_later_requests() {
    let (mut queue, _) = physical_fixture::setup();
    let mut cpu = LensActuatorCpu::new();
    cpu.iris(&mut queue, Some(OwnerToken(3))).unwrap();
    let previous = queue.pop_publication().unwrap();
    cpu.reset();
    cpu.iris(&mut queue, Some(OwnerToken(4))).unwrap();
    let current = queue.pop_publication().unwrap();
    assert_eq!(cpu.publication(previous), None);
    assert_eq!(cpu.publication(current).unwrap().owner, OwnerToken(4));
    cpu.cancel(&mut queue).unwrap();
    assert_eq!(cpu.publication(current), None);
}

#[test]
fn typed_reply_adapters_keep_signed_and_flagged_values() {
    let (mut queue, _) = physical_fixture::setup();
    let mut cpu = LensActuatorCpu::new();
    cpu.execute(&mut queue, Some(OwnerToken(1)), ActuatorRequest::QuerySigned { selector: Selector(0), first: false, second: false }).unwrap();
    let Publication::Command { callback, operation, .. } = queue.pop_publication().unwrap() else { panic!() };
    let result = cpu.publication(Publication::Command { callback, operation, status: Status::Driver(0x1234), data: ReplyData::Halfword(ReplyHalfword(0xff80)) }).unwrap();
    assert_eq!(result.data, ActuatorData::SignedPosition(-128));
    assert_eq!(result.status, Status::Driver(0x1234));
    assert!(result.failed);
    cpu.execute(&mut queue, Some(OwnerToken(2)), ActuatorRequest::QueryFlags).unwrap();
    let Publication::Command { callback, operation, .. } = queue.pop_publication().unwrap() else { panic!() };
    assert_eq!(cpu.publication(Publication::Command { callback, operation, status: Status::Complete, data: ReplyData::ThreeFlags([true, false, false]) }).unwrap().data, ActuatorData::Flags([false, true, false]));
}

#[test]
fn observer_is_one_shot_and_does_not_confuse_stale_or_dma_tokens() {
    let events = ActuatorEvents::new();
    events.arm(CallbackId(10));
    assert!(!events.signal(CallbackId(9)));
    assert!(events.signal(CallbackId(10)));
    assert!(!events.signal(CallbackId(10)));
    assert_eq!(events.take(), Some(CallbackId(10)));
    assert_eq!(events.take(), None);
    assert!(!events.signal(CallbackId(10)));
    events.arm(CallbackId(11));
    events.cancel();
    assert!(!events.signal(CallbackId(11)));
    events.arm(CallbackId(12));
    assert!(events.signal(CallbackId(12)));
    events.arm(CallbackId(13));
    assert_eq!(events.take(), None);
    assert!(events.signal(CallbackId(13)));
    assert_eq!(events.take(), Some(CallbackId(13)));
}

#[test]
fn missing_flags_snapshot_reorders_real_fields_and_preserves_failure() {
    use xm1_lens_transport::control_types::CompletionStatus;
    let snapshot = MissingFlagsSnapshot::from_bytes(CompletionStatus(-255), [0, 1, 1, 1, 0x80, 1, 1, 2, 1]);
    assert_eq!(snapshot.flags, [true, false, true]);
    assert_eq!(snapshot.status, CompletionStatus(-255));
    assert!(snapshot.failed);
    let snapshot = MissingFlagsSnapshot::from_bytes(CompletionStatus::SUCCESS, [1, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(snapshot.flags, [false, true, false]);
    assert!(!snapshot.failed);
}
