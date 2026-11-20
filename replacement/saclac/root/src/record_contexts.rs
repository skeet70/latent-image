#![forbid(unsafe_code)]

use xm1_saclac_controllers::root::{Allocation, Context, Generation, Input};
use xm1_saclac_interface::{stock::record_handle_from_a2, CommandKind, Message, RecordHandle};
use xm1_saclac_profile_records::{stock::{self, record}, Access, Grants, Header, Memory, OpenMode, StockEntries, Tag};
use xm1_saclac_shared::stock::Machine;
use crate::services::Services;

#[derive(Clone, Copy, Default)]
struct Pair { current: Option<RecordHandle>, previous: Option<RecordHandle> }

/// f149e8's four current owners and their previous generations.
pub struct Contexts { slots: [Pair; 4] }
impl Default for Contexts { fn default() -> Self { Self::new() } }
impl Contexts {
    pub const fn new() -> Self { Self { slots: [Pair { current: None, previous: None }; 4] } }
    pub fn reset(&mut self) { self.slots.fill(Pair::default()); }

    pub fn record(&self, context: Context) -> RecordHandle {
        self.slots[index(context)].current.map_or_else(no_record, configured)
    }

    pub fn acquire<M: Machine + Clone, R: Memory, E: StockEntries, P>(&mut self, context: Context, allocation: Allocation, trigger: Input<'_>, services: &mut Services<M, R, E, P>) -> RecordHandle {
        let slot = &mut self.slots[index(context)];
        if allocation == Allocation::Replace {
            if let Some(current) = slot.current { slot.previous = Some(current); slot.current = None; }
        }
        if slot.current.is_none() {
            let mode = if allocation == Allocation::Replace { OpenMode::Copy } else { OpenMode::Selected };
            let owner = services.entries.open(mode, Grants::Saclac);
            if owner != no_record() { slot.current = Some(owner); }
        }
        let handle = slot.current.map_or_else(no_record, configured);
        apply_trigger(services, handle, trigger);
        handle
    }

    pub fn release<E: StockEntries>(&mut self, context: Context, generation: Generation, entries: &mut E) {
        let pair = &mut self.slots[index(context)];
        let slot = match generation { Generation::Current => &mut pair.current, Generation::Previous => &mut pair.previous };
        if let Some(owner) = *slot { entries.close(owner); *slot = None; }
    }
}

const fn index(context: Context) -> usize { match context { Context::Root => 0, Context::Lens => 1, Context::Live => 2, Context::S1 => 3 } }
fn no_record() -> RecordHandle { record_handle_from_a2(u32::MAX) }
fn configured(owner: RecordHandle) -> RecordHandle {
    stock::handle(owner).and_then(|handle| handle.derive(None, Access::ReadWrite, Tag::from_nibbles(1, 1)).ok()).map_or_else(no_record, stock::record_handle)
}

fn frame<const N: usize>(machine: &impl Machine, at: u32) -> [u8; N] {
    const { assert!(N % 4 == 0); }
    let mut bytes = [0; N];
    for (index, word) in bytes.chunks_exact_mut(4).enumerate() {
        word.copy_from_slice(&machine.read_u32(at.wrapping_add(4 * index as u32)).to_le_bytes());
    }
    bytes
}
fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("context trigger word")) }

/// f147bc reads the staged header at receive payload+8; Start also has three frame pointers.
pub(crate) fn apply_trigger<M: Machine + Clone, R: Memory, E: StockEntries, P>(services: &mut Services<M, R, E, P>, handle: RecordHandle, trigger: Input<'_>) {
    let Input::Message(Message::Command(command)) = trigger else { return };
    if !matches!(command.kind, CommandKind::Start | CommandKind::Autofocus) { return; }
    let bytes = command.parameters.bytes();
    let header = Header::new(bytes[8..8 + xm1_saclac_profile_records::image::HEADER_BYTES].try_into().expect("context trigger header"));
    if command.kind == CommandKind::Autofocus {
        let _ = services.records.apply_changes(handle, header);
        return;
    }
    services.entries.replace_header(handle, header);
    let Services { machine, records, .. } = services;
    let _ = records.write_with(handle, &record::group_00::AT_20B0_104, || frame::<104>(&*machine, word(bytes, 152)));
    let _ = records.write_with(handle, &record::group_00::AT_0160_12, || frame::<12>(&*machine, word(bytes, 156)));
    let _ = records.write_with(handle, &record::group_00::AT_2118, || frame::<40>(&*machine, word(bytes, 160)));
}

#[cfg(saclac_qualification)]
pub mod qualification;
