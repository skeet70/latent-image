#![forbid(unsafe_code)]

use xm1_saclac_config::{pipeline::Part, stock::GuestMemory as ConfigurationMemory};
use xm1_saclac_imaging::stock::arguments::GuestMemory as ArgumentMemory;
use xm1_saclac_shared::stock::{Argument::{Buffer, Word}, Machine};

use super::acquisition::Aligned;
fn record(client: u32, session: u32) -> Aligned<8> {
    let mut bytes = Aligned([0; 8]);
    bytes[..4].copy_from_slice(&client.to_le_bytes());
    bytes[4..].copy_from_slice(&session.to_le_bytes());
    bytes
}

pub fn acquire(machine: &impl Machine, client: u32, session: &mut u32) -> i32 {
    let mut bytes = record(client, *session);
    let status = machine.enter_with(0x65_df68, &[Buffer(0)], &mut bytes) as i32;
    *session = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    status
}
pub fn release(machine: &impl Machine, client: u32, session: u32) -> i32 { machine.enter(0x65_e0c8, &[client as usize, session as usize]) as i32 }
pub fn enable(machine: &impl Machine, client: u32, session: u32, enabled: bool) -> i32 {
    machine.enter_with(0x65_fcb4, &[Buffer(0), Word(u32::from(enabled))], &mut record(client, session)) as i32
}
pub fn reset(machine: &impl Machine) -> i32 { machine.enter(0x66_049c, &[]) as i32 }

pub fn program(machine: &impl Machine, client: u32, session: u32, part: Part, address: u32, value: u32) -> i32 {
    let (entry, arguments): (u32, &[xm1_saclac_shared::stock::Argument]) = match part {
        Part::InputMode => (0x65_f3d8, &[Buffer(0), Word(value)]),
        Part::InputControl => (0x65_fd70, &[Buffer(0), Word(address)]),
        Part::InputRegister3c => (0x66_032c, &[Buffer(0), Word(address)]),
        Part::InputRegister14 => (0x65_f51c, &[Buffer(0), Word(0), Word(address)]),
        Part::InputRegister34 | Part::InputRegister34Alternate => (0x66_0040, &[Buffer(0), Word(address)]),
        Part::InputRegister28 => (0x65_f9c8, &[Buffer(0), Word(address)]),
        _ => panic!("not a CCDIF part"),
    };
    machine.enter_with(entry, arguments, &mut record(client, session)) as i32
}

#[derive(Clone, Copy, Default)]
pub struct StageMemory;
impl ConfigurationMemory for StageMemory {
    fn byte(&self, at: u32) -> u8 { ConfigurationMemory::byte(&xm1_saclac_config::stock::StockMemory, at) }
    fn halfword(&self, at: u32) -> u16 { ConfigurationMemory::halfword(&xm1_saclac_config::stock::StockMemory, at) }
    fn word(&self, at: u32) -> u32 { ConfigurationMemory::word(&xm1_saclac_config::stock::StockMemory, at) }
    fn set_byte(&self, at: u32, value: u8) { ConfigurationMemory::set_byte(&xm1_saclac_config::stock::StockMemory, at, value) }
    fn set_halfword(&self, at: u32, value: u16) { ConfigurationMemory::set_halfword(&xm1_saclac_config::stock::StockMemory, at, value) }
    fn set_word(&self, at: u32, value: u32) { ConfigurationMemory::set_word(&xm1_saclac_config::stock::StockMemory, at, value) }
    fn fill(&self, at: u32, value: u8, count: u32) { ConfigurationMemory::fill(&xm1_saclac_config::stock::StockMemory, at, value, count) }
}
impl ArgumentMemory for StageMemory {
    fn byte(&self, at: u32) -> u8 { ConfigurationMemory::byte(self, at) }
    fn halfword(&self, at: u32) -> u16 { ConfigurationMemory::halfword(self, at) }
    fn word(&self, at: u32) -> u32 { ConfigurationMemory::word(self, at) }
    fn words(&self, at: u32, count: usize) -> &[u32] { xm1_saclac_imaging::stock::arguments::StockMemory.words(at, count) }
    fn table<const N: usize>(&self, at: u32) -> &[u32; N] { xm1_saclac_imaging::stock::arguments::StockMemory.table(at) }
}
