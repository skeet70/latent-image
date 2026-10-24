mod audit;
mod engine;
mod fetch_guard;
mod ffi;
mod fff63;
mod fuji;
mod raster;
mod gradient;
mod hooks;
mod observe;
mod access_probe;
mod tcm;
mod scene;
mod state;
mod menu;
mod phase_clock;
mod spi0;
#[cfg(test)]
mod tests;

use engine::{Inner, Runner};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::pyclass::{PyTraverseError, PyVisit};
use pyo3::types::{PyBytes, PyDict, PyInt, PyList, PyTuple};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

pyo3::create_exception!(_native, NativeError, PyRuntimeError);
fn error(message: String) -> PyErr {
    NativeError::new_err(message)
}

fn conversion_integer(value: &Bound<'_, PyAny>, name: &str) -> PyResult<u64> {
    if !value.is_exact_instance_of::<PyInt>() {
        return Err(PyValueError::new_err(format!(
            "{name} must be an actual Python int, not bool or a coercible value",
        )));
    }
    value.extract::<u64>()
        .map_err(|_| PyValueError::new_err(format!("{name} must be in 0..2^64-1")))
}

#[pyclass(unsendable, module = "x70_native._native")]
struct Machine {
    inner: Box<Inner>,
    // `(pc text, byte text)` tuples already built for `state()`'s recent-
    // instruction trace, by (address, instruction bytes, size): consecutive
    // quanta mostly execute the same instructions, and the tuples are
    // immutable, so they are shared rather than formatted again.
    trace_text: RefCell<HashMap<(u64, u32, u32), Py<PyTuple>>>,
}
// Distinct (address, bytes) pairs kept; the cache restarts from empty above this.
const TRACE_TEXT_LIMIT: usize = 1 << 18;

type CoreInput = (u64, bool, u32, u32, u64, u64, u64);
type MonitorInput = Option<(u64, bool)>;

#[pymethods]
impl Machine {
    // Every Python object the engine holds: the event callback and the
    // devices (and flag names) of the plain registers. Devices refer to
    // their board, which owns this Machine, so an unreported reference
    // would keep the whole board (engines, DRAM) alive forever.
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        if let Ok(callback) = self.inner.callback.try_borrow() {
            if let Some(callback) = callback.as_ref() {
                visit.call(callback)?;
            }
        }
        if let Ok(plain) = self.inner.plain.try_borrow() {
            for register in plain.values() {
                visit.call(&register.device)?;
                if let Some(engine::PlainRead::Formula(_, _, Some(flag))) = &register.read {
                    visit.call(flag)?;
                }
            }
        }
        Ok(())
    }
    fn __clear__(&self) {
        self.inner.callback.borrow_mut().take();
        if let Ok(mut plain) = self.inner.plain.try_borrow_mut() {
            *plain = Default::default();
        }
    }
    #[new]
    #[pyo3(signature=(library, ram_size, rom, low_rom_alias=true, entries=Vec::new(), tcm_parameters=None))]
    fn new(
        py: Python<'_>,
        library: &str,
        ram_size: usize,
        rom: &[u8],
        low_rom_alias: bool,
        entries: Vec<u64>,
        tcm_parameters: Option<tcm::Parameters>,
    ) -> PyResult<Self> {
        let tcm_parameters=match tcm_parameters {
            Some(p)=>p,
            None=>PyModule::import(py,"emulator.boards.cpu_parameters")?
                .getattr("default_tcm_parameters")?.call0()?.extract()?,
        };
        Ok(Self {
            inner: Inner::new(Path::new(library), ram_size, rom, low_rom_alias, entries,tcm_parameters)
                .map_err(error)?,
            trace_text: RefCell::new(HashMap::new()),
        })
    }
    /// Install the event handler. With `counters=True` each event is called
    /// as `(kind, core, address, size, value, instructions, virtual_ticks,
    /// tick_phase, core_instructions, aux_version)`; the extras are the
    /// counters at that moment, the executing core's instruction count and
    /// `aux_version()`. Without it the five original arguments are passed.
    /// Plain device registers: (address, device) pairs whose 32-bit reads
    /// return `device.registers.get(address, 0)` and whose writes only store
    /// `value & 0xffffffff` there, with no other effect (declared by the
    /// device, checked by RustBoard). The engine serves them without an
    /// event; an empty list turns this off.
    #[allow(clippy::type_complexity)]
    fn set_plain_registers(
        &self,
        entries: Vec<(u64, Py<PyAny>, Option<Bound<'_, PyAny>>, Option<u32>)>,
    ) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        let mut plain = crate::state::AddressMap::default();
        for (address, device, read, write) in entries {
            // read: None, "direct", "timed", or (clear, set, flag-or-None).
            let read = match read {
                None => None,
                Some(spec) if spec.extract::<String>().is_ok_and(|kind| kind == "direct") => {
                    Some(engine::PlainRead::Direct)
                }
                Some(spec) if spec.extract::<String>().is_ok_and(|kind| kind == "timed") => {
                    Some(engine::PlainRead::Timed)
                }
                Some(spec) => {
                    let (clear, set, flag) = spec.extract::<(u32, u32, Option<Py<pyo3::types::PyString>>)>()?;
                    Some(engine::PlainRead::Formula(clear, set, flag))
                }
            };
            plain.insert(address, engine::PlainRegister { device, read, write });
        }
        *self.inner.plain.borrow_mut() = plain;
        Ok(())
    }
    fn plain_accesses(&self) -> u64 {
        unsafe { (*self.inner.state.get()).plain_accesses }
    }
    #[pyo3(signature=(callback,counters=false))]
    fn set_handler(&self, callback: Py<PyAny>, counters: bool) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        *self.inner.callback.borrow_mut() = Some(callback);
        self.inner.counted.set(counters);
        Ok(())
    }
    /// Version of the native-mutated state a mirror must re-read (see
    /// `state()`): bumped by every native change to waiting/PMU/CP14 fields,
    /// exclusive monitors and counts, CP14 sites and pending invalidations.
    fn aux_version(&self) -> u64 {
        unsafe { (*self.inner.state.get()).aux_version }
    }
    fn configure_tcm(&self,parameters:tcm::Parameters)->PyResult<()> {
        self.inner.paused().map_err(error)?;
        if parameters.len()!=2 { return Err(error("Require two TCM parameter sets".into())); }
        for (i,(size,reset)) in parameters.iter().enumerate() {
            let b=unsafe { &(&*self.inner.tcm.get()).banks[0][i] };
            if b.memory.len()!=*size || b.reset!=tcm::normalize(*size,*reset).map_err(error)? {
                return Err(error("TCM SoC differs from constructor parameters; explicit constructor parameters required".into()));
            }
        }
        Ok(())
    }
    fn tcm_export(&self,core:usize)->PyResult<tcm::Snapshot> {
        self.inner.paused().map_err(error)?;self.inner.core(core).map_err(error)?;
        Ok(unsafe { (&*self.inner.tcm.get()).snapshot(core) })
    }
    fn tcm_restore(&self,core:usize,snapshot:tcm::Snapshot)->PyResult<()> {
        self.inner.paused().map_err(error)?;self.inner.core(core).map_err(error)?;
        tcm::restore(&self.inner,core,&snapshot).map_err(error)
    }
    fn tcm_default_layout(&self)->bool { unsafe { (&*self.inner.tcm.get()).default_layout() } }
    fn tcm_regions(&self,core:usize)->PyResult<Vec<(u64,usize)>> {
        self.inner.core(core).map_err(error)?;Ok(tcm::regions(&self.inner,core))
    }
    fn tcm_base_layout(&self,core:usize)->PyResult<Vec<(u64,u64,u32)>> {
        self.inner.core(core).map_err(error)?;tcm::base_layout(&self.inner,core).map_err(error)
    }
    #[pyo3(signature=(core,stack_start,stack_end,text_begin=0,text_end=0,fault_address=0,trace=false,data_begin=0,data_end=0))]
    #[allow(clippy::too_many_arguments)]
    fn audit_begin(
        &self,
        core: usize,
        stack_start: u64,
        stack_end: u64,
        text_begin: u64,
        text_end: u64,
        fault_address: u64,
        trace: bool,
        data_begin: u64,
        data_end: u64,
    ) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        self.inner.core(core).map_err(error)?;
        let audit = audit::Audit::new(
            self.inner.ram.len() as u64,
            stack_start,
            stack_end,
            text_begin,
            text_end,
            fault_address,
            trace,
            data_begin,
            data_end,
        )
        .map_err(error)?;
        self.inner.ensure_audit_hooks(core).map_err(error)?;
        unsafe {
            (*self.inner.audits.get())[core] = Some(audit);
        }
        Ok(())
    }
    fn audit_state<'py>(&self, py: Python<'py>, core: usize) -> PyResult<Bound<'py, PyDict>> {
        self.inner.core(core).map_err(error)?;
        // Snapshot before converting into Python: no mutable/native borrow can
        // survive an arbitrary Python-visible boundary or a later engine run.
        let snapshot = unsafe { (*self.inner.audits.get())[core].clone() };
        let result = PyDict::new(py);
        result.set_item("enabled", snapshot.is_some())?;
        if let Some(audit) = snapshot {
            result.set_item("instructions", audit.instructions)?;
            result.set_item("minimum_sp", audit.minimum_sp)?;
            result.set_item("failure", audit.failure)?;
            result.set_item("writes", audit.writes)?;
            result.set_item(
                "dirty_pages",
                audit.dirty_pages.into_iter().collect::<Vec<_>>(),
            )?;
            result.set_item("pcs", audit.pcs)?;
        } else {
            result.set_item("instructions", 0)?;
            result.set_item("minimum_sp", 0)?;
            result.set_item("failure", py.None())?;
            result.set_item("writes", Vec::<audit::Write>::new())?;
            result.set_item("dirty_pages", Vec::<u64>::new())?;
            result.set_item("pcs", Vec::<u64>::new())?;
        }
        Ok(result)
    }
    #[pyo3(signature=(core,ranges,vector_ranges=Vec::new(),vector_opcode=0xe59ff018,fault_address=0,stop_addresses=Vec::new()))]
    fn fetch_guard_begin(
        &self,
        core: usize,
        ranges: Vec<fetch_guard::Range>,
        vector_ranges: Vec<fetch_guard::Range>,
        vector_opcode: u32,
        fault_address: u64,
        stop_addresses: Vec<u64>,
    ) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        self.inner.core(core).map_err(error)?;
        let guard = fetch_guard::FetchGuard::new(
            ranges,
            vector_ranges,
            vector_opcode,
            fault_address,
            stop_addresses,
        )
        .map_err(error)?;
        unsafe {
            (*self.inner.fetch_guards.get())[core] = Some(guard);
        }
        Ok(())
    }
    fn fetch_guard_end(&self, core: usize) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        self.inner.core(core).map_err(error)?;
        unsafe {
            (*self.inner.fetch_guards.get())[core] = None;
        }
        Ok(())
    }
    fn fetch_guard_state<'py>(&self, py: Python<'py>, core: usize) -> PyResult<Bound<'py, PyDict>> {
        self.inner.core(core).map_err(error)?;
        let snapshot = unsafe { (*self.inner.fetch_guards.get())[core].clone() };
        let result = PyDict::new(py);
        result.set_item("enabled", snapshot.is_some())?;
        result.set_item(
            "checked_fetches",
            snapshot.as_ref().map_or(0, |g| g.checked_fetches),
        )?;
        result.set_item(
            "accepted_fetches",
            snapshot.as_ref().map_or(0, |g| g.accepted_fetches),
        )?;
        result.set_item(
            "vector_fetches",
            snapshot.as_ref().map_or(0, |g| g.vector_fetches),
        )?;
        result.set_item(
            "halted",
            snapshot.as_ref().is_some_and(|g| g.halt.is_some()),
        )?;
        if let Some(guard) = snapshot {
            result.set_item("range_fetches", guard.range_fetches)?;
            result.set_item("vector_range_fetches", guard.vector_range_fetches)?;
            result.set_item("ranges", guard.ranges)?;
            result.set_item("vector_ranges", guard.vector_ranges)?;
            result.set_item("vector_opcode", guard.vector_opcode)?;
            result.set_item("fault_address", guard.fault_address)?;
            result.set_item("stop_addresses", guard.stop_addresses)?;
            if let Some(halt) = guard.halt {
                let record = PyDict::new(py);
                record.set_item("kind", halt.kind)?;
                record.set_item("reason", halt.reason)?;
                record.set_item("pc", halt.pc)?;
                record.set_item("size", halt.size)?;
                record.set_item("registers", halt.registers)?;
                record.set_item("opcode", halt.opcode)?;
                result.set_item("halt", record)?;
            } else {
                result.set_item("halt", py.None())?;
            }
        } else {
            result.set_item("range_fetches", Vec::<u64>::new())?;
            result.set_item("vector_range_fetches", Vec::<u64>::new())?;
            result.set_item("halt", py.None())?;
        }
        Ok(result)
    }
    /// Replace one core's breakpoint set; an empty list clears it. Hits are
    /// reported through the event handler on every accepted instruction entry.
    fn set_breakpoints(&self, core: usize, addresses: Vec<u64>) -> PyResult<()> {
        self.inner.core(core).map_err(error)?;
        if addresses.len() > 4096 || addresses.iter().any(|&pc| pc > u32::MAX as u64) {
            return Err(error(
                "Breakpoints must be ARM addresses (at most 4096 per core)".into(),
            ));
        }
        let mut addresses = addresses;
        addresses.sort_unstable();
        addresses.dedup();
        self.inner.set_breakpoints(core, addresses).map_err(error)
    }
    fn release_menu_key(&self, session: &Bound<'_, PyAny>, key: &str, cap: u64) -> PyResult<()> {
        menu::release(&self.inner, session, menu::ReleaseInput::parse(key)?, cap)
    }
    fn menu_keys_released(&self) -> PyResult<bool> {
        menu::keys_released(&self.inner)
    }
    #[pyo3(signature=(core,stack_start,stack_end,allowed=None,limit=1048576,include_stack=false))]
    fn store_log_begin(
        &self,
        core: usize,
        stack_start: u64,
        stack_end: u64,
        allowed: Option<Vec<(u64, u64)>>,
        limit: usize,
        include_stack: bool,
    ) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        self.inner.core(core).map_err(error)?;
        let log = observe::StoreLog::new(stack_start, stack_end, allowed, limit, include_stack)
            .map_err(error)?;
        unsafe {
            (*self.inner.store_logs.get())[core] = Some(log);
        }
        Ok(())
    }
    fn store_log_end(&self, core: usize) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        self.inner.core(core).map_err(error)?;
        unsafe {
            (*self.inner.store_logs.get())[core] = None;
        }
        Ok(())
    }
    /// `stores` are the original `(address, width, value, pc)` tuples; with
    /// `records` also `(address, width, value, pc, virtual_address, flags)`
    /// (flags: 1 stack window, 2 exclusive store, 4 straddles the window).
    #[pyo3(signature=(core,stores=true,records=false))]
    fn store_log_state<'py>(
        &self,
        py: Python<'py>,
        core: usize,
        stores: bool,
        records: bool,
    ) -> PyResult<Bound<'py, PyDict>> {
        self.inner.core(core).map_err(error)?;
        let log = unsafe { (*self.inner.store_logs.get())[core].as_ref() };
        let result = PyDict::new(py);
        result.set_item("enabled", log.is_some())?;
        result.set_item("count", log.map_or(0, |l| l.records.len()))?;
        result.set_item("violation", log.and_then(|l| l.violation.clone()))?;
        if stores {
            result.set_item(
                "stores",
                log.map_or_else(Vec::new, |l| {
                    l.records
                        .iter()
                        .map(|&(a, w, v, pc, _, _)| (a, w, v, pc))
                        .collect::<Vec<observe::Store>>()
                }),
            )?;
        }
        if records {
            result.set_item("records", log.map_or_else(Vec::new, |l| l.records.clone()))?;
        }
        Ok(result)
    }
    /// Collect the set of executed instruction addresses for one core.
    fn coverage_begin(&self, core: usize) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        self.inner.core(core).map_err(error)?;
        unsafe {
            (*self.inner.coverage.get())[core] = Some(observe::Coverage::new());
        }
        Ok(())
    }
    fn coverage_end(&self, core: usize) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        self.inner.core(core).map_err(error)?;
        unsafe {
            (*self.inner.coverage.get())[core] = None;
        }
        Ok(())
    }
    fn coverage_state(&self, core: usize) -> PyResult<Option<Vec<u64>>> {
        self.inner.core(core).map_err(error)?;
        let set = unsafe { (*self.inner.coverage.get())[core].as_ref() };
        Ok(set.map(observe::Coverage::addresses))
    }
    /// Count executed instructions per half-open range and keep the first
    /// `trace` executed PCs in order (both natively; no Python callback).
    /// Ranges are canonical (DRAM offsets; the 0x40000000 alias is folded).
    #[pyo3(signature=(core,ranges=Vec::new(),trace=0))]
    fn exec_probe_begin(&self, core: usize, ranges: Vec<(u64, u64)>, trace: usize) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        self.inner.core(core).map_err(error)?;
        let probe =
            observe::ExecProbe::new(ranges, trace, self.inner.ram.len() as u64).map_err(error)?;
        unsafe {
            (*self.inner.exec_probe.get())[core] = Some(probe);
        }
        Ok(())
    }
    fn exec_probe_end(&self, core: usize) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        self.inner.core(core).map_err(error)?;
        unsafe {
            (*self.inner.exec_probe.get())[core] = None;
        }
        Ok(())
    }
    /// `(counts, trace, trace_total)` of the active probe, or None.
    fn exec_probe_state(&self, core: usize) -> PyResult<Option<observe::ProbeState>> {
        self.inner.core(core).map_err(error)?;
        let probe = unsafe { (*self.inner.exec_probe.get())[core].as_ref() };
        Ok(probe.map(|p| (p.counts.clone(), p.trace.clone(), p.trace_total)))
    }
    /// Passive access watch, with physical DRAM extents and both fixed aliases.
    /// All memory hooks are installed globally to include boundary-crossing
    /// loads/stores; exact range filtering is native and supplies no values.
    #[pyo3(signature=(ranges,limit=100000))]
    fn access_probe_begin(&self, ranges: Vec<(u64, u64)>, limit: usize) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        if access_probe::enabled(&self.inner) {
            return Err(error("Access probe is already enabled".into()));
        }
        let probe = access_probe::AccessProbe::new(ranges, limit, self.inner.ram.len() as u64)
            .map_err(error)?;
        for core in 0..2 {
            let mut handles = self.inner.access_probe_hooks.get();
            if handles[core].is_none() {
                let ptr = (&*self.inner.hook_data[core] as *const engine::HookData)
                    .cast_mut().cast::<std::ffi::c_void>();
                handles[core] = Some(self.inner.add_hook(core, (1 << 10) | (1 << 11),
                    hooks::access_probe_memory as *const () as *mut std::ffi::c_void,
                    ptr, 1, 0).map_err(error)?);
                self.inner.access_probe_hooks.set(handles);
            }
        }
        unsafe { *self.inner.access_probe.get() = Some(probe); }
        Ok(())
    }
    fn access_probe_end(&self) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        unsafe { *self.inner.access_probe.get() = None; }
        Ok(())
    }
    fn access_probe_enabled(&self) -> bool {
        access_probe::enabled(&self.inner)
    }
    fn access_probe_state<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let result = PyDict::new(py);
        let probe = unsafe { (*self.inner.access_probe.get()).as_ref() };
        result.set_item("enabled", probe.is_some())?;
        result.set_item("violation", probe.and_then(|p| p.violation.clone()))?;
        let records = pyo3::types::PyList::empty(py);
        if let Some(probe) = probe {
            result.set_item("ranges", &probe.ranges)?;
            result.set_item("limit", probe.limit)?;
            for r in &probe.records {
                let row = PyDict::new(py);
                row.set_item("source", r.source)?;
                row.set_item("core", r.core)?;
                row.set_item("pc", r.pc)?;
                row.set_item("address", r.address)?;
                row.set_item("physical", r.physical)?;
                row.set_item("size", r.size)?;
                row.set_item("instructions", r.instructions)?;
                row.set_item("ticks", r.ticks)?;
                row.set_item("pc_kind", if r.source.starts_with("cpu-") { "accessing-instruction" } else { "core-context" })?;
                records.append(row)?;
            }
        }
        result.set_item("records", records)?;
        Ok(result)
    }
    /// Read-only MemoryBus instrumentation; counters belong to the model clock.
    fn access_probe_bus_read(&self, core: usize, address: u64, size: u64,
                             instructions: u64, ticks: u64) -> PyResult<()> {
        self.inner.core(core).map_err(error)?;
        access_probe::record(&self.inner, core, address, size, "model-bus-read",
            Some((instructions, ticks))).map_err(error)
    }
    /// The running core ends its call before the current breakpoint
    /// instruction (not executed, not counted). Valid inside a breakpoint
    /// handler; the flag is consumed by the hook that raised the event.
    fn breakpoint_halt(&self, core: usize) -> PyResult<()> {
        self.inner.core(core).map_err(error)?;
        let mut halts = self.inner.breakpoint_halt.get();
        halts[core] = true;
        self.inner.breakpoint_halt.set(halts);
        Ok(())
    }
    /// Stop (before executing) at the first instruction entry once virtual
    /// ticks reach `deadline`; u64::MAX disables. Allowed while executing so
    /// a handler that schedules a completion can lower it.
    /// From a callback during a fast-mode run: end the run once the board's
    /// instruction count reaches `instructions` (never extends a limit).
    fn clamp_run(&self, instructions: u64) -> PyResult<()> {
        let state = unsafe { &mut *self.inner.state.get() };
        if self.inner.executing.get().is_none() || !state.fast || state.entry_limit == 0 {
            return Err(PyValueError::new_err("clamp_run needs a counted fast-mode run in progress"));
        }
        let limit = state.entered + instructions.saturating_sub(state.instructions);
        if limit < state.entry_limit {
            state.entry_limit = limit;
        }
        Ok(())
    }
    /// Declare the MMIO read just answered on `core` repeatable: until Python
    /// next runs (any event or emu_start), the engine serves it from the value
    /// already written to its MMIO page, without an event.
    fn cache_mmio_read(&self, core: usize, address: u64, size: u32) -> PyResult<()> {
        self.inner.core(core).map_err(error)?;
        let state = unsafe { &mut *self.inner.state.get() };
        let cache = &mut state.mmio_read_cache[core];
        if !cache.contains(&(address, size)) {
            cache.push((address, size));
        }
        Ok(())
    }
    #[pyo3(signature=(deadline=u64::MAX))]
    fn set_tick_deadline(&self, deadline: u64) {
        unsafe {
            (*self.inner.state.get()).tick_deadline = deadline;
        }
    }
    fn tick_deadline(&self) -> u64 {
        unsafe { (*self.inner.state.get()).tick_deadline }
    }
    /// Override the CP14 CPU-identity word returned by `core`'s stock read.
    fn set_cp14_identity(&self, core: usize, value: u32) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        self.inner.core(core).map_err(error)?;
        let mut identity = self.inner.cp14_identity.get();
        identity[core] = value;
        self.inner.cp14_identity.set(identity);
        Ok(())
    }
    fn cp14_identity(&self) -> (u32, u32) {
        let identity = self.inner.cp14_identity.get();
        (identity[0], identity[1])
    }
    fn memory_address(&self, name: &str) -> PyResult<usize> {
        // Compatibility-only backing addresses, never engine handles. The
        // adapter keeps Machine alive on each ctypes view. Mapping validates
        // ranges against these native-owned fixed buffers.
        let memory = match name {
            "ram" => &self.inner.ram,
            "sram" => &self.inner.sram,
            "vectors" => &self.inner.vectors,
            "rom" => &self.inner.rom,
            _ => return Err(PyValueError::new_err("Unknown backing")),
        };
        Ok(memory.as_ptr() as usize)
    }
    fn reg_read<'py>(
        &self,
        py: Python<'py>,
        core: usize,
        reg: i32,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let mut data = [0u8; 32];
        let mut size = data.len();
        let status = unsafe {
            (self.inner.api.uc_reg_read2)(
                self.inner.core(core).map_err(error)?,
                reg,
                data.as_mut_ptr().cast(),
                &mut size,
            )
        };
        self.inner.check(status).map_err(error)?;
        if size > data.len() {
            return Err(error("Register read exceeded native buffer".into()));
        }
        Ok(PyBytes::new(py, &data[..size]))
    }
    fn reg_write(&self, core: usize, reg: i32, data: &[u8]) -> PyResult<()> {
        self.inner.core(core).map_err(error)?;
        if !state::portable_host_register(reg) {
            unsafe {
                (*self.inner.state.get()).portable_unsafe(
                    core,
                    "Host register write is outside the portable register manifest",
                    None,
                    Some(reg as u32),
                );
            }
        } else if reg == ffi::CPSR && data.len() >= 4 {
            let mode = u32::from_le_bytes(data[..4].try_into().unwrap()) & 31;
            if !matches!(mode, 0x10 | 0x11 | 0x12 | 0x13 | 0x17 | 0x1b | 0x1f) {
                unsafe {
                    (*self.inner.state.get()).portable_unsafe(
                        core,
                        "Host CPSR write selects an unsupported CPU bank",
                        None,
                        Some(mode),
                    );
                }
            }
        }
        let mut size = data.len();
        self.inner
            .check(unsafe {
                (self.inner.api.uc_reg_write2)(
                    self.inner.core(core).map_err(error)?,
                    reg,
                    data.as_ptr().cast(),
                    &mut size,
                )
            })
            .map_err(error)
    }
    #[pyo3(signature=(core,cp,opc1,crn,crm,opc2,sec,is64,value=None))]
    #[allow(clippy::too_many_arguments)]
    fn cpr(
        &self,
        core: usize,
        cp: u32,
        opc1: u32,
        crn: u32,
        crm: u32,
        opc2: u32,
        sec: u32,
        is64: bool,
        value: Option<u64>,
    ) -> PyResult<u64> {
        let mut reg = ffi::CpReg {
            cp,
            is64: is64 as u32,
            sec,
            crn,
            crm,
            opc1,
            opc2,
            val: value.unwrap_or(0),
        };
        let cpu = self.inner.core(core).map_err(error)?;
        if cp==15 && opc1==0 && crn==9 && crm==1 && opc2<=1 && !is64 {
            if let Some(v)=value {
                self.inner.paused().map_err(error)?;
                tcm::set(&self.inner,core,opc2 as usize,v as u32).map_err(error)?;
            }
            return Ok(tcm::register(&self.inner,core,opc2 as usize) as u64);
        }
        if state::host_pmu(cp, opc1, crn, crm, is64) {
            // Even a raw PMCCNTR read updates QEMU's unexported cycle delta.
            unsafe {
                (*self.inner.state.get()).portable_unsafe(
                    core,
                    "Host PMU access bypasses the Rust PMU model",
                    None,
                    None,
                );
            }
        } else if value.is_some() && !state::portable_host_cp15(cp, opc1, crn, crm, opc2, sec, is64)
        {
            unsafe {
                (*self.inner.state.get()).portable_unsafe(
                    core,
                    "Host coprocessor write is outside the portable register manifest",
                    None,
                    None,
                );
            }
        }
        let status = unsafe {
            if value.is_some() {
                (self.inner.api.uc_reg_write)(cpu, 139, (&reg as *const ffi::CpReg).cast())
            } else {
                (self.inner.api.uc_reg_read)(cpu, 139, (&mut reg as *mut ffi::CpReg).cast())
            }
        };
        self.inner.check(status).map_err(error)?;
        Ok(reg.val)
    }
    fn mem_read<'py>(
        &self,
        py: Python<'py>,
        core: usize,
        address: u64,
        size: usize,
    ) -> PyResult<Bound<'py, PyBytes>> {
        access_probe::record(&self.inner, core, address, size as u64,
            "host-cpu-view-read", None).map_err(error)?;
        Ok(PyBytes::new(
            py,
            &self.inner.read_cpu(core, address, size).map_err(error)?,
        ))
    }
    fn mem_write(&self, core: usize, address: u64, data: &[u8]) -> PyResult<()> {
        access_probe::record(&self.inner, core, address, data.len() as u64,
            "host-cpu-view-write", None).map_err(error)?;
        self.inner.write_cpu(core, address, data).map_err(error)
    }
    fn mem_regions(&self, core: usize) -> PyResult<Vec<(u64, u64, u32)>> {
        let mut ptr = std::ptr::null_mut();
        let mut count = 0u32;
        self.inner
            .check(unsafe {
                (self.inner.api.uc_mem_regions)(
                    self.inner.core(core).map_err(error)?,
                    &mut ptr,
                    &mut count,
                )
            })
            .map_err(error)?;
        let result = if count == 0 {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(ptr, count as usize) }
                .iter()
                .map(|r| (r.begin, r.end, r.perms))
                .collect()
        };
        self.inner
            .check(unsafe { (self.inner.api.uc_free)(ptr.cast()) })
            .map_err(error)?;
        Ok(result)
    }
    fn write_bus(&self, core: usize, address: u64, data: &[u8]) -> PyResult<()> {
        self.inner.core(core).map_err(error)?;
        access_probe::record(&self.inner, core, address, data.len() as u64,
            "model-bus-write", None).map_err(error)?;
        self.inner.write_bus(core, address, data).map_err(error)
    }
    fn prepare_declared_ui_conversion(
        &self,
        src_phys: &Bound<'_, PyAny>,
        dst_phys: &Bound<'_, PyAny>,
        width: &Bound<'_, PyAny>,
        height: &Bound<'_, PyAny>,
        stride: &Bound<'_, PyAny>,
    ) -> PyResult<raster::DeclaredUIConversionPlan> {
        raster::DeclaredUIConversionPlan::prepare(
            conversion_integer(src_phys, "src_phys")?,
            conversion_integer(dst_phys, "dst_phys")?,
            conversion_integer(width, "width")?,
            conversion_integer(height, "height")?,
            conversion_integer(stride, "stride")?,
            self.inner.ram.len(),
        ).map_err(PyValueError::new_err)
    }

    fn convert_declared_ui(
        &self,
        plan: &Bound<'_, PyAny>,
        core: &Bound<'_, PyAny>,
        instructions: &Bound<'_, PyAny>,
        ticks: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let plan = plan.extract::<PyRef<'_, raster::DeclaredUIConversionPlan>>()
            .map_err(|_| PyValueError::new_err("Expected a DeclaredUIConversionPlan"))?;
        let origin = engine::ConversionOrigin::new(
            conversion_integer(core, "core")?,
            conversion_integer(instructions, "instructions")?,
            conversion_integer(ticks, "ticks")?,
        ).map_err(PyValueError::new_err)?;
        self.inner.convert_declared_ui(&plan, origin)
    }

    fn mem_unmap(&self, core: usize, address: u64, size: usize) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        self.inner
            .check(unsafe {
                (self.inner.api.uc_mem_unmap)(self.inner.core(core).map_err(error)?, address, size)
            })
            .map_err(error)
    }
    fn mem_map_ptr(
        &self,
        core: usize,
        address: u64,
        size: usize,
        protection: u32,
        pointer: usize,
    ) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        self.inner
            .map_owned(core, address, size, protection, pointer)
            .map_err(error)
    }
    #[pyo3(signature=(core,address,size,protection=7))]
    fn mem_map(&self, core: usize, address: u64, size: usize, protection: u32) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        self.inner
            .check(unsafe {
                (self.inner.api.uc_mem_map)(
                    self.inner.core(core).map_err(error)?,
                    address,
                    size,
                    protection,
                )
            })
            .map_err(error)
    }
    fn mem_protect(&self, core: usize, address: u64, size: usize, protection: u32) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        self.inner
            .check(unsafe {
                (self.inner.api.uc_mem_protect)(
                    self.inner.core(core).map_err(error)?,
                    address,
                    size,
                    protection,
                )
            })
            .map_err(error)
    }
    fn add_mmio_hook(&self, core: usize, begin: u64, end: u64) -> PyResult<usize> {
        self.inner.paused().map_err(error)?;
        self.inner.core(core).map_err(error)?;
        let data = (&*self.inner.hook_data[core] as *const engine::HookData)
            .cast_mut()
            .cast();
        self.inner
            .add_hook(
                core,
                (1 << 10) | (1 << 11),
                hooks::mmio as *const () as *mut std::ffi::c_void,
                data,
                begin,
                end,
            )
            .map_err(error)
    }
    fn remove_cache(&self, core: usize, address: u64, end: u64) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        self.inner
            .check(unsafe {
                (self.inner.api.uc_ctl)(
                    self.inner.core(core).map_err(error)?,
                    ffi::ctl(9, 2, 1),
                    address,
                    end,
                )
            })
            .map_err(error)
    }
    fn flush_tb(&self, core: usize) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        self.inner
            .check(unsafe {
                (self.inner.api.uc_ctl)(self.inner.core(core).map_err(error)?, ffi::ctl(10, 0, 1))
            })
            .map_err(error)
    }
    fn clear_caches(&self) -> PyResult<()> {
        self.inner.clear_caches().map_err(error)
    }
    fn portable_checkpoint_status<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        self.inner.paused().map_err(error)?;
        let s = unsafe { &*self.inner.state.get() };
        let result = PyDict::new(py);
        result.set_item("supported", s.portable_unsafe.is_empty())?;
        result.set_item("contract", "lifetime-arm32-rust-state-v1")?;
        let reasons: Vec<String> = s
            .portable_unsafe
            .iter()
            .map(|((core, reason), (pc, opcode))| {
                format!(
                    "CPU{core}: {reason}{}{}",
                    pc.map_or(String::new(), |p| format!(" at {p:#x}")),
                    opcode.map_or(String::new(), |w| format!(" ({w:#x})"))
                )
            })
            .collect();
        result.set_item("reasons", reasons)?;
        Ok(result)
    }
    fn flush_pending(&self) -> PyResult<()> {
        self.inner.flush_pending().map_err(error)
    }
    #[pyo3(signature=(core,address,size,exclusive=true))]
    fn invalidate_write(
        &self,
        core: usize,
        address: u64,
        size: u64,
        exclusive: bool,
    ) -> PyResult<()> {
        self.inner.core(core).map_err(error)?;
        let s = unsafe { &mut *self.inner.state.get() };
        s.invalidate_code(address, size, core, self.inner.ram.len() as u64);
        if exclusive {
            s.invalidate_exclusive(address, size, core, self.inner.ram.len() as u64);
        }
        Ok(())
    }
    fn instruction<'py>(
        &self,
        py: Python<'py>,
        core: usize,
        address: u64,
        size: u32,
    ) -> PyResult<(u32, Bound<'py, PyBytes>, u32, bool)> {
        self.inner.core(core).map_err(error)?;
        let entry = self.inner.instruction(core, address, size).map_err(error)?;
        Ok((
            entry.size,
            PyBytes::new(py, &entry.raw[..entry.size as usize]),
            entry.word,
            entry.ordinary,
        ))
    }
    #[pyo3(signature=(instructions,ticks,cores,monitors,counts,remap,cache_enabled,cp14_sites=None,seen_entries=None,tick_ratio=None,tick_phase=None))]
    #[allow(clippy::too_many_arguments)]
    fn load_state(
        &self,
        instructions: u64,
        ticks: u64,
        cores: Vec<CoreInput>,
        monitors: Vec<MonitorInput>,
        counts: (u64, u64, u64, u64),
        remap: bool,
        cache_enabled: bool,
        cp14_sites: Option<Vec<(usize, u64)>>,
        seen_entries: Option<Vec<(usize, u64)>>,
        tick_ratio: Option<(u64, u64)>,
        tick_phase: Option<u64>,
    ) -> PyResult<()> {
        if cores.len() != 2 || monitors.len() != 2 {
            return Err(PyValueError::new_err("Exactly two cores required"));
        }
        let state = unsafe { &mut *self.inner.state.get() };
        state.instructions = instructions;
        state.ticks = ticks;
        if let Some((num, den)) = tick_ratio {
            if num == 0 || den == 0 || num > 1 << 32 || den > 1 << 32 {
                return Err(PyValueError::new_err("Invalid instructions-per-tick ratio"));
            }
            state.tick_num = num;
            state.tick_den = den;
        }
        if let Some(phase) = tick_phase {
            if phase >= state.tick_num {
                return Err(PyValueError::new_err("Tick phase exceeds its ratio"));
            }
            state.tick_phase = phase;
        }
        for (index, (instructions, waiting, pmcr, pmcntenset, pmu_start, pmu_frozen, cp14_reads)) in
            cores.into_iter().enumerate()
        {
            state.cores[index] = state::CoreState {
                instructions,
                waiting,
                pmcr,
                pmcntenset,
                pmu_start,
                pmu_frozen,
                cp14_reads,
            };
            state.reservations[index] =
                monitors[index].map(|(address, valid)| state::Reservation { address, valid });
            if let Some((address, _)) = monitors[index] {
                state.watch_page(index, address & !0xfff, self.inner.ram.len() as u64);
            }
        }
        state.exclusive = state::ExclusiveStats {
            loads: counts.0,
            stores_succeeded: counts.1,
            stores_failed: counts.2,
            invalidations: counts.3,
        };
        state.remap_requested = remap;
        state.cache_enabled = cache_enabled;
        if let Some(sites) = cp14_sites {
            state.cp14_sites = sites.into_iter().collect();
        }
        if let Some(entries) = seen_entries {
            state.seen_entries = entries.into_iter().collect();
        }
        for core in 0..2 {
            let on = unsafe { (*self.inner.state.get()).acct_on[core] };
            self.inner.acct_enable(core, on);
        }
        Ok(())
    }
    fn load_trace(&self, traces: Vec<Vec<(u64, Vec<u8>)>>) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        if traces.len() != 2 || traces.iter().any(|t| t.len() > 32) {
            return Err(PyValueError::new_err("Invalid trace rings"));
        }
        let state = unsafe { &mut *self.inner.state.get() };
        for (core, trace) in traces.into_iter().enumerate() {
            state.trace[core].clear();
            for (address, bytes) in trace {
                if bytes.len() != 2 && bytes.len() != 4 {
                    return Err(PyValueError::new_err("Invalid instruction trace size"));
                }
                let mut raw = [0; 4];
                raw[..bytes.len()].copy_from_slice(&bytes);
                state.trace[core].push(state::Trace {
                    address,
                    raw,
                    size: bytes.len() as u32,
                });
            }
        }
        for core in 0..2 {
            let on = unsafe { (*self.inner.state.get()).acct_on[core] };
            self.inner.acct_enable(core, on);
        }
        Ok(())
    }
    #[pyo3(signature=(trace=true))]
    fn state<'py>(&self, py: Python<'py>, trace: bool) -> PyResult<Bound<'py, PyDict>> {
        let s = unsafe { &*self.inner.state.get() };
        let result = PyDict::new(py);
        result.set_item("instructions", s.instructions)?;
        result.set_item("virtual_ticks", s.ticks)?;
        result.set_item("tick_ratio", (s.tick_num, s.tick_den))?;
        result.set_item("tick_phase", s.tick_phase)?;
        result.set_item("aux_version", s.aux_version)?;
        result.set_item(
            "cores",
            s.cores
                .iter()
                .map(|c| {
                    (
                        c.instructions,
                        c.waiting,
                        c.pmcr,
                        c.pmcntenset,
                        c.pmu_start,
                        c.pmu_frozen,
                        c.cp14_reads,
                    )
                })
                .collect::<Vec<_>>(),
        )?;
        result.set_item(
            "monitors",
            s.reservations
                .iter()
                .map(|r| r.map(|r| (r.address, r.valid)))
                .collect::<Vec<_>>(),
        )?;
        result.set_item(
            "counts",
            (
                s.exclusive.loads,
                s.exclusive.stores_succeeded,
                s.exclusive.stores_failed,
                s.exclusive.invalidations,
            ),
        )?;
        result.set_item(
            "cp14_sites",
            s.cp14_sites.iter().copied().collect::<Vec<_>>(),
        )?;
        result.set_item(
            "pending",
            s.pending
                .iter()
                .map(|p| p.iter().copied().collect::<Vec<_>>())
                .collect::<Vec<_>>(),
        )?;
        if trace {
            let rings = PyList::empty(py);
            for ring in &s.trace {
                rings.append(self.ring_list(py, ring)?)?;
            }
            result.set_item("recent", rings)?;
        }
        Ok(result)
    }
    /// After a run of `core`: `(instructions, virtual_ticks, tick_phase,
    /// core0 instructions, core1 instructions, aux_version, core's recent
    /// trace)`. Everything else `state` returns changes only with
    /// `aux_version`, and only the running core's trace moved.
    fn run_state<'py>(
        &self,
        py: Python<'py>,
        core: usize,
    ) -> PyResult<(u64, u64, u64, u64, u64, u64, Bound<'py, PyList>)> {
        let s = unsafe { &*self.inner.state.get() };
        let ring = self.ring_list(py, s.trace.get(core).ok_or_else(|| PyValueError::new_err("core"))?)?;
        Ok((s.instructions, s.ticks, s.tick_phase, s.cores[0].instructions, s.cores[1].instructions,
            s.aux_version, ring))
    }
    fn metrics<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let s = unsafe { &*self.inner.state.get() };
        let result = PyDict::new(py);
        result.set_item("native_code_hooks", s.code_hooks)?;
        result.set_item("native_ram_store_hooks", s.store_hooks)?;
        result.set_item("python_event_callbacks", s.python_callbacks)?;
        result.set_item("fast_execution", s.fast)?;
        result.set_item("fast_downgrades", s.downgrades)?;
        result.set_item("filtered", s.filtered.to_vec())?;
        result.set_item("retried_blocks", s.retried_blocks)?;
        result.set_item("restarts", s.restarts)?;
        result.set_item("cached_mmio_reads", s.cached_mmio_reads)?;
        result.set_item("bulk_blocks", s.bulk_blocks)?;
        result.set_item("blocks_entered", s.blocks_entered)?;
        result.set_item("precise_instructions", s.precise_instructions)?;
        result.set_item("special_blocks", s.special_blocks.to_vec())?;
        result.set_item("code_page_evictions", s.code_generation)?;
        result.set_item(
            "cached_sites",
            s.cache.iter().map(|c| c.len()).collect::<Vec<_>>(),
        )?;
        result.set_item(
            "executed_pages",
            s.pages.iter().map(|c| c.len()).collect::<Vec<_>>(),
        )?;
        Ok(result)
    }
    #[pyo3(signature=(core,begin,until,timeout=0,count=0))]
    fn emu_start(
        &self,
        py: Python<'_>,
        core: usize,
        begin: u64,
        until: u64,
        timeout: u64,
        count: usize,
    ) -> PyResult<i32> {
        self.inner.paused().map_err(error)?;
        self.inner.core(core).map_err(error)?;
        // Python may have changed device state while paused.
        unsafe {
            for cache in (*self.inner.state.get()).mmio_read_cache.iter_mut() {
                cache.clear();
            }
        }
        self.inner.callback_error.borrow_mut().take();
        self.inner.native_error.borrow_mut().take();
        self.inner.executing.set(Some(core));
        let (mut begin, mut count) = (begin, count);
        let status = loop {
            // Fast execution passes no count to Unicorn (that would install its
            // count hook and the slow two-hook path); its instruction hook
            // enforces the limit.
            let fast = unsafe { (*self.inner.state.get()).fast };
            unsafe {
                let state = &mut *self.inner.state.get();
                state.entered = 0;
                state.entry_limit = if fast { count as u64 } else { 0 };
            }
            // Write hooks need the guest PC, and every store, only for
            // audits and store logs.
            let every_store = access_probe::enabled(&self.inner)
                || (0..2).any(|c| audit::enabled(&self.inner, c) || observe::enabled(&self.inner, c));
            if let Some(sync) = self.inner.api.x70.write_hook_pc_sync {
                for engine in self.inner.engines {
                    unsafe { sync(engine, every_store) };
                }
            }
            self.inner.set_store_watch(!every_store).map_err(error)?;
            self.inner.set_filter(core, fast && !every_store).map_err(error)?;
            unsafe { (*self.inner.state.get()).restart_pc = [None; 2] };
            let runner = Runner {
                inner: &*self.inner,
                core,
                begin,
                until,
                timeout,
                count: if fast { 0 } else { count },
            };
            self.inner.redirects.set([None; 2]);
            let instruction_start=unsafe { (&*self.inner.state.get()).instructions };
            let status = py.detach(move || runner.run());
            // A filtered block ends early only through a fault, whose
            // instruction was entered; otherwise the run stopped between blocks.
            if unsafe { (*self.inner.state.get()).acct_on[core] } {
                let tail = if status != 0 {
                    let pc = self.inner.read_reg(core, ffi::PC).map_err(error)? as u64;
                    hooks::acct_tail(&self.inner, core, pc, true)
                } else {
                    0
                };
                self.inner.acct_settle(core, tail, true).map_err(error)?;
                self.inner.acct_reset(core);
            } else if status != 0 && unsafe { (*self.inner.state.get()).bulk[core].filtered } {
                let pc = self.inner.read_reg(core, ffi::PC).map_err(error)? as u64;
                unsafe { (*self.inner.state.get()).settle_at(core, pc, true) };
            }
            let state = unsafe { &mut *self.inner.state.get() };
            state.settle(core);
            let pending=unsafe { (&mut *self.inner.tcm.get()).pending.take() };
            if let Some((owner,bank,reg))=pending {
                let executed=state.instructions-instruction_start;
                if status!=0 || self.inner.callback_error.borrow().is_some() || self.inner.native_error.borrow().is_some() { break status; }
                self.inner.executing.set(None);
                let changed=tcm::set(&self.inner,owner,bank,reg);
                self.inner.executing.set(Some(core));
                if let Err(message)=changed { *self.inner.native_error.borrow_mut()=Some(message);break 0; }
                let pc=self.inner.redirects.get()[core].unwrap();
                self.inner.redirects.set([None;2]);
                if let Err(message)=self.inner.write_reg(core,ffi::PC,pc) { *self.inner.native_error.borrow_mut()=Some(message);break 0; }
                begin=pc as u64;
                if count!=0 { if executed>=count as u64 { break 0; } count-=executed as usize; }
                continue;
            }
            if !state.downgrade {
                break status;
            }
            // A Thumb block was entered in fast mode and has not executed.
            // Select the exact mode for good and resume that block in it.
            state.downgrade = false;
            let executed = state.entered;
            if status != 0
                || self.inner.callback_error.borrow().is_some()
                || self.inner.native_error.borrow().is_some()
            {
                break status;
            }
            state.set_fast(false);
            state.downgrades += 1;
            let mut resume = || -> Result<(), String> {
                for engine in self.inner.engines {
                    self.inner
                        .check(unsafe { (self.inner.api.uc_ctl)(engine, ffi::ctl(10, 0, 1)) })?;
                }
                let pc = self.inner.read_reg(core, ffi::PC)?;
                let thumb = self.inner.read_reg(core, ffi::CPSR)? & 32 != 0;
                begin = pc as u64 | thumb as u64;
                Ok(())
            };
            if let Err(message) = resume() {
                *self.inner.native_error.borrow_mut() = Some(message);
                break 0;
            }
            if count != 0 {
                if executed >= count as u64 {
                    break 0;
                }
                count -= executed as usize;
            }
        };
        self.inner.executing.set(None);
        if let Some(pc) = self.inner.redirects.get()[core] {
            self.inner.redirects.set([None; 2]);
            self.inner.write_reg(core, ffi::PC, pc).map_err(error)?;
        }
        if let Some(error) = self.inner.callback_error.borrow_mut().take() {
            return Err(error);
        }
        if let Some(message) = self.inner.native_error.borrow_mut().take() {
            return Err(error(message));
        }
        Ok(status)
    }
    /// Select the execution mode while paused: "instructions" (default; every
    /// instruction through Unicorn's count hook) or "fast" (one inline
    /// instruction hook; limits are enforced natively). Both modes are
    /// bit-exact for all state Python or devices can observe.
    fn set_execution(&self, mode: &str) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        let fast = match mode {
            "instructions" => false,
            "fast" => true,
            _ => {
                return Err(PyValueError::new_err(
                    "Execution mode must be 'instructions' or 'fast'",
                ));
            }
        };
        let state = unsafe { &mut *self.inner.state.get() };
        if state.fast != fast {
            state.set_fast(fast);
            state.downgrade = false;
            // Translated code embeds the hook arrangement it was built under.
            for engine in self.inner.engines {
                self.inner
                    .check(unsafe { (self.inner.api.uc_ctl)(engine, ffi::ctl(10, 0, 1)) })
                    .map_err(error)?;
            }
        }
        Ok(())
    }
    fn execution(&self) -> &'static str {
        if unsafe { (*self.inner.state.get()).fast } {
            "fast"
        } else {
            "instructions"
        }
    }
    fn emu_stop(&self, core: usize) -> PyResult<()> {
        self.inner.core(core).map_err(error)?;
        self.inner.stop(core);
        Ok(())
    }
    fn context_save(slf: PyRef<'_, Self>, core: usize) -> PyResult<CpuContext> {
        slf.inner.paused().map_err(error)?;
        let engine = slf.inner.core(core).map_err(error)?;
        let mut ptr = std::ptr::null_mut();
        slf.inner
            .check(unsafe { (slf.inner.api.uc_context_alloc)(engine, &mut ptr) })
            .map_err(error)?;
        let ctx = CpuContext {
            api: Rc::clone(&slf.inner.api),
            ptr,
            engine,
            tcm: unsafe { (&*slf.inner.tcm.get()).snapshot(core) },
            owner: slf.into(),
        };
        ctx.api_check(unsafe { (ctx.api.uc_context_save)(engine, ptr) })?;
        Ok(ctx)
    }
    fn context_restore(&self, core: usize, context: PyRef<'_, CpuContext>) -> PyResult<()> {
        self.inner.paused().map_err(error)?;
        let engine = self.inner.core(core).map_err(error)?;
        if engine != context.engine {
            return Err(error(
                "CPU context belongs to a different engine; cross-engine restore refused".into(),
            ));
        }
        self.inner
            .check(unsafe { (self.inner.api.uc_context_restore)(engine, context.ptr) })
            .map_err(error)?;
        tcm::restore(&self.inner,core,&context.tcm).map_err(error)
    }
}

#[pyclass(unsendable, module = "x70_native._native")]
struct CpuContext {
    api: Rc<ffi::Api>,
    ptr: ffi::Context,
    engine: ffi::Engine,
    tcm: tcm::Snapshot,
    #[allow(dead_code)]
    owner: Py<Machine>,
}
impl CpuContext {
    fn api_check(&self, status: i32) -> PyResult<()> {
        if status == 0 {
            Ok(())
        } else {
            Err(error(format!("Unicorn context error {status}")))
        }
    }
}
impl Drop for CpuContext {
    fn drop(&mut self) {
        unsafe {
            (self.api.uc_context_free)(self.ptr);
        }
    }
}

#[pymethods]
impl CpuContext {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.owner)
    }
}

impl Machine {
    /// One core's recent trace as `[(hex pc, hex bytes), ...]`, oldest first.
    fn ring_list<'py>(&self, py: Python<'py>, ring: &state::TraceRing) -> PyResult<Bound<'py, PyList>> {
        let mut cache = self.trace_text.borrow_mut();
        if cache.len() > TRACE_TEXT_LIMIT {
            cache.clear();
        }
        let ring_list = PyList::empty(py);
        for e in ring.iter() {
            let size = e.size as usize;
            let mut bytes = [0u8; 4];
            bytes[..size].copy_from_slice(&e.raw[..size]);
            let key = (e.address, u32::from_le_bytes(bytes), e.size);
            let text = match cache.get(&key) {
                Some(text) => text.bind(py).clone(),
                None => {
                    let mut hex = String::with_capacity(2 * size);
                    for b in &bytes[..size] {
                        hex.push_str(&format!("{b:02x}"));
                    }
                    let text = PyTuple::new(py, [format!("{:#x}", e.address), hex])?;
                    cache.insert(key, text.clone().unbind());
                    text
                }
            };
            ring_list.append(text)?;
        }
        Ok(ring_list)
    }
}

#[pymodule(gil_used = true)]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Machine>()?;
    m.add_class::<CpuContext>()?;
    m.add_class::<raster::DeclaredUIConversionPlan>()?;
    m.add("NativeError", m.py().get_type::<NativeError>())?;
    m.add("UNICORN_ABI", "2.1.4")?;
    m.add("VERSION", env!("CARGO_PKG_VERSION"))?;
    m.add("RUSTC_VERSION", env!("X70_RUSTC_VERSION"))?;
    m.add("TARGET", env!("X70_TARGET"))?;
    m.add("PYO3_VERSION", "0.29.3")?;
    m.add("VIRTUAL_SCENE_PROFILE", scene::PROFILE)?;
    m.add_function(wrap_pyfunction!(scene::virtual_scene_yycbcr, m)?)?;
    m.add_function(wrap_pyfunction!(scene::virtual_scene_histogram, m)?)?;
    m.add_function(wrap_pyfunction!(scene::virtual_scene_fixed_frame, m)?)?;
    m.add("XM1_STATISTICS_PROFILE", scene::XM1_STATISTICS_PROFILE)?;
    m.add_function(wrap_pyfunction!(scene::virtual_scene_xm1_statistics, m)?)?;
    m.add_function(wrap_pyfunction!(fuji::fuji_lz_feed, m)?)?;
    m.add_function(wrap_pyfunction!(raster::ui_fill_counts, m)?)?;
    m.add_function(wrap_pyfunction!(raster::ui_composite, m)?)?;
    m.add_function(wrap_pyfunction!(raster::ui_composite_tiled, m)?)?;
    m.add_function(wrap_pyfunction!(raster::ui_stream_parse, m)?)?;
    m.add_function(wrap_pyfunction!(gradient::ui_gradient, m)?)?;
    m.add_function(wrap_pyfunction!(gradient::ui_gradient_colors, m)?)?;
    m.add_function(wrap_pyfunction!(scene::xm1_source_tick, m)?)?;
    m.add_function(wrap_pyfunction!(phase_clock::tick_xm1_clock, m)?)?;
    m.add_class::<fff63::Device>()?;
    m.add("FFF63_PROFILES", (fff63::PROFILE,))?;
    m.add_function(wrap_pyfunction!(spi0::spi0_status_acknowledged, m)?)?;
    Ok(())
}
