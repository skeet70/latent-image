use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};
use crate::shutdown::TaskId;
#[path = "record_log_format.rs"]
mod format;
pub use format::{helper_721b04, helper_722218, DecimalStyle, DecimalWidth};

pub trait LogEffects {
    type Critical;
    fn current_task(&mut self) -> TaskId;
    fn enter(&mut self) -> Self::Critical;
    fn leave(&mut self, saved: Self::Critical);
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogWorkspace { pub task: GuestAddress, pub output: GuestAddress }
const STATE: GuestAddress = GuestAddress::new(0x409c1e10);
fn configuration<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    crate::record_configuration::helper_69f8fc(context, 0xf7)
}
fn put_byte<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: &mut GuestAddress, value: u8) -> Result<(), MemoryError> {
    context.write_u8(*output, value)?;
    *output = output.checked_add(1)?;
    Ok(())
}
fn append_locked<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, snapshot: GuestAddress, length: ByteCount, line_break: bool) -> Result<(), MemoryError> {
    if context.read_u32(STATE.checked_add(12)?)? == 1 { return Ok(()); }
    let mut excess = 0;
    let mut tail = 0;
    if context.read_u32(STATE.checked_add(8)?)? == 0 {
        let write = context.read_u32(STATE)?;
        let end = length.0.wrapping_add(write);
        if end > 8192 {
            let read = context.read_u32(STATE.checked_add(4)?)?;
            excess = end.wrapping_sub(8192);
            if read == 8192 {
                if write != 8192 { context.write_u32(STATE.checked_add(12)?, 1)?; return Ok(()); }
            } else {
                if read < excess { context.write_u32(STATE.checked_add(12)?, 1)?; return Ok(()); }
                tail = 8192u32.wrapping_sub(write);
            }
        }
    } else {
        let write = context.read_u32(STATE)?;
        if context.read_u32(STATE.checked_add(4)?)? < length.0.wrapping_add(write) {
            context.write_u32(STATE.checked_add(12)?, 1)?;
            return Ok(());
        }
    }
    if tail != 0 {
        let base = GuestAddress::new(context.read_u32(STATE.checked_add(28)?)?);
        let write = context.read_u32(STATE)?;
        context.copy_bytes(base.checked_add(write)?, snapshot, ByteCount(tail))?;
        context.write_u32(STATE, 0)?;
        let base = GuestAddress::new(context.read_u32(STATE.checked_add(28)?)?);
        context.copy_bytes(base, snapshot.checked_add(tail)?, ByteCount(excess))?;
        let write = context.read_u32(STATE)?;
        context.write_u32(STATE, write.wrapping_add(excess))?;
        context.write_u32(STATE.checked_add(8)?, 1)?;
    } else if excess != 0 {
        context.write_u32(STATE, 0)?;
        let base = GuestAddress::new(context.read_u32(STATE.checked_add(28)?)?);
        context.copy_bytes(base, snapshot, ByteCount(excess))?;
        let write = context.read_u32(STATE)?;
        context.write_u32(STATE, write.wrapping_add(excess))?;
        context.write_u32(STATE.checked_add(8)?, 1)?;
    } else {
        let base = GuestAddress::new(context.read_u32(STATE.checked_add(28)?)?);
        let write = context.read_u32(STATE)?;
        context.copy_bytes(base.checked_add(write)?, snapshot, length)?;
        let write = context.read_u32(STATE)?;
        context.write_u32(STATE, write.wrapping_add(length.0))?;
    }
    context.write_u32(STATE.checked_add(16)?, u32::from(line_break))
}
pub fn helper_646ebc<M: GuestAddressSpace, E: LogEffects>(context: &mut RecordContext<M, E>, text: GuestAddress) -> Result<(), MemoryError> {
    context.with_guest_frame(ByteCount(296), StackAlignment::Doubleword, |context, frame| {
        append_in_workspace(context, text, frame.at(ByteCount(4), ByteCount(256))?)
    })
}
fn append_in_workspace<M: GuestAddressSpace, E: LogEffects>(context: &mut RecordContext<M, E>, text: GuestAddress, snapshot: GuestAddress) -> Result<(), MemoryError> {
    if configuration(context)? == 0 || context.read_u32(GuestAddress::new(0x89edd4))? == 0 { return Ok(()); }
    let mut length = 0;
    let mut line_break = false;
    while length != 256 {
        let value = context.read_u8(text.checked_add(length)?)?;
        if matches!(value, b'\n' | b'\r') { line_break = true; }
        if value == 0 { break; }
        context.write_u8(snapshot.checked_add(length)?, value)?;
        length += 1;
    }
    let saved = context.effects.enter();
    let result = append_locked(context, snapshot, ByteCount(length), line_break);
    context.effects.leave(saved);
    result
}
#[derive(Clone, Copy)]
enum Conversion { Decimal, HexUpper, HexLower, String, Literal(u8), End }
struct Spec { left: bool, zero: bool, width: i32, conversion: Conversion }
fn spec<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, cursor: &mut GuestAddress) -> Result<(Spec, GuestAddress), MemoryError> {
    let mut result = Spec { left: false, zero: false, width: 0, conversion: Conversion::End };
    let mut width_started = false;
    loop {
        let last = *cursor;
        let value = context.read_u8(last)?;
        *cursor = cursor.checked_add(1)?;
        if value == 0 { return Ok((result, last)); }
        if !width_started && value == b'-' { result.left = true; }
        else if !width_started && value == b'0' { result.zero = true; }
        else {
            width_started = true;
            if value.is_ascii_digit() { result.width = result.width.wrapping_mul(10).wrapping_add(i32::from(value - b'0')); }
        }
        if value.is_ascii_digit() || value == b'-' { continue; }
        result.conversion = match value { b'd' | b'D' => Conversion::Decimal, b'x' => Conversion::HexLower, b'X' => Conversion::HexUpper, b's' | b'S' => Conversion::String, other => Conversion::Literal(other) };
        return Ok((result, last));
    }
}
fn argument<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, cursor: &mut GuestAddress) -> Result<u32, MemoryError> {
    let result = context.read_u32(*cursor)?;
    *cursor = cursor.checked_add(4)?;
    Ok(result)
}
fn decimal<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: &mut GuestAddress, value: u32, mut spec: Spec) -> Result<(), MemoryError> {
    if spec.width < 1 { spec.width = 10; spec.left = true; }
    let style = if spec.left { DecimalStyle::UnpaddedTerminated } else if spec.zero { DecimalStyle::ZeroTerminated } else { DecimalStyle::SpaceTerminated };
    let digits = helper_722218(context, *output, value, DecimalWidth(spec.width as u32), style)?;
    if digits > 0 {
        while context.read_u8(*output)? != 0 { *output = output.checked_add(1)?; }
    } else if digits == 0 && value == 0 {
        if spec.zero {
            for _ in 0..spec.width { put_byte(context, output, b'0')?; }
        } else if spec.left { put_byte(context, output, b'0')?; }
        else {
            for _ in 1..spec.width { put_byte(context, output, b' ')?; }
            put_byte(context, output, b'0')?;
        }
    }
    Ok(())
}
fn hexadecimal<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: &mut GuestAddress, value: u32, spec: Spec) -> Result<(), MemoryError> {
    let width = spec.width.min(8);
    let digits = if value == 0 { 1 } else { (32 - value.leading_zeros() + 3) / 4 };
    let padding = width.saturating_sub(digits as i32).max(0);
    if !spec.left { for _ in 0..padding { put_byte(context, output, if spec.zero { b'0' } else { b' ' })?; } }
    if value == 0 { put_byte(context, output, b'0')?; }
    else {
        let alphabet = GuestAddress::new(if matches!(spec.conversion, Conversion::HexLower) { 0x7749d4 } else { 0x7749c4 });
        for shift in (0..digits).rev() {
            let value = context.read_u8(alphabet.checked_add(value >> (shift * 4) & 15)?)?;
            put_byte(context, output, value)?;
        }
    }
    if spec.left { for _ in 0..padding { put_byte(context, output, b' ')?; } }
    Ok(())
}
pub fn trace_in_workspace<M: GuestAddressSpace, E: LogEffects>(context: &mut RecordContext<M, E>, header: Option<GuestAddress>, mut format: GuestAddress, mut arguments: GuestAddress, workspace: LogWorkspace) -> Result<(), MemoryError> {
    if configuration(context)? != 1 || context.read_u32(GuestAddress::new(0x89edd4))? == 0 { return Ok(()); }
    let task = context.effects.current_task();
    if helper_722218(context, workspace.task, u32::from(task.0), DecimalWidth(2), DecimalStyle::ZeroTerminated)? < 0 {
        context.write_u8(workspace.task, b'9')?;
        context.write_u8(workspace.task.checked_add(1)?, b'9')?;
        context.write_u8(workspace.task.checked_add(2)?, 0)?;
    }
    if context.read_u32(STATE.checked_add(16)?)? == 1 {
        helper_646ebc(context, workspace.task)?;
        helper_646ebc(context, GuestAddress::new(0x78191c))?;
        if let Some(header) = header {
            if context.read_u8(header)? != 0 {
                helper_646ebc(context, header)?;
                helper_646ebc(context, GuestAddress::new(0x78191c))?;
            }
        }
    }
    let mut output = workspace.output;
    loop {
        let value = context.read_u8(format)?;
        if value == 0 || output.get().wrapping_sub(workspace.output.get()) as i32 > 256 { break; }
        format = format.checked_add(1)?;
        if value != b'%' { put_byte(context, &mut output, value)?; continue; }
        let (spec, last) = spec(context, &mut format)?;
        match spec.conversion {
            Conversion::Decimal => { let value = argument(context, &mut arguments)?; decimal(context, &mut output, value, spec)?; }
            Conversion::HexLower | Conversion::HexUpper => { let value = argument(context, &mut arguments)?; hexadecimal(context, &mut output, value, spec)?; }
            Conversion::String => {
                let address = argument(context, &mut arguments)?;
                let source = GuestAddress::new(if address == 0 { 0x781920 } else { address });
                output = helper_721b04(context, output, source)?;
            }
            Conversion::Literal(value) => put_byte(context, &mut output, value)?,
            Conversion::End => (),
        }
        if context.read_u8(last.checked_add(1)?)? == 0 { break; }
    }
    context.write_u8(output, 0)?;
    helper_646ebc(context, workspace.output)
}
pub fn helper_64744c<M: GuestAddressSpace, E: LogEffects>(context: &mut RecordContext<M, E>, header: Option<GuestAddress>, format: GuestAddress, arguments: GuestAddress) -> Result<(), MemoryError> {
    context.with_guest_frame(ByteCount(384), StackAlignment::Doubleword, |context, frame| {
        let workspace = LogWorkspace { task: frame.at(ByteCount(68), ByteCount(3))?, output: frame.at(ByteCount(72), ByteCount(260))? };
        trace_in_workspace(context, header, format, arguments, workspace)
    })
}
