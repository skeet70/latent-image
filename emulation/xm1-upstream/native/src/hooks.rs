//! Direct native callbacks: ordinary instruction/store hooks never call Python.
use crate::audit;
use crate::engine::{HookData, Inner};
use crate::fetch_guard;
use crate::ffi::{self, Engine};
use crate::observe;
use crate::access_probe;
use crate::state::{Reservation, Trace, condition_passes};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::ffi::{c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn guarded(data: *mut c_void, body: impl FnOnce(&Inner, usize) -> Result<(), String>) {
    let data = unsafe { &*data.cast::<HookData>() };
    let inner = unsafe { &*data.inner };
    let failure = match catch_unwind(AssertUnwindSafe(|| body(inner, data.core))) {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error),
        Err(_) => Some("Rust hook panic; execution stopped at C boundary".into()),
    };
    if let Some(message) = failure {
        if audit::enabled(inner, data.core) {
            audit::fail(inner, data.core, message);
        } else {
            inner.fail_native(data.core, message);
        }
    }
}
pub unsafe extern "C" fn code(_cpu: Engine, address: u64, size: u32, data: *mut c_void) {
    // Bulk block accounting (see `block`): an instruction of a plain block
    // only counts down. Pending code invalidation or remap still stops at
    // this instruction through the full path, which settles first.
    {
        let hook = unsafe { &*data.cast::<HookData>() };
        let state = unsafe { &mut *(*hook.inner).state.get() };
        if state.acct_on[hook.core] {
            // A filtered block's only hooked instruction is its last (a
            // special one): the instructions before it have run; it is
            // counted by the instruction path, so its budget stays spent.
            let inner = unsafe { &*hook.inner };
            let tail = acct_tail(inner, hook.core, address, false);
            if let Err(message) = inner.acct_settle(hook.core, tail, false) {
                inner.fail_native(hook.core, message);
                return;
            }
        } else if state.bulk[hook.core].filtered {
            // A filtered block's only hooked instruction is its last (a
            // special one): the instructions before it have executed.
            state.settle_at(hook.core, address, false);
        } else if state.bulk[hook.core].remaining != 0
            && !state.remap_requested
            && state.pending[0].is_empty()
            && state.pending[1].is_empty()
        {
            state.bulk[hook.core].remaining -= 1;
            return;
        }
    }
    guarded(data, |inner, core| instruction(inner, core, address, size));
}
/// Translation filter (patched Unicorn, ARM state): 0 = no code hook for an
/// instruction `block` can count in bulk, 2 = hook it and end its block after
/// it. Decided from the instruction word being translated, exactly as
/// `block_info` classifies plain blocks.
pub unsafe extern "C" fn filter(data: *mut c_void, address: u64, word: u32) -> c_int {
    let hook = unsafe { &*data.cast::<HookData>() };
    let inner = unsafe { &*hook.inner };
    let state = unsafe { &mut *inner.state.get() };
    let decoded = crate::state::classify(4, word.to_le_bytes());
    let plain = (decoded.ordinary || passive_coprocessor(word))
        && !state.entries.contains(&address)
        && !decoded.unmodeled_atomic
        && !decoded.unmodeled_pmu;
    // With budgeted accounting (features bit 2) a fast block may run without
    // its block hook: translated bytes are code from now on (store
    // invalidation), and a page newly watched needs the block hook (4), which
    // declares it before the block runs.
    let mut hook_every_entry = 0;
    if inner.api.x70.features & 2 != 0 {
        let queued = state.watch_queue.len();
        state.note_code(hook.core, address, 4, inner.ram.len() as u64);
        if state.watch_queue.len() != queued {
            hook_every_entry = 4;
        }
    }
    (if plain { 0 } else { 2 }) | hook_every_entry
}
/// Patched Unicorn restarts the instruction at `pc` as a block of its own (a
/// device access or a store over translated code): the instructions before
/// it in the current block have executed, it has not.
pub unsafe extern "C" fn restart(data: *mut c_void, pc: u64) {
    let hook = unsafe { &*data.cast::<HookData>() };
    let inner = unsafe { &*hook.inner };
    let state = unsafe { &mut *inner.state.get() };
    state.restarts += 1;
    if state.acct_on[hook.core] {
        let tail = acct_tail(inner, hook.core, pc, false);
        if let Err(message) = inner.acct_settle(hook.core, tail, true) {
            inner.fail_native(hook.core, message);
        }
        inner.acct_reset(hook.core);
    } else {
        state.settle_at(hook.core, pc, false);
    }
    let state = unsafe { &mut *inner.state.get() };
    state.restart_pc[hook.core] = Some(pc);
}
pub unsafe extern "C" fn block(_cpu: Engine, address: u64, size: u32, data: *mut c_void) {
    guarded(data, |inner, core| {
        // CPU state is constant within a translated block. One read per block
        // detects Thumb32 too, without a CPSR read for every ARM instruction;
        // with the translation filter installed, bit 31 of the size carries it.
        let filtered = unsafe { (*inner.state.get()).filtered[core] } && inner.api.x70.features & 1 != 0;
        let (size, thumb) = if filtered {
            (size & 0x3fff_ffff, size & 0x8000_0000 != 0)
        } else {
            (size, inner.read_reg(core, ffi::CPSR)? & 32 != 0)
        };
        inner.acct_settle(core, 0, true)?;
        let state = unsafe { &mut *inner.state.get() };
        state.blocks_entered += 1;
        state.block_thumb[core] = thumb;
        if thumb && state.fast {
            // Unicorn runs Thumb IT-block instructions through its helper
            // path, which ignores stops requested at those instructions. Only
            // the default mode reproduces that, so stop before this block
            // executes; `emu_start` resumes it in default mode.
            state.downgrade = true;
            inner.stop(core);
            return Ok(());
        }
        if !thumb && state.filtered[core] {
            return filtered_block(inner, core, address, size);
        }
        // A block is noted once per translation: `block_info` notes it when
        // it decodes the block, otherwise every entry notes it.
        if !(!thumb && state.fast && start_bulk(inner, core, address, size)?) {
            unsafe { (*inner.state.get()).note_code(core, address, size as u64, inner.ram.len() as u64) };
        }
        inner.apply_watches()
    });
}

/// A block translated under the filter: ordinary instructions have no hook,
/// and only the last may be hooked (a special instruction). It runs whole
/// within the run's entry limit and tick deadline, or is retranslated to end
/// exactly where the per-instruction checks would stop, or stops here.
fn filtered_block(inner: &Inner, core: usize, address: u64, size: u32) -> Result<(), String> {
    let state = unsafe { &mut *inner.state.get() };
    if size == 0 || size % 4 != 0 {
        return Err(format!("Filtered ARM block at {address:#x} has size {size}"));
    }
    let n = (size / 4) as u64;
    // Room before the next per-instruction check, ignoring a pending stop
    // for the restarted instruction alone (it caused it).
    let restarted = state.restart_pc[core] == Some(address);
    if restarted && n == 1 {
        state.restart_pc[core] = None;
    }
    let pending_stop = state.remap_requested || state.pending.iter().any(|p| !p.is_empty());
    let mut room = if restarted && pending_stop {
        let (remap, pending) = (state.remap_requested, std::mem::take(&mut state.pending));
        state.remap_requested = false;
        let room = state.room();
        state.remap_requested = remap;
        state.pending = pending;
        room
    } else {
        state.room()
    };
    if restarted {
        room = room.min(1);
    }
    let allowed = n.min(room);
    if allowed == 0 {
        inner.acct_reset(core);
        inner.stop(core);
        return Ok(());
    }
    if allowed < n {
        inner.acct_reset(core);
        state.retried_blocks += 1;
        let retry = inner.api.x70.retry_block.ok_or("Filtered execution without uc_x70_retry_block")?;
        inner.check(unsafe { retry(inner.engines[core], allowed as u32) })?;
        return Ok(());
    }
    let info = block_info(inner, core, address, size)?;
    let state = unsafe { &mut *inner.state.get() };
    state.special_blocks[info.reason as usize] += 1;
    state.bulk_blocks += 1;
    if state.acct_on[core] {
        inner.acct_admit(core, address, n, room);
    } else {
        state.bulk[core] = crate::state::Bulk {
            n,
            remaining: 0,
            entries: Some(info.entries),
            filtered: true,
            start: address,
        };
    }
    inner.apply_watches()
}

/// Count a plain ARM block per block instead of per instruction when that is
/// exactly equivalent: no optional per-instruction feature is enabled, the
/// block holds only ordinary, non-milestone instructions, and no instruction
/// in it can reach the run's entry limit or the tick deadline. True when the
/// block went through `block_info`.
fn start_bulk(inner: &Inner, core: usize, address: u64, size: u32) -> Result<bool, String> {
    if size == 0 || size % 4 != 0 {
        return Ok(false);
    }
    unsafe {
        if !(*inner.breakpoints.get())[core].is_empty()
            || (*inner.coverage.get())[core].is_some()
            || (*inner.exec_probe.get())[core].is_some()
            || (*inner.fetch_guards.get())[core].is_some()
            || (*inner.audits.get())[core].is_some()
            || access_probe::enabled(inner)
        {
            return Ok(false);
        }
    }
    let n = (size / 4) as u64;
    {
        let state = unsafe { &*inner.state.get() };
        if !state.cache_enabled
            || state.remap_requested
            || state.pending.iter().any(|p| !p.is_empty())
            || (state.entry_limit != 0 && state.entered + n > state.entry_limit)
            || (state.tick_deadline != u64::MAX
                && state.ticks + (state.tick_phase + (n - 1) * state.tick_den) / state.tick_num
                    >= state.tick_deadline)
        {
            return Ok(false);
        }
    }
    let info = block_info(inner, core, address, size)?;
    unsafe { (*inner.state.get()).special_blocks[info.reason as usize] += 1 };
    if info.plain {
        let state = unsafe { &mut *inner.state.get() };
        state.bulk_blocks += 1;
        state.bulk[core] = crate::state::Bulk {
            n,
            remaining: n,
            entries: Some(info.entries),
            ..Default::default()
        };
    }
    Ok(true)
}

/// A block's decoded instructions (cached with `block_info`).
pub fn block_entries(inner: &Inner, core: usize, address: u64, size: u32) -> Result<std::rc::Rc<[Trace]>, String> {
    Ok(block_info(inner, core, address, size)?.entries)
}
fn block_info(
    inner: &Inner,
    core: usize,
    address: u64,
    size: u32,
) -> Result<crate::state::BlockInfo, String> {
    {
        let state = unsafe { &mut *inner.state.get() };
        let slot = crate::state::block_front_index(address);
        if let Some((cached, info)) = &state.block_front[core][slot]
            && *cached == address
            && info.size == size
        {
            return Ok(info.clone());
        }
        if let Some(info) = state.blocks[core].get(&address)
            && info.size == size
        {
            let info = info.clone();
            state.block_front[core][slot] = Some((address, info.clone()));
            return Ok(info);
        }
    }
    let mut entries = Vec::with_capacity(size as usize / 4);
    let mut plain = true;
    let mut reason = 0u8;
    for pc in (address..address + size as u64).step_by(4) {
        let decoded = inner.instruction(core, pc, 4)?;
        let ok = (decoded.ordinary || passive_coprocessor(decoded.word))
            && !decoded.milestone
            && !decoded.unmodeled_atomic
            && !decoded.unmodeled_pmu;
        if !ok && reason == 0 {
            reason = special_kind(&decoded);
        }
        plain &= ok;
        entries.push(Trace {
            address: pc,
            raw: decoded.raw,
            size: decoded.size,
        });
    }
    let state = unsafe { &mut *inner.state.get() };
    let info = crate::state::BlockInfo {
        size,
        reason,
        plain,
        entries: entries.into(),
    };
    // Decodes in the uncached vector range are re-read on every entry;
    // blocks there are not cached either.
    let end = address + size as u64;
    state.note_code(core, address, size as u64, inner.ram.len() as u64);
    if !(address < 0xffff0000 && end > 0xfff01000) {
        state.blocks[core].insert(address, info.clone());
        state.block_front[core][crate::state::block_front_index(address)] = Some((address, info.clone()));
        for page in address >> 12..=(end - 1) >> 12 {
            state.block_pages[core].entry(page).or_default().push(address);
        }
    }
    Ok(info)
}
/// Instructions of `core`'s newest accounted block after the one at `pc`
/// (`entered`: it ran) or from it on (it did not); 0 when `pc` is outside it.
pub fn acct_tail(inner: &Inner, core: usize, pc: u64, entered: bool) -> u64 {
    match inner.acct_top(core) {
        Some((start, count)) if pc >= start && pc < start + 4 * count => {
            (start + 4 * count - pc) / 4 - entered as u64
        }
        _ => 0,
    }
}
/// Settle a filtered block at the instruction executing now (PC synced by
/// Unicorn for exceptions and memory hooks): it has been entered.
fn settle_here(inner: &Inner, core: usize) -> Result<(), String> {
    if unsafe { (*inner.state.get()).acct_on[core] } {
        let pc = inner.read_reg(core, ffi::PC)? as u64;
        let tail = acct_tail(inner, core, pc, true);
        return inner.acct_settle(core, tail, true);
    }
    if unsafe { (*inner.state.get()).bulk[core].filtered } {
        let pc = inner.read_reg(core, ffi::PC)? as u64;
        unsafe { (*inner.state.get()).settle_at(core, pc, true) };
    }
    Ok(())
}
pub unsafe extern "C" fn interrupt(_cpu: Engine, number: u32, data: *mut c_void) {
    guarded(data, |inner, core| {
        settle_here(inner, core)?;
        if audit::enabled(inner, core) {
            let pc = inner.read_reg(core, ffi::PC)? as u64;
            audit::fail(
                inner,
                core,
                format!("Unmodeled CPU interrupt {number} at {pc:#x}"),
            );
            return Ok(());
        }
        let pc = inner.read_reg(core, ffi::PC)? as u64;
        inner.event(&format!("interrupt:{number}"), core, pc, 0, 0);
        Ok(())
    });
}
pub unsafe extern "C" fn store(
    _cpu: Engine,
    _kind: i32,
    address: u64,
    size: i32,
    value: i64,
    data: *mut c_void,
) {
    guarded(data, |inner, core| {
        if !audit::write(inner, core, address, size, value)? {
            return Ok(());
        }
        if observe::enabled(inner, core) {
            let pc = inner.read_reg(core, ffi::PC)? as u64;
            observe::record(inner, core, address, size, value, pc, false);
        }
        let state = unsafe { &mut *inner.state.get() };
        state.store_hooks += 1;
        let generation = state.code_generation;
        state.invalidate_code(address, size as u64, core, inner.ram.len() as u64);
        if state.code_generation != generation {
            inner.acct_freeze()?;
        }
        let state = unsafe { &mut *inner.state.get() };
        state.invalidate_exclusive(address, size as u64, core, inner.ram.len() as u64);
        // This store overwrote translated code: redo it as a block of its own
        // so the stop for the pending invalidation lands right after it.
        if state.filtered[core] && state.pending.iter().any(|p| !p.is_empty()) {
            if let Some(request) = inner.api.x70.request_restart {
                inner.check(unsafe { request(inner.engines[core]) })?;
            }
        }
        Ok(())
    });
}
pub unsafe extern "C" fn audit_read(
    _cpu: Engine,
    _kind: i32,
    address: u64,
    size: i32,
    _value: i64,
    data: *mut c_void,
) {
    guarded(data, |inner, core| {
        audit::read(inner, core, address, size);
        Ok(())
    });
}
pub unsafe extern "C" fn access_probe_memory(
    _cpu: Engine, kind: i32, address: u64, size: i32, _value: i64, data: *mut c_void,
) {
    guarded(data, |inner, core| {
        access_probe::record(inner, core, address, size.max(0) as u64,
            if kind == 16 { "cpu-read" } else { "cpu-write" }, None)
    });
}
pub unsafe extern "C" fn audit_write_guard(
    _cpu: Engine,
    _kind: i32,
    address: u64,
    size: i32,
    value: i64,
    data: *mut c_void,
) {
    guarded(data, |inner, core| {
        if !audit::ordinary_store_address(address, inner.ram.len() as u64) {
            audit::write(inner, core, address, size, value)?;
        }
        Ok(())
    });
}
pub unsafe extern "C" fn mmio(
    _cpu: Engine,
    kind: i32,
    address: u64,
    size: i32,
    value: i64,
    data: *mut c_void,
) {
    guarded(data, |inner, core| {
        if crate::tcm::locate(inner,core,address,size.max(0) as usize).is_some() { return Ok(()); }
        settle_here(inner, core)?;
        if audit::enabled(inner, core) {
            if kind == 17 {
                audit::write(inner, core, address, size, value)?;
            } else {
                audit::read(inner, core, address, size);
            }
            return Ok(());
        }
        if size == 4 && (kind == 16 || kind == 17) && !access_probe::enabled(inner) && plain(inner, core, kind, address, value)? {
            return Ok(());
        }
        // A repeatable read (declared by Python since it last ran) still
        // has its value in the engine's MMIO page: no device can have changed.
        if kind == 16 {
            let state = unsafe { &mut *inner.state.get() };
            if state.mmio_read_cache[core].contains(&(address, size as u32)) {
                state.cached_mmio_reads += 1;
                return Ok(());
            }
        }
        // The two access kinds Unicorn reports here need no formatting.
        let name = match kind {
            16 => "mmio:16",
            17 => "mmio:17",
            _ => &format!("mmio:{kind}"),
        };
        inner.event(name, core, address, size as u32, value);
        Ok(())
    });
}
/// Serve a declared device register (`Machine.set_plain_registers`) from its
/// device's Python `registers` dict, exactly as the device's read/write would:
/// no Python code runs. False when the access is not declared or a stored
/// value is not a plain integer, so the ordinary event handles it.
fn plain(inner: &Inner, core: usize, kind: i32, address: u64, value: i64) -> Result<bool, String> {
    let handled = Python::attach(|py| -> PyResult<Option<Option<u32>>> {
        let plain = inner.plain.borrow();
        let Some(register) = plain.get(&address) else { return Ok(None) };
        let device = register.device.bind(py);
        let stored = |registers: &Bound<'_, PyDict>| -> PyResult<Option<u32>> {
            Ok(match registers.get_item(address)? {
                None => Some(0),
                Some(stored) => stored.extract::<u64>().ok().map(|v| v as u32),
            })
        };
        if kind == 16 {
            match &register.read {
                None => return Ok(None),
                Some(crate::engine::PlainRead::Direct) => {
                    let word = device
                        .call_method1(pyo3::intern!(py, "read"), (core, address, 4))?
                        .extract::<u64>()?;
                    return Ok(Some(Some(word as u32)));
                }
                Some(crate::engine::PlainRead::Timed) => {
                    // The tick count the event path would pass (Inner::event).
                    inner.acct_settle(core, 0, true).map_err(pyo3::exceptions::PyRuntimeError::new_err)?;
                    let ticks = unsafe { (*inner.state.get()).ticks };
                    let word = device.call_method1(pyo3::intern!(py, "native_read"), (address, ticks))?;
                    if word.is_none() {
                        return Ok(None);
                    }
                    return Ok(Some(Some(word.extract::<u64>()? as u32)));
                }
                Some(crate::engine::PlainRead::Formula(clear, set, flag)) => {
                    let registers = device.getattr(pyo3::intern!(py, "registers"))?;
                    let Some(word) = stored(registers.cast::<PyDict>()?)? else { return Ok(None) };
                    let on = match flag {
                        Some(name) => device.getattr(name.bind(py))?.is_truthy()?,
                        None => true,
                    };
                    return Ok(Some(Some(word & !clear | if on { *set } else { 0 })));
                }
            }
        }
        let Some(keep) = register.write else { return Ok(None) };
        let registers = device.getattr(pyo3::intern!(py, "registers"))?;
        let registers = registers.cast::<PyDict>()?;
        let value = value as u64 as u32;
        let new = if keep == u32::MAX {
            value
        } else {
            let Some(old) = stored(registers)? else { return Ok(None) };
            value & keep | old & !keep & !value
        };
        registers.set_item(address, new as u64)?;
        Ok(Some(None))
    });
    let read = match handled {
        Ok(Some(read)) => read,
        Ok(None) | Err(_) => return Ok(false),
    };
    let state = unsafe { &mut *inner.state.get() };
    state.plain_accesses += 1;
    match read {
        Some(word) => inner.write_cpu(core, address, &word.to_le_bytes())?,
        // A device's repeatable reads may depend on its registers.
        None => state.mmio_read_cache.iter_mut().for_each(|cache| cache.clear()),
    }
    Ok(true)
}
pub unsafe extern "C" fn invalid(
    _cpu: Engine,
    kind: i32,
    address: u64,
    size: i32,
    value: i64,
    data: *mut c_void,
) -> bool {
    guarded(data, |inner, core| {
        settle_here(inner, core)?;
        if fetch_guard::invalid_fetch(inner, core, kind, address, size)? {
            return Ok(());
        }
        if audit::enabled(inner, core) {
            audit::fail(
                inner,
                core,
                format!("Unmapped/protected access {kind} at {address:#x}"),
            );
            return Ok(());
        }
        inner.event(
            &format!("invalid:{kind}"),
            core,
            address,
            size as u32,
            value,
        );
        Ok(())
    });
    false
}

fn instruction(inner: &Inner, core: usize, address: u64, size: u32) -> Result<(), String> {
    // Leaving bulk accounting mid-block (pending invalidation or remap):
    // apply the entered instructions first, then handle this one fully.
    unsafe { (*inner.state.get()).settle(core) };
    // Fast execution has no Unicorn count hook: enforce the run's instruction
    // limit exactly as that hook does. It counts every hook invocation and
    // stops, before anything else observes the instruction, once the count
    // exceeds the limit.
    {
        let state = unsafe { &mut *inner.state.get() };
        if state.fast && state.entry_limit != 0 {
            state.entered += 1;
            if state.entered > state.entry_limit {
                inner.stop(core);
                return Ok(());
            }
        }
    }
    // A device completion is due: yield before anything observes this entry,
    // so the scheduler delivers it at exactly its tick (u64::MAX when unused)
    // and the re-entered instruction is observed once, as at quantum 1.
    if unsafe { (*inner.state.get()).ticks >= (*inner.state.get()).tick_deadline } {
        inner.stop(core);
        return Ok(());
    }
    if !fetch_guard::code(inner, core, address, size)? {
        return Ok(());
    }
    if !audit::code(inner, core, address, size)? {
        return Ok(());
    }
    {
        let state = unsafe { &mut *inner.state.get() };
        state.code_hooks += 1;
        if state.remap_requested || state.pending.iter().any(|p| !p.is_empty()) {
            inner.stop(core);
            return Ok(());
        }
    }
    // Explicit breakpoints report on every accepted entry, before the
    access_probe::fetch(inner, core, address, size as u64)?;
    // Architected fetch above is observed before any diagnostic breakpoint.
    // Explicit breakpoints report on every accepted entry, before the
    // instruction is counted or executed. The handler may rewrite registers;
    // if it moves PC, this instruction must not run and is not counted.
    if !unsafe { &(*inner.breakpoints.get())[core] }.is_empty()
        && unsafe { &(*inner.breakpoints.get())[core] }
            .binary_search(&address)
            .is_ok()
    {
        inner.event("breakpoint", core, address, size, 0);
        let mut halts = inner.breakpoint_halt.get();
        if halts[core] {
            // The handler ended the call: this instruction neither executes
            // nor counts (a stop, not a skipped or substituted instruction).
            halts[core] = false;
            inner.breakpoint_halt.set(halts);
            inner.stop(core);
            return Ok(());
        }
        let pc = inner.read_reg(core, ffi::PC)?;
        if pc as u64 != address {
            let mut redirects = inner.redirects.get();
            redirects[core] = Some(pc);
            inner.redirects.set(redirects);
            inner.stop(core);
            return Ok(());
        }
    }
    {
        let state = unsafe { &mut *inner.state.get() };
        state.instructions += 1;
        state.precise_instructions += 1;
        // Ratio num/den >= 1 and phase < num: at most one tick per
        // instruction, so subtraction replaces the division.
        state.tick_phase += state.tick_den;
        while state.tick_phase >= state.tick_num {
            state.ticks += 1;
            state.tick_phase -= state.tick_num;
        }
        state.cores[core].instructions += 1;
        if let Some(executed) = unsafe { &mut (*inner.coverage.get())[core] } {
            executed.insert(address);
        }
        if let Some(probe) = unsafe { &mut (*inner.exec_probe.get())[core] } {
            probe.record(address);
        }
    }
    let decoded = inner.instruction(core, address, size)?;
    let thumb = unsafe { (*inner.state.get()).block_thumb[core] };
    if thumb {
        unsafe {
            (*inner.state.get()).portable_unsafe(
                core,
                "Thumb execution is outside portable ARM state",
                Some(address),
                Some(decoded.word),
            );
        }
    } else if decoded.unmodeled_atomic || decoded.unmodeled_pmu {
        let condition = decoded.word >> 28;
        if condition == 14
            || (condition == 15 && decoded.unmodeled_pmu)
            || condition_passes(condition, inner.read_reg(core, ffi::CPSR)?)
        {
            let reason = if decoded.unmodeled_atomic {
                "Nonword ARM exclusives use unexported Unicorn monitor state"
            } else {
                "CP15 PMU transfer is outside the Rust PMU model"
            };
            unsafe {
                (*inner.state.get()).portable_unsafe(
                    core,
                    reason,
                    Some(address),
                    Some(decoded.word),
                );
            }
        }
    }
    if audit::enabled(inner, core) && !decoded.ordinary {
        audit::fail(
            inner,
            core,
            format!("Unmodeled native instruction boundary at {address:#x}"),
        );
        return Ok(());
    }
    let milestone = {
        let state = unsafe { &mut *inner.state.get() };
        state.trace[core].push(Trace {
            address,
            raw: decoded.raw,
            size: decoded.size,
        });
        decoded.milestone && state.seen_entries.insert((core, address))
    };
    if milestone && !audit::enabled(inner, core) {
        inner.event("milestone", core, address, size, 0);
    }
    if decoded.ordinary || size != 4 {
        return Ok(());
    }
    let word = decoded.word;
    match word {
        0xeafffffe => {
            inner.event("halt", core, address, size, 0);
            return Ok(());
        }
        0xe320f003 => {
            inner.write_reg(core, ffi::PC, (address as u32).wrapping_add(4))?;
            unsafe {
                let state = &mut *inner.state.get();
                state.cores[core].waiting = true;
                state.touch();
            }
            inner.event("wfi", core, address, size, 0);
            inner.stop(core);
            return Ok(());
        }
        0xf57ff01f => {
            unsafe {
                let state = &mut *inner.state.get();
                state.reservations[core] = None;
                state.touch();
            }
            return Ok(());
        }
        _ => (),
    }
    let atomic = word & 0x0ff00fff == 0x01900f9f || word & 0x0ff00ff0 == 0x01800f90;
    if !atomic && word & 0x0f000010 != 0x0e000010 {
        return Ok(());
    }
    let condition = word >> 28;
    if condition != 14 && !condition_passes(condition, inner.read_reg(core, ffi::CPSR)?) {
        return Ok(());
    }
    let target = ((word >> 12) & 15) as usize;
    if atomic {
        let base = inner.read_reg(core, ffi::ARM_REGS[((word >> 16) & 15) as usize])? as u64;
        let canonical = unsafe { (&*inner.state.get()).canonical_address(base, core, inner.ram.len() as u64) };
        if canonical.is_none() || base & 3 != 0 {
            inner.event("exclusive_fault", core, address, size, base as i64);
            return Ok(());
        }
        let canonical = canonical.unwrap();
        if word & 0x0ff00fff == 0x01900f9f {
            access_probe::record(inner, core, base, 4, "cpu-exclusive-read", None)?;
            let value = inner.read_cpu(core, base, 4)?;
            inner.write_reg(
                core,
                ffi::ARM_REGS[target],
                u32::from_le_bytes(value.try_into().unwrap()),
            )?;
            let state = unsafe { &mut *inner.state.get() };
            state.reservations[core] = Some(Reservation {
                address: canonical,
                valid: true,
            });
            state.exclusive.loads += 1;
            state.touch();
            // Stores from either core must reach the reservation.
            state.watch_page(core, base & !0xfff, inner.ram.len() as u64);
            inner.apply_watches()?;
        } else {
            let success = {
                let state = unsafe { &mut *inner.state.get() };
                let monitor = state.reservations[core].take();
                state.touch();
                monitor.is_some_and(|m| m.valid && m.address == canonical)
            };
            if success {
                access_probe::record(inner, core, base, 4, "cpu-exclusive-write", None)?;
                let value = inner.read_reg(core, ffi::ARM_REGS[(word & 15) as usize])?;
                if crate::tcm::locate(inner,core,base,4).is_some() {
                    inner.write_cpu(core,base,&value.to_le_bytes())?;
                    unsafe {
                        (&mut *inner.state.get()).invalidate_exclusive(base,4,core,inner.ram.len() as u64);
                        (&mut *inner.state.get()).invalidate_code(base,4,core,inner.ram.len() as u64);
                    }
                } else { inner.write_bus(core, base, &value.to_le_bytes())?; }
                if observe::enabled(inner, core) {
                    // Exclusive stores bypass the ordinary store hook: log them
                    // in the same ordered log with the same range checks.
                    observe::record(inner, core, base, 4, value as i64, address, true);
                }
            }
            inner.write_reg(core, ffi::ARM_REGS[target], if success { 0 } else { 1 })?;
            let state = unsafe { &mut *inner.state.get() };
            if success {
                state.exclusive.stores_succeeded += 1;
            } else {
                state.exclusive.stores_failed += 1;
            }
            state.touch();
        }
        inner.write_reg(core, ffi::PC, (address as u32).wrapping_add(4))?;
        return Ok(());
    }
    let cp = (word >> 8) & 15;
    let read = word & (1 << 20) != 0;
    let (crn, crm, opc1, opc2) = (
        (word >> 16) & 15,
        word & 15,
        (word >> 21) & 7,
        (word >> 5) & 7,
    );
    if cp==15 && opc1==0 && crn==9 && crm==1 && opc2<=1 {
        if inner.read_reg(core,ffi::CPSR)? & 0x1f == 0x10 {
            return Err("TCM region access requires privileged ARM mode".into());
        }
        if read {
            if target==15 { return Err("TCM MRC into PC is outside the modeled contract".into()); }
            inner.write_reg(core,ffi::ARM_REGS[target],crate::tcm::register(inner,core,opc2 as usize))?;
            inner.write_reg(core,ffi::PC,(address as u32).wrapping_add(4))?;
        } else {
            let value=inner.read_reg(core,ffi::ARM_REGS[target])?;
            if crate::tcm::request(inner,core,opc2 as usize,value)? {
                let mut redirects=inner.redirects.get(); redirects[core]=Some((address as u32).wrapping_add(4));
                inner.redirects.set(redirects); inner.stop(core);
            }
        }
        return Ok(());
    }
    if cp == 15
        && opc1 == 0
        && crn == 9
        && matches!((crm, opc2), (12, 0) | (12, 1) | (12, 2) | (13, 0))
    {
        let (count, pmcr, pmcntenset) = {
            let state = unsafe { &*inner.state.get() };
            let c = state.cores[core];
            let count = if c.pmcr & 1 != 0 && c.pmcntenset & 0x80000000 != 0 {
                state.ticks.wrapping_sub(c.pmu_start)
            } else {
                c.pmu_frozen
            };
            (count, c.pmcr, c.pmcntenset)
        };
        if read && (crm, opc2) == (13, 0) {
            inner.write_reg(core, ffi::ARM_REGS[target], count as u32)?;
        } else if !read {
            let value = inner.read_reg(core, ffi::ARM_REGS[target])?;
            if (crm, opc2) == (13, 0) {
                inner.event("pmu_fault", core, address, size, word as i64);
                return Ok(());
            }
            let state = unsafe { &mut *inner.state.get() };
            let tick = state.ticks;
            state.touch();
            let c = &mut state.cores[core];
            match (crm, opc2) {
                (12, 0) => {
                    c.pmcr = value & !6;
                    if value & 4 != 0 {
                        c.pmu_start = tick;
                        c.pmu_frozen = 0;
                    }
                }
                (12, 1) => {
                    c.pmcntenset |= value;
                }
                (12, 2) => {
                    c.pmu_frozen = count;
                    c.pmcntenset &= !value;
                }
                _ => unreachable!(),
            }
        } else {
            inner.write_reg(
                core,
                ffi::ARM_REGS[target],
                if opc2 == 0 { pmcr } else { pmcntenset },
            )?;
        }
        inner.write_reg(core, ffi::PC, (address as u32).wrapping_add(4))?;
        return Ok(());
    }
    if word & 0x0f000f10 == 0x0e000e10 {
        if read && (crn, crm, opc1, opc2) == (2, 0, 0, 0) {
            inner.write_reg(core, ffi::ARM_REGS[target], inner.cp14_identity.get()[core])?;
            inner.write_reg(core, ffi::PC, (address as u32).wrapping_add(4))?;
            let first = {
                let state = unsafe { &mut *inner.state.get() };
                state.cores[core].cp14_reads += 1;
                state.touch();
                state.cp14_sites.insert((core, address))
            };
            if first {
                inner.event("cp14", core, address, size, 0);
            }
        } else {
            inner.event("cp14_fault", core, address, size, word as i64);
        }
    }
    Ok(())
}

/// Why an instruction needs per-instruction handling (diagnostic metric):
/// 1 milestone, 2 word exclusive or CLREX, 3 WFI/halt, 4 CP14, 5 PMU,
/// 6 other coprocessor transfer, 7 other.
fn special_kind(decoded: &crate::state::Instruction) -> u8 {
    let word = decoded.word;
    if decoded.milestone {
        1
    } else if word & 0x0ff00fff == 0x01900f9f || word & 0x0ff00ff0 == 0x01800f90 || word == 0xf57ff01f {
        2
    } else if word == 0xe320f003 || word == 0xeafffffe {
        3
    } else if word & 0x0f000f10 == 0x0e000e10 {
        4
    } else if word & 0x0f000f10 == 0x0e000f10 && (word >> 16) & 15 == 9 {
        5
    } else if word & 0x0f000010 == 0x0e000010 {
        6
    } else {
        7
    }
}

/// A conditional coprocessor transfer that `instruction` leaves entirely to
/// Unicorn: not CP14, not a natively modeled PMU register, not in the
/// unconditional (cond 15) space. Its hook path only counts and traces it,
/// so a block containing it may be counted in bulk.
fn passive_coprocessor(word: u32) -> bool {
    if word & 0x0f000010 != 0x0e000010 || word >> 28 == 15 || word & 0x0f000f10 == 0x0e000e10 {
        return false;
    }
    let (opc1, crn, crm, opc2) = ((word >> 21) & 7, (word >> 16) & 15, word & 15, (word >> 5) & 7);
    if (word>>8)&15==15 && opc1==0 && crn==9 && crm==1 && opc2<=1 { return false; }
    !((word >> 8) & 15 == 15
        && opc1 == 0
        && crn == 9
        && matches!((crm, opc2), (12, 0) | (12, 1) | (12, 2) | (13, 0)))
}
