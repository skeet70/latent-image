use super::{ClockScratch, RandomError, RandomPool, RtosClock};
use xm1_rtos_own_first::{Cpsr, DebugIdentity, Domain, GuestAddress, Machine, ServiceStatus, SystemTime};
use core::cell::{Cell, RefCell};
use std::collections::BTreeMap;

struct Memory { words: RefCell<BTreeMap<u32, u32>>, cpsr: Cell<Cpsr> }
impl Memory {
    fn new(domain: Domain, time: u32) -> Self {
        let memory = Self { words: RefCell::new(BTreeMap::new()), cpsr: Cell::new(Cpsr(0x13)) };
        memory.store_word(domain.words().timer_base, time);
        memory
    }
}
impl Machine for Memory {
    fn load_word(&self, at: GuestAddress) -> u32 { self.words.borrow().get(&at.0).copied().unwrap_or(0) }
    fn load_byte(&self, at: GuestAddress) -> u8 { (self.load_word(GuestAddress(at.0 & !3)) >> ((at.0 & 3) * 8)) as u8 }
    fn store_word(&self, at: GuestAddress, value: u32) { self.words.borrow_mut().insert(at.0, value); }
    fn cpsr(&self) -> Cpsr { self.cpsr.get() }
    fn write_cpsr_control(&self, value: Cpsr) { self.cpsr.set(value); }
    fn debug_identity(&self) -> DebugIdentity { DebugIdentity::PRIMARY }
}

#[test]
fn clock_reads_selected_domain_with_wrapping_offset_and_preserves_irq_state() {
    let memory = Memory::new(Domain::B, u32::MAX - 2);
    memory.store_word(Domain::B.words().time_offset, 7);
    memory.store_word(Domain::A.words().timer_base, 99);
    let clock = RtosClock::new(&memory, Domain::B, ClockScratch::new(GuestAddress(0x2000)).unwrap());
    assert_eq!(clock.sample(), Ok(SystemTime(4)));
    assert_eq!(memory.cpsr(), Cpsr(0x13));
}

#[test]
fn context_failure_erases_output_instead_of_publishing_partial_nonce() {
    let memory = Memory::new(Domain::A, 123);
    memory.store_word(Domain::A.words().handler_context, 1);
    let clock = RtosClock::new(&memory, Domain::A, ClockScratch::new(GuestAddress(0x2000)).unwrap());
    let mut output = [0xa5; 32];
    assert_eq!(RandomPool::new().generate(&clock, &mut output), Err(RandomError::Clock(ServiceStatus::CONTEXT_ERROR)));
    assert_eq!(output, [0; 32]);
}

#[test]
fn source_pool_stream_agrees_across_full_extraction_boundaries() {
    let memory = Memory::new(Domain::A, 123);
    let clock = RtosClock::new(&memory, Domain::A, ClockScratch::new(GuestAddress(0x2000)).unwrap());
    let mut whole = RandomPool::new();
    let mut split = RandomPool::new();
    whole.add_randomness(&clock, b"synthetic peer").unwrap();
    split.add_randomness(&clock, b"synthetic peer").unwrap();
    let mut contiguous = [0; 32]; let mut pieces = [0; 32];
    whole.generate(&clock, &mut contiguous).unwrap();
    split.generate(&clock, &mut pieces[..16]).unwrap();
    split.generate(&clock, &mut pieces[16..]).unwrap();
    assert_eq!(contiguous, pieces);
    let mut later = [0; 32]; whole.generate(&clock, &mut later).unwrap();
    assert_ne!(contiguous, later);
}
