use super::*;

#[test]
fn discover_uses_subnet_broadcast_and_selected_port() {
    let packet = DiscoveryPacket::discover(Ipv4Address([192, 168, 1, 5]), Ipv4Mask([255, 255, 255, 0]), None, DiscoveryPort::Register).unwrap();
    assert_eq!(packet.as_bytes(), b"DISCOVER * HTTP/1.1\r\nMX:3\r\nHOST:192.168.1.255:51542\r\nDSCADDR:192.168.1.5\r\nSERVICE:PCAUTOSAVE/1.0\r\n");
}
#[test]
fn import_uses_fixed_host_port_and_requested_transfer_port() {
    let packet = DiscoveryPacket::import(Ipv4Address([10, 20, 30, 40]), DeviceName::parse(b"Camera-1").unwrap(), DiscoveryPort::Register).unwrap();
    assert_eq!(packet.as_bytes(), b"IMPORT /guest HTTP/1.1\r\nHOST:10.20.30.40:51541\r\nDSCNAME:Camera-1\r\nDSCPORT:51542\r\n");
}
#[test]
fn register_quotes_models_with_an_interior_space() {
    let packet = DiscoveryPacket::register(Ipv4Address([10, 0, 0, 1]), DeviceName::parse(b"Camera").unwrap(), DeviceModel::parse(b"FUJIFILM X-M1").unwrap()).unwrap();
    assert_eq!(packet.as_bytes(), b"REGISTER * HTTP/1.1\r\nHOST:10.0.0.1:51542\r\nDSCNAME:Camera\r\nDSCMODEL:\"FUJIFILM X-M1\"\r\n");
}
#[test]
fn device_name_rejects_invalid_prefix_and_internal_character_but_trims_suffix() {
    assert_eq!(DeviceName::parse(b" Camera"), Err(PacketError::Name));
    assert_eq!(DeviceName::parse(b"Cam era"), Err(PacketError::Name));
    assert_eq!(DeviceName::parse(b"Camera... ").unwrap().as_bytes(), b"Camera");
    assert_eq!(DeviceName::parse(b"0123456789012345678901234"), Err(PacketError::Name));
    assert_eq!(DeviceName::parse(b"1").unwrap().as_bytes(), b"1");
}
#[test]
fn discover_rejects_name_port_mismatch() {
    assert!(matches!(DiscoveryPacket::discover(Ipv4Address([1, 2, 3, 4]), Ipv4Mask([255; 4]), None, DiscoveryPort::Import), Err(PacketError::Name)));
}
#[test]
fn service_registry_preserves_first_match_and_replaces_previous_list() {
    let name = ServiceName::parse(b"desktop").unwrap();
    let mut registry = ServiceRegistry::new();
    let first = ServiceEntry { id: ServiceId(7), name, address: crate::vendor_wireless_network::MacAddress([1; 6]) };
    registry.replace(&[first, ServiceEntry { id: ServiceId(8), ..first }]).unwrap();
    assert_eq!(registry.find(name), Some(ServiceId(7)));
    registry.replace(&[]).unwrap();
    assert_eq!(registry.find(name), None);
}
