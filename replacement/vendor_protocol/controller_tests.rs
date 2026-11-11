use super::*;
use crate::commands::{CompletionCode, PairCompletion, ProtocolCompletion, SingleCompletion};

fn controller(state: ControllerState, request: u32) -> QueueController {
    QueueController::new(state, CompletionHandle(2), RequestHandle(request))
}

fn completion(kind: u32, first: u32, second: u32) -> VendorCompletion {
    let mut payload = [0; 24];
    payload[..4].copy_from_slice(&kind.to_le_bytes());
    payload[4..8].copy_from_slice(&first.to_le_bytes());
    payload[8..12].copy_from_slice(&second.to_le_bytes());
    VendorCompletion::parse(&payload)
}

#[test]
fn request_handles_two_and_four_open_the_file_phase() {
    assert_eq!(RequestHandle(2).kind(), RequestKind::Protocol);
    assert_eq!(RequestHandle(3).kind(), RequestKind::Service);
    assert_eq!(RequestKind::parse(4), RequestKind::FileOnly);
    assert_eq!(RequestKind::parse(0), RequestKind::Unselected);
    assert_eq!([0, 2, 3, 4, 5].map(|raw| RequestKind::parse(raw).opens_files()), [false, true, false, true, false]);
}

#[test]
fn service_completion_in_the_active_state_waits_for_the_pending_state() {
    let mut controller = controller(ControllerState::ActiveVendor, 3);
    let mut commands = Commands::new();
    let mut reply = [0; 512];
    let completed = VendorCompletion::Command { status: CompletionCode(0) };
    assert_eq!(controller.consume_completion(completed, &mut commands, &mut reply, None), Ok(CompletionDisposition::Deferred));
    assert_eq!(controller.state(), ControllerState::ActiveVendor);
    controller.transition(ControllerState::PendingVendor).unwrap();
    assert_eq!(controller.consume_completion(completed, &mut commands, &mut reply, None), Ok(CompletionDisposition::Ignored));
    assert_eq!(controller.state(), ControllerState::ActiveVendor);
}

#[test]
fn protocol_completions_are_delivered_in_every_completing_state_without_deferral() {
    let read = ProtocolCompletion::Pair { kind: PairCompletion::Read, first: 0, second: 512 };
    for request in [2, 4] {
        for (state, after) in [
            (ControllerState::ActiveVendor, ControllerState::ActiveVendor), (ControllerState::PendingVendor, ControllerState::ActiveVendor),
            (ControllerState::State4, ControllerState::State4), (ControllerState::State7, ControllerState::State7),
        ] {
            let mut controller = controller(state, request);
            let mut reply = [0xa5; 512];
            assert_eq!(controller.consume_completion(completion(7, 0, 512), &mut Commands::new(), &mut reply, None), Ok(CompletionDisposition::Protocol(read)));
            assert_eq!(controller.state(), after);
            assert_eq!(reply, [0xa5; 512]);
        }
    }
    let mut controller = controller(ControllerState::State8, 2);
    assert_eq!(controller.consume_completion(completion(7, 0, 512), &mut Commands::new(), &mut [0; 512], None), Ok(CompletionDisposition::Ignored));
}

#[test]
fn completion_kinds_six_to_twenty_one_carry_the_words_stock_passes() {
    assert_eq!(completion(6, 1, 2), VendorCompletion::Protocol(ProtocolCompletion::Pair { kind: PairCompletion::Pair, first: 1, second: 2 }));
    assert_eq!(completion(8, 0, 9), VendorCompletion::Protocol(ProtocolCompletion::Pair { kind: PairCompletion::Write, first: 0, second: 9 }));
    let singles = [
        (9, SingleCompletion::Control), (10, SingleCompletion::Argument), (12, SingleCompletion::Argument12),
        (13, SingleCompletion::Argument13), (14, SingleCompletion::Argument14), (16, SingleCompletion::Path),
        (17, SingleCompletion::Argument17), (18, SingleCompletion::Directory), (19, SingleCompletion::Argument19),
        (20, SingleCompletion::Argument20), (21, SingleCompletion::Argument21),
    ];
    for (kind, expected) in singles {
        assert_eq!(completion(kind, 7, 0xffff), VendorCompletion::Protocol(ProtocolCompletion::Single { kind: expected, value: 7 }));
    }
    for kind in [0, 3, 11, 15, 22, u32::MAX] {
        assert_eq!(completion(kind, 7, 8), VendorCompletion::Ignored { kind });
    }
}

#[test]
fn only_kinds_six_and_ten_with_a_zero_first_word_leave_the_transport_unarmed() {
    assert!(!completion(6, 0, 5).rearms_transport());
    assert!(!completion(10, 0, 0).rearms_transport());
    for (kind, first) in [(6, 1), (10, 1), (7, 0), (4, 0), (5, 0), (11, 0), (21, 0)] {
        assert!(completion(kind, first, 0).rearms_transport());
    }
}

#[test]
fn late_completion_does_not_change_stopped_controller_or_reply() {
    let mut controller = controller(ControllerState::Stopped, 3);
    let mut commands = Commands::new();
    let mut reply = [0xa5; 512];
    let completed = VendorCompletion::Command { status: CompletionCode(1) };
    assert_eq!(controller.consume_completion(completed, &mut commands, &mut reply, None), Ok(CompletionDisposition::Ignored));
    assert_eq!(controller.state(), ControllerState::Stopped);
    assert_eq!(reply, [0xa5; 512]);
}

#[test]
fn ignored_vendor_kind_still_completes_controller_pending_phase() {
    let mut controller = controller(ControllerState::PendingVendor, 3);
    let mut commands = Commands::new();
    let mut reply = [0; 512];
    assert_eq!(controller.consume_completion(VendorCompletion::Ignored { kind: 15 }, &mut commands, &mut reply, None), Ok(CompletionDisposition::Ignored));
    assert_eq!(controller.state(), ControllerState::ActiveVendor);
}

#[derive(Default)]
struct Notifications { sent: std::vec::Vec<(RequestId, QueueId, std::vec::Vec<u8>)>, fail: bool }
impl ShutdownRtos for Notifications {
    fn current_task(&mut self) -> xm1_power_services::shutdown::TaskId { xm1_power_services::shutdown::TaskId(36) }
    fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
        if self.fail { return Err(KernelStatus(-9)); }
        assert_eq!(message.sender, xm1_power_services::shutdown::TaskId(36));
        self.sent.push((message.operation, message.destination, message.payload.to_vec()));
        Ok(())
    }
    fn set_event_flag(&mut self, _: u32, _: u32) -> Result<(), KernelStatus> { unreachable!() }
}

#[test]
fn installed_status_consumers_publish_distinct_task7_messages() {
    let mut notifications = Notifications::default();
    publish_controller_status(ControllerPublication::Completion { handle: CompletionHandle(21) }, &mut notifications).unwrap();
    publish_controller_status(ControllerPublication::Request { handle: RequestHandle(3) }, &mut notifications).unwrap();
    assert_eq!(notifications.sent, [
        (RequestId(0x1714), QueueId(7), [21, 0, 0, 0, 0, 0, 0, 0].to_vec()),
        (RequestId(0xb403), QueueId(7), [3, 0, 0, 0, 0, 0, 0, 0].to_vec()),
    ]);
}

#[test]
fn installed_status_consumer_propagates_queue_failure() {
    let mut notifications = Notifications { fail: true, ..Notifications::default() };
    assert_eq!(publish_controller_status(ControllerPublication::Request { handle: RequestHandle(2) }, &mut notifications), Err(KernelStatus(-9)));
    assert!(notifications.sent.is_empty());
}

#[test]
fn secondary_cancellation_uses_signed_four_byte_notification() {
    let mut notifications = Notifications::default();
    publish_secondary_status(xm1_power_services::primitives::ServiceStatus(-11), &mut notifications).unwrap();
    assert_eq!(notifications.sent, [(RequestId(0x171a), QueueId(7), [0xf5, 0xff, 0xff, 0xff].to_vec())]);
}
