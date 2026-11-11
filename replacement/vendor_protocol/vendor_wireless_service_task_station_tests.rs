use super::*;

fn slot(value: u32) -> SlotId { SlotId::new(value).unwrap() }

#[test]
fn socket_results_are_consumed_once_and_isolated_by_slot() {
    let mut runtime = SocketServiceRuntime::new();
    runtime.store_primary(slot(0), SocketResult { status: -50, value: 71 });
    runtime.store_primary(slot(9), SocketResult { status: 4, value: 81 });
    runtime.store_secondary(slot(0), SocketResult { status: 3, value: 91 });
    assert_eq!(runtime.take_primary(slot(0)), SocketResult { status: -50, value: 71 });
    assert_eq!(runtime.take_primary(slot(0)), SocketResult::default());
    assert_eq!(runtime.take_secondary(slot(0)), SocketResult { status: 3, value: 91 });
    assert_eq!(runtime.take_primary(slot(9)), SocketResult { status: 4, value: 81 });
}

#[test]
fn socket_lookup_excludes_opening_slots_and_uses_first_connected_match() {
    let mut runtime = SocketServiceRuntime::new();
    for index in 0..3 { runtime.set_handle(slot(index), 41); }
    runtime.set_phase(slot(0), SocketPhase::AwaitingOpen);
    runtime.set_phase(slot(1), SocketPhase::Connected);
    runtime.set_phase(slot(2), SocketPhase::Transferring);
    assert_eq!(runtime.find_socket(41), Some(slot(1)));
    assert_eq!(runtime.find_socket(42), None);
}

#[test]
fn station_lookup_uses_selected_station_slot_without_matching_handle() {
    let mut runtime = SocketServiceRuntime::new();
    runtime.set_station_socket(SocketPhase::Opening, slot(4));
    assert_eq!(runtime.find_socket(77), None);
    runtime.set_station_socket(SocketPhase::Connected, slot(4));
    assert_eq!(runtime.find_socket(77), Some(slot(4)));
}

#[test]
fn publication_sticks_to_first_selected_slot_and_reads_current_count() {
    let mut runtime = SocketServiceRuntime::new();
    let identity = SocketIdentity { name: [7; 32], address: [8; 6], kind: 2 };
    runtime.set_identity(slot(2), identity);
    assert_eq!(runtime.select_socket(slot(2), true, 3), Some(SocketPublication {
        slot: slot(2), count: 3, identity, enabled: true, disconnected: false,
    }));
    assert_eq!(runtime.select_socket(slot(3), true, 3), None);
    assert!(runtime.select_socket(slot(2), false, 0).unwrap().disconnected);
}

#[test]
fn slot_boundary_has_ten_entries() {
    assert!(SlotId::new(9).is_some());
    assert!(SlotId::new(10).is_none());
    assert!(SlotId::new(u32::MAX).is_none());
}

#[test]
fn queue_rejection_does_not_publish_a_started_socket() {
    let mut socket = SocketTaskRuntime::new();
    let transport_slot = SocketTaskSlot::parse(0).unwrap();
    while socket.close(transport_slot).is_ok() {}
    let mut runtime = StationServiceRuntime::new();
    assert_eq!(runtime.start(slot(0), &mut socket), Err(StationError::Socket(SocketTaskError::QueueFull)));
    assert_eq!(runtime.sockets.phase(slot(0)), SocketPhase::Inactive);
}

#[test]
fn repeat_start_while_waiting_for_open_is_handled_without_reopening() {
    let mut socket = SocketTaskRuntime::new();
    let mut runtime = StationServiceRuntime::new();
    assert_eq!(runtime.start(slot(0), &mut socket), Ok(true));
    assert_eq!(runtime.sockets.phase(slot(0)), SocketPhase::AwaitingOpen);
    while socket.close(SocketTaskSlot::parse(9).unwrap()).is_ok() {}
    assert_eq!(runtime.start(slot(0), &mut socket), Ok(true));
}

#[test]
fn completions_match_operation_identity_and_expected_result() {
    let mut socket = SocketTaskRuntime::new();
    let mut runtime = SocketServiceRuntime::new();
    let operation = runtime.send(slot(0), &[1, 2, 3], &mut socket).unwrap();
    let unrelated = socket.close(SocketTaskSlot::parse(0).unwrap()).unwrap();
    let completion = |operation, status| SocketTaskSocketCompletion {
        operation, slot: SocketTaskSlot::parse(0).unwrap(), status,
    };
    assert_eq!(runtime.completion(completion(unrelated, Ok(SocketTaskSocketResult::Sent(3)))), None);
    assert_eq!(runtime.completion(completion(operation, Ok(SocketTaskSocketResult::Configured))), None);
    assert_eq!(runtime.completion(completion(operation, Ok(SocketTaskSocketResult::Sent(3)))), Some((slot(0), ServiceSocketCompletion::Sent(Ok(())))));
    assert_eq!(runtime.completion(completion(operation, Ok(SocketTaskSocketResult::Sent(3)))), None);
}

fn with_discovery(test: impl FnOnce(&mut NetworkDiscoveryRuntime<'_>)) {
    use super::super::discovery::{NetworkPacketPool, PacketStorage};
    use super::super::interface_modes::catalog::CatalogRuntime;
    let mut bytes = [0u8; 256];
    let mut storage = [PacketStorage::new(&mut bytes)];
    let pool = NetworkPacketPool::new(&mut storage);
    let mut objects = [];
    let mut scratch = [];
    let catalog = CatalogRuntime::new(&mut objects, &mut scratch);
    let mut discovery = NetworkDiscoveryRuntime::new(pool, catalog);
    test(&mut discovery);
}

fn message(kind: u16, words: [u32; 3]) -> TaskMessage {
    use super::super::{TaskMessageKind, TaskPayload};
    let mut bytes = [0u8; 200];
    for (index, word) in words.into_iter().enumerate() { bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes()); }
    TaskMessage::new(TaskMessageKind(kind), TaskPayload::parse(&bytes).unwrap())
}

fn connect(runtime: &mut StationServiceRuntime, socket: &mut SocketTaskRuntime, discovery: &mut NetworkDiscoveryRuntime<'_>) {
    assert_eq!(runtime.start(slot(0), socket), Ok(true));
    assert_eq!(runtime.process(&mut message(0x6610, [0, 0, 77]), socket, discovery), Ok(false));
    assert_eq!(runtime.sockets.phase(slot(0)), SocketPhase::Connected);
}

#[test]
fn stop_waits_for_first_slot_before_touching_later_slots() {
    with_discovery(|discovery| {
        let mut socket = SocketTaskRuntime::new();
        let mut runtime = StationServiceRuntime::new();
        runtime.start(slot(0), &mut socket).unwrap();
        runtime.start(slot(1), &mut socket).unwrap();
        assert_eq!(runtime.process(&mut message(0x6603, [0; 3]), &mut socket, discovery), Ok(true));
        assert_eq!(runtime.sockets.phase(slot(0)), SocketPhase::Opening);
        assert_eq!(runtime.sockets.phase(slot(1)), SocketPhase::AwaitingOpen);
        assert_eq!(runtime.process(&mut message(0x6603, [0; 3]), &mut socket, discovery), Ok(true));
        assert_eq!(runtime.sockets.phase(slot(1)), SocketPhase::AwaitingOpen);
        runtime.process(&mut message(0x6611, [0, 0, 0]), &mut socket, discovery).unwrap();
        runtime.process(&mut message(0x6603, [0; 3]), &mut socket, discovery).unwrap();
        assert_eq!(runtime.sockets.phase(slot(1)), SocketPhase::Opening);
    });
}

#[test]
fn configuration_broadcast_starts_every_enabled_slot() {
    with_discovery(|discovery| {
        use super::super::{TaskMessageKind, TaskPayload};
        let mut bytes = [0u8; 200];
        bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
        bytes[28..32].copy_from_slice(&1u32.to_le_bytes());
        let mut configure = TaskMessage::new(TaskMessageKind(0x660f), TaskPayload::parse(&bytes).unwrap());
        let mut socket = SocketTaskRuntime::new();
        let mut runtime = StationServiceRuntime::new();
        assert_eq!(runtime.process(&mut configure, &mut socket, discovery), Ok(true));
        assert_eq!(runtime.sockets.phase(slot(0)), SocketPhase::AwaitingOpen);
        assert_eq!(runtime.sockets.phase(slot(1)), SocketPhase::AwaitingOpen);
        assert_eq!(runtime.sockets.phase(slot(2)), SocketPhase::Inactive);
    });
}

#[test]
fn reset_releases_exclusive_pending_packet_storage() {
    with_discovery(|discovery| {
        let mut socket = SocketTaskRuntime::new();
        let mut runtime = StationServiceRuntime::new();
        connect(&mut runtime, &mut socket, discovery);
        let packet = discovery.packets.insert(&[1, 2, 3]).unwrap();
        let mut send = message(0x6607, [77, 1, 3]).with_packet(packet);
        runtime.process(&mut send, &mut socket, discovery).unwrap();
        discovery.packets.release(send.packet.take().unwrap()).unwrap();
        let packet = discovery.packets.insert(&[4, 5, 6]).unwrap();
        let mut pending = message(0x6607, [77, 1, 3]).with_packet(packet);
        runtime.process(&mut pending, &mut socket, discovery).unwrap();
        assert!(pending.packet.is_none());
        assert_eq!(discovery.packets.insert(&[7]), Err(PacketPoolError::Full));
        runtime.reset(discovery).unwrap();
        let packet = discovery.packets.insert(&[7]).unwrap();
        assert_eq!(discovery.packets.bytes(&packet).unwrap(), &[7]);
        discovery.packets.release(packet).unwrap();
        assert_eq!(runtime.sockets.phase(slot(0)), SocketPhase::Inactive);
    });
}

#[test]
fn rejected_send_preserves_packet_and_connection_for_retry() {
    with_discovery(|discovery| {
        let mut socket = SocketTaskRuntime::new();
        let mut runtime = StationServiceRuntime::new();
        connect(&mut runtime, &mut socket, discovery);
        while socket.close(SocketTaskSlot::parse(9).unwrap()).is_ok() {}
        let packet = discovery.packets.insert(&[1, 2]).unwrap();
        let mut send = message(0x6607, [77, 1, 2]).with_packet(packet);
        assert_eq!(runtime.process(&mut send, &mut socket, discovery), Err(StationError::Socket(SocketTaskError::QueueFull)));
        assert!(send.packet.is_some());
        assert_eq!(runtime.sockets.phase(slot(0)), SocketPhase::Connected);
        discovery.packets.release(send.packet.take().unwrap()).unwrap();
    });
}

#[test]
fn packet_length_mismatch_is_rejected_before_socket_publication() {
    with_discovery(|discovery| {
        let mut socket = SocketTaskRuntime::new();
        let mut runtime = StationServiceRuntime::new();
        connect(&mut runtime, &mut socket, discovery);
        let packet = discovery.packets.insert(&[1]).unwrap();
        let mut send = message(0x6607, [77, 1, 2]).with_packet(packet);
        assert_eq!(runtime.process(&mut send, &mut socket, discovery), Err(StationError::InvalidPayload));
        assert_eq!(runtime.sockets.phase(slot(0)), SocketPhase::Connected);
        discovery.packets.release(send.packet.take().unwrap()).unwrap();
    });
}

#[test]
fn timer_callback_preserves_task_queue_order_and_retries_queue_full() {
    with_discovery(|discovery| {
        use super::super::TaskQueue;
        use super::super::discovery::LogicalSocketHandle;
        let mut storage = [None];
        let mut queue = TaskQueue::new(&mut storage).unwrap();
        let mut runtime = StationServiceRuntime::new();
        discovery.send_timer.arm(TransportTick(u32::MAX - 1000), LogicalSocketHandle(77));
        queue.enqueue(message(0x660a, [0; 3])).unwrap();
        assert_eq!(runtime.poll(TransportTick(998), &mut queue, discovery), Ok(false));
        assert!(runtime.poll(TransportTick(999), &mut queue, discovery).is_err());
        assert_eq!(queue.dequeue().unwrap().kind.0, 0x660a);
        assert_eq!(runtime.poll(TransportTick(999), &mut queue, discovery), Ok(true));
        let expired = queue.dequeue().unwrap();
        assert_eq!(expired.kind.0, 0x6614);
        assert_eq!(expired.payload.word(0).unwrap() as i32, -50);
        assert_eq!(expired.payload.word(4).unwrap(), 77);
        assert_eq!(runtime.poll(TransportTick(1000), &mut queue, discovery), Ok(false));
    });
}

#[test]
fn slot_configuration_publishes_source_peer_address_and_normalizes_device_kind() {
    let mut runtime = SocketServiceRuntime::new();
    runtime.configure_slot(slot(0), SocketConfiguration { words: [0x04030201, 0xabcd0605, 1, 9, 0] });
    let publication = runtime.select_socket(slot(0), true, 1).unwrap();
    assert_eq!(publication.identity.address, [1, 2, 3, 4, 5, 6]);
    assert_eq!(publication.identity.kind, 0);
}
