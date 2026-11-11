use super::*;

/// (address, bmAttributes, wMaxPacketSize, bInterval)
type Endpoint = (u8, u8, u16, u8);

/// A configuration descriptor with one interface descriptor per
/// (number, alternate, class triple, endpoints) entry.
fn configuration(value: u8, interfaces: &[(u8, u8, [u8; 3], &[Endpoint])]) -> Vec<u8> {
    let mut numbers: Vec<u8> = interfaces.iter().map(|interface| interface.0).collect();
    numbers.dedup();
    let mut bytes = vec![9, 2, 0, 0, numbers.len() as u8, value, 0, 0x80, 50];
    for &(number, alternate, [class, subclass, protocol], endpoints) in interfaces {
        bytes.extend([9, 4, number, alternate, endpoints.len() as u8, class, subclass, protocol, 0]);
        for &(address, attributes, packet, interval) in endpoints {
            let [low, high] = packet.to_le_bytes();
            bytes.extend([7, 5, address, attributes, low, high, interval]);
        }
    }
    let [low, high] = (bytes.len() as u16).to_le_bytes();
    (bytes[2], bytes[3]) = (low, high);
    bytes
}

fn fields_of(bytes: &[u8]) -> Result<[c_int; FIELDS], c_int> {
    let mut fields = [0; FIELDS];
    descriptor_fields(&ConfigurationDescriptor::new(bytes).expect("well-formed configuration"), &mut fields)?;
    Ok(fields)
}

const VENDOR_CLASS: [u8; 3] = [0xff, 0, 0];

#[test]
fn listing_describes_eligible_devices_only() {
    let devices = [(0x1d6b_u16, 0x0002_u16), (VENDOR, PRODUCT)];
    let mut described = Vec::new();
    let entries = inventory(&devices, |device| *device, |device| {
        described.push(*device);
        let mut entry = Entry::ineligible(device.0, device.1);
        entry.path = device_path(1, &[2, 3])?;
        Ok(entry)
    })
    .unwrap();
    assert_eq!(described, [(VENDOR, PRODUCT)]);
    assert_eq!(entries[0], Entry::ineligible(0x1d6b, 0x0002));
    assert_eq!(entries[0].path, "");
    assert_eq!(entries[1].path, "001:2.3");
    assert!(entries[1].eligible() && !entries[0].eligible());
}

#[test]
fn listing_refuses_more_than_64_devices() {
    let devices = [(0_u16, 0_u16); MAX_DEVICES + 1];
    assert_eq!(inventory(&devices, |device| *device, |_| unreachable!()), Err(refusal::TOO_MANY));
}

#[test]
fn a_failed_description_fails_the_listing() {
    let devices = [(VENDOR, PRODUCT)];
    assert_eq!(inventory(&devices, |device| *device, |_| Err(refusal::DESCRIPTORS)), Err(refusal::DESCRIPTORS));
}

#[test]
fn paths_are_bus_and_port_chain() {
    assert_eq!(device_path(5, &[1]).unwrap(), "005:1");
    assert_eq!(device_path(255, &[1, 2, 3, 4, 5, 6, 7, 8]).unwrap(), "255:1.2.3.4.5.6.7.8");
    assert_eq!(device_path(1, &[]), Err(refusal::PORTS));
    assert_eq!(device_path(1, &[1; 9]), Err(refusal::PORTS));
}

#[test]
fn extra_endpoints_and_bulk_pair_fill_their_slots() {
    let bytes = configuration(
        1,
        &[(0, 0, VENDOR_CLASS, &[(0x81, 2, 512, 2), (0x02, 2, 512, 1), (0x83, 3, 16, 8)])],
    );
    let fields = fields_of(&bytes).unwrap();
    assert_eq!(fields[12..=16], [1, 0x83, 3, 16, 8]);
    assert_eq!((fields[8], fields[10], fields[37]), (0x81, 512, 2));
    assert_eq!((fields[9], fields[11], fields[38]), (0x02, 512, 1));
}

/// The S01 jig-mode layout (E-JIG-USB-DESCRIPTORS), OUT listed before IN.
#[test]
fn jig_layout_fills_every_slot_the_haskell_provider_reads() {
    let bytes = configuration(
        1,
        &[(0, 0, [0xff, 0x21, 0xff], &[(0x01, 2, 512, 1), (0x82, 2, 512, 0), (0x83, 3, 8, 4)])],
    );
    let fields = fields_of(&bytes).unwrap();
    let expected = [
        (2, 1), (3, 0), (4, 0), (5, 0xff), (6, 0x21), (7, 0xff), (8, 0x82), (9, 0x01), (10, 512), (11, 512),
        (12, 1), (13, 0x83), (14, 3), (15, 8), (16, 4), (37, 0), (38, 1),
    ];
    for (slot, value) in expected {
        assert_eq!(fields[slot], value, "slot {slot}");
    }
}

#[test]
fn other_descriptor_shapes_are_refused() {
    let pair: &[Endpoint] = &[(0x81, 2, 512, 0), (0x02, 2, 512, 0)];
    let refused: [Vec<u8>; 7] = [
        configuration(1, &[(0, 0, VENDOR_CLASS, pair), (1, 0, VENDOR_CLASS, pair)]),
        configuration(1, &[(0, 0, VENDOR_CLASS, pair), (0, 1, VENDOR_CLASS, pair)]),
        configuration(1, &[(1, 0, VENDOR_CLASS, pair)]),
        configuration(1, &[(0, 0, VENDOR_CLASS, &[(0x81, 2, 512, 0)])]),
        configuration(1, &[(0, 0, VENDOR_CLASS, &[(0x81, 2, 512, 0), (0x82, 2, 512, 0)])]),
        configuration(1, &[(0, 0, VENDOR_CLASS, &[(0x81, 0x12, 512, 0), (0x02, 2, 512, 0)])]),
        configuration(
            1,
            &[(
                0,
                0,
                VENDOR_CLASS,
                &[(0x81, 2, 512, 0), (0x02, 2, 512, 0), (0x83, 3, 8, 4), (0x84, 3, 8, 4), (0x85, 3, 8, 4), (0x86, 3, 8, 4), (0x87, 3, 8, 4), (0x88, 3, 8, 4), (0x89, 3, 8, 4)],
            )],
        ),
    ];
    for (index, bytes) in refused.iter().enumerate() {
        assert_eq!(fields_of(bytes), Err(refusal::DESCRIPTORS), "shape {index}");
    }
}

#[test]
fn sysfs_descriptors_yield_each_configuration() {
    let pair: &[Endpoint] = &[(0x81, 2, 512, 0), (0x02, 2, 512, 0)];
    let mut descriptors = vec![18, 1];
    descriptors.resize(18, 0);
    descriptors.extend(configuration(1, &[(0, 0, VENDOR_CLASS, pair)]));
    descriptors.extend(configuration(2, &[(0, 0, [0xff, 0x21, 0xff], pair)]));
    let values: Vec<u8> = configurations(&descriptors).map(|configuration| configuration.configuration_value()).collect();
    assert_eq!(values, [1, 2]);
    assert_eq!(configurations(&descriptors[..20]).count(), 0);
}

#[test]
fn a_bound_driver_refuses_the_claim_before_claiming() {
    let mut claimed = false;
    assert_eq!(claim(Ok(true), || { claimed = true; Ok(()) }), Err(refusal::DRIVER_BOUND));
    assert!(!claimed);
    assert_eq!(claim(Err(libusb::NO_DEVICE), || -> Result<(), c_int> { unreachable!() }), Err(libusb::NO_DEVICE));
    assert_eq!(claim(Ok(false), || Ok(7)), Ok(7));
}

#[test]
fn partial_transfers_keep_their_bytes_and_status() {
    assert_eq!(completion_outcome(Err(TransferError::Cancelled), 3, 128), (libusb::TIMEOUT, 3));
    assert_eq!(completion_outcome(Err(TransferError::Stall), 0, 12), (libusb::PIPE, 0));
    assert_eq!(completion_outcome(Ok(()), 12, 12), (0, 12));
    assert_eq!(completion_outcome(Ok(()), 512, 128), (libusb::OVERFLOW, 128));
}

#[test]
fn size_gate_admits_one_chunk_in_and_292_bytes_otherwise() {
    for (kind, size, admitted) in [
        (2, 1, true), (2, 0x10_0000, true), (2, 0x10_0001, false), (2, 0, false),
        (1, 292, true), (1, 293, false), (1, 0x10_0000, false), (0, 12, true), (0, 293, false), (0, 0x10_0000, false),
    ] {
        assert_eq!(size_admitted(kind, size), admitted, "kind {kind}, {size} bytes");
    }
}
