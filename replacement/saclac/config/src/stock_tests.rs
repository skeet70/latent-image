use std::cell::RefCell;
use std::collections::HashMap;

use xm1_saclac_shared::image_registers::{RegisterGroup, RegisterSnapshot, WindowLimits};

use crate::compiled::{attach, restore, Optioned};
use crate::registers::{PipelineHardware, RESTORE_CLIENT};
use crate::scan::{apply, Part, ScanBlocks, ScanLibrary, StockPart};
use crate::stock::{GuestMemory, StockScanBlocks};

/// Guest memory as bytes by address; a byte never stored reads 0.
#[derive(Default)]
struct Memory(RefCell<HashMap<u32, u8>>);

impl GuestMemory for Memory {
    fn byte(&self, at: u32) -> u8 { self.0.borrow().get(&at).copied().unwrap_or(0) }
    fn halfword(&self, at: u32) -> u16 { u16::from_le_bytes([self.byte(at), self.byte(at + 1)]) }
    fn word(&self, at: u32) -> u32 { u32::from_le_bytes([0, 1, 2, 3].map(|offset| self.byte(at + offset))) }
    fn set_byte(&self, at: u32, value: u8) { self.0.borrow_mut().insert(at, value); }
    fn set_halfword(&self, at: u32, value: u16) {
        for (offset, byte) in (0..).zip(value.to_le_bytes()) {
            self.set_byte(at + offset, byte);
        }
    }
    fn set_word(&self, at: u32, value: u32) {
        for (offset, byte) in (0..).zip(value.to_le_bytes()) {
            self.set_byte(at + offset, byte);
        }
    }
    fn fill(&self, at: u32, value: u8, count: u32) {
        for offset in 0..count {
            self.set_byte(at + offset, value);
        }
    }
}

/// The scan configuration block the profile layer's 0xedccd8 fills (the word at 0xedd11c), its
/// request block (0xedd018..0xedd01c) and the compiled scan it attaches (0xedd0ec).
const CONFIGURATION: u32 = 0x0169_9d8c;
const REQUEST: u32 = CONFIGURATION + 0x1260;
const COMPILED: u32 = CONFIGURATION + 0x244;

type Blocks<'m> = StockScanBlocks<&'m Memory>;

/// A scan library whose entries leave both blocks as they are.
struct Library;

impl<'m> ScanLibrary<Blocks<'m>> for Library {
    type Output = ();
    fn apply(&mut self, _: StockPart, _: &mut Blocks<'m>) {}
    fn finish_defaults(&mut self, _: &mut Blocks<'m>) {}
    fn fill_settings(&mut self, _: &Blocks<'m>, _: &mut ()) {}
    fn fill_geometry(&mut self, _: &Blocks<'m>, _: &mut ()) {}
    fn nine(&mut self, _: &Blocks<'m>) -> Optioned<9> { Optioned { option: 0, values: [0; 9] } }
    fn four(&mut self, _: &Blocks<'m>) -> Optioned<4> { Optioned { option: 0, values: [0; 4] } }
}

#[derive(Debug, Eq, PartialEq)]
enum Snapshotted {
    Packed(usize),
    Restored(usize),
}

#[derive(Default)]
struct Snapshot(Vec<Snapshotted>);

impl RegisterSnapshot for Snapshot {
    type Configuration = ();
    fn restore(&mut self, group: RegisterGroup) { self.0.push(Snapshotted::Restored(group.index())); }
    fn window_limits(&self, _: ()) -> WindowLimits { WindowLimits([1, 2, 3]) }
    fn window_words(&self) -> [u32; 2] { [0, 0] }
    fn set_window_words(&mut self, _: [u32; 2]) { self.0.push(Snapshotted::Packed(2)); }
    fn pack(&mut self, group: RegisterGroup, _: ()) { self.0.push(Snapshotted::Packed(group.index())); }
}

#[derive(Default)]
struct Hardware(Vec<(u32, u32)>);

impl PipelineHardware for Hardware {
    fn acquire(&mut self, client: u32) -> u32 {
        self.0.push((client, 0));
        42
    }
    fn release(&mut self, client: u32, grant: u32) { self.0.push((client, grant)); }
}

#[test]
fn the_register_groups_follow_the_parts_applied_to_the_scan_configuration() {
    let memory = Memory::default();
    // The request enables the second part (0xd5e258 reads request byte 1).
    memory.set_byte(REQUEST + 1, 1);
    // The compile copies configuration word 0xd8 to the compiled scan's first word
    // (0xd5da20..0xd5da24), which 0xd5e1d4 and 0xd5e1f4 load and drop.
    memory.set_word(COMPILED, 1 << 19);
    let mut blocks: Blocks = StockScanBlocks { configuration: CONFIGURATION, request: REQUEST, memory: &memory };
    let mut library = Library;
    assert!(apply(Part::Geometry, &mut blocks, &mut library));
    assert!(apply(Part::Second, &mut blocks, &mut library));
    // 0xedd100..0xedd110 passes 0xd5e1d4 the word at configuration + 0x14c.
    assert_eq!(memory.word(CONFIGURATION + 0x14c), 0x3_0000);

    let (mut snapshot, mut hardware) = (Snapshot::default(), Hardware::default());
    attach(blocks.applied_parts(), (), &mut snapshot);
    restore(blocks.applied_parts(), &mut snapshot, &mut hardware);
    use Snapshotted::{Packed, Restored};
    assert_eq!(snapshot.0, [Packed(0), Packed(1), Restored(0), Restored(1)]);
    assert_eq!(hardware.0, [(RESTORE_CLIENT, 0), (RESTORE_CLIENT, 42)]);

    // 0xedc5fc restores, then clears the word through 0xd5e1b8 (0xedc618..0xedc61c).
    blocks.clear_applied_parts();
    assert_eq!(memory.word(CONFIGURATION + 0x14c), 0);
    restore(blocks.applied_parts(), &mut snapshot, &mut hardware);
    assert_eq!(snapshot.0.len(), 4, "nothing is applied");
}
