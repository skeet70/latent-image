use super::*;
use std::collections::BTreeMap;
use xm1_power_services::osal::{CpuCore, LockStamp, SavedInterrupts};

struct Hardware {
    bytes: BTreeMap<u32, u8>,
    interrupt_mask: SavedInterrupts,
    lock: LockStamp,
    nesting: u32,
    write_protected: bool,
}
impl Hardware {
    fn new() -> Self {
        Self { bytes: BTreeMap::new(), interrupt_mask: SavedInterrupts(0), lock: LockStamp(0), nesting: 0, write_protected: false }
    }
    fn install_word(&mut self, address: u32, value: u32) {
        for (index, byte) in value.to_le_bytes().into_iter().enumerate() { self.bytes.insert(address + index as u32, byte); }
    }
    fn word(&self, address: u32) -> u32 {
        u32::from_le_bytes(core::array::from_fn(|index| *self.bytes.get(&(address + index as u32)).expect("unmapped fixture word")))
    }
}
impl PhysicalBus for Hardware {
    fn read_word(&mut self, address: PhysicalAddress) -> u32 { self.word(address.get()) }
    fn write_word(&mut self, address: PhysicalAddress, value: u32) {
        let address = address.get();
        if (0xfff89000..0xfff89400).contains(&address) && (address - 0xfff89000) % 0x40 == 0 {
            let previous = self.word(address);
            let mask = value >> 16;
            self.install_word(address, (previous & !mask) | (value & mask));
        } else { self.install_word(address, value); }
    }
    fn read_byte(&mut self, address: PhysicalAddress) -> u8 { *self.bytes.get(&address.get()).expect("unmapped fixture byte") }
    fn write_byte(&mut self, address: PhysicalAddress, value: u8) {
        if !self.write_protected { self.bytes.insert(address.get(), value); }
    }
}
impl CriticalKernel for Hardware {
    fn core(&self) -> CpuCore { CpuCore::First }
    fn mask_interrupts(&mut self) -> SavedInterrupts { let saved = self.interrupt_mask; self.interrupt_mask = SavedInterrupts(0xc0); saved }
    fn restore_interrupts(&mut self, saved: SavedInterrupts) { self.interrupt_mask = saved; }
    fn observe_global_lock(&mut self) -> LockStamp { self.lock }
    fn try_global_lock(&mut self, stamp: LockStamp) -> bool {
        if self.lock != LockStamp(0) { return false; }
        self.lock = stamp;
        true
    }
    fn release_global_lock(&mut self) { self.lock = LockStamp(0); }
    fn nesting(&self) -> u32 { self.nesting }
    fn set_nesting(&mut self, depth: u32) { self.nesting = depth; }
}
fn images(bytes: &mut [u8]) -> ConfigImages<'_> { ConfigImages::adopt(&[], &[], &[], bytes, &[]) }
fn execute_action(action: u16, bytes: &mut [u8], bus: &mut Hardware, frame: PeripheralFrameResidue) -> Result<ActionCompletion, BackendError> {
    execute(LowAction::parse(action)?, &mut images(bytes), bus, &mut Hardware::new(), frame)
}
const FRAME: PeripheralFrameResidue = PeripheralFrameResidue { first_pin: 0x00070000, channel_halfword: 0x5ac3 };

#[test]
fn owned_immediate_actions_keep_their_distinct_notification_contracts() {
    for action in [7, 8, 11, 21, 30] {
        assert_eq!(execute_action(action, &mut [], &mut Hardware::new(), FRAME), Ok(ActionCompletion::service_success()));
    }
    for action in [12, 13] {
        assert_eq!(execute_action(action, &mut [], &mut Hardware::new(), FRAME), Ok(ActionCompletion::silent_success()));
    }
    assert_eq!(execute_action(3, &mut [], &mut Hardware::new(), FRAME), Ok(ActionCompletion { return_code: CpuReturn::SUCCESS, notification: ActionNotification::ScanEvent4(ResultFlags::FAILURE) }));
    for elsewhere in [0, 1, 9, 10, 14, 20, 22, 29, 32, 34, 35, 101, 144] { assert_eq!(LowAction::parse(elsewhere), Err(BackendError::InvalidCommand)); }
}

#[test]
fn gpio_updates_only_nonexcluded_pins_in_the_selected_bank() {
    let mut hardware = Hardware::new();
    for bank in 0..16 { hardware.install_word(0xfff89000 + bank * 0x40, 0xaaaa); }
    let mut bytes = [0u8; 139];
    bytes[129..134].copy_from_slice(&[3, 0x55, 0x55, 0x00, 0xff]);
    assert_eq!(execute_action(2, &mut bytes, &mut hardware, FRAME), Ok(ActionCompletion::service_success()));
    assert_eq!(hardware.word(0xfff890c0), 0x55aa);
    for bank in [0, 1, 2, 4, 15] { assert_eq!(hardware.word(0xfff89000 + bank * 0x40), 0xaaaa); }
    bytes[132..134].fill(0xff);
    assert_eq!(execute_action(2, &mut bytes, &mut hardware, FRAME), Ok(ActionCompletion::service_success()));
    assert_eq!(hardware.word(0xfff891c0), 0xaaaa);
}

#[test]
fn gpio_rejects_all_out_of_range_banks_without_accessing_hardware() {
    for bank in 16..=255 {
        let mut bytes = [0u8; 139];
        bytes[129] = bank;
        for action in [2, 4] { assert_eq!(execute_action(action, &mut bytes, &mut Hardware::new(), FRAME), Ok(ActionCompletion::service_rejected())); }
    }
}

#[test]
fn input_bank_and_channel_publish_only_the_low_halfword_in_wire_order() {
    for (action, selector, address) in [(4, 15, 0xfff893d4), (5, 11, 0xfff8a05c)] {
        let mut bytes = [0u8; 139];
        bytes[129] = selector;
        let mut hardware = Hardware::new();
        hardware.install_word(address, 0x9876abcd);
        assert_eq!(execute_action(action, &mut bytes, &mut hardware, FRAME), Ok(ActionCompletion::service_success()));
        assert_eq!(&bytes[130..132], &[0xab, 0xcd]);
    }
}

#[test]
fn invalid_channel_preserves_the_abi_stack_halfword_without_hardware_read() {
    for channel in 12..=255 {
        let mut bytes = [0u8; 139];
        bytes[129] = channel;
        assert_eq!(execute_action(5, &mut bytes, &mut Hardware::new(), FRAME), Ok(ActionCompletion::service_success()));
        assert_eq!(&bytes[130..132], &[0x5a, 0xc3]);
    }
}

#[test]
fn timer_channel_writes_are_low_ten_bits_and_require_live_running_state() {
    for state in [0, 1, 2, u32::MAX] {
        for channel in [0, 15, 16, 17, 255] {
            let mut bytes = [0u8; 139];
            bytes[129..132].copy_from_slice(&[channel, 0xda, 0xbc]);
            let mut hardware = Hardware::new();
            hardware.install_word(0x9743e8, state);
            hardware.install_word(0xfff86008, 0x155);
            assert_eq!(execute_action(6, &mut bytes, &mut hardware, FRAME), Ok(ActionCompletion::service_success()));
            assert_eq!(hardware.word(0xfff86008), if state == 1 && channel == 16 { 0x2bc } else { 0x155 });
        }
    }
}

fn memory_request(mode: u8, address: u32) -> [u8; 139] {
    let mut bytes = [0u8; 139];
    bytes[129] = mode;
    bytes[130] = 0x77;
    bytes[131..135].copy_from_slice(&address.to_be_bytes());
    bytes[135..139].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
    bytes
}

#[test]
fn memory_read_and_write_preserve_the_wire_big_endian_representation() {
    for address in [0, 0x1ffffffc, 0x40000000, 0x5ffffffc] {
        let mut hardware = Hardware::new();
        hardware.install_word(address, 0xa1b2c3d4);
        let mut bytes = memory_request(0, address);
        assert_eq!(execute_action(31, &mut bytes, &mut hardware, FRAME), Ok(ActionCompletion::service_success()));
        assert_eq!(&bytes[135..139], &[0xa1, 0xb2, 0xc3, 0xd4]);
        assert_eq!(bytes[130], 1);
        bytes[129] = 1;
        bytes[135..139].copy_from_slice(&[0x87, 0x65, 0x43, 0x21]);
        assert_eq!(execute_action(31, &mut bytes, &mut hardware, FRAME), Ok(ActionCompletion::service_success()));
        assert_eq!(hardware.word(address), 0x87654321);
        assert_eq!(bytes[130], 1);
    }
}

#[test]
fn memory_rejection_and_write_mismatch_are_reported_in_the_configuration_byte() {
    for address in [1, 3, 0x20000000, 0x3ffffffc, 0x60000000, 0xfffffffc] {
        let mut bytes = memory_request(0, address);
        assert_eq!(execute_action(31, &mut bytes, &mut Hardware::new(), FRAME), Ok(ActionCompletion::service_success()));
        assert_eq!(bytes[130], 0);
        assert_eq!(&bytes[135..139], &[0x12, 0x34, 0x56, 0x78]);
    }
    let mut hardware = Hardware::new();
    hardware.install_word(0x1000, 0);
    hardware.write_protected = true;
    let mut bytes = memory_request(1, 0x1000);
    assert_eq!(execute_action(31, &mut bytes, &mut hardware, FRAME), Ok(ActionCompletion::service_success()));
    assert_eq!(bytes[130], 0);
    assert_eq!(hardware.word(0x1000), 0);
}

#[test]
fn unknown_memory_mode_preserves_status_and_output_without_accessing_memory() {
    for mode in 2..=255 {
        let mut bytes = memory_request(mode, 0x1000);
        assert_eq!(execute_action(31, &mut bytes, &mut Hardware::new(), FRAME), Ok(ActionCompletion::service_success()));
        assert_eq!(bytes[130], 0x77);
        assert_eq!(&bytes[135..139], &[0x12, 0x34, 0x56, 0x78]);
    }
}

#[test]
fn incomplete_configuration_is_a_capacity_error_not_a_completed_action() {
    assert_eq!(execute_action(2, &mut [0; 129], &mut Hardware::new(), FRAME), Err(BackendError::Capacity));
    assert_eq!(execute_action(31, &mut [0; 134], &mut Hardware::new(), FRAME), Err(BackendError::Capacity));
}
