use super::*;
use crate::callback_registry::CallbackFn;
use std::cell::Cell;
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicUsize, Ordering};

static LOG_CALLS: AtomicU32 = AtomicU32::new(0);
static LOG_BUFFER: AtomicU32 = AtomicU32::new(0);
static LOG_ENTRY: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn record_log(buffer: u32, entry: u32) {
    LOG_CALLS.fetch_add(1, Ordering::SeqCst);
    LOG_BUFFER.store(buffer, Ordering::SeqCst);
    LOG_ENTRY.store(entry, Ordering::SeqCst);
}
fn log_callback() -> unsafe extern "C" fn(u32, u32) { record_log }

#[test]
fn log_hook_enters_the_installed_word_once_with_the_buffer_and_entry() {
    let registry = CallbackRegistry::new();
    registry.ensure(log_callback()).unwrap();
    let result = log_hook(&registry, log_callback().word(), 0x400a0800, 0x400a0834, |_, _| panic!("stock default ran"));
    assert_eq!(result, Ok(()));
    assert_eq!(LOG_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(LOG_BUFFER.load(Ordering::SeqCst), 0x400a0800);
    assert_eq!(LOG_ENTRY.load(Ordering::SeqCst), 0x400a0834);
}

#[test]
fn log_hook_zero_word_returns_without_calling() {
    let registry = CallbackRegistry::new();
    let defaults = Cell::new(0);
    let result = log_hook(&registry, CallbackWord(0 as usize), 0x400a0800, 0x400a0834, |_, _| defaults.set(defaults.get() + 1));
    assert_eq!(result, Ok(()));
    assert_eq!(defaults.get(), 0);
}

#[test]
fn log_hook_refuses_an_unregistered_word() {
    let registry = CallbackRegistry::new();
    let defaults = Cell::new(0);
    let result = log_hook(&registry, CallbackWord(0x0060_1000 as usize), 0x400a0800, 0x400a0834, |_, _| defaults.set(defaults.get() + 1));
    assert_eq!(result, Err(CallbackError::Unadmitted));
    assert_eq!(defaults.get(), 0);
}

#[test]
fn log_hook_runs_the_stock_default_for_the_app_word() {
    let registry = CallbackRegistry::new();
    let seen = Cell::new((0u32, 0u32, 0u32));
    let result = log_hook(&registry, LOG_HOOK_DEFAULT, 0x400a0800, 0x400a0834, |buffer, entry| {
        let (_, _, calls) = seen.get();
        seen.set((buffer, entry, calls + 1));
    });
    assert_eq!(result, Ok(()));
    assert_eq!(seen.get(), (0x400a0800, 0x400a0834, 1));
}

static ERROR_CALLS: AtomicU32 = AtomicU32::new(0);
static ERROR_STATUS: AtomicI32 = AtomicI32::new(0);

unsafe extern "C" fn record_error(status: i32) {
    ERROR_CALLS.fetch_add(1, Ordering::SeqCst);
    ERROR_STATUS.store(status, Ordering::SeqCst);
}
fn error_callback() -> unsafe extern "C" fn(i32) { record_error }

#[test]
fn error_handler_enters_the_installed_word_once_with_the_status() {
    let registry = CallbackRegistry::new();
    registry.ensure(error_callback()).unwrap();
    let result = error_handler(&registry, error_callback().word(), -5, |_| panic!("stock default ran"));
    assert_eq!(result, Ok(()));
    assert_eq!(ERROR_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(ERROR_STATUS.load(Ordering::SeqCst), -5);
}

#[test]
fn error_handler_refuses_a_zero_word() {
    let registry = CallbackRegistry::new();
    assert_eq!(error_handler(&registry, CallbackWord(0 as usize), -5, |_| panic!("stock default ran")), Err(CallbackError::Invalid));
}

#[test]
fn error_handler_does_nothing_for_the_empty_stub() {
    let registry = CallbackRegistry::new();
    assert_eq!(error_handler(&registry, EMPTY_STUB, -5, |_| panic!("stock default ran")), Ok(()));
}

#[test]
fn error_handler_runs_the_stock_default_for_the_installed_status_handler() {
    let registry = CallbackRegistry::new();
    let seen = Cell::new((0i32, 0u32));
    let result = error_handler(&registry, ERROR_HANDLER_DEFAULT, -62, |status| {
        let (_, calls) = seen.get();
        seen.set((status, calls + 1));
    });
    assert_eq!(result, Ok(()));
    assert_eq!(seen.get(), (-62, 1));
}

static TRACE_CALLS: AtomicU32 = AtomicU32::new(0);
static TRACE_HEADER: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn record_trace(header: u32) {
    TRACE_CALLS.fetch_add(1, Ordering::SeqCst);
    TRACE_HEADER.store(header, Ordering::SeqCst);
}
fn trace_callback() -> unsafe extern "C" fn(u32) { record_trace }

#[test]
fn service_trace_enters_the_installed_word_once_with_the_header() {
    let registry = CallbackRegistry::new();
    registry.ensure(trace_callback()).unwrap();
    let result = service_trace(&registry, trace_callback().word(), 0x40a5b5e0, |_| panic!("stock default ran"));
    assert_eq!(result, Ok(()));
    assert_eq!(TRACE_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(TRACE_HEADER.load(Ordering::SeqCst), 0x40a5b5e0);
}

#[test]
fn service_trace_does_nothing_for_the_empty_stub() {
    let registry = CallbackRegistry::new();
    assert_eq!(service_trace(&registry, EMPTY_STUB, 0x40a5b5e0, |_| panic!("stock default ran")), Ok(()));
}

#[test]
fn service_trace_refuses_a_zero_word() {
    let registry = CallbackRegistry::new();
    assert_eq!(service_trace(&registry, CallbackWord(0 as usize), 0x40a5b5e0, |_| panic!("stock default ran")), Err(CallbackError::Invalid));
}

#[test]
fn service_trace_runs_the_stock_default_for_the_installed_trace() {
    let registry = CallbackRegistry::new();
    let seen = Cell::new((0u32, 0u32));
    let result = service_trace(&registry, SERVICE_TRACE_DEFAULT, 0x40a5b5e0, |header| {
        let (_, calls) = seen.get();
        seen.set((header, calls + 1));
    });
    assert_eq!(result, Ok(()));
    assert_eq!(seen.get(), (0x40a5b5e0, 1));
}

static NOTIFICATION_CALLS: AtomicU32 = AtomicU32::new(0);
static NOTIFICATION_RECORD: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn record_notification(record: *const u8) -> i32 {
    NOTIFICATION_CALLS.fetch_add(1, Ordering::SeqCst);
    NOTIFICATION_RECORD.store(record as usize, Ordering::SeqCst);
    0
}
fn notification_callback() -> unsafe extern "C" fn(*const u8) -> i32 { record_notification }

unsafe extern "C" fn refuse_notification(_: *const u8) -> i32 { 7 }

#[test]
fn device_notification_enters_the_installed_word_once_with_the_record_pointer() {
    let registry = CallbackRegistry::new();
    registry.ensure(notification_callback()).unwrap();
    let record = [0u8; 76];
    let result = device_notification(&registry, notification_callback().word(), record.as_ptr(), |_| panic!("stock default ran"));
    assert_eq!(result, Ok(0));
    assert_eq!(NOTIFICATION_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(NOTIFICATION_RECORD.load(Ordering::SeqCst), record.as_ptr() as usize);
}

#[test]
fn device_notification_returns_the_installed_status() {
    let registry = CallbackRegistry::new();
    registry.ensure(refuse_notification as unsafe extern "C" fn(*const u8) -> i32).unwrap();
    let installed = (refuse_notification as unsafe extern "C" fn(*const u8) -> i32).word();
    assert_eq!(device_notification(&registry, installed, core::ptr::null(), |_| 0), Ok(7));
}

#[test]
fn device_notification_runs_the_stock_default_for_the_router_word() {
    let registry = CallbackRegistry::new();
    let record = [0u8; 76];
    let seen = Cell::new((0usize, 0u32));
    let result = device_notification(&registry, DEVICE_NOTIFICATION_ROUTER, record.as_ptr(), |record| {
        let (_, calls) = seen.get();
        seen.set((record as usize, calls + 1));
        0
    });
    assert_eq!(result, Ok(0));
    assert_eq!(seen.get(), (record.as_ptr() as usize, 1));
}

#[test]
fn device_notification_refuses_a_zero_word() {
    let registry = CallbackRegistry::new();
    assert_eq!(device_notification(&registry, CallbackWord(0 as usize), core::ptr::null(), |_| panic!("stock default ran")), Err(CallbackError::Invalid));
}

