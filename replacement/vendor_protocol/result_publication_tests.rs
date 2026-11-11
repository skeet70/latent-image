use super::*;
use crate::{commands::{Commands, CompletionDisposition, ControllerState, VendorCompletion}, controller::{CompletionHandle, QueueController, RequestHandle}};
use xm1_power_services::shutdown::TaskId;

fn completion(payload: &[u8]) -> VendorCompletion { VendorCompletion::parse(payload.try_into().unwrap()) }

#[derive(Default)]
struct Queue {
    messages: std::vec::Vec<(RequestId, TaskId, QueueId, std::vec::Vec<u8>)>,
    failure: Option<KernelStatus>,
}
impl ShutdownRtos for Queue {
    fn current_task(&mut self) -> TaskId { TaskId(99) }
    fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
        if let Some(error) = self.failure { return Err(error); }
        self.messages.push((message.operation, message.sender, message.destination, message.payload.to_vec()));
        Ok(())
    }
    fn set_event_flag(&mut self, _: u32, _: u32) -> Result<(), KernelStatus> { unreachable!() }
}

#[test]
fn success_and_failure_publish_controller_consumable_messages() {
    let mut queue = Queue::default();
    for flags in [ResultFlags::SUCCESS, ResultFlags::FAILURE] {
        scan_send_event4(flags, &mut queue).unwrap();
    }
    assert_eq!(queue.messages.len(), 2);
    for ((operation, sender, destination, payload), flags) in queue.messages.iter().zip([ResultFlags::SUCCESS, ResultFlags::FAILURE]) {
        assert_eq!((*operation, *sender, *destination), (RequestId(0x3408), TaskId(99), QueueId(36)));
        assert_eq!(payload.len(), 24);
        assert_eq!(&payload[..8], &[4, 0, 0, 0, flags.get() as u8, (flags.get() >> 8) as u8, (flags.get() >> 16) as u8, (flags.get() >> 24) as u8]);
        let message = completion(payload);
        let mut controller = QueueController::new(ControllerState::PendingVendor, CompletionHandle(1), RequestHandle(2));
        let mut commands = Commands::new();
        let mut reply = [0xa5; 512];
        assert_eq!(controller.consume_completion(message, &mut commands, &mut reply, None), Ok(CompletionDisposition::Ignored));
        assert_eq!(controller.state(), ControllerState::ActiveVendor);
        assert_eq!(reply, [0xa5; 512]);
    }
}

#[test]
fn late_result_after_stop_leaves_reply_and_controller_stopped() {
    let mut queue = Queue::default();
    scan_send_event4(ResultFlags::FAILURE, &mut queue).unwrap();
    let message = completion(&queue.messages[0].3);
    let mut controller = QueueController::new(ControllerState::Stopped, CompletionHandle(1), RequestHandle(2));
    let mut commands = Commands::new();
    let mut reply = [0x5a; 512];
    assert_eq!(controller.consume_completion(message, &mut commands, &mut reply, None), Ok(CompletionDisposition::Ignored));
    assert_eq!(controller.state(), ControllerState::Stopped);
    assert_eq!(reply, [0x5a; 512]);
}

#[test]
fn service_completion_before_pending_is_deferred_and_consumed_once_pending() {
    let mut queue = Queue::default();
    scan_send_event4(ResultFlags::SUCCESS, &mut queue).unwrap();
    let message = completion(&queue.messages[0].3);
    let mut controller = QueueController::new(ControllerState::ActiveVendor, CompletionHandle(1), RequestHandle(3));
    let mut commands = Commands::new();
    let mut reply = [0; 512];
    assert_eq!(controller.consume_completion(message, &mut commands, &mut reply, None), Ok(CompletionDisposition::Deferred));
    assert_eq!(controller.state(), ControllerState::ActiveVendor);
    controller.transition(ControllerState::PendingVendor).unwrap();
    assert_eq!(controller.consume_completion(message, &mut commands, &mut reply, None), Ok(CompletionDisposition::Ignored));
    assert_eq!(controller.state(), ControllerState::ActiveVendor);
}

#[test]
fn queue_failure_does_not_report_publication_success() {
    let mut queue = Queue { failure: Some(KernelStatus(-9)), ..Queue::default() };
    assert_eq!(scan_send_event4(ResultFlags::SUCCESS, &mut queue), Err(KernelStatus(-9)));
    assert!(queue.messages.is_empty());
}

struct PendingConfiguration;
impl crate::commands::Backend for PendingConfiguration {
    fn configuration_capacity(&self) -> u32 { 256 }
    fn identity_byte(&self, _: u32) -> Option<u8> { unreachable!() }
    fn config_read(&mut self, _: crate::commands::ConfigNamespace, _: u32, _: &mut [u8]) -> crate::commands::BackendStatus { crate::commands::BackendStatus::Pending }
    fn config_write(&mut self, _: crate::commands::ConfigNamespace, _: u32, _: &[u8]) -> crate::commands::BackendStatus { unreachable!() }
    fn debug_query(&mut self, _: &mut [u8]) -> crate::commands::DebugQuery { unreachable!() }
    fn debug_read(&mut self, _: crate::commands::DebugAddress, _: &mut [u8]) -> crate::commands::BackendStatus { unreachable!() }
    fn debug_write(&mut self, _: crate::commands::DebugAddress, _: &[u8]) -> crate::commands::BackendStatus { unreachable!() }
    fn debug_execute(&mut self, _: &[u8]) -> crate::commands::BackendStatus { unreachable!() }
}

#[test]
fn queued_configuration_results_complete_actual_command_reply() {
    for (flags, expected_status) in [(ResultFlags::SUCCESS, [0; 8]), (ResultFlags::FAILURE, [1, 0, 0, 0, 255, 255, 0, 0])] {
        let mut commands = Commands::new();
        let mut request = [0; 512];
        request[..4].copy_from_slice(&0x1a0002u32.to_le_bytes());
        request[16..20].copy_from_slice(&2u32.to_le_bytes());
        request[32..36].copy_from_slice(&1u32.to_le_bytes());
        let mut reply = [0; 512];
        assert!(!commands.dispatch(&mut request, 36, &mut reply, &mut [], &mut PendingConfiguration).unwrap().ready);
        let mut queue = Queue::default();
        scan_send_event4(flags, &mut queue).unwrap();
        let message = completion(&queue.messages[0].3);
        let mut controller = QueueController::new(ControllerState::PendingVendor, CompletionHandle(1), RequestHandle(2));
        assert_eq!(controller.consume_completion(message, &mut commands, &mut reply, None), Ok(CompletionDisposition::Command { ready: true }));
        assert_eq!(&reply[4..12], &expected_status);
        assert_eq!(controller.state(), ControllerState::ActiveVendor);
    }
}

#[test]
fn cancellation_before_queued_delivery_preserves_pending_reply() {
    let mut commands = Commands::new();
    let mut request = [0; 512];
    request[..4].copy_from_slice(&0x1a0002u32.to_le_bytes());
    request[16..20].copy_from_slice(&2u32.to_le_bytes());
    let mut reply = [0; 512];
    assert!(!commands.dispatch(&mut request, 36, &mut reply, &mut [], &mut PendingConfiguration).unwrap().ready);
    let before = reply;
    let mut queue = Queue::default();
    scan_send_event4(ResultFlags::FAILURE, &mut queue).unwrap();
    let message = completion(&queue.messages[0].3);
    let mut controller = QueueController::new(ControllerState::PendingVendor, CompletionHandle(1), RequestHandle(2));
    controller.transition(ControllerState::Stopped).unwrap();
    assert_eq!(controller.consume_completion(message, &mut commands, &mut reply, None), Ok(CompletionDisposition::Ignored));
    assert_eq!(reply, before);
    assert_eq!(controller.state(), ControllerState::Stopped);
}
