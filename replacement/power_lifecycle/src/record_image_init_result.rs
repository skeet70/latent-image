//! X-M1 1.50 result buffer initialization at 1039580.
//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3 and actual image-5 bytes.

use crate::record_address_space::{CallbackId, CallbackRegistry, GuestAddress, GuestAddressSpace, RecordContext};
use crate::record_image_init_state::{clear, put, put_byte, put_half, word, ImageInitError};
use crate::osal::{CriticalKernel, interrupt_lock, interrupt_unlock};
const RESULT: usize = 0x016fec34;

fn reset(context: &mut impl GuestAddressSpace) -> Result<(), ImageInitError> {
    put(context, RESULT + 0x4460, 0)?;
    put(context, RESULT + 0x4464, 3)?;
    clear(context, RESULT + 0x4468, 0x10)?;
    for offset in [0x4478,0x447c,0x4480,0x4484,0x4488,0x4490,0x4494,0x449c] { put(context, RESULT + offset, 0)?; }
    clear(context, RESULT + 0x44a0, 0x18)?;
    put(context, RESULT + 0x44b8, 0)?;
    clear(context, RESULT + 0x44c0, 0x10)?;
    put(context, RESULT + 0x44d0, 0)?;
    clear(context, RESULT + 0x44d8, 0x10)?;
    put(context, RESULT + 0x44e8, 0)?;
    clear(context, RESULT + 0x44ec, 0x38)?;
    put(context, RESULT + 0x4570, 0)?;
    clear(context, RESULT + 0x4574, 0x1c)?;
    clear(context, RESULT + 0x45b4, 0x1c)?;
    put(context, RESULT + 0x4668, 0)?;
    put(context, RESULT + 0x466c, 0)?;
    put_half(context, RESULT + 0x4684, 0xffff)?;
    for offset in [0x4690,0x4694,0x467c,0x4680] { put(context, RESULT + offset, 0)?; }
    put(context, RESULT + 0x468c, 3)?;
    put(context, RESULT + 0x46ec, 0)?;
    put(context, RESULT + 0x46f0, 0)?;
    put(context, RESULT + 0x4528, 0)?;
    put(context, RESULT + 0x452c, 0)?;
    put_byte(context, RESULT + 0x4545, 0)?;
    for offset in [0x4530,0x4534,0x453c,0x4540] { put(context, RESULT + offset, 0)?; }
    put_byte(context, RESULT + 0x4548, 0)?;
    put(context, RESULT + 0x4560, 1)?;
    put(context, RESULT + 0x4568, 1)?;
    put(context, RESULT + 0x456c, 0)?;
    clear(context, 0x016fe9bc, 0x6c)?;
    put(context, RESULT + 0x45d0, 0)?;
    put(context, RESULT + 0x45d4, 0)?;
    put_half(context, RESULT + 0x4bb0, 0)?;
    for offset in [0x4bb8,0x4c6c,0x4cc0,0x4cc4,0x4cd4,0x445c] { put(context, RESULT + offset, 0)?; }
    put_half(context, RESULT + 0x4410, 0xffff)?;
    put_half(context, RESULT + 0x4412, 0xffff)?;
    put_byte(context, RESULT + 0x44bc, 0)?;
    put_byte(context, RESULT + 0x44d4, 0)?;
    put(context, RESULT + 0x44d0, 0)?;
    put(context, RESULT + 0x44b8, 0)?;
    Ok(())
}
fn descriptors(context: &mut impl GuestAddressSpace, offset: usize, count: usize, base: u32, step: u32) -> Result<(), ImageInitError> {
    let mut address = base;
    for index in 0..count {
        let slot = RESULT + offset + 24 * index;
        put(context, slot, address)?;
        for field in [4,8,12,16] { put(context, slot + field, 0)?; }
        address = address.wrapping_add(step);
    }
    Ok(())
}
fn initialize_layout<M: GuestAddressSpace, E: CriticalKernel>(context: &mut RecordContext<M,E>) -> Result<(), ImageInitError> {
    let saved = interrupt_lock(&mut context.effects);
    let result = (|| {
    let primary = word(context, RESULT + 0x1c)?;
    if primary != 0 {
        descriptors(context, 0x94, 4, primary, 0x96000)?;
        descriptors(context, 0x19c, 5, primary.wrapping_add(0x258000), 0x96000)?;
        descriptors(context, 0x4350, 4, primary.wrapping_add(0x258000), 0xa8c00)?;
        descriptors(context, 0xf4, 5, primary.wrapping_add(0x546000), 0xa8c00)?;
        let secondary_end = word(context, RESULT + 0x20)?.wrapping_add(0x891c00);
        put(context, RESULT + 0x4444, secondary_end)?;
        descriptors(context, 0x16c, 1, 0x576728a0, 0)?;
        descriptors(context, 0x184, 1, 0x5771b4a0, 0)?;
    }
    let secondary = word(context, RESULT + 0x20)?;
    if secondary != 0 {
        descriptors(context, 0x2bc, 5, secondary, 0x3fc000)?;
        descriptors(context, 0x334, 5, secondary, 0x96000)?;
        descriptors(context, 0x3ac, 34, secondary, 0x96000)?;
        descriptors(context, 0x6dc, 136, secondary, 0x25800)?;
        descriptors(context, 0x43b0, 4, secondary, 0xa8c00)?;
        descriptors(context, 0x214, 7, secondary.wrapping_add(0x13ec000), 0xa8c00)?;
        descriptors(context, 0x427c, 3, secondary.wrapping_add(0x1889400), 0x96000)?;
        put(context, RESULT + 0x4448, secondary.wrapping_add(0x1a4b400))?;
    }
    Ok(())
    })();
    interrupt_unlock(&mut context.effects, saved);
    result
}

pub fn helper_1039580<M: GuestAddressSpace, E: CriticalKernel, const N: usize>(context: &mut RecordContext<M,E>, window: GuestAddress, callback: Option<CallbackId>, registry: &CallbackRegistry<RecordContext<M,E>,N>) -> Result<u32, ImageInitError> {
    reset(context)?;
    unsafe { crate::record_image_init_display::reset_display_path(context)? };
    for offset in [0,4,8,0x10,0x14,0x4648] { put(context, RESULT + offset, 0)?; }
    put(context, RESULT + 0x18, 1)?;
    let window = window.get() as usize;
    let primary = word(context, window)?;
    if primary != 0 {
        put(context, RESULT + 0x1c, primary)?;
        let size = word(context, window + 0x10)?;
        put(context, RESULT + 0x2c, size)?;
    }
    let secondary = word(context, window + 4)?;
    if secondary != 0 {
        put(context, RESULT + 0x20, secondary)?;
        let size = word(context, window + 0x14)?;
        put(context, RESULT + 0x30, size)?;
    }
    let tertiary = word(context, window + 8)?;
    if tertiary != 0 { put(context, RESULT + 0x24, tertiary)?; }
    initialize_layout(context)?;
    put(context, RESULT + 0x4cc8, 1)?;
    if let Some(callback) = callback { registry.invoke_status(callback, context, 0)?; }
    Ok(0)
}
