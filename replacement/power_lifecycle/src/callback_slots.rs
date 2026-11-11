//! Entries for words that arrive in stock-shaped callback slots.

use xm1_power_services::callback_registry::{CallbackError, CallbackRegistry, CallbackWord};

pub type Application = unsafe extern "C" fn(*const u8) -> i32;

pub fn register_application(registry: &CallbackRegistry, callback: Application) -> Result<(), CallbackError> {
    registry.ensure(callback)
}

pub fn resolve_application(registry: &CallbackRegistry, word: CallbackWord) -> Result<Application, CallbackError> {
    registry.resolve(word)
}

pub fn call_application(registry: &CallbackRegistry, word: CallbackWord, record: *const u8) -> Result<i32, CallbackError> {
    let callback = resolve_application(registry, word)?;
    Ok(unsafe { callback(record) })
}

pub fn call_argument(registry: &CallbackRegistry, word: CallbackWord, argument: u32) -> Result<(), CallbackError> {
    let callback: unsafe extern "C" fn(u32) = registry.resolve(word)?;
    unsafe { callback(argument) };
    Ok(())
}

pub fn call_pair(registry: &CallbackRegistry, word: CallbackWord, first: u32, second: u32) -> Result<(), CallbackError> {
    let callback: unsafe extern "C" fn(u32, u32) = registry.resolve(word)?;
    unsafe { callback(first, second) };
    Ok(())
}

pub fn call_signed_argument(registry: &CallbackRegistry, word: CallbackWord, argument: i32) -> Result<(), CallbackError> {
    let callback: unsafe extern "C" fn(i32) = registry.resolve(word)?;
    unsafe { callback(argument) };
    Ok(())
}

pub fn call_block(registry: &CallbackRegistry, word: CallbackWord, status: i32, value: u32) -> Result<(), CallbackError> {
    let callback: unsafe extern "C" fn(i32, u32) = registry.resolve(word)?;
    unsafe { callback(status, value) };
    Ok(())
}

pub fn call_no_arguments(registry: &CallbackRegistry, word: CallbackWord) -> Result<(), CallbackError> {
    let callback: unsafe extern "C" fn() = registry.resolve(word)?;
    unsafe { callback() };
    Ok(())
}

#[cfg(test)]
#[path = "callback_slots_tests.rs"]
mod tests;
