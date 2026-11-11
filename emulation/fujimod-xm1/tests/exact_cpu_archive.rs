use fujimod_xm1::exact_cpu_archive::*;

fn encode<T: serde::Serialize>(value: &T) -> Vec<u8> {
    let mut bytes = Vec::new();
    ciborium::into_writer(value, &mut bytes).unwrap();
    bytes
}
fn reservation_bytes(epoch: u128, address: u64, size: u64) -> Vec<u8> {
    let mut bytes = b"FJEXCL01".to_vec();
    bytes.extend_from_slice(&epoch.to_le_bytes());
    bytes.extend_from_slice(&address.to_le_bytes());
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes
}
fn fixture() -> CpuState {
    let architecture = ArchitectureDocument {
        schema: 1,
        architecture: Architecture::CortexR5f(R5State {
            identity: [1, 2, 3, 4, 5, 6], cpsr: 0xd3, registers: [0; 16], scalar: [0; 4],
            vectors: [[0; 16]; 16],
            banks: std::array::from_fn(|i| Bank { registers: [0; 7], spsr: (!matches!(i, 0 | 6)).then_some(0) }),
            cp15: [[0; 2]; 16], mpu_selected: 0, mpu: [[0; 3]; 16],
        }),
    };
    let mut pages = b"FJMPAGE1".to_vec();
    pages.extend_from_slice(&4096u64.to_le_bytes());
    pages.extend_from_slice(&1u64.to_le_bytes());
    pages.extend_from_slice(&0u64.to_le_bytes());
    pages.extend_from_slice(&[0x37; 4096]);
    let mut absent = b"FJMPAGE1".to_vec();
    absent.extend_from_slice(&[0; 16]);
    CpuState {
        format: CpuFormat::RprofileR5fV1, context: encode(&architecture), identity_input: 0x81,
        clock: Some((19, 3, (24, 1))), clock_overshoot: 2, policy: Policy::Exact,
        pmu: Pmu { control: 1, enabled: 0x80000000, interrupts: 0, overflow: 0, frozen: 2, start: 3, observed: 4 },
        tcm: [Tcm { register: 0x7000000d, len: 4096, pages }, Tcm { register: 0, len: 0, pages: absent }],
        maps: vec![(0, Some(0), 16384, 7)], mmio: vec![(0xffff0000, 4096)],
        reservation: reservation_bytes((1u128 << 100) + 7, 0x3000, 4),
        ring: (0..64).collect(), budget: -3, head: 17, count: 11, cap: 23, synced: -5,
        settled_head: 13, thumb: false, restarted: Some(0x1000), user_stop: false, irq: true,
        hint: Some(2), exception: Some((1, 0xabcdef)), failure: Some("retained failure".into()),
    }
}

#[test]
fn separate_epoch_reservation_architectural_tcm_and_accounting_mutations_are_exact() {
    let original = fixture();
    let decoded = decode_cpu(&encode(&original)).unwrap();
    assert_eq!(decoded.reservation.epoch, (1u128 << 100) + 7);
    assert_eq!(decoded.reservation.reservation, Some(Reservation { address: 0x3000, size: 4 }));
    assert_eq!(decoded.tcm[0].pages[0].bytes, vec![0x37; 4096]);
    assert_eq!(decoded.state.budget, -3);
    assert_eq!(decoded.state.synced, -5);
    let mut epoch = original.clone();
    epoch.reservation[8..24].copy_from_slice(&((1u128 << 110) + 9).to_le_bytes());
    let changed = decode_cpu(&encode(&epoch)).unwrap();
    assert_eq!(changed.reservation.epoch, (1u128 << 110) + 9);
    assert_eq!(changed.architecture, decoded.architecture);
    let mut reservation = original.clone();
    reservation.reservation[24..32].copy_from_slice(&0x3010u64.to_le_bytes());
    let changed = decode_cpu(&encode(&reservation)).unwrap();
    assert_eq!(changed.reservation.reservation.unwrap().address, 0x3010);
    assert_eq!(changed.reservation.epoch, decoded.reservation.epoch);
    let mut architectural = original.clone();
    let mut architecture = decoded.architecture.clone();
    let Architecture::CortexR5f(registers) = &mut architecture.architecture;
    registers.registers[0] = 0x98765432;
    registers.vectors[15][15] = 0xfe;
    architectural.context = encode(&architecture);
    let changed = decode_cpu(&encode(&architectural)).unwrap();
    assert_eq!(changed.architecture, architecture);
    assert_eq!(changed.reservation, decoded.reservation);
    let mut tcm = original.clone();
    tcm.tcm[0].pages[32 + 101] = 0xff;
    let changed = decode_cpu(&encode(&tcm)).unwrap();
    assert_eq!(changed.tcm[0].pages[0].bytes[101], 0xff);
    assert_eq!(changed.architecture, decoded.architecture);
    let mut accounting = original.clone();
    accounting.budget = i64::MIN + 7;
    accounting.ring[63] = u64::MAX;
    accounting.pmu.observed = 0x123456789abcdef0;
    let changed = decode_cpu(&encode(&accounting)).unwrap();
    assert_eq!(changed.state.budget, i64::MIN + 7);
    assert_eq!(changed.state.ring[63], u64::MAX);
    assert_eq!(changed.state.pmu.observed, 0x123456789abcdef0);
    assert_eq!(changed.architecture, decoded.architecture);
    assert_eq!(changed.tcm, decoded.tcm);
    for changed in [epoch, reservation, architectural, tcm, accounting] {
        assert_ne!(encode(&changed), encode(&original));
        assert_ne!(blob_component_digest(&encode(&changed)), blob_component_digest(&encode(&original)));
    }
}

#[test]
fn shared_archive_retains_high_epoch_bits_and_each_core_reservation() {
    let mut bytes = b"FJMONI01".to_vec();
    let epoch = (1u128 << 127) + 0x1234;
    bytes.extend_from_slice(&epoch.to_le_bytes());
    bytes.extend_from_slice(&2u64.to_le_bytes());
    for (granule, address, size) in [(64u64, 0x3000u64, 4u64), (128, 0x4000, 8)] {
        for word in [granule, address, size] { bytes.extend_from_slice(&word.to_le_bytes()); }
    }
    let decoded = decode_exclusive(&bytes).unwrap();
    assert_eq!(decoded.epoch, epoch);
    assert_eq!(decoded.slots[0].granule, 64);
    assert_eq!(decoded.slots[1].granule, 128);
    assert_eq!(decoded.slots[1].reservation, Some(Reservation { address: 0x4000, size: 8 }));
    let mut changed = bytes.clone();
    changed[8] ^= 1;
    assert_eq!(decode_exclusive(&changed).unwrap().epoch, epoch ^ 1);
    changed = bytes.clone();
    changed[64..72].copy_from_slice(&0x5000u64.to_le_bytes());
    assert_eq!(decode_exclusive(&changed).unwrap().slots[1].reservation.as_ref().unwrap().address, 0x5000);
    for end in [0, 7, 24, 31, 79] { assert!(decode_exclusive(&bytes[..end]).is_err()); }
    bytes.push(0);
    assert!(decode_exclusive(&bytes).is_err());
}

#[test]
fn nested_architecture_tcm_and_reservation_decoders_reject_partial_unknown_or_trailing_state() {
    let original = fixture();
    for mutate in 0..6 {
        let mut state = original.clone();
        match mutate {
            0 => state.context.push(0),
            1 => state.reservation.push(0),
            2 => state.tcm[0].pages.push(0),
            3 => { state.ring.pop().unwrap(); }
            4 => state.hint = Some(4),
            5 => state.tcm[0].register = 0x70000001,
            _ => unreachable!(),
        }
        assert!(decode_cpu(&encode(&state)).is_err(), "{mutate}");
    }
    let mut architecture: ciborium::Value = ciborium::from_reader(original.context.as_slice()).unwrap();
    let ciborium::Value::Map(fields) = &mut architecture else { panic!("architecture document") };
    fields.push((ciborium::Value::Text("unknown".into()), ciborium::Value::Null));
    let mut changed = original;
    changed.context = encode(&architecture);
    assert!(decode_cpu(&encode(&changed)).is_err());
}
