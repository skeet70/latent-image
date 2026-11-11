use super::*;

struct SerialDevice { banks: [[u32; 8]; 2], output: [u32; 16], elapsed: u32, busy_reads: u32 }
impl SerialDevice { fn new() -> Self { Self { banks: [[0; 8]; 2], output: [0; 16], elapsed: 0, busy_reads: 0 } } }
impl Hardware for SerialDevice {
    fn read(&mut self, channel: Channel, register: Register) -> u32 {
        if register == Register::Control && self.busy_reads != 0 { self.busy_reads -= 1; return self.banks[channel.index()][0] | 1; }
        self.banks[channel.index()][register.offset() / 4]
    }
    fn write(&mut self, channel: Channel, register: Register, value: u32) {
        let word = &mut self.banks[channel.index()][register.offset() / 4];
        match register {
            Register::Status => *word = (*word & !(value & 0xffff)) | (value & 0xffff0000),
            Register::Control => *word = value & !1,
            _ => *word = value,
        }
    }
    fn select(&mut self, pin: Pin, active: bool) { self.output[pin.port as usize] = pin.output(active); }
    fn delay(&mut self, delay: Delay) { self.elapsed += delay.0 as u32; }
}
#[test]
fn charge_transfer_observes_device_flags_and_response() {
    let mut state = State::new(); let mut device = SerialDevice::new(); let mut sequence = 0;
    let handle = state.open(&mut device, Endpoint::Charge, OpenMode::Duplex, &mut sequence).unwrap();
    state.configure(&mut device, handle, Delay(100)).unwrap();
    state.write(&mut device, handle, SerialWord(0xb5), StatusResidue(0)).unwrap();
    assert_eq!(device.banks[0][7], 0xb5);
    assert_eq!(device.output[11], 1 << 18);
    assert_eq!(device.elapsed, 100);
    assert_eq!(state.status(&mut device, handle).unwrap().transmit_flag, 0);
    device.banks[0][3] = 1 << 9;
    device.banks[0][4] = 0x802;
    device.banks[0][6] = 0xdead1234;
    let status = state.status(&mut device, handle).unwrap();
    assert_eq!((status.transmit_flag, status.receive_threshold), (1, 2));
    assert_eq!(state.read(&mut device, handle), Ok(SerialWord(0x1234)));
    device.busy_reads = 3;
    state.stop(&mut device, handle).unwrap();
    assert_eq!(device.output[11], (1 << 18) | 4);
    state.close(&mut device, handle).unwrap();
    assert_eq!(state.read(&mut device, handle), Err(Error::InvalidHandle));
}
#[test]
fn audio_and_backup_share_channel_exclusively() {
    let mut state = State::new(); let mut device = SerialDevice::new(); let mut sequence = 0;
    let codec = state.open(&mut device, Endpoint::AudioCodec, OpenMode::Duplex, &mut sequence).unwrap();
    assert_eq!(device.banks[1][1] & 0x3ffff, 0x30004);
    assert_eq!(state.open(&mut device, Endpoint::Backup, OpenMode::Duplex, &mut sequence), Err(Error::InvalidHandle));
    assert_eq!(state.stop(&mut device, codec), Err(Error::AlreadyStopped));
    state.configure(&mut device, codec, Delay(1)).unwrap();
    assert_eq!(state.configure(&mut device, codec, Delay(1)), Err(Error::AlreadyStarted));
    state.stop(&mut device, codec).unwrap();
    state.close(&mut device, codec).unwrap();
    let backup = state.open(&mut device, Endpoint::Backup, OpenMode::Duplex, &mut sequence).unwrap();
    assert_ne!(codec, backup);
    assert_eq!(device.banks[1][1] & 0x3ffff, 6);
    assert_eq!(state.status(&mut device, codec), Err(Error::InvalidHandle));
}
#[test]
fn status_returns_all_seven_hardware_fields_without_completion_invention() {
    let mut state = State::new(); let mut device = SerialDevice::new(); let mut sequence = u32::MAX;
    let handle = state.open(&mut device, Endpoint::Backup, OpenMode::Transmit, &mut sequence).unwrap();
    assert_ne!(handle.raw(), 0);
    device.banks[1][3] = 0x1100;
    device.banks[1][4] = 31;
    device.banks[1][5] = 17;
    device.banks[1][2] = (1 << 18) | (1 << 26);
    assert_eq!(state.status(&mut device, handle), Ok(Status { receive_flag: 1, transmit_flag: 0, error_flag: 1, receive_threshold: 31, transmit_threshold: 17, receive_dma: 1, transmit_dma: 1 }));
    assert_eq!(Endpoint::parse(14), None);
}
#[test]
fn write_preserves_explicit_caller_residue_in_status_transaction() {
    let mut state = State::new(); let mut device = SerialDevice::new(); let mut sequence = 0;
    let handle = state.open(&mut device, Endpoint::Backup, OpenMode::Duplex, &mut sequence).unwrap();
    device.banks[1][3] = 0x80000002;
    state.write(&mut device, handle, SerialWord(0x1234), StatusResidue(0x40000002)).unwrap();
    assert_eq!(device.banks[1][3], 0xc0000000);
    assert_eq!(device.banks[1][7], 0x1234);
}
#[test]
fn lcd_panel_uses_bank_one_and_port_eleven_chip_select() {
    let mut state = State::new(); let mut device = SerialDevice::new(); let mut sequence = 0;
    let endpoint = Endpoint::parse(1).unwrap();
    let handle = state.open(&mut device, endpoint, OpenMode::from_abi(0), &mut sequence).unwrap();
    state.configure(&mut device, handle, Delay(1)).unwrap();
    state.write(&mut device, handle, SerialWord(0x601), StatusResidue(0x40)).unwrap();
    assert_eq!(device.banks[1][7], 0x601);
    assert_eq!(device.banks[1][1] & 0x3ffff, 6);
    assert_eq!(device.banks[1][0] & 0xf0, 0xe0);
    assert_eq!(device.output[11], 1 << 19);
    assert_eq!(device.output[12], 0);
    assert_eq!(state.open(&mut device, Endpoint::Backup, OpenMode::Duplex, &mut sequence), Err(Error::InvalidHandle));
    state.stop(&mut device, handle).unwrap();
    assert_eq!(device.output[11], (1 << 19) | 8);
    state.close(&mut device, handle).unwrap();
}

#[test]
fn configure_failure_residue_clears_the_additional_status_bits() {
    for (residue, remaining) in [(0, 0xccee), (0x16, 0xcce8)] {
        let mut state = State::new();
        let mut device = SerialDevice::new();
        let mut sequence = 0;
        let handle = state.open(&mut device, Endpoint::Backup, OpenMode::Duplex, &mut sequence).unwrap();
        device.banks[1][3] = 0xa500ffff;
        state.write(&mut device, handle, SerialWord(0x6800), StatusResidue(residue)).unwrap();
        assert_eq!(device.banks[1][3], 0xa5000000 | remaining);
        assert_eq!(device.banks[1][7], 0x6800);
    }
}

struct UnmappedSerial;
impl Hardware for UnmappedSerial {
    fn read(&mut self, _: Channel, _: Register) -> u32 { panic!("unmapped serial read") }
    fn write(&mut self, _: Channel, _: Register, _: u32) { panic!("unmapped serial write") }
    fn select(&mut self, _: Pin, _: bool) { panic!("unmapped chip select") }
    fn delay(&mut self, _: Delay) { panic!("unmapped delay") }
}

#[test]
fn adopted_phase_zero_rejects_data_operations_without_accessing_hardware() {
    let abi_words = [0u32, 0, 0, 0, u32::MAX, 9, u32::MAX, u32::MAX, 0, 37, 0, 0];
    let mut state: State = unsafe { core::mem::transmute(abi_words) };
    let handle = Handle::from_abi(37);
    assert_eq!(state.write(&mut UnmappedSerial, handle, SerialWord(0x1234), StatusResidue(0x16)), Err(Error::Closed));
    assert_eq!(state.read(&mut UnmappedSerial, handle), Err(Error::Closed));
    assert_eq!(state.status(&mut UnmappedSerial, handle), Err(Error::Closed));
    assert_eq!(state.configure(&mut UnmappedSerial, handle, Delay(1)), Err(Error::AlreadyStarted));
    let mut device = SerialDevice::new();
    state.stop(&mut device, handle).unwrap();
    device.banks[1][6] = 0x5678;
    assert_eq!(state.read(&mut device, handle), Ok(SerialWord(0x5678)));
    state.close(&mut device, handle).unwrap();
    assert_eq!(state.read(&mut UnmappedSerial, handle), Err(Error::InvalidHandle));
}
