use super::*;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Owner { Boot, Query }
fn event(sequence: u8, status: u8) -> [u8; 12] { [12, 0, 1, 0, 1, sequence, 0, 0, 8, status, 0, 0] }
fn parsed_event(bytes: &[u8]) -> FirmwareEvent<'_> {
    match Packet::parse(bytes).unwrap() { Packet::Event(event) => event, _ => panic!() }
}
#[test]
fn event_sequence_comes_from_device_header_not_command_echo() {
    let mut pending = PendingCommands::<Owner, 2>::new();
    let id = pending.register(Owner::Query, &[8, 0, 0, 0x50, 8, 0, 42, 0]).unwrap();
    let wire = event(42, 0);
    let completed = pending.receive(&parsed_event(&wire)).unwrap().unwrap();
    assert_eq!(completed.id(), id);
    assert_eq!(completed.owner(), Owner::Query);
    assert_eq!(completed.result(), CommandResult::Accepted);
    assert!(pending.receive(&parsed_event(&wire)).unwrap().is_none());
}
#[test]
fn unrelated_and_cancelled_events_do_not_complete_commands() {
    let mut pending = PendingCommands::<Owner, 2>::new();
    let id = pending.register(Owner::Boot, &[8, 0, 0, 0x50, 1, 0, 9, 0]).unwrap();
    assert!(pending.receive(&parsed_event(&event(10, 0))).unwrap().is_none());
    assert_eq!(pending.cancel(id), Some(Owner::Boot));
    assert!(pending.receive(&parsed_event(&event(9, 0))).unwrap().is_none());
}
#[test]
fn firmware_rejection_and_unsupported_status_remain_distinct() {
    for (status, result) in [(1, CommandResult::Rejected), (2, CommandResult::Unsupported)] {
        let mut pending = PendingCommands::<Owner, 1>::new();
        pending.register(Owner::Query, &[8, 0, 0, 0x50, 8, 0, 1, 0]).unwrap();
        assert_eq!(pending.receive(&parsed_event(&event(1, status))).unwrap().unwrap().result(), result);
    }
}
#[test]
fn truncated_ack_does_not_consume_pending_owner() {
    let mut pending = PendingCommands::<Owner, 1>::new();
    pending.register(Owner::Boot, &[8, 0, 0, 0x50, 1, 0, 5, 0]).unwrap();
    let short = [8, 0, 1, 0, 1, 5, 0, 0];
    assert_eq!(pending.receive(&parsed_event(&short)), Err(ReceiveError::Truncated));
    assert!(pending.receive(&parsed_event(&event(5, 0))).unwrap().is_some());
}
#[test]
fn asynchronous_scan_is_not_command_acknowledgement() {
    let mut pending = PendingCommands::<Owner, 1>::new();
    pending.register(Owner::Boot, &[8, 0, 0, 0x50, 1, 0, 5, 0]).unwrap();
    let scan = [12, 0, 1, 0, 21, 5, 0, 0, 1, 0, 0, 0];
    assert_eq!(parsed_event(&scan).kind(), FirmwareEventKind::ScanComplete);
    assert!(pending.receive(&parsed_event(&scan)).unwrap().is_none());
    assert!(pending.receive(&parsed_event(&event(5, 0))).unwrap().is_some());
}
#[test]
fn aggregate_plan_selects_fifo_and_rounds_large_reads_to_blocks() {
    let mut snapshot = [0; 80];
    snapshot[14..16].copy_from_slice(&2u16.to_le_bytes());
    snapshot[48..50].copy_from_slice(&300u16.to_le_bytes());
    snapshot[50..52].copy_from_slice(&300u16.to_le_bytes());
    let plan = ReceivePlan::from_interrupt_snapshot(&snapshot, ReceivePort::One, 174).unwrap();
    assert_eq!(plan.packet_count(), 2);
    assert_eq!(plan.buffer_length(), 1024);
    assert_eq!(plan.argument(), 0x1800_6802);
}
#[test]
fn malformed_aggregate_sizes_are_rejected_before_host_request() {
    let mut snapshot = [0; 80];
    snapshot[12..14].copy_from_slice(&17u16.to_le_bytes());
    assert_eq!(ReceivePlan::from_interrupt_snapshot(&snapshot, ReceivePort::Zero, 174), Err(ReceiveError::Capacity));
    snapshot[12..14].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(ReceivePlan::from_interrupt_snapshot(&snapshot, ReceivePort::Zero, 174), Err(ReceiveError::InvalidLength));
}
#[test]
fn management_payload_and_status_follow_padding_field() {
    let mut wire = [0; 24];
    wire[..2].copy_from_slice(&19u16.to_le_bytes());
    wire[2] = 3;
    wire[4] = (24 << 2) | 1;
    wire[8] = 7;
    wire[9] = 100;
    wire[13..19].copy_from_slice(&[0xb0, 0, 1, 2, 3, 4]);
    wire[20..24].copy_from_slice(&0x12345678u32.to_le_bytes());
    let Packet::Management(frame) = Packet::parse(&wire).unwrap() else { panic!() };
    assert_eq!(frame.bytes(), &[0xb0, 0, 1, 2, 3, 4]);
    assert_eq!(frame.header_length(), 24);
    assert_eq!(frame.peer().get(), 7);
    assert_eq!(frame.signal().raw(), 100);
    assert_eq!(frame.status(), 0x12345678);
}
#[test]
fn failed_transfer_returns_original_buffer_without_parsing() {
    let mut snapshot = [0; 80];
    snapshot[12] = 1;
    snapshot[16] = 12;
    let plan = ReceivePlan::from_interrupt_snapshot(&snapshot, ReceivePort::Zero, 174).unwrap();
    let mut bytes = [0xa5; 16];
    let mut transfer = ReceiveTransfer::new(plan, &mut bytes).ok().unwrap();
    transfer.request().unwrap();
    assert_eq!(transfer.finish(Err(ReceiveError::Physical)), Err(ReceiveError::Physical));
    let Err((error, returned)) = transfer.into_batch() else { panic!() };
    assert_eq!(error, ReceiveError::Physical);
    assert_eq!(returned, &[0xa5; 16]);
}
#[test]
fn aggregate_iteration_releases_all_packet_storage() {
    let mut snapshot = [0; 80];
    snapshot[12] = 2;
    snapshot[16] = 12;
    snapshot[18] = 12;
    let plan = ReceivePlan::from_interrupt_snapshot(&snapshot, ReceivePort::Zero, 174).unwrap();
    let mut bytes = [0; 32];
    bytes[..12].copy_from_slice(&event(1, 0));
    bytes[16..28].copy_from_slice(&event(2, 1));
    let mut transfer = ReceiveTransfer::new(plan, &mut bytes).ok().unwrap();
    transfer.request().unwrap();
    transfer.finish(Ok(())).unwrap();
    let mut batch = transfer.into_batch().ok().unwrap();
    assert!(matches!(batch.next_packet(), Some(Ok(Packet::Event(_)))));
    assert!(matches!(batch.next_packet(), Some(Ok(Packet::Event(_)))));
    assert!(batch.next_packet().is_none());
    let returned = batch.release();
    returned.fill(0);
    assert_eq!(returned, &[0; 32]);
}

#[test]
fn teardown_revokes_pending_commands_without_lazy_draining() {
    let mut pending = PendingCommands::<Owner, 2>::new();
    pending.register(Owner::Query, &[8, 0, 0, 0x50, 1, 0, 9, 0]).unwrap();
    let returned = pending.teardown();
    assert_eq!(returned, [Some(Owner::Query), None]);
    assert!(pending.receive(&parsed_event(&event(9, 0))).unwrap().is_none());
}
#[test]
fn ownership_gate_emits_actual_register_requests_and_clears_on_driver_own_bit() {
    use gate::*;
    let mut gate = OwnershipGate::new(FirmwareOwnership::Firmware, Tick(10));
    let request = gate.request().unwrap();
    assert_eq!(request.argument.wire(), 0x1000_0804);
    let Buffer::Fifo(buffer) = request.buffer else { panic!() };
    buffer.copy_from_slice(&0u32.to_le_bytes());
    gate.finish(PhysicalOutcome::Completed, Tick(10), StopFlags { bus_removed: false, service_stopped: false }).unwrap();
    let request = gate.request().unwrap();
    assert_eq!(request.argument.wire(), 0x9000_0804);
    let Buffer::Fifo(buffer) = request.buffer else { panic!() };
    assert_eq!(buffer, &0x200u32.to_le_bytes());
    gate.finish(PhysicalOutcome::Completed, Tick(10), StopFlags { bus_removed: false, service_stopped: false }).unwrap();
    assert_eq!(gate.delay_ticks(), Some(1));
    gate.finish_delay().unwrap();
    let request = gate.request().unwrap();
    let Buffer::Fifo(buffer) = request.buffer else { panic!() };
    buffer.copy_from_slice(&0x100u32.to_le_bytes());
    gate.finish(PhysicalOutcome::Completed, Tick(11), StopFlags { bus_removed: false, service_stopped: false }).unwrap();
    assert_eq!(gate.outcome(), Some(Ok(())));
    assert_eq!(gate.ownership(), FirmwareOwnership::Driver);
    assert!(gate.request().is_none());
}
#[test]
fn ownership_timeout_uses_wrapping_elapsed_ticks() {
    use gate::*;
    let mut gate = OwnershipGate::new(FirmwareOwnership::Firmware, Tick(u32::MAX - 4));
    gate.request().unwrap();
    gate.finish(PhysicalOutcome::Failed, Tick(8190), StopFlags { bus_removed: false, service_stopped: false }).unwrap();
    assert_eq!(gate.outcome(), Some(Err(GateError::Timeout)));
}

#[test]
fn snapshot_read_and_dispatch_preserve_source_action_order() {
    use interrupt::*;
    let mut snapshot = SnapshotRead::new(InterruptPolicy { mailbox_enabled: true, receive_suppressed: false, bus_removed: false, service_stopped: false });
    let request = snapshot.request().unwrap();
    assert_eq!(request.argument.wire(), 0x1000_2058);
    let Buffer::Fifo(buffer) = request.buffer else { panic!() };
    assert_eq!(buffer.len(), 88);
    buffer[0] = 8 | 2;
    buffer[4] = 1;
    buffer[12] = 1;
    buffer[16] = 12;
    buffer[80] = 1;
    snapshot.finish(Ok(())).unwrap();
    assert_eq!(snapshot.next_action(), Some(InterruptAction::ReadAbnormalStatus));
    assert_eq!(snapshot.next_action(), Some(InterruptAction::ReturnedCredits([1, 0, 0, 0, 0, 0, 0, 0])));
    assert_eq!(snapshot.next_action(), Some(InterruptAction::ReceiveQueues));
    assert_eq!(snapshot.next_action(), None);
    assert_eq!(snapshot.remaining().wire(), 0);
    assert_eq!(snapshot.plan(ReceivePort::Zero, 174).unwrap().packet_count(), 1);
    snapshot.finish_port(ReceivePort::Zero).unwrap();
    assert_eq!(snapshot.plan(ReceivePort::Zero, 174).unwrap().packet_count(), 0);
}

#[test]
fn snapshot_cancel_rejects_late_physical_receipt() {
    use interrupt::*;
    let mut snapshot = SnapshotRead::new(InterruptPolicy { mailbox_enabled: false, receive_suppressed: false, bus_removed: false, service_stopped: false });
    snapshot.request().unwrap();
    snapshot.cancel();
    assert_eq!(snapshot.finish(Ok(())), Err(ReceiveError::Stale));
    assert_eq!(snapshot.next_action(), None);
}

#[test]
fn interrupt_restore_combines_enable_and_firmware_sleep_requests() {
    use interrupt::*;
    let mut control = InterruptControl::new(InterruptEnable::Disabled);
    let request = control.enable().unwrap();
    assert_eq!(request.operation, Operation::Direct52);
    assert_eq!(request.argument.wire(), 0x9000_0801);
    control.request_sleep();
    let request = control.enable().unwrap();
    assert_eq!(request.operation, Operation::Extended53);
    let Buffer::Fifo(buffer) = request.buffer else { panic!() };
    assert_eq!(buffer, &0x100u32.to_le_bytes());
    control.disable();
    control.request_sleep();
    let request = control.enable().unwrap();
    let Buffer::Fifo(buffer) = request.buffer else { panic!() };
    assert_eq!(buffer, &0x101u32.to_le_bytes());
}

#[test]
fn validated_extent_includes_status_without_exposing_block_padding() {
    let mut snapshot = [0; 88];
    snapshot[12] = 2;
    snapshot[16] = 12;
    snapshot[18] = 13;
    let plan = ReceivePlan::from_interrupt_snapshot(&snapshot, ReceivePort::Zero, 174).unwrap();
    let first = plan.packet_extent(0).unwrap();
    let second = plan.packet_extent(1).unwrap();
    assert_eq!(first.range(), 0..16);
    assert_eq!(first.length(), 12);
    assert_eq!(second.range(), 16..36);
    assert_eq!(second.length(), 13);
    assert_eq!(second.stride(), 20);
    assert!(plan.packet_extent(2).is_none());
}
