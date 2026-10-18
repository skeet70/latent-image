//! Stable engine/backing ownership and explicitly bounded reentrancy.
use crate::audit::Audit;
use crate::fetch_guard::FetchGuard;
use crate::ffi::{self, Api, CpReg, Engine};
use crate::hooks;
use crate::observe::{Coverage, ExecProbe, StoreLog};
use crate::state::{Instruction, Slot, State, Trace, TRACE_CAPACITY, classify, front_index};
use pyo3::prelude::*;
use std::cell::{Cell, RefCell, UnsafeCell};
use std::ffi::{CStr, c_void};
use std::path::Path;
use std::rc::Rc;

enum ConversionCore {
    Primary,
    Secondary,
}

pub(crate) struct ConversionOrigin {
    core: ConversionCore,
    instructions: u64,
    ticks: u64,
}

impl ConversionOrigin {
    pub(crate) fn new(core: u64, instructions: u64, ticks: u64) -> Result<Self, String> {
        let core = match core {
            0 => ConversionCore::Primary,
            1 => ConversionCore::Secondary,
            _ => return Err("Declared UI conversion CPU index must be 0 or 1".into()),
        };
        Ok(Self { core, instructions, ticks })
    }

    fn core(&self) -> usize {
        match self.core {
            ConversionCore::Primary => 0,
            ConversionCore::Secondary => 1,
        }
    }
}

pub struct HookData {
    pub inner: *const Inner,
    pub core: usize,
}

unsafe extern "C" {
    fn mmap(
        addr: *mut c_void,
        len: usize,
        prot: i32,
        flags: i32,
        fd: i32,
        offset: i64,
    ) -> *mut c_void;
    fn munmap(addr: *mut c_void, len: usize) -> i32;
}
#[cfg(target_vendor = "apple")]
const MAP_ANON: i32 = 0x1000;
#[cfg(not(target_vendor = "apple"))]
const MAP_ANON: i32 = 0x20;
const ZERO_MAP_THRESHOLD: usize = 1 << 20;

enum Storage {
    Boxed(Box<[UnsafeCell<u8>]>),
    // Anonymous zero-filled pages, faulted in on first touch. A freshly mapped
    // 512 MiB DRAM costs microseconds; a recycled calloc block is re-cleared.
    Mapped(*mut u8, usize),
}

pub struct Backing {
    storage: Storage,
}
impl Backing {
    pub(crate) fn from_bytes(bytes: Box<[u8]>) -> Self {
        // UnsafeCell<u8> is repr(transparent). Ownership/allocation layout and
        // slice metadata are unchanged. FFI guest writes then have explicit
        // interior mutability; no shared &[u8] aliases a writable guest buffer.
        let raw = Box::into_raw(bytes) as *mut [UnsafeCell<u8>];
        Self {
            storage: Storage::Boxed(unsafe { Box::from_raw(raw) }),
        }
    }
    pub(crate) fn zeros(size: usize) -> Self {
        if size >= ZERO_MAP_THRESHOLD {
            let pointer = unsafe {
                mmap(
                    std::ptr::null_mut(),
                    size,
                    3,            // PROT_READ | PROT_WRITE
                    2 | MAP_ANON, // MAP_PRIVATE | MAP_ANON
                    -1,
                    0,
                )
            };
            if pointer as isize != -1 && !pointer.is_null() {
                return Self {
                    storage: Storage::Mapped(pointer.cast(), size),
                };
            }
        }
        Self::from_bytes(vec![0; size].into_boxed_slice())
    }
    pub fn as_ptr(&self) -> *mut u8 {
        match &self.storage {
            Storage::Boxed(cells) => cells.as_ptr().cast::<u8>().cast_mut(),
            Storage::Mapped(pointer, _) => *pointer,
        }
    }
    pub fn len(&self) -> usize {
        match &self.storage {
            Storage::Boxed(cells) => cells.len(),
            Storage::Mapped(_, size) => *size,
        }
    }
}
impl Drop for Backing {
    fn drop(&mut self) {
        if let Storage::Mapped(pointer, size) = self.storage {
            unsafe {
                munmap(pointer.cast(), size);
            }
        }
    }
}

/// A device register whose access is a fixed function of the device's
/// `registers` dict (and one boolean attribute), declared by the device.
pub enum PlainRead {
    /// (stored & !clear) | (set if `flag` is truthy, or if no flag).
    Formula(u32, u32, Option<Py<pyo3::types::PyString>>),
    /// `device.read(core, address, 4)`, called directly: the device declares
    /// this read independent of time, deferred ticks and every other device,
    /// and without effects.
    Direct,
    /// `device.native_read(address, virtual_ticks)` at the access's tick: the
    /// value `read` would return then, computed without effects, or None to
    /// send this access through the event path.
    Timed,
}
pub struct PlainRegister {
    pub device: Py<PyAny>,
    // None leaves reads to Python's event path.
    pub read: Option<PlainRead>,
    // Write: None leaves writes to Python; Some(keep) stores
    // (value & keep) | (old & !keep & !value) (keep = all ones: a plain store).
    pub write: Option<u32>,
}
/// Blocks kept per core for the trace (budgeted accounting); at least
/// TRACE_CAPACITY, a power of two.
pub const ACCT_RING: usize = 64;
pub struct Inner {
    pub api: Rc<Api>,
    pub engines: [Engine; 2],
    // Buffers never resize and outlive both engines. Only these owned buffers
    // can be mapped via the compatibility map-pointer operation.
    pub ram: Backing,
    pub sram: Backing,
    pub vectors: Backing,
    pub rom: Backing,
    pub core_private: [Backing; 2],
    pub core_rom: [Backing; 2],
    pub tcm: UnsafeCell<crate::tcm::Tcm>,
    pub state: UnsafeCell<State>,
    pub audits: UnsafeCell<[Option<Audit>; 2]>,
    pub fetch_guards: UnsafeCell<[Option<FetchGuard>; 2]>,
    pub breakpoints: UnsafeCell<[Vec<u64>; 2]>,
    // A breakpoint handler's PC rewrite is lost when Unicorn restores the
    // stopped instruction's PC, so it is re-applied once emu_start returns.
    pub redirects: Cell<[Option<u32>; 2]>,
    pub store_logs: UnsafeCell<[Option<StoreLog>; 2]>,
    // Opt-in per-core set of executed instruction addresses (None = disabled).
    pub coverage: UnsafeCell<[Option<Coverage>; 2]>,
    // Opt-in per-core range counts and ordered PC trace (None = disabled).
    pub exec_probe: UnsafeCell<[Option<ExecProbe>; 2]>,
    pub access_probe: UnsafeCell<Option<crate::access_probe::AccessProbe>>,
    pub access_probe_hooks: Cell<[Option<usize>; 2]>,
    // A breakpoint handler asked the call to end before this instruction.
    pub breakpoint_halt: Cell<[bool; 2]>,
    // Value returned by the stock CP14 CPU-identity read (MRC p14,0,Rt,c2,c0,0).
    pub cp14_identity: Cell<[u32; 2]>,
    pub audit_hooks: Cell<[[Option<usize>; 2]; 2]>,
    pub executing: Cell<Option<usize>>,
    pub callback: RefCell<Option<Py<PyAny>>>,
    // Device registers the engine serves itself (`hooks::plain`).
    pub plain: RefCell<crate::state::AddressMap<PlainRegister>>,
    // The handler asked for counters: each event then also carries the
    // instruction/tick counters, tick phase, the executing core's instruction
    // count and the aux version, so it need not copy state back and forth.
    pub counted: Cell<bool>,
    pub callback_error: RefCell<Option<PyErr>>,
    pub native_error: RefCell<Option<String>>,
    // Each address is registered with C before the Vec may grow; Box keeps
    // those callback addresses stable across reallocation of the Vec itself.
    #[allow(clippy::vec_box)]
    pub hook_data: Vec<Box<HookData>>,
    // (core, handle) of the RAM store hooks; watched ones (patched Unicorn)
    // see fast-path stores only on pages `State::watch_page` declared.
    pub store_hooks: Vec<(usize, usize)>,
    // Budgeted block accounting records (null without the export) and the
    // rings they point at.
    pub acct: [*mut ffi::Accounting; 2],
    pub acct_rings: [Box<[u64]>; 2],
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Hook user-data, library and native RAM remain alive until close.
        for engine in self.engines {
            if !engine.is_null() {
                unsafe {
                    (self.api.uc_close)(engine);
                }
            }
        }
    }
}

impl Inner {
    pub fn new(
        library: &Path,
        ram_size: usize,
        rom: &[u8],
        alias: bool,
        entries: Vec<u64>,
        tcm_parameters: crate::tcm::Parameters,
    ) -> Result<Box<Self>, String> {
        if !(0x20000..=0x20000000).contains(&ram_size)
            || !ram_size.is_multiple_of(0x1000)
            || rom.len() != 0x4000
        {
            return Err("Expected page-aligned 128KiB..512MiB DRAM and exactly 16KiB ROM".into());
        }
        let api = Rc::new(unsafe { Api::load(library)? });
        let (mut major, mut minor) = (0, 0);
        let version = unsafe { (api.uc_version)(&mut major, &mut minor) };
        if major != 2 || minor != 1 || ((version >> 8) & 255) != 4 {
            return Err(format!("Unicorn 2.1.4 required; loaded {version:#x}"));
        }
        let mut inner = Box::new(Self {
            api,
            engines: [std::ptr::null_mut(); 2],
            ram: Backing::zeros(ram_size),
            sram: Backing::zeros(0x1000),
            vectors: Backing::zeros(0x1000),
            rom: Backing::from_bytes(rom.to_vec().into_boxed_slice()),
            core_private: std::array::from_fn(|_| Backing::zeros(0xff000)),
            core_rom: std::array::from_fn(|_| Backing::from_bytes(rom.to_vec().into_boxed_slice())),
            tcm: UnsafeCell::new(crate::tcm::Tcm::new(&tcm_parameters)?),
            state: UnsafeCell::new(State::new(entries)),
            audits: UnsafeCell::new([None, None]),
            fetch_guards: UnsafeCell::new([None, None]),
            breakpoints: UnsafeCell::new([Vec::new(), Vec::new()]),
            redirects: Cell::new([None; 2]),
            store_logs: UnsafeCell::new([None, None]),
            coverage: UnsafeCell::new([None, None]),
            exec_probe: UnsafeCell::new([None, None]),
            access_probe: UnsafeCell::new(None),
            access_probe_hooks: Cell::new([None; 2]),
            breakpoint_halt: Cell::new([false; 2]),
            cp14_identity: Cell::new([0x4003, 0x5003]),
            audit_hooks: Cell::new([[None; 2]; 2]),
            executing: Cell::new(None),
            callback: RefCell::new(None),
            plain: RefCell::new(Default::default()),
            counted: Cell::new(false),
            callback_error: RefCell::new(None),
            native_error: RefCell::new(None),
            hook_data: Vec::new(),
            store_hooks: Vec::new(),
            acct: [std::ptr::null_mut(); 2],
            acct_rings: [vec![0; ACCT_RING].into_boxed_slice(), vec![0; ACCT_RING].into_boxed_slice()],
        });
        for core in 0..2 {
            let status = unsafe { (inner.api.uc_open)(1, 0, &mut inner.engines[core]) };
            inner.check(status)?;
            let cpu = inner.engines[core];
            inner.check(unsafe { (inner.api.uc_ctl)(cpu, ffi::ctl(7, 1, 1), 13i32) })?;
            // Memory hooks slow down only the pages their ranges cover.
            if let Some(page_hooks) = inner.api.x70.page_hooks {
                inner.check(unsafe { page_hooks(cpu, true) })?;
            }
            if let (Some(accounting), true) = (inner.api.x70.accounting, inner.api.x70.features & 2 != 0) {
                let record = unsafe { accounting(cpu) };
                unsafe {
                    (*record).ring = inner.acct_rings[core].as_mut_ptr();
                    (*record).mask = ACCT_RING as u64 - 1;
                    (*record).enabled = 0;
                    (*record).budget = 0;
                }
                inner.acct[core] = record;
            }
            inner.map_owned(core, 0, ram_size, 7, inner.ram.as_ptr() as usize)?;
            inner.map_owned(core, 0x40000000, ram_size, 7, inner.ram.as_ptr() as usize)?;
            if alias {
                inner.check(unsafe { (inner.api.uc_mem_unmap)(cpu, 0, 0x4000) })?;
                inner.map_owned(core, 0, 0x4000, 5, inner.rom.as_ptr() as usize)?;
            }
            for address in [0x3fff0000, 0x7fff0000] {
                inner.map_owned(core, address, 0x1000, 7, inner.vectors.as_ptr() as usize)?;
            }
            inner.map_owned(core, 0x80000000, 0x4000, 5, inner.core_rom[core].as_ptr() as usize)?;
            inner.map_owned(core, 0xfff00000, 0x1000, 7, inner.sram.as_ptr() as usize)?;
            inner.map_owned(core, 0xfff01000, 0xef000, 7, inner.core_private[core].as_ptr() as usize)?;
            inner.map_owned(core, 0xffff0000, 0x10000, 7, inner.core_private[core].as_ptr() as usize + 0xef000)?;
            if core == 1 {
                inner.check(unsafe { (inner.api.uc_mem_unmap)(cpu, 0xffff0000, 0x1000) })?;
                inner.map_owned(core, 0xffff0000, 0x1000, 7, inner.vectors.as_ptr() as usize)?;
                let mut reg = CpReg {
                    cp: 15,
                    is64: 0,
                    sec: 0,
                    crn: 1,
                    crm: 0,
                    opc1: 0,
                    opc2: 0,
                    val: 0,
                };
                inner.check(unsafe {
                    (inner.api.uc_reg_read)(cpu, 139, (&mut reg as *mut CpReg).cast())
                })?;
                reg.val |= 1 << 13;
                inner.check(unsafe {
                    (inner.api.uc_reg_write)(cpu, 139, (&reg as *const CpReg).cast())
                })?;
            }
            inner.write_reg(core, ffi::PC, if alias { 0 } else { core as u32 * 0x10000 })?;
            let data = Box::new(HookData {
                inner: &*inner,
                core,
            });
            let ptr = (&*data as *const HookData).cast_mut().cast::<c_void>();
            inner.add_hook(core, 4, hooks::code as *const () as *mut c_void, ptr, 1, 0)?;
            inner.add_hook(core, 8, hooks::block as *const () as *mut c_void, ptr, 1, 0)?;
            inner.add_hook(
                core,
                1,
                hooks::interrupt as *const () as *mut c_void,
                ptr,
                1,
                0,
            )?;
            inner.add_hook(
                core,
                0x3f0,
                hooks::invalid as *const () as *mut c_void,
                ptr,
                1,
                0,
            )?;
            inner.add_hook(
                core,
                (1 << 10) | (1 << 11),
                hooks::mmio as *const () as *mut c_void,
                ptr,
                0xfff01000,
                0xfffeffff,
            )?;
            for (begin, end) in [
                (0, ram_size as u64 - 1),
                (0x40000000, 0x40000000 + ram_size as u64 - 1),
                (0xfff00000, 0xfff00fff),
                (0x3fff0000, 0x3fff0fff),
                (0x7fff0000, 0x7fff0fff),
                (0xffff0000, 0xffffffff),
            ] {
                let hook = inner.add_hook(
                    core,
                    1 << 11,
                    hooks::store as *const () as *mut c_void,
                    ptr,
                    begin,
                    end,
                )?;
                inner.store_hooks.push((core, hook));
            }
            inner.hook_data.push(data);
        }
        for core in 0..2 {
            for bank in 0..2 {
                crate::tcm::initial_map(&inner,core,bank)?;
                unsafe { (*inner.state.get()).tcm_windows[core][bank] = (&*inner.tcm.get()).extent(core,bank); }
            }
        }
        Ok(inner)
    }
    pub fn memory_regions(&self, core: usize) -> Result<Vec<(u64,u64,u32)>,String> {
        let mut ptr=std::ptr::null_mut(); let mut count=0;
        self.check(unsafe { (self.api.uc_mem_regions)(self.core(core)?,&mut ptr,&mut count) })?;
        let regions=if count==0 { Vec::new() } else {
            unsafe { std::slice::from_raw_parts(ptr,count as usize) }.iter().map(|r|(r.begin,r.end,r.perms)).collect()
        };
        self.check(unsafe { (self.api.uc_free)(ptr.cast()) })?;
        Ok(regions)
    }
    pub fn owned_pointer(&self, core: usize, address: u64, size: usize, permissions: u32) -> Option<usize> {
        let end=address.checked_add(size as u64)?;
        let ptr=if address<0x4000 && end<=0x4000 && permissions==5 { self.rom.as_ptr() as usize+address as usize }
        else if address<self.ram.len() as u64 && end<=self.ram.len() as u64 { self.ram.as_ptr() as usize+address as usize }
        else if address>=0x40000000 && end<=0x40000000+self.ram.len() as u64 { self.ram.as_ptr() as usize+(address-0x40000000) as usize }
        else if address>=0xfff00000 && end<=0xfff01000 { self.sram.as_ptr() as usize+(address-0xfff00000) as usize }
        else if address>=0x80000000 && end<=0x80004000 { self.core_rom[core].as_ptr() as usize+(address-0x80000000) as usize }
        else if (address>=0x3fff0000 && end<=0x3fff1000)||(address>=0x7fff0000 && end<=0x7fff1000) {
            self.vectors.as_ptr() as usize+(address&0xfff) as usize
        } else if core==1 && address>=0xffff0000 && end<=0xffff1000 {
            self.vectors.as_ptr() as usize+(address-0xffff0000) as usize
        } else if address>=0xfff01000 && end<=1<<32 {
            self.core_private[core].as_ptr() as usize+(address-0xfff01000) as usize
        } else { return None; };
        Some(ptr)
    }
    pub fn check(&self, error: i32) -> Result<(), String> {
        if error == 0 {
            Ok(())
        } else {
            let msg = unsafe { CStr::from_ptr((self.api.uc_strerror)(error)) }.to_string_lossy();
            Err(format!("Unicorn error {error}: {msg}"))
        }
    }
    pub fn core(&self, core: usize) -> Result<Engine, String> {
        self.engines
            .get(core)
            .copied()
            .ok_or_else(|| "CPU index must be 0 or 1".into())
    }
    pub fn paused(&self) -> Result<(), String> {
        if self.executing.get().is_some() {
            Err(
                "Operation requires paused engines; nested execution/mapping/checkpoint is refused"
                    .into(),
            )
        } else {
            Ok(())
        }
    }
    pub fn add_hook(
        &self,
        core: usize,
        kind: i32,
        callback: *mut c_void,
        data: *mut c_void,
        begin: u64,
        end: u64,
    ) -> Result<usize, String> {
        let mut hook = 0;
        self.check(unsafe {
            (self.api.uc_hook_add)(
                self.core(core)?,
                &mut hook,
                kind,
                callback,
                data,
                begin,
                end,
            )
        })?;
        Ok(hook)
    }
    pub fn ensure_audit_hooks(&self, core: usize) -> Result<(), String> {
        self.paused()?;
        self.core(core)?;
        let ptr = (&*self.hook_data[core] as *const HookData)
            .cast_mut()
            .cast::<c_void>();
        for (index, (kind, callback)) in [
            (1 << 10, hooks::audit_read as *const () as *mut c_void),
            (
                1 << 11,
                hooks::audit_write_guard as *const () as *mut c_void,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let mut handles = self.audit_hooks.get();
            if handles[core][index].is_none() {
                handles[core][index] = Some(self.add_hook(core, kind, callback, ptr, 1, 0)?);
                self.audit_hooks.set(handles);
            }
        }
        Ok(())
    }
    pub fn map_owned(
        &self,
        core: usize,
        address: u64,
        size: usize,
        protection: u32,
        pointer: usize,
    ) -> Result<(), String> {
        let valid = [&self.ram, &self.sram, &self.vectors, &self.rom]
            .iter()
            .copied()
            .chain(self.core_private.iter()).chain(self.core_rom.iter())
            .chain(unsafe { (&*self.tcm.get()).banks.iter().flat_map(|banks|banks.iter().map(|b|&b.memory)) })
            .any(|buffer| {
                let base = buffer.as_ptr() as usize;
                pointer >= base
                    && pointer
                        .checked_add(size)
                        .is_some_and(|end| end <= base + buffer.len())
            });
        if !valid {
            return Err("Only retained native memory backings may be mapped".into());
        }
        self.check(unsafe {
            (self.api.uc_mem_map_ptr)(
                self.core(core)?,
                address,
                size,
                protection,
                pointer as *mut c_void,
            )
        })
    }
    pub fn read_reg(&self, core: usize, reg: i32) -> Result<u32, String> {
        let mut value = 0u64;
        self.check(unsafe {
            (self.api.uc_reg_read)(self.core(core)?, reg, (&mut value as *mut u64).cast())
        })?;
        Ok(value as u32)
    }
    pub fn write_reg(&self, core: usize, reg: i32, value: u32) -> Result<(), String> {
        self.check(unsafe {
            (self.api.uc_reg_write)(self.core(core)?, reg, (&value as *const u32).cast())
        })
    }
    pub fn read_cpu(&self, core: usize, address: u64, size: usize) -> Result<Vec<u8>, String> {
        if size > self.ram.len() + 0x100000 {
            return Err("Memory request exceeds modeled memory bound".into());
        }
        let mut data = vec![0; size];
        self.check(unsafe {
            (self.api.uc_mem_read)(self.core(core)?, address, data.as_mut_ptr().cast(), size)
        })?;
        Ok(data)
    }
    pub fn write_cpu(&self, core: usize, address: u64, data: &[u8]) -> Result<(), String> {
        self.check(unsafe {
            (self.api.uc_mem_write)(self.core(core)?, address, data.as_ptr().cast(), data.len())
        })
    }
    pub fn write_bus(&self, core: usize, address: u64, data: &[u8]) -> Result<(), String> {
        let physical = if (0x40000000..0x40000000 + self.ram.len() as u64).contains(&address) {
            address - 0x40000000
        } else {
            address
        };
        // No Rust reference to State may survive a call into Python.
        unsafe {
            let state = &mut *self.state.get();
            if physical.checked_add(data.len() as u64).is_some_and(|end|end<=self.ram.len() as u64) {
                state.bus_write_dram(physical,data.len() as u64,self.ram.len() as u64);
            } else {
                state.invalidate_exclusive(address, data.len() as u64, core, self.ram.len() as u64);
                state.invalidate_code(address, data.len() as u64, core, self.ram.len() as u64);
            }
        }
        if physical
            .checked_add(data.len() as u64)
            .is_some_and(|end| end <= self.ram.len() as u64)
        {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    data.as_ptr(),
                    self.ram.as_ptr().add(physical as usize),
                    data.len(),
                );
            }
        } else {
            self.write_cpu(core, address, data)?;
        }
        if self.executing.get().is_none() {
            self.flush_pending()?;
        }
        Ok(())
    }
    pub(crate) fn convert_declared_ui(
        &self,
        plan: &crate::raster::DeclaredUIConversionPlan,
        origin: ConversionOrigin,
    ) -> PyResult<()> {
        plan.validate_bounds(self.ram.len()).map_err(pyo3::exceptions::PyValueError::new_err)?;
        if (0..2).any(|core| crate::audit::enabled(self, core)) {
            return Err(crate::error("Declared UI conversion is unavailable during CPU routine audits".into()));
        }
        let destination = plan.destination_range();
        {
            let tcm = unsafe { &*self.tcm.get() };
            for core in 0..2 {
                for bank in 0..2 {
                    if let Some((base, size)) = tcm.extent(core, bank) {
                        let end = base + size as u64;
                        for alias in [0, 0x40000000] {
                            let start = destination.start as u64 + alias;
                            let finish = destination.end as u64 + alias;
                            if start >= base && start < end && finish > end {
                                return Err(crate::error(
                                    "Declared UI destination crosses a live TCM endpoint".into(),
                                ));
                            }
                        }
                    }
                }
            }
        }
        let core = origin.core();
        let counters = Some((origin.instructions, origin.ticks));
        for row in plan.source_rows() {
            crate::access_probe::record(self, core, row.start as u64, row.len() as u64,
                "model-bus-read", counters).map_err(crate::error)?;
        }
        crate::access_probe::record(self, core, destination.start as u64, destination.len() as u64,
            "model-bus-write", counters).map_err(crate::error)?;
        let source = plan.source_range();
        // SAFETY: the immutable plan bounds disjoint spans in this fixed backing.
        // The owner thread holds the GIL; no Python, FFI or detachment occurs during conversion.
        unsafe {
            let input = std::slice::from_raw_parts(self.ram.as_ptr().add(source.start), source.len());
            let output = std::slice::from_raw_parts_mut(
                self.ram.as_ptr().add(destination.start), destination.len(),
            );
            plan.convert(input, output);
        }
        unsafe {
            (*self.state.get()).bus_write_dram(
                destination.start as u64, destination.len() as u64, self.ram.len() as u64,
            );
        }
        if self.executing.get().is_none() {
            self.flush_pending().map_err(crate::error)?;
        }
        Ok(())
    }

    pub fn instruction(&self, core: usize, address: u64, size: u32) -> Result<Instruction, String> {
        if size != 2 && size != 4 {
            return Err(format!("Unsupported instruction size {size}"));
        }
        let state = unsafe { &mut *self.state.get() };
        if state.cache_enabled {
            if state.fast {
                // Direct-mapped front of the cache: no hashing on a hit.
                let slot = &state.front[core][front_index(address)];
                if slot.address == address && slot.entry.size == size {
                    return Ok(slot.entry);
                }
            }
            if let Some(entry) = state.cache[core].get(&address).copied()
                && entry.size == size
            {
                if state.fast {
                    state.front[core][front_index(address)] = Slot { address, entry };
                }
                return Ok(entry);
            }
        }
        let mut raw = [0; 4];
        self.check(unsafe {
            (self.api.uc_mem_read)(
                self.core(core)?,
                address,
                raw.as_mut_ptr().cast(),
                size as usize,
            )
        })?;
        let mut entry = classify(size, raw);
        entry.milestone = state.entries.contains(&address);
        if !(address < 0xffff0000 && address + size as u64 > 0xfff01000) {
            if state.cache_enabled {
                state.cache[core].insert(address, entry);
                if state.fast {
                    state.front[core][front_index(address)] = Slot { address, entry };
                }
            }
            for page in address >> 12..=(address + size as u64 - 1) >> 12 {
                state.pages[core].entry(page).or_default().insert(address);
                state.watch_page(core, page << 12, self.ram.len() as u64);
            }
            self.apply_watches()?;
        }
        Ok(entry)
    }
    pub fn flush_pending(&self) -> Result<(), String> {
        self.paused()?;
        for core in 0..2 {
            let pending = unsafe { std::mem::take(&mut (*self.state.get()).pending[core]) };
            if !pending.is_empty() {
                unsafe { (*self.state.get()).touch() };
            }
            for page in pending {
                self.check(unsafe {
                    (self.api.uc_ctl)(
                        self.engines[core],
                        ffi::ctl(9, 2, 1),
                        page << 12,
                        (page + 1) << 12,
                    )
                })?;
            }
        }
        Ok(())
    }
    pub fn clear_caches(&self) -> Result<(), String> {
        self.paused()?;
        unsafe {
            (*self.state.get()).clear_derived();
        }
        for engine in self.engines {
            self.check(unsafe { (self.api.uc_ctl)(engine, ffi::ctl(10, 0, 1)) })?;
        }
        Ok(())
    }
    /// Settle budgeted accounting for `core`: count the instructions run
    /// since the last settle (the budget consumed, less the `tail` of the
    /// newest block that did not run, which `give_back` returns to the
    /// budget) and append the last of them to the trace from the ring.
    pub fn acct_settle(&self, core: usize, tail: u64, give_back: bool) -> Result<(), String> {
        let state = unsafe { &mut *self.state.get() };
        if !state.acct_on[core] {
            state.settle(core);
            return Ok(());
        }
        let record = self.acct[core];
        let (budget, head) = unsafe { ((*record).budget, (*record).head) };
        let executed = state.acct_synced[core] - budget - tail as i64;
        if executed < 0 {
            return Err(format!("Budgeted accounting went negative on CPU{core}"));
        }
        if give_back {
            unsafe { (*record).budget += tail as i64 };
        }
        state.acct_synced[core] = unsafe { (*record).budget };
        let ring = &self.acct_rings[core];
        let from = state.acct_traced[core].max(head.saturating_sub(ring.len() as u64));
        // The newest TRACE_CAPACITY instructions, filled from the back.
        let mut recent = [Trace::default(); TRACE_CAPACITY];
        let mut filled = 0;
        'blocks: for index in (from..head).rev() {
            let entry = ring[(index & (ring.len() as u64 - 1)) as usize];
            let (start, count) = (entry >> 32, entry & 0xffff_ffff);
            let ran = if index + 1 == head { count.saturating_sub(tail) } else { count };
            let frozen = state.acct_frozen[core].iter().find(|(i, _)| *i == index).map(|(_, w)| w.clone());
            let words = match frozen {
                Some(words) => words,
                None => crate::hooks::block_entries(self, core, start, (count * 4) as u32)?,
            };
            for k in (0..ran as usize).rev() {
                filled += 1;
                recent[TRACE_CAPACITY - filled] = words[k];
                if filled == TRACE_CAPACITY {
                    break 'blocks;
                }
            }
        }
        let state = unsafe { &mut *self.state.get() };
        state.trace[core].extend(&recent[TRACE_CAPACITY - filled..]);
        state.acct_traced[core] = head;
        state.acct_frozen[core].clear();
        state.count_executed(core, executed as u64);
        Ok(())
    }
    /// After settling: make the next block go through the hook (budget 0).
    pub fn acct_reset(&self, core: usize) {
        let state = unsafe { &mut *self.state.get() };
        if state.acct_on[core] {
            unsafe { (*self.acct[core]).budget = 0 };
            state.acct_synced[core] = 0;
        }
    }
    /// The newest accounted block of `core`: (start, instruction count).
    pub fn acct_top(&self, core: usize) -> Option<(u64, u64)> {
        let record = self.acct[core];
        let head = unsafe { (*record).head };
        let state = unsafe { &*self.state.get() };
        if !state.acct_on[core] || head == state.acct_traced[core] {
            return None;
        }
        let ring = &self.acct_rings[core];
        let entry = ring[((head - 1) & (ring.len() as u64 - 1)) as usize];
        Some((entry >> 32, entry & 0xffff_ffff))
    }
    /// Admit a block from the hook: it runs now, and `room` more instructions
    /// may follow before the next check.
    pub fn acct_admit(&self, core: usize, start: u64, count: u64, room: u64) {
        let record = self.acct[core];
        let room = room.min(i64::MAX as u64 / 2) as i64;
        unsafe {
            // The ring is written through the pointer Unicorn also writes.
            let head = (*record).head;
            *(*record).ring.add((head & (*record).mask) as usize) = start << 32 | count;
            (*record).head = head + 1;
            (*record).budget = room - count as i64;
            (*self.state.get()).acct_synced[core] = room;
        }
    }
    /// A store is about to overwrite decoded code: capture the words of the
    /// blocks not yet in the trace while they still hold what ran.
    pub fn acct_freeze(&self) -> Result<(), String> {
        for core in 0..2 {
            let state = unsafe { &*self.state.get() };
            if !state.acct_on[core] {
                continue;
            }
            let head = unsafe { (*self.acct[core]).head };
            let ring = &self.acct_rings[core];
            let from = state.acct_traced[core].max(head.saturating_sub(ring.len() as u64));
            for index in from..head {
                if state.acct_frozen[core].iter().any(|(i, _)| *i == index) {
                    continue;
                }
                let entry = ring[(index & (ring.len() as u64 - 1)) as usize];
                let (start, count) = (entry >> 32, entry & 0xffff_ffff);
                // Raw reads: the decode cache was just evicted and must stay so.
                let mut words = Vec::with_capacity(count as usize);
                for k in 0..count {
                    let bytes = self.read_cpu(core, start + 4 * k, 4)?;
                    words.push(Trace { address: start + 4 * k, raw: bytes[..4].try_into().unwrap(), size: 4 });
                }
                unsafe { (*self.state.get()).acct_frozen[core].push((index, words.into())) };
            }
        }
        Ok(())
    }
    /// Turn budgeted accounting on or off for `core` (with the filter).
    pub fn acct_enable(&self, core: usize, on: bool) {
        let record = self.acct[core];
        let state = unsafe { &mut *self.state.get() };
        let on = on && !record.is_null();
        state.acct_on[core] = on;
        if !record.is_null() {
            unsafe {
                (*record).enabled = on as u64;
                (*record).budget = 0;
                state.acct_traced[core] = (*record).head;
            }
        }
        state.acct_synced[core] = 0;
        state.acct_frozen[core].clear();
    }
    /// Declare the pages queued by `State::watch_page` to both engines, so
    /// stores there reach the store hook while it is watched.
    #[inline]
    pub fn apply_watches(&self) -> Result<(), String> {
        let state = unsafe { &mut *self.state.get() };
        if state.watch_queue.is_empty() {
            return Ok(());
        }
        let queue = std::mem::take(&mut state.watch_queue);
        if let Some(watch) = self.api.x70.watch {
            for page in queue {
                for engine in self.engines {
                    self.check(unsafe { watch(engine, page, 0x1000) })?;
                }
            }
        }
        Ok(())
    }
    /// Install the patched Unicorn's translation filter on `core` when `want`
    /// and nothing needs a hook on every instruction (instruction mode, the
    /// decode cache off, breakpoints, coverage, probes, fetch guards, audits);
    /// remove it otherwise. Either change flushes that engine's translations.
    pub fn set_filter(&self, core: usize, want: bool) -> Result<(), String> {
        let (Some(install), Some(_), Some(_)) =
            (self.api.x70.insn_filter, self.api.x70.retry_block, self.api.x70.request_restart)
        else {
            return Ok(());
        };
        let state = unsafe { &mut *self.state.get() };
        let want = want
            && state.cache_enabled
            && unsafe {
                (*self.breakpoints.get())[core].is_empty()
                    && (*self.coverage.get())[core].is_none()
                    && (*self.exec_probe.get())[core].is_none()
                    && (*self.fetch_guards.get())[core].is_none()
                    && (*self.audits.get())[core].is_none()
            };
        if state.filtered[core] == want {
            return Ok(());
        }
        self.acct_settle(core, 0, true)?;
        let state = unsafe { &mut *self.state.get() };
        state.settle(core);
        state.filtered[core] = want;
        let user = (&*self.hook_data[core] as *const HookData).cast_mut().cast::<c_void>();
        self.check(unsafe {
            if want {
                install(self.engines[core], Some(hooks::filter), Some(hooks::restart), user)
            } else {
                install(self.engines[core], None, None, std::ptr::null_mut())
            }
        })?;
        self.acct_enable(core, want);
        Ok(())
    }
    /// Watched store hooks need every page whose stores matter declared;
    /// audits and store logs see every store instead.
    pub fn set_store_watch(&self, watched: bool) -> Result<(), String> {
        if let (Some(set), Some(_)) = (self.api.x70.hook_watched, self.api.x70.watch) {
            self.apply_watches()?;
            for &(core, hook) in &self.store_hooks {
                self.check(unsafe { set(self.engines[core], hook, watched) })?;
            }
        }
        Ok(())
    }
    pub fn stop(&self, core: usize) {
        unsafe {
            (self.api.uc_emu_stop)(self.engines[core]);
        }
    }
    pub fn event(&self, kind: &str, core: usize, address: u64, size: u32, value: i64) {
        // All machine references/RefCell borrows are dropped before Python runs;
        // MMIO may safely query registers/memory, write DMA data, or stop CPU.
        // Python may change any device state: repeatable reads end here.
        // Bulk block accounting ends too: the event sees exact counts and the
        // rest of the block runs per instruction (Python may move limits).
        if let Err(message) = self.acct_settle(core, 0, true) {
            self.fail_native(core, message);
        }
        unsafe {
            let state = &mut *self.state.get();
            for cache in state.mmio_read_cache.iter_mut() {
                cache.clear();
            }
        }
        Python::attach(|py| {
            let callback = self.callback.borrow().as_ref().map(|c| c.clone_ref(py));
            let result = if let Some(callback) = callback {
                let state = unsafe { &mut *self.state.get() };
                state.python_callbacks += 1;
                if self.counted.get() {
                    callback
                        .call1(
                            py,
                            (
                                kind,
                                core,
                                address,
                                size,
                                value,
                                state.instructions,
                                state.ticks,
                                state.tick_phase,
                                state.cores[core].instructions,
                                state.aux_version,
                            ),
                        )
                        .map(|_| ())
                } else {
                    callback
                        .call1(py, (kind, core, address, size, value))
                        .map(|_| ())
                }
            } else {
                Err(pyo3::exceptions::PyRuntimeError::new_err(
                    "No Python peripheral/event handler installed",
                ))
            };
            if let Err(error) = result {
                *self.callback_error.borrow_mut() = Some(error);
                self.stop(core);
            }
        });
        // Python may have moved a limit, deadline or remap: recheck at the
        // next block.
        self.acct_reset(core);
    }
    pub fn fail_native(&self, core: usize, message: String) {
        *self.native_error.borrow_mut() = Some(message);
        self.stop(core);
    }
}

// Detach executes synchronously on the original calling thread. This wrapper
// permits only that bounded call, never sends an engine to another thread.
// Machine is #[pyclass(unsendable)] and nested execution is checked above.
pub struct Runner {
    pub inner: *const Inner,
    pub core: usize,
    pub begin: u64,
    pub until: u64,
    pub timeout: u64,
    pub count: usize,
}
unsafe impl Send for Runner {}
impl Runner {
    pub fn run(self) -> i32 {
        let inner = unsafe { &*self.inner };
        unsafe {
            (inner.api.uc_emu_start)(
                inner.engines[self.core],
                self.begin,
                self.until,
                self.timeout,
                self.count,
            )
        }
    }
}
