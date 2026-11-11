use super::*;
use super::super::discovery::{NetworkPacketPool, PacketStorage};

fn with_discovery(test: impl FnOnce(&mut NetworkDiscoveryRuntime<'_>)) {
    let mut buffers = [[0; 64]; 4];
    let [a, b, c, d] = &mut buffers;
    let mut storage = [PacketStorage::new(a), PacketStorage::new(b), PacketStorage::new(c), PacketStorage::new(d)];
    let packets = NetworkPacketPool::new(&mut storage);
    let mut objects = [];
    let mut scratch = [];
    let catalog = catalog::CatalogRuntime::new(&mut objects, &mut scratch);
    let mut discovery = NetworkDiscoveryRuntime::new(packets, catalog);
    test(&mut discovery);
}
fn configuration(discovery: &mut NetworkDiscoveryRuntime<'_>, name: &[u8], bytes: &[u8]) -> InterfaceConfiguration {
    InterfaceConfiguration {
        name: ObjectName::parse(name).unwrap(),
        timestamp: ObjectTimestamp::parse(b"26/11/10 12:34:56").unwrap(),
        format: ObjectFormat::Jpeg,
        payload: discovery.packets.insert(bytes).unwrap(),
        offset: 10,
        alternate: discovery.packets.insert(b"preview").unwrap(),
    }
}

#[test]
fn configure_control_mode_publishes_properties_and_retains_readable_packet_windows() {
    with_discovery(|discovery| {
        let mut modes = InterfaceModes::new();
        let configuration = configuration(discovery, b"DSCF0001.JPG", b"abcdef");
        modes.configure(InterfaceMode::Mode2, configuration, discovery).unwrap();
        assert_eq!(modes.transfer_data(InterfaceMode::Mode1, TransferOffset::Primary(2), Some(TransferLength(3)), &discovery.packets).unwrap(), b"cde");
        assert_eq!(modes.transfer_data(InterfaceMode::Mode2, TransferOffset::Alternate, None, &discovery.packets).unwrap(), b"preview");
        assert_eq!(modes.transfer_data(InterfaceMode::Mode1, TransferOffset::Primary(9), None, &discovery.packets), Err(InterfaceError::TransferBounds));
        let properties: Vec<_> = discovery.interface_control.take_properties().entries().map(|entry| (entry.code.wire(), entry.value)).collect();
        assert_eq!(properties, [(0xdf00, 1), (0xd220, 1)]);
        assert_eq!(modes.completion(), None);
        modes.teardown(InterfaceMode::Mode1, discovery).unwrap();
    });
}

#[test]
fn cancellation_retains_current_bytes_until_next_configuration_then_releases_previous_leases() {
    with_discovery(|discovery| {
        let mut modes = InterfaceModes::new();
        let first = configuration(discovery, b"FIRST.JPG", b"first");
        modes.configure(InterfaceMode::Mode1, first, discovery).unwrap();
        let generation = modes.configuration_generation(InterfaceMode::Mode1);
        modes.cancel_configure(InterfaceMode::Mode1, discovery).unwrap();
        assert_eq!(modes.configuration_generation(InterfaceMode::Mode2), generation);
        assert_eq!(modes.transfer_data(InterfaceMode::Mode1, TransferOffset::Primary(0), None, &discovery.packets).unwrap(), b"first");
        let second = configuration(discovery, b"SECOND.JPG", b"second");
        modes.configure(InterfaceMode::Mode2, second, discovery).unwrap();
        assert_ne!(modes.configuration_generation(InterfaceMode::Mode1), generation);
        assert_eq!(modes.transfer_data(InterfaceMode::Mode1, TransferOffset::Primary(0), None, &discovery.packets).unwrap(), b"second");
        let returned_primary = discovery.packets.insert(b"available").unwrap();
        let returned_alternate = discovery.packets.insert(b"available").unwrap();
        discovery.packets.release(returned_primary).unwrap();
        discovery.packets.release(returned_alternate).unwrap();
        modes.teardown(InterfaceMode::Mode1, discovery).unwrap();
    });
}

#[test]
fn mode_five_rejects_configuration_and_returns_both_packet_slots() {
    with_discovery(|discovery| {
        let mut modes = InterfaceModes::new();
        let config = configuration(discovery, b"REJECT.JPG", b"bytes");
        modes.configure(InterfaceMode::Mode5, config, discovery).unwrap();
        assert_eq!(modes.completion(), Some(InterfaceCompletion { kind: InterfaceCompletionKind::Configured, status: InterfaceStatus::Failed }));
        assert_eq!(modes.metadata(InterfaceMode::Mode5), None);
        let mut leases = Vec::new();
        for _ in 0..4 { leases.push(discovery.packets.insert(b"returned").unwrap()); }
        for lease in leases { discovery.packets.release(lease).unwrap(); }
    });
}

#[test]
fn pending_file_read_becomes_readable_only_after_actual_configuration_binding() {
    with_discovery(|discovery| {
        let mut modes = InterfaceModes::new();
        let request = InterfaceDataRequest { offset: TransferOffset::Primary(11), maximum: Some(TransferLength(2)) };
        modes.bind_data_request(InterfaceMode::Mode3, request);
        assert_eq!(modes.take_data_ready(), None);
        let config = configuration(discovery, b"READ.JPG", b"payload");
        modes.configure(InterfaceMode::Mode3, config, discovery).unwrap();
        let (mode, ready) = modes.take_data_ready().unwrap();
        assert_eq!(modes.transfer_data(mode, ready.offset, ready.maximum, &discovery.packets).unwrap(), b"ay");
        assert_eq!(modes.take_data_ready(), None);
        modes.teardown(InterfaceMode::Mode3, discovery).unwrap();
    });
}

#[test]
fn memory_object_deletion_removes_metadata_and_releases_both_windows() {
    with_discovery(|discovery| {
        let mut modes = InterfaceModes::new();
        let config = configuration(discovery, b"DELETE.JPG", b"bytes");
        modes.configure(InterfaceMode::Mode1, config, discovery).unwrap();
        let status = modes.delete_memory_object(InterfaceMode::Mode2, MemoryObject::Record1, &mut discovery.packets, &mut discovery.interface_control).unwrap();
        assert_eq!(status, InterfaceStatus::Success);
        assert_eq!(modes.metadata(InterfaceMode::Mode1), None);
        assert!(!discovery.interface_control.configuring());
        let status = modes.delete_memory_object(InterfaceMode::Mode1, MemoryObject::Record1, &mut discovery.packets, &mut discovery.interface_control).unwrap();
        assert_eq!(status.raw(), 0x80000100u32 as i32);
        let mut leases = Vec::new();
        for _ in 0..4 { leases.push(discovery.packets.insert(b"released").unwrap()); }
        for lease in leases { discovery.packets.release(lease).unwrap(); }
    });
}
