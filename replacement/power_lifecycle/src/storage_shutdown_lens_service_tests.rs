use super::*;
use crate::shutdown::{KernelStatus, QueueId, RequestId, ServiceMessage, TaskId};
use crate::storage_shutdown_lifecycle::{LensPageOperation, LensTransferPlan};
use crate::storage_shutdown_load::LensChannel;
use crate::storage_shutdown_pages::{ConfigurationAddress, PageNumber};
use std::vec::Vec;

struct Rtos {
    task: TaskId,
    failure: Option<KernelStatus>,
    posted: Vec<(RequestId, TaskId, QueueId, Vec<u8>)>,
}
impl Rtos {
    fn new() -> Self { Self { task: TaskId(7), failure: None, posted: Vec::new() } }
}
impl ShutdownRtos for Rtos {
    fn current_task(&mut self) -> TaskId { self.task }
    fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
        if let Some(status) = self.failure { return Err(status); }
        self.posted.push((message.operation, message.sender, message.destination, message.payload.to_vec()));
        Ok(())
    }
    fn set_event_flag(&mut self, _: u32, _: u32) -> Result<(), KernelStatus> { Ok(()) }
}

struct Diagnostic {
    failed: Vec<LensServiceCommand>,
}
impl Diagnostic {
    fn new() -> Self { Self { failed: Vec::new() } }
}
impl LensPostDiagnostic for Diagnostic {
    fn send_failed(&mut self, operation: LensServiceCommand) { self.failed.push(operation) }
}

fn word(payload: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([payload[offset], payload[offset + 1], payload[offset + 2], payload[offset + 3]])
}
fn half(payload: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([payload[offset], payload[offset + 1]])
}

#[test]
fn select_write_posts_the_two_word_packet() {
    let mut rtos = Rtos::new();
    let mut payload = [0u8; 0x30];
    let mut diagnostic = Diagnostic::new();
    let plan = LensTransferPlan { pages: 0x123, operation: LensPageOperation::Bytes16, page_bytes: 16 };
    assert_eq!(select(&mut rtos, &mut payload, plan, true, LensCallback(0x44), &mut diagnostic), Ok(()));
    assert_eq!(rtos.posted.len(), 1);
    let (operation, sender, destination, posted) = &rtos.posted[0];
    assert_eq!((*operation, *sender, *destination), (RequestId(0x7258), TaskId(7), QueueId(98)));
    assert_eq!(word(posted, 0), 0x44);
    assert_eq!(half(posted, 4), 0x600);
    assert_eq!(half(posted, 6), 0xa123);
    assert_eq!(half(posted, 8), 0x2480);
    assert_eq!(posted[0x2c], 2);
    assert!(diagnostic.failed.is_empty());
}

#[test]
fn select_read_uses_the_read_control_word() {
    let mut rtos = Rtos::new();
    let mut payload = [0u8; 0x30];
    let mut diagnostic = Diagnostic::new();
    let plan = LensTransferPlan { pages: 1, operation: LensPageOperation::Bytes512, page_bytes: 512 };
    assert_eq!(select(&mut rtos, &mut payload, plan, false, LensCallback(0x44), &mut diagnostic), Ok(()));
    let (operation, _, _, posted) = &rtos.posted[0];
    assert_eq!(*operation, RequestId(0x7259));
    assert_eq!(half(posted, 6), 0xc801);
    assert_eq!(half(posted, 8), 0x2440);
}

#[test]
fn read_ready_posts_the_single_word_packet() {
    let mut rtos = Rtos::new();
    let mut payload = [0u8; 0x30];
    let mut diagnostic = Diagnostic::new();
    assert_eq!(read_ready(&mut rtos, &mut payload, LensCallback(0x44), &mut diagnostic), Ok(()));
    let (operation, _, _, posted) = &rtos.posted[0];
    assert_eq!(*operation, RequestId(0x7257));
    assert_eq!(word(posted, 0), 0x44);
    assert_eq!(half(posted, 4), 0x2400);
    assert_eq!(posted[0x2c], 1);
}

#[test]
fn read_page_posts_page_stack_and_operation() {
    let mut rtos = Rtos::new();
    let mut payload = [0u8; 0x14];
    let mut diagnostic = Diagnostic::new();
    let result = read_page(
        &mut rtos,
        &mut payload,
        PageNumber(0x1234),
        StackHalfword(0x5678),
        LensPageOperation::Bytes32,
        LensCallback(0x44),
        &mut diagnostic,
    );
    assert_eq!(result, Ok(()));
    let (operation, _, _, posted) = &rtos.posted[0];
    assert_eq!(*operation, RequestId(0x7254));
    assert_eq!(half(posted, 0), 0x1234);
    assert_eq!(half(posted, 2), 0x5678);
    assert_eq!(word(posted, 4), 5);
    assert_eq!(word(posted, 8), 1);
    assert_eq!(word(posted, 12), 1);
    assert_eq!(word(posted, 16), 0x44);
}

#[test]
fn write_page_posts_the_configuration_address() {
    let mut rtos = Rtos::new();
    let mut payload = [0u8; 0x14];
    let mut diagnostic = Diagnostic::new();
    let result = write_page(
        &mut rtos,
        &mut payload,
        PageNumber(0x1234),
        StackHalfword(0x5678),
        ConfigurationAddress(0x9abc),
        LensPageOperation::Bytes64,
        LensCallback(0x44),
        &mut diagnostic,
    );
    assert_eq!(result, Ok(()));
    let (operation, _, _, posted) = &rtos.posted[0];
    assert_eq!(*operation, RequestId(0x7252));
    assert_eq!(half(posted, 0), 0x1234);
    assert_eq!(half(posted, 2), 0x5678);
    assert_eq!(word(posted, 4), 0x9abc);
    assert_eq!(word(posted, 8), 6);
    assert_eq!(word(posted, 12), 1);
    assert_eq!(word(posted, 16), 0x44);
}

#[test]
fn prepare_auxiliary_posts_the_auxiliary_word() {
    let mut rtos = Rtos::new();
    let mut payload = [0u8; 0x30];
    let mut diagnostic = Diagnostic::new();
    assert_eq!(prepare_auxiliary(&mut rtos, &mut payload, LensCallback(0x44), &mut diagnostic), Ok(()));
    let (operation, _, _, posted) = &rtos.posted[0];
    assert_eq!(*operation, RequestId(0x7234));
    assert_eq!(half(posted, 4), 0x4540);
}

#[test]
fn extended_masks_the_mode_nibble() {
    for (mode, expected) in [(0x1f, 0x0f), (3, 3)] {
        let mut rtos = Rtos::new();
        let mut payload = [0u8; 0x30];
        let mut diagnostic = Diagnostic::new();
        assert_eq!(extended(&mut rtos, &mut payload, mode, LensCallback(0x44), &mut diagnostic), Ok(()));
        let (operation, _, _, posted) = &rtos.posted[0];
        assert_eq!(*operation, RequestId(0x7233));
        assert_eq!(half(posted, 4), 0x4500);
        assert_eq!(posted[6], expected);
    }
}

#[test]
fn channel_posts_the_channel_nibble() {
    for (selection, expected) in [(LensChannel::Header, 0), (LensChannel::Registered, 1), (LensChannel::Full, 6)] {
        let mut rtos = Rtos::new();
        let mut payload = [0u8; 0x30];
        let mut diagnostic = Diagnostic::new();
        assert_eq!(channel(&mut rtos, &mut payload, selection, LensCallback(0x44), &mut diagnostic), Ok(()));
        let (operation, _, _, posted) = &rtos.posted[0];
        assert_eq!(*operation, RequestId(0x722e));
        assert_eq!(half(posted, 4), 0x3000);
        assert_eq!(posted[6], expected);
    }
}

#[test]
fn prepare_primary_posts_the_two_residue_bytes() {
    let mut rtos = Rtos::new();
    let mut payload = [0u8; 0x30];
    let mut diagnostic = Diagnostic::new();
    assert_eq!(prepare_primary(&mut rtos, &mut payload, LensCallback(0x44), &mut diagnostic), Ok(()));
    let (operation, _, _, posted) = &rtos.posted[0];
    assert_eq!(*operation, RequestId(0x722b));
    assert_eq!(half(posted, 4), 0x2800);
    assert_eq!((posted[6], posted[7]), (0x10, 0x80));
}

#[test]
fn post_failure_is_reported_to_the_diagnostic() {
    let mut rtos = Rtos::new();
    rtos.failure = Some(KernelStatus(-20));
    let mut payload = [0u8; 0x30];
    let mut diagnostic = Diagnostic::new();
    let plan = LensTransferPlan { pages: 1, operation: LensPageOperation::Bytes16, page_bytes: 16 };
    assert_eq!(select(&mut rtos, &mut payload, plan, true, LensCallback(0x44), &mut diagnostic), Err(KernelStatus(-20)));
    assert!(rtos.posted.is_empty());
    assert_eq!(diagnostic.failed, std::vec![LensServiceCommand::SelectWrite]);
}

#[test]
fn service_commands_carry_their_operation_and_failure_codes() {
    for (command, operation, failure) in [
        (LensServiceCommand::SelectRead, 0x7259, 0x51),
        (LensServiceCommand::SelectWrite, 0x7258, 0x50),
        (LensServiceCommand::ReadReady, 0x7257, 0x4f),
        (LensServiceCommand::ReadPage, 0x7254, 0x4e),
        (LensServiceCommand::WritePage, 0x7252, 0x4d),
        (LensServiceCommand::PrepareAuxiliary, 0x7234, 0x2d),
        (LensServiceCommand::Extended, 0x7233, 0x2c),
        (LensServiceCommand::Channel, 0x722e, 0x28),
        (LensServiceCommand::PreparePrimary, 0x722b, 0x25),
    ] {
        assert_eq!(command.operation(), RequestId(operation));
        assert_eq!(command.failure_code(), failure);
    }
}
