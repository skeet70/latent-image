//! Minimal, version-pinned Unicorn 2.1.4 public C ABI. No Python binding handles.
//! Definitions checked against include/unicorn/{unicorn,arm}.h at tag 2.1.4.
use libloading::Library;
use std::ffi::{c_char, c_int, c_void};
use std::path::Path;

pub type Engine = *mut c_void;
pub type Context = *mut c_void;
pub type Error = c_int;
pub const PC: c_int = 11;
pub const CPSR: c_int = 3;
pub const ARM_REGS: [c_int; 16] = [
    66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 12, 10, 11,
];
pub const fn ctl(kind: u32, count: u32, direction: u32) -> u32 {
    kind | (count << 26) | (direction << 30)
}

#[repr(C)]
pub struct CpReg {
    pub cp: u32,
    pub is64: u32,
    pub sec: u32,
    pub crn: u32,
    pub crm: u32,
    pub opc1: u32,
    pub opc2: u32,
    pub val: u64,
}
#[repr(C)]
pub struct MemRegion {
    pub begin: u64,
    pub end: u64,
    pub perms: u32,
}

/// Optional exports of a patched 2.1.4 build (tools/build_unicorn.py); None
/// with the stock wheel, whose behavior the callers then keep.
pub struct Extensions {
    pub write_hook_pc_sync: Option<unsafe extern "C" fn(Engine, bool) -> Error>,
    pub page_hooks: Option<unsafe extern "C" fn(Engine, bool) -> Error>,
    pub hook_watched: Option<unsafe extern "C" fn(Engine, usize, bool) -> Error>,
    pub watch: Option<unsafe extern "C" fn(Engine, u64, u64) -> Error>,
    pub insn_filter: Option<unsafe extern "C" fn(Engine, Option<InsnFilter>, Option<Restart>, *mut c_void) -> Error>,
    pub retry_block: Option<unsafe extern "C" fn(Engine, u32) -> Error>,
    pub request_restart: Option<unsafe extern "C" fn(Engine) -> Error>,
    // uc_x70_features bits (0 without the export): 1 = block hook size bit 31
    // is the Thumb state while a translation filter is installed.
    pub features: u32,
    // uc_x70_accounting (features bit 2): the engine's accounting record.
    pub accounting: Option<unsafe extern "C" fn(Engine) -> *mut Accounting>,
}
/// Unicorn's x70 budgeted block accounting record (uc_priv.h x70_acct).
#[repr(C)]
pub struct Accounting {
    pub budget: i64,
    pub head: u64,
    pub ring: *mut u64,
    pub mask: u64,
    pub enabled: u64,
}
pub type InsnFilter = unsafe extern "C" fn(*mut c_void, u64, u32) -> c_int;
pub type Restart = unsafe extern "C" fn(*mut c_void, u64);
macro_rules! api {
    ($($field:ident : $ty:ty),+ $(,)?) => {
        pub struct Api { $(pub $field: $ty,)+ pub x70: Extensions, _library: Library }
        impl Api {
            pub unsafe fn load(path: &Path) -> Result<Self, String> {
                // SAFETY: caller supplies the checked Unicorn library. Its
                // handle is retained until every engine/context is destroyed.
                let library = unsafe { Library::new(path) }.map_err(|e| e.to_string())?;
                $(let $field = *unsafe { library.get::<$ty>(concat!(stringify!($field), "\0").as_bytes()) }
                    .map_err(|e| e.to_string())?;)+
                let x70 = Extensions {
                    write_hook_pc_sync: unsafe {
                        library.get::<unsafe extern "C" fn(Engine, bool) -> Error>(b"uc_x70_set_write_hook_pc_sync\0")
                    }.ok().map(|f| *f),
                    page_hooks: unsafe {
                        library.get::<unsafe extern "C" fn(Engine, bool) -> Error>(b"uc_x70_set_page_hooks\0")
                    }.ok().map(|f| *f),
                    hook_watched: unsafe {
                        library.get::<unsafe extern "C" fn(Engine, usize, bool) -> Error>(b"uc_x70_set_hook_watched\0")
                    }.ok().map(|f| *f),
                    watch: unsafe {
                        library.get::<unsafe extern "C" fn(Engine, u64, u64) -> Error>(b"uc_x70_watch\0")
                    }.ok().map(|f| *f),
                    insn_filter: unsafe {
                        library.get::<unsafe extern "C" fn(Engine, Option<InsnFilter>, Option<Restart>, *mut c_void) -> Error>(b"uc_x70_set_insn_filter\0")
                    }.ok().map(|f| *f),
                    retry_block: unsafe {
                        library.get::<unsafe extern "C" fn(Engine, u32) -> Error>(b"uc_x70_retry_block\0")
                    }.ok().map(|f| *f),
                    request_restart: unsafe {
                        library.get::<unsafe extern "C" fn(Engine) -> Error>(b"uc_x70_request_restart\0")
                    }.ok().map(|f| *f),
                    accounting: unsafe {
                        library.get::<unsafe extern "C" fn(Engine) -> *mut Accounting>(b"uc_x70_accounting\0")
                    }.ok().map(|f| *f),
                    features: unsafe {
                        library.get::<unsafe extern "C" fn() -> u32>(b"uc_x70_features\0")
                    }.ok().map(|f| unsafe { f() }).unwrap_or(0),
                };
                Ok(Self { $($field,)+ x70, _library: library })
            }
        }
    }
}
api! {
    uc_version: unsafe extern "C" fn(*mut u32, *mut u32) -> u32,
    uc_open: unsafe extern "C" fn(c_int, c_int, *mut Engine) -> Error,
    uc_close: unsafe extern "C" fn(Engine) -> Error,
    uc_strerror: unsafe extern "C" fn(Error) -> *const c_char,
    uc_ctl: unsafe extern "C" fn(Engine, u32, ...) -> Error,
    uc_reg_read: unsafe extern "C" fn(Engine, c_int, *mut c_void) -> Error,
    uc_reg_write: unsafe extern "C" fn(Engine, c_int, *const c_void) -> Error,
    uc_reg_read2: unsafe extern "C" fn(Engine, c_int, *mut c_void, *mut usize) -> Error,
    uc_reg_write2: unsafe extern "C" fn(Engine, c_int, *const c_void, *mut usize) -> Error,
    uc_mem_read: unsafe extern "C" fn(Engine, u64, *mut c_void, usize) -> Error,
    uc_mem_write: unsafe extern "C" fn(Engine, u64, *const c_void, usize) -> Error,
    uc_mem_map: unsafe extern "C" fn(Engine, u64, usize, u32) -> Error,
    uc_mem_map_ptr: unsafe extern "C" fn(Engine, u64, usize, u32, *mut c_void) -> Error,
    uc_mem_unmap: unsafe extern "C" fn(Engine, u64, usize) -> Error,
    uc_mem_protect: unsafe extern "C" fn(Engine, u64, usize, u32) -> Error,
    uc_mem_regions: unsafe extern "C" fn(Engine, *mut *mut MemRegion, *mut u32) -> Error,
    uc_free: unsafe extern "C" fn(*mut c_void) -> Error,
    uc_emu_start: unsafe extern "C" fn(Engine, u64, u64, u64, usize) -> Error,
    uc_emu_stop: unsafe extern "C" fn(Engine) -> Error,
    uc_hook_add: unsafe extern "C" fn(Engine, *mut usize, c_int, *mut c_void, *mut c_void, u64, u64, ...) -> Error,
    uc_hook_del: unsafe extern "C" fn(Engine, usize) -> Error,
    uc_context_alloc: unsafe extern "C" fn(Engine, *mut Context) -> Error,
    uc_context_save: unsafe extern "C" fn(Engine, Context) -> Error,
    uc_context_restore: unsafe extern "C" fn(Engine, Context) -> Error,
    uc_context_free: unsafe extern "C" fn(Context) -> Error,
}
