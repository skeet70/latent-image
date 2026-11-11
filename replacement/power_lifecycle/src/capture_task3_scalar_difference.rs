use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::capture_settings_predicates::{evaluate, SettingsDifference};
use crate::capture_settings_predicates_runtime::CapturePredicateError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChangedSettings {
    pub operations: [u32; 3],
    pub category: u32,
    pub flags: [u32; 2],
    pub selected: u32,
}

pub fn helper_dd1254<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, before: GuestAddress, after: GuestAddress, configuration: u32, output: GuestAddress) -> Result<(), CapturePredicateError> {
    context.write_u32(GuestAddress::new(0xdd1ab8), if configuration == 0x97 || configuration == 0xa3 { configuration } else { 0xa3 })?;
    let mut difference = ChangedSettings { operations: [0; 3], category: 0, flags: [0; 2], selected: 0 };
    let mut entry = GuestAddress::new(0xe40450);
    loop {
        let raw = context.read_u32(entry.checked_add(24)?)?;
        if raw == 0 { break; }
        let predicate = SettingsDifference::try_from(raw).map_err(|_| MemoryError::Unmapped(GuestAddress::new(raw)))?;
        if evaluate(context, predicate, before, after)? {
            for (index, word) in difference.operations.iter_mut().enumerate() { *word |= context.read_u32(entry.checked_add(index as u32 * 4)?)?; }
            let selected = context.read_u32(entry.checked_add(12)?)?;
            if selected != 7 && (selected == 4 || difference.category != 4) { difference.category = selected; }
            difference.flags[0] |= context.read_u32(entry.checked_add(16)?)?;
            difference.flags[1] |= context.read_u32(entry.checked_add(20)?)?;
        }
        entry = entry.checked_add(32)?;
    }
    if difference.operations[1] & 4 != 0 || difference.operations[0] & (1 << 27) != 0 {
        let mut entry = GuestAddress::new(0xe41e2c);
        loop {
            let raw = context.read_u32(entry.checked_add(4)?)?;
            if raw == 0 { break; }
            let predicate = SettingsDifference::try_from(raw).map_err(|_| MemoryError::Unmapped(GuestAddress::new(raw)))?;
            if evaluate(context, predicate, before, after)? { difference.selected |= context.read_u32(entry)?; }
            entry = entry.checked_add(8)?;
        }
    }
    write_difference(context, output, difference)?;
    if configuration == 0xbe {
        let mut filtered = [0u32; 3];
        for operation in [13u32, 38, 22, 29, 23, 32, 24, 31, 25, 30, 26, 33, 27, 34, 28] {
            let word = (operation / 32) as usize;
            let mask = 1 << (operation % 32);
            filtered[word] |= difference.operations[word] & mask;
        }
        difference.operations = filtered;
        difference.selected = 0;
        difference.category = 7;
        for (index, word) in difference.operations.into_iter().enumerate() { context.write_u32(output.checked_add(index as u32 * 4)?, word)?; }
        context.write_u32(output.checked_add(32)?, 0)?;
        context.write_u32(output.checked_add(12)?, 7)?;
        context.write_u32(output.checked_add(24)?, 0)?;
        context.write_u32(output.checked_add(16)?, difference.flags[0])?;
        context.write_u32(output.checked_add(20)?, difference.flags[1])?;
    }
    context.write_u32(GuestAddress::new(0xdd1ab8), 0xa3)?;
    Ok(())
}

fn write_difference<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, difference: ChangedSettings) -> Result<(), MemoryError> {
    for (index, word) in difference.operations.into_iter().enumerate() { context.write_u32(destination.checked_add(index as u32 * 4)?, word)?; }
    context.write_u32(destination.checked_add(32)?, difference.selected)?;
    context.write_u32(destination.checked_add(12)?, difference.category)?;
    context.write_u32(destination.checked_add(24)?, 0)?;
    context.write_u32(destination.checked_add(16)?, difference.flags[0])?;
    context.write_u32(destination.checked_add(20)?, difference.flags[1])
}
