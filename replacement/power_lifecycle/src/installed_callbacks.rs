//! The callback slots that stock enters through a word installed in RAM.
//!
//! A refused word is not called.

use crate::callback_registry::{CallbackError, CallbackRegistry, CallbackWord};

/// The app installs `0x6470fc` at `0x4004f15c` (`0x647380`, `0x6473cc`); kernel0 `0x41568` calls it.
pub const LOG_HOOK_DEFAULT: CallbackWord = CallbackWord(0x0064_70fc as usize);
/// The empty body at `0x723330`, the initial word of the `0x4089ea8c` and `0x4089ea94` slots.
pub const EMPTY_STUB: CallbackWord = CallbackWord(0x0072_3330 as usize);
/// `0x72835c`, the status handler installed at `0x4089ea8c`.
pub const ERROR_HANDLER_DEFAULT: CallbackWord = CallbackWord(0x0072_835c as usize);
/// `0x726478`, the trace body installed at `0x4089ea94`.
pub const SERVICE_TRACE_DEFAULT: CallbackWord = CallbackWord(0x0072_6478 as usize);
/// `0x72b210`, the router `0x72b6f4` installs at `0x829a0c`.
pub const DEVICE_NOTIFICATION_ROUTER: CallbackWord = CallbackWord(0x0072_b210 as usize);

/// kernel0 `0x41568`: `if (hook != 0) hook(&buffer, &entry)`.
pub fn log_hook(
    registry: &CallbackRegistry,
    installed: CallbackWord,
    buffer: u32,
    entry: u32,
    stock_default: impl FnOnce(u32, u32),
) -> Result<(), CallbackError> {
    if installed.0 == 0 { return Ok(()); }
    if installed == LOG_HOOK_DEFAULT {
        stock_default(buffer, entry);
        return Ok(());
    }
    let callback: unsafe extern "C" fn(u32, u32) = registry.resolve(installed)?;
    unsafe { callback(buffer, entry) };
    Ok(())
}

/// App `0x723398`: `ldr r3,[0x4089ea8c]; blx r3`.
pub fn error_handler(
    registry: &CallbackRegistry,
    installed: CallbackWord,
    status: i32,
    stock_default: impl FnOnce(i32),
) -> Result<(), CallbackError> {
    if installed == EMPTY_STUB { return Ok(()); }
    if installed == ERROR_HANDLER_DEFAULT {
        stock_default(status);
        return Ok(());
    }
    let callback: unsafe extern "C" fn(i32) = registry.resolve(installed)?;
    unsafe { callback(status) };
    Ok(())
}

/// `dispatch_send_message` calls the word at `0x4089ea94` without checking it.
pub fn service_trace(
    registry: &CallbackRegistry,
    installed: CallbackWord,
    header: u32,
    stock_default: impl FnOnce(u32),
) -> Result<(), CallbackError> {
    if installed == EMPTY_STUB { return Ok(()); }
    if installed == SERVICE_TRACE_DEFAULT {
        stock_default(header);
        return Ok(());
    }
    let callback: unsafe extern "C" fn(u32) = registry.resolve(installed)?;
    unsafe { callback(header) };
    Ok(())
}

/// `0x72b680`: `ldr r3,[0x829a0c]; blx r3`, with the record pointer in r0.
pub fn device_notification(
    registry: &CallbackRegistry,
    installed: CallbackWord,
    record: *const u8,
    stock_default: impl FnOnce(*const u8) -> i32,
) -> Result<i32, CallbackError> {
    if installed == DEVICE_NOTIFICATION_ROUTER { return Ok(stock_default(record)); }
    let callback: unsafe extern "C" fn(*const u8) -> i32 = registry.resolve(installed)?;
    Ok(unsafe { callback(record) })
}

#[cfg(test)]
#[path = "installed_callbacks_tests.rs"]
mod tests;
