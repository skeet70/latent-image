use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};
use std::rc::Rc;

/// Multiplicative hash for the integer-keyed decode/page maps touched on
/// every store and decode miss (std's SipHash showed up in profiles).
/// Keys are guest addresses and page numbers, never attacker-chosen input.
#[derive(Default, Clone, Copy)]
pub struct AddressHasher(u64);
impl Hasher for AddressHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u64(byte as u64);
        }
    }
    #[inline]
    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    #[inline]
    fn write_usize(&mut self, value: usize) {
        self.write_u64(value as u64);
    }
}
pub type AddressMap<V> = HashMap<u64, V, BuildHasherDefault<AddressHasher>>;
pub type AddressSet = HashSet<u64, BuildHasherDefault<AddressHasher>>;

#[derive(Clone, Copy, Default)]
pub struct CoreState {
    pub instructions: u64,
    pub waiting: bool,
    pub pmcr: u32,
    pub pmcntenset: u32,
    pub pmu_start: u64,
    pub pmu_frozen: u64,
    pub cp14_reads: u64,
}
#[derive(Clone, Copy)]
pub struct Reservation {
    pub address: u64,
    pub valid: bool,
}
#[derive(Clone, Copy, Default)]
pub struct ExclusiveStats {
    pub loads: u64,
    pub stores_succeeded: u64,
    pub stores_failed: u64,
    pub invalidations: u64,
}
#[derive(Clone, Copy)]
pub struct Instruction {
    pub size: u32,
    pub raw: [u8; 4],
    pub word: u32,
    pub ordinary: bool,
    pub unmodeled_atomic: bool,
    pub unmodeled_pmu: bool,
    // Entry-milestone address; set once by `Inner::instruction` (the entry
    // set is fixed at construction), so the hook needs no hash lookup.
    pub milestone: bool,
}
// Direct-mapped front of the per-core decode cache used by fast execution.
// A slot is valid only while the same address is in `State::cache`.
pub const FRONT_SLOTS: usize = 4096;
#[derive(Clone, Copy)]
pub struct Slot {
    pub address: u64,
    pub entry: Instruction,
}
pub fn front_index(address: u64) -> usize {
    (address >> 2) as usize & (FRONT_SLOTS - 1)
}
#[derive(Clone, Copy, Default)]
pub struct Trace {
    pub address: u64,
    pub raw: [u8; 4],
    pub size: u32,
}
/// One translated block's instructions, decoded when the block was entered.
/// `plain` blocks hold only instructions whose per-instruction hook work is
/// counting and tracing (see `State::settle`). Valid until a decode-cache
/// eviction of a page it covers (`State::block_pages`) or `clear_derived`.
#[derive(Clone)]
pub struct BlockInfo {
    pub size: u32,
    // First reason a block is not plain (see `hooks::special_kind`), 0 if plain.
    pub reason: u8,
    pub plain: bool,
    pub entries: Rc<[Trace]>,
}
/// A plain block being run with block-level accounting: the instruction hook
/// only decrements `remaining`; `settle` applies the counts of the
/// instructions entered so far, exactly as per-instruction accounting would.
#[derive(Clone, Default)]
pub struct Bulk {
    pub n: u64,
    pub remaining: u64,
    pub entries: Option<Rc<[Trace]>>,
    // Filtered blocks (patched Unicorn) have no per-instruction hook to count
    // down `remaining`: they are settled whole, or up to an instruction the
    // engine names (`settle_at`), from the block at `start`.
    pub filtered: bool,
    pub start: u64,
}
pub const TRACE_CAPACITY: usize = 32;
pub const BLOCK_FRONT_SLOTS: usize = 4096;
#[inline]
pub fn block_front_index(address: u64) -> usize {
    (address >> 2) as usize & (BLOCK_FRONT_SLOTS - 1)
}
// The most recent executed instructions of one core (oldest first), as a
// fixed ring: failure reports and checkpoints need only address and bytes.
#[derive(Clone)]
pub struct TraceRing {
    entries: [Trace; TRACE_CAPACITY],
    next: usize,
    len: usize,
}
impl Default for TraceRing {
    fn default() -> Self {
        Self {
            entries: [Trace::default(); TRACE_CAPACITY],
            next: 0,
            len: 0,
        }
    }
}
impl TraceRing {
    pub fn clear(&mut self) {
        self.next = 0;
        self.len = 0;
    }
    #[inline]
    pub fn push(&mut self, entry: Trace) {
        self.entries[self.next] = entry;
        self.next = (self.next + 1) % TRACE_CAPACITY;
        if self.len < TRACE_CAPACITY {
            self.len += 1;
        }
    }
    /// Push every entry of `entries` in order (as repeated `push`).
    #[inline]
    pub fn extend(&mut self, entries: &[Trace]) {
        let entries = &entries[entries.len().saturating_sub(TRACE_CAPACITY)..];
        let first = (TRACE_CAPACITY - self.next).min(entries.len());
        self.entries[self.next..self.next + first].copy_from_slice(&entries[..first]);
        self.entries[..entries.len() - first].copy_from_slice(&entries[first..]);
        self.next = (self.next + entries.len()) % TRACE_CAPACITY;
        self.len = (self.len + entries.len()).min(TRACE_CAPACITY);
    }
    pub fn iter(&self) -> impl Iterator<Item = &Trace> {
        let start = (self.next + TRACE_CAPACITY - self.len) % TRACE_CAPACITY;
        (0..self.len).map(move |i| &self.entries[(start + i) % TRACE_CAPACITY])
    }
}

type PortableUnsafeHistory = BTreeMap<(usize, &'static str), (Option<u64>, Option<u32>)>;

pub struct State {
    pub instructions: u64,
    pub ticks: u64,
    // Declared CPU-speed knob: `tick_num/tick_den` instructions per virtual
    // tick (default 1/1: one tick per instruction). `tick_phase` is the
    // fractional accumulator, in units of 1/tick_num ticks.
    pub tick_num: u64,
    pub tick_den: u64,
    pub tick_phase: u64,
    pub cores: [CoreState; 2],
    pub reservations: [Option<Reservation>; 2],
    pub tcm_windows: [[Option<(u64,usize)>; 2]; 2],
    pub exclusive: ExclusiveStats,
    pub remap_requested: bool,
    pub cache_enabled: bool,
    pub cache: [AddressMap<Instruction>; 2],
    pub pages: [AddressMap<AddressSet>; 2],
    pub pending: [AddressSet; 2],
    pub trace: [TraceRing; 2],
    pub entries: AddressSet,
    pub seen_entries: HashSet<(usize, u64)>,
    pub cp14_sites: HashSet<(usize, u64)>,
    // Bumped by every native mutation of the state Python mirrors besides the
    // always-moving counters (waiting/PMU/CP14 fields, exclusive monitors and
    // counts, CP14 sites, pending invalidations). Python-originated loads do
    // not bump it. An event handler compares it with the value its mirror
    // was last synchronised at, and re-reads that state only when it differs.
    pub aux_version: u64,
    pub code_hooks: u64,
    pub store_hooks: u64,
    pub python_callbacks: u64,
    // Instruction entries stop (before executing) once `ticks` reaches this
    // value. u64::MAX disables it; the driver harness uses it to deliver
    // scheduled device completions at their exact due tick.
    pub tick_deadline: u64,
    // Lifetime eligibility is deliberately not part of load_state or derived
    // caches. Same-engine rollback cannot prove that hidden Unicorn state was
    // undone after an instruction outside the portable register manifest.
    pub portable_unsafe: PortableUnsafeHistory,
    pub block_thumb: [bool; 2],
    // Fast execution: one inline instruction hook, no Unicorn count hook. The
    // run's instruction limit is enforced by that hook (`entered` counts its
    // invocations exactly as Unicorn's count hook would; 0 = unlimited).
    pub fast: bool,
    pub entered: u64,
    pub entry_limit: u64,
    // A Thumb block was entered in fast mode: the run stops at that block and
    // continues in exact (default) mode, which then stays selected.
    pub downgrade: bool,
    pub downgrades: u64,
    pub front: [Vec<Slot>; 2],
    // MMIO reads Python declared repeatable: (address, size) per core whose
    // last returned value is still in the engine's MMIO page. Valid only until
    // Python next runs, so every event and every emu_start clears them.
    pub mmio_read_cache: [Vec<(u64, u32)>; 2],
    pub cached_mmio_reads: u64,
    pub blocks: [AddressMap<BlockInfo>; 2],
    // Block addresses by the code pages they cover, for eviction.
    pub block_pages: [AddressMap<Vec<u64>>; 2],
    // Direct-mapped front of `blocks` (`block_front_index`), cleared with it.
    pub block_front: [Vec<Option<(u64, BlockInfo)>>; 2],
    // Bytes of translated blocks per code page, one bit each (`note_code`).
    pub code_bytes: [AddressMap<Box<[u64; 64]>>; 2],
    // 4 KiB pages (canonical where aliased) declared watched to both
    // engines, never cleared like the engines' own watch sets; and the page
    // addresses still to declare (`Inner::apply_watches`).
    pub watched: AddressSet,
    pub watch_queue: Vec<u64>,
    // Per core: the patched Unicorn's translation filter is installed, so
    // ordinary instructions have no hook (`hooks::filter`, `hooks::block`).
    pub filtered: [bool; 2],
    pub retried_blocks: u64,
    pub restarts: u64,
    // PC of an instruction patched Unicorn restarted as its own block: it was
    // admitted before the restart, so a pending stop must not stop it.
    pub restart_pc: [Option<u64>; 2],
    // Budgeted block accounting (patched Unicorn `uc_x70_accounting`), per
    // core: on; the budget left at the last settle; the ring index up to
    // which blocks are in `trace`; blocks whose words were captured before a
    // store overwrote them (ring index, words).
    pub acct_on: [bool; 2],
    pub acct_synced: [i64; 2],
    pub acct_traced: [u64; 2],
    pub acct_frozen: [Vec<(u64, Rc<[Trace]>)>; 2],
    pub plain_accesses: u64,
    pub code_generation: u64,
    pub bulk: [Bulk; 2],
    pub bulk_blocks: u64,
    pub blocks_entered: u64,
    pub precise_instructions: u64,
    pub special_blocks: [u64; 8],
}
impl State {
    pub fn new(entries: Vec<u64>) -> Self {
        Self {
            instructions: 0,
            ticks: 0,
            tick_num: 1,
            tick_den: 1,
            tick_phase: 0,
            cores: [CoreState::default(); 2],
            reservations: [None; 2],
            tcm_windows: [[None; 2]; 2],
            exclusive: ExclusiveStats::default(),
            remap_requested: false,
            cache_enabled: true,
            cache: Default::default(),
            pages: Default::default(),
            pending: Default::default(),
            trace: Default::default(),
            entries: entries.into_iter().collect(),
            seen_entries: HashSet::new(),
            cp14_sites: HashSet::new(),
            aux_version: 0,
            code_hooks: 0,
            store_hooks: 0,
            python_callbacks: 0,
            tick_deadline: u64::MAX,
            portable_unsafe: BTreeMap::new(),
            block_thumb: [false; 2],
            fast: false,
            entered: 0,
            entry_limit: 0,
            downgrade: false,
            downgrades: 0,
            mmio_read_cache: Default::default(),
            cached_mmio_reads: 0,
            blocks: Default::default(),
            block_pages: Default::default(),
            block_front: [vec![None; BLOCK_FRONT_SLOTS], vec![None; BLOCK_FRONT_SLOTS]],
            code_bytes: Default::default(),
            watched: Default::default(),
            watch_queue: Vec::new(),
            filtered: [false; 2],
            retried_blocks: 0,
            restarts: 0,
            restart_pc: [None; 2],
            acct_on: [false; 2],
            acct_synced: [0; 2],
            acct_traced: [0; 2],
            acct_frozen: Default::default(),
            plain_accesses: 0,
            code_generation: 0,
            bulk: Default::default(),
            bulk_blocks: 0,
            blocks_entered: 0,
            precise_instructions: 0,
            special_blocks: [0; 8],
            front: Default::default(),
        }
    }
    pub fn set_fast(&mut self, fast: bool) {
        self.fast = fast;
        for front in &mut self.front {
            front.clear();
            if fast {
                front.resize(
                    FRONT_SLOTS,
                    Slot {
                        address: u64::MAX,
                        entry: classify(4, [0; 4]),
                    },
                );
            }
        }
    }
    pub fn canonical(address: u64, core: usize, ram_size: u64) -> Option<u64> {
        if (0x40000000..0x40000000 + ram_size).contains(&address)
            || (0x7fff0000..0x7fff1000).contains(&address)
        {
            Some(address - 0x40000000)
        } else if core == 1 && (0xffff0000..0xffff1000).contains(&address) {
            Some(address - 0xc0000000)
        } else if address < ram_size
            || (0xfff00000..0xfff01000).contains(&address)
            || (0x3fff0000..0x3fff1000).contains(&address)
        {
            Some(address)
        } else {
            None
        }
    }
    pub fn invalidate_exclusive(&mut self, address: u64, size: u64, core: usize, ram_size: u64) {
        if !self.reservations.iter().flatten().any(|reservation| reservation.valid) {
            return;
        }
        if let Some(canonical) = self.canonical_address(address, core, ram_size) {
            let mut touched = false;
            for r in self.reservations.iter_mut().flatten() {
                if r.valid && canonical < r.address + 4 && r.address < canonical + size {
                    r.valid = false;
                    self.exclusive.invalidations += 1;
                    touched = true;
                }
            }
            if touched {
                self.touch();
            }
        }
    }
    pub fn canonical_address(&self,address:u64,core:usize,ram_size:u64)->Option<u64> {
        for (bank,extent) in self.tcm_windows[core].iter().enumerate() {
            if let Some((base,size))=extent {
                if address>=*base && address<*base+*size as u64 {
                    return Some(0x1_0000_0000+((core as u64)<<24)+((bank as u64)<<23)+address-base);
                }
            }
        }
        Self::canonical(address,core,ram_size)
    }
    pub fn bus_write_dram(&mut self,physical:u64,size:u64,ram_size:u64) {
        let mut touched=false;
        for reservation in self.reservations.iter_mut().flatten() {
            if reservation.valid && physical<reservation.address+4 && reservation.address<physical+size {
                reservation.valid=false;self.exclusive.invalidations+=1;touched=true;
            }
        }
        if touched { self.touch(); }
        for core in 0..2 {
            for address in [physical,physical+0x40000000] {
                if !self.tcm_windows[core].iter().flatten().any(|(base,n)|address>=*base && address<*base+*n as u64) {
                    self.evict(core,address,size);
                }
            }
        }
        let _=ram_size;
    }
    /// Apply the counts of the bulk block's instructions entered so far (and
    /// end bulk accounting for it): instruction, tick, entry and core counts,
    /// the code-hook metric and the recent-instruction trace advance exactly
    /// as the per-instruction hook would have advanced them.
    pub fn settle(&mut self, core: usize) {
        let bulk = &self.bulk[core];
        let executed = if bulk.filtered { bulk.n } else { bulk.n - bulk.remaining };
        self.settle_count(core, executed);
    }
    /// Settle a filtered bulk block up to the instruction at `pc` (exclusive,
    /// or inclusive when it was entered: a fault or device access in it). A
    /// PC past the block (an exception that already advanced it) settles all.
    pub fn settle_at(&mut self, core: usize, pc: u64, inclusive: bool) {
        let bulk = &self.bulk[core];
        if bulk.n == 0 {
            return;
        }
        let executed = if bulk.filtered && pc >= bulk.start {
            ((pc - bulk.start) / 4 + inclusive as u64).min(bulk.n)
        } else if bulk.filtered {
            bulk.n
        } else {
            bulk.n - bulk.remaining
        };
        self.settle_count(core, executed);
    }
    fn settle_count(&mut self, core: usize, executed: u64) {
        let bulk = &mut self.bulk[core];
        if bulk.n == 0 {
            return;
        }
        let entries = bulk.entries.take();
        bulk.n = 0;
        bulk.remaining = 0;
        bulk.filtered = false;
        if executed == 0 {
            return;
        }
        self.count_executed(core, executed);
        if let Some(entries) = entries {
            let executed = (executed as usize).min(entries.len());
            self.trace[core].extend(&entries[..executed]);
        }
    }
    /// Advance every per-instruction count by `executed` instructions of
    /// `core`, exactly as that many instruction hooks would have.
    pub fn count_executed(&mut self, core: usize, executed: u64) {
        if executed == 0 {
            return;
        }
        if self.fast && self.entry_limit != 0 {
            self.entered += executed;
        }
        self.code_hooks += executed;
        self.instructions += executed;
        self.tick_phase += executed * self.tick_den;
        self.ticks += self.tick_phase / self.tick_num;
        self.tick_phase %= self.tick_num;
        self.cores[core].instructions += executed;
    }
    /// Instructions `core` may still run before a per-instruction check
    /// would stop it (entry limit, tick deadline), or 0 when a pending code
    /// invalidation or remap must stop it at the next block.
    pub fn room(&self) -> u64 {
        if self.remap_requested || self.pending.iter().any(|p| !p.is_empty()) {
            return 0;
        }
        let mut room = u64::MAX;
        if self.entry_limit != 0 {
            room = room.min(self.entry_limit.saturating_sub(self.entered));
        }
        if self.tick_deadline != u64::MAX {
            // Instruction i runs while ticks + (phase + i * den) / num < deadline.
            room = if self.ticks >= self.tick_deadline {
                0
            } else {
                let need = (self.tick_deadline - self.ticks) as u128 * self.tick_num as u128
                    - self.tick_phase as u128;
                room.min(need.div_ceil(self.tick_den as u128).min(u64::MAX as u128) as u64)
            };
        }
        room
    }
    #[inline]
    pub fn touch(&mut self) {
        self.aux_version = self.aux_version.wrapping_add(1);
    }
    /// Record a translated block's bytes (block hook) for `evict`.
    /// Queue the 4 KiB page at `page` (as `core` addresses it) and all its
    /// aliases for watching, once: stores there must reach the store hook.
    pub fn watch_page(&mut self, core: usize, page: u64, ram_size: u64) {
        // TCM-private canonical addresses (beyond 32 bits) are not engine
        // addresses; TCM accesses already reach Rust through the device hook.
        if page > 0xffff_ffff {
            return;
        }
        let canonical = Self::canonical(page, core, ram_size);
        if !self.watched.insert(canonical.unwrap_or(page)) {
            return;
        }
        self.watch_queue.push(page);
        if let Some(c) = canonical {
            self.watch_queue.push(c);
            if c < ram_size || (0x3fff0000..0x3fff1000).contains(&c) {
                self.watch_queue.push(c + 0x40000000);
            }
            if (0x3fff0000..0x3fff1000).contains(&c) {
                self.watch_queue.push(c + 0xc0000000);
            }
        }
    }
    pub fn note_code(&mut self, core: usize, address: u64, size: u64, ram_size: u64) {
        for page in address >> 12..=(address + size.max(1) - 1) >> 12 {
            self.watch_page(core, page << 12, ram_size);
            let bits = self.code_bytes[core].entry(page).or_insert_with(|| Box::new([0; 64]));
            let first = (address.max(page << 12) & 0xfff) as usize;
            let end = ((address + size).min((page + 1) << 12) - (page << 12)) as usize;
            for word in first / 64..end.div_ceil(64) {
                let low = first.saturating_sub(word * 64);
                let high = (end - word * 64).min(64);
                bits[word] |= (u64::MAX << low) & (u64::MAX >> (64 - high));
            }
        }
    }
    fn evict(&mut self, core: usize, address: u64, size: u64) {
        for page in address >> 12..=(address + size - 1) >> 12 {
            // Firmware data shares pages with code. A small store overlapping
            // no translated or decoded instruction byte leaves every decode
            // and translation of the page valid; larger writes evict pages.
            if size <= 8 {
                let first = address.saturating_sub(3) & !1;
                let decoded = self.pages[core].get(&page).is_some_and(|entries| {
                    (first..address + size).step_by(2).any(|pc| entries.contains(&pc))
                });
                let translated = self.code_bytes[core].get(&page).is_some_and(|bits| {
                    (address.max(page << 12)..(address + size).min((page + 1) << 12)).any(|byte| {
                        let offset = (byte & 0xfff) as usize;
                        bits[offset / 64] >> (offset % 64) & 1 != 0
                    })
                });
                if !decoded && !translated {
                    continue;
                }
            } else if !self.pages[core].contains_key(&page) && !self.code_bytes[core].contains_key(&page) {
                continue;
            }
            self.code_bytes[core].remove(&page);
            for pc in self.pages[core].remove(&page).unwrap_or_default() {
                self.cache[core].remove(&pc);
                if let Some(slot) = self.front[core].get_mut(front_index(pc))
                    && slot.address == pc
                {
                    slot.address = u64::MAX;
                }
            }
            for block in self.block_pages[core].remove(&page).unwrap_or_default() {
                self.blocks[core].remove(&block);
                let slot = &mut self.block_front[core][block_front_index(block)];
                if slot.as_ref().is_some_and(|(address, _)| *address == block) {
                    *slot = None;
                }
            }
            self.pending[core].insert(page);
            self.code_generation += 1;
            self.touch();
        }
    }
    pub fn invalidate_code(&mut self, address: u64, size: u64, core: usize, ram_size: u64) {
        if size == 0
            || (self.pages.iter().all(|p| p.is_empty()) && self.code_bytes.iter().all(|p| p.is_empty()))
        {
            return;
        }
        if self.tcm_windows[core].iter().flatten().any(|(base,n)|address>=*base && address<*base+*n as u64) {
            self.evict(core,address,size);
            return;
        }
        if let Some(canonical) = Self::canonical(address, core, ram_size) {
            let aliased = canonical < ram_size || (0x3fff0000..0x3fff1000).contains(&canonical);
            if (0x3fff0000..0x3fff1000).contains(&canonical) {
                self.evict(1, canonical + 0xc0000000, size);
            }
            for target in 0..2 {
                self.evict(target, canonical, size);
                if aliased {
                    self.evict(target, canonical + 0x40000000, size);
                }
            }
        } else {
            self.evict(core, address, size);
        }
    }
    pub fn clear_derived(&mut self) {
        if self.pending.iter().any(|p| !p.is_empty()) {
            self.touch();
        }
        self.code_generation += 1;
        for core in 0..2 {
            self.cache[core].clear();
            self.pages[core].clear();
            self.pending[core].clear();
            self.blocks[core].clear();
            self.block_pages[core].clear();
            self.block_front[core].iter_mut().for_each(|slot| *slot = None);
            self.code_bytes[core].clear();
            for slot in &mut self.front[core] {
                slot.address = u64::MAX;
            }
        }
    }
    pub fn clear_blocks(&mut self, core: usize) {
        self.blocks[core].clear();
        self.block_pages[core].clear();
        self.block_front[core].iter_mut().for_each(|slot| *slot = None);
    }
    pub fn portable_unsafe(
        &mut self,
        core: usize,
        reason: &'static str,
        pc: Option<u64>,
        opcode: Option<u32>,
    ) {
        self.portable_unsafe
            .entry((core, reason))
            .or_insert((pc, opcode));
    }
}

pub fn classify(size: u32, raw: [u8; 4]) -> Instruction {
    let word = u32::from_le_bytes(raw);
    // A1 LDREX{B,H,D}/STREX{B,H,D}; word-only variants belong to Rust.
    // Classification is cached, but the execution guard still checks condition
    // flags and ARM/Thumb state before recording unsupported history.
    let word_load = word & 0x0ff00fff == 0x01900f9f;
    let word_store = word & 0x0ff00ff0 == 0x01800f90;
    let unmodeled_atomic = size == 4
        && ((word & 0x0f900fff == 0x01900f9f && !word_load)
            || (word & 0x0f900ff0 == 0x01800f90 && !word_store));
    let cp15_transfer = word & 0x0f000f10 == 0x0e000f10;
    let (opc1, crn, crm, opc2) = (
        (word >> 21) & 7,
        (word >> 16) & 15,
        word & 15,
        (word >> 5) & 7,
    );
    let native_pmu = word >> 28 != 15
        && opc1 == 0
        && crn == 9
        && matches!((crm, opc2), (12, 0) | (12, 1) | (12, 2) | (13, 0));
    // Includes the directly indexed event counters/types and cycle filter.
    let pmu_space = opc1 == 0
        && ((crn == 9 && (12..=14).contains(&crm)) || (crn == 14 && (8..=15).contains(&crm)));
    // Conservatively refuse a 64-bit CP15 cycle-counter transfer as well.
    let pmu64 = word & 0x0fe00f00 == 0x0c400f00 && crm == 9;
    let unmodeled_pmu = size == 4 && ((cp15_transfer && pmu_space && !native_pmu) || pmu64);
    let ordinary = size != 4
        || (!matches!(word, 0xeafffffe | 0xe320f003 | 0xf57ff01f)
            && word & 0x0ff00fff != 0x01900f9f
            && word & 0x0ff00ff0 != 0x01800f90
            && word & 0x0f000010 != 0x0e000010);
    Instruction {
        size,
        raw,
        word,
        ordinary,
        unmodeled_atomic,
        unmodeled_pmu,
        milestone: false,
    }
}

// Host writes use the raw Unicorn state, including for Rust-covered guest PMU
// selectors. Permit only fields explicitly exported by architecture.py.
pub fn portable_host_cp15(
    cp: u32,
    opc1: u32,
    crn: u32,
    crm: u32,
    opc2: u32,
    sec: u32,
    is64: bool,
) -> bool {
    cp == 15
        && sec <= 1
        && !is64
        && matches!(
            (opc1, crn, crm, opc2),
            (0, 1, 0, 0 | 2)
                | (0, 3, 0, 0)
                | (2, 0, 0, 0)
                | (0, 5, 0, 0 | 1)
                | (0, 6, 0, 0 | 2)
                | (0, 12, 0, 0)
                | (0, 13, 0, 0..=4)
                | (0, 10, 2, 0 | 1)
                | (0, 7, 4, 0)
                | (0, 6, 2, 0)
                | (0, 6, 1, 0 | 2 | 4)
        )
}

pub fn host_pmu(cp: u32, opc1: u32, crn: u32, crm: u32, is64: bool) -> bool {
    cp == 15
        && ((is64 && crm == 9)
            || (opc1 == 0
                && ((crn == 9 && (12..=14).contains(&crm))
                    || (crn == 14 && (8..=15).contains(&crm)))))
}

pub fn portable_host_register(reg: i32) -> bool {
    // General/banked, Q/D/S aliases, APSR flags, raw CPACR/TPIDRURO, ESR.
    matches!(reg, 1..=4 | 6 | 10..=13 | 14..=45 | 50..=110 | 111 | 113 | 140)
}

pub fn condition_passes(condition: u32, cpsr: u32) -> bool {
    let n = cpsr & (1 << 31) != 0;
    let z = cpsr & (1 << 30) != 0;
    let c = cpsr & (1 << 29) != 0;
    let v = cpsr & (1 << 28) != 0;
    [
        z,
        !z,
        c,
        !c,
        n,
        !n,
        v,
        !v,
        c && !z,
        !c || z,
        n == v,
        n != v,
        !z && n == v,
        z || n != v,
        true,
        false,
    ][condition as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aliases_and_same_value_write_invalidate() {
        let mut s = State::new(vec![]);
        s.reservations[0] = Some(Reservation {
            address: 0x2000,
            valid: true,
        });
        s.invalidate_exclusive(0x40002000, 4, 1, 0x200000);
        assert!(!s.reservations[0].unwrap().valid);
        assert_eq!(s.exclusive.invalidations, 1);
        assert_eq!(State::canonical(0xffff0080, 1, 0x200000), Some(0x3fff0080));
    }
    #[test]
    fn stores_without_live_reservations_leave_exclusive_counts_unchanged() {
        let mut s = State::new(vec![]);
        s.invalidate_exclusive(0x2000, 4, 0, 0x200000);
        assert_eq!(s.exclusive.invalidations, 0);
        assert_eq!(s.aux_version, 0);
        s.reservations[1] = Some(Reservation { address: 0x2000, valid: true });
        s.invalidate_exclusive(0x40002000, 4, 0, 0x200000);
        assert_eq!(s.exclusive.invalidations, 1);
        let version = s.aux_version;
        s.invalidate_exclusive(0x2000, 4, 1, 0x200000);
        assert_eq!(s.exclusive.invalidations, 1);
        assert_eq!(s.aux_version, version);
    }
    #[test]
    fn invalidation_tracks_both_cpus_and_aliases() {
        let mut s = State::new(vec![]);
        for core in 0..2 {
            for addr in [0x2000, 0x40002000] {
                s.pages[core].entry(addr >> 12).or_default().insert(addr);
                s.cache[core].insert(addr, classify(4, 0xe3a00001u32.to_le_bytes()));
            }
        }
        s.invalidate_code(0x40002000, 4, 0, 0x200000);
        assert!(s.cache.iter().all(HashMap::is_empty));
        assert!(s.pending.iter().all(|p| p.len() == 2));
    }
    #[test]
    fn stores_beside_code_keep_the_page() {
        let mut s = State::new(vec![]);
        s.pages[0].entry(2).or_default().insert(0x2000);
        s.cache[0].insert(0x2000, classify(4, 0xe3a00001u32.to_le_bytes()));
        s.note_code(0, 0x2000, 0x10, 0x200000);
        s.invalidate_code(0x2010, 4, 0, 0x200000);
        assert_eq!(s.cache[0].len(), 1);
        assert!(s.pending[0].is_empty());
        // Translated but never decoded (an unexecuted block tail).
        s.invalidate_code(0x200c, 1, 0, 0x200000);
        assert!(s.cache[0].is_empty());
        assert!(s.pending[0].contains(&2));
        assert!(s.code_bytes[0].is_empty());
    }
    #[test]
    fn translated_byte_boundaries_preserve_neighboring_data_stores() {
        for (start, size) in [
            (0x2000, 1),
            (0x203f, 1),
            (0x203f, 2),
            (0x2040, 64),
            (0x203f, 130),
            (0x2fff, 1),
            (0x2fff, 2),
            (0x2fff, 4098),
        ] {
            for (address, overlaps) in [
                (start - 1, false),
                (start, true),
                (start + size / 2, true),
                (start + size - 1, true),
                (start + size, false),
            ] {
                let mut s = State::new(vec![]);
                s.note_code(0, start, size, 0x200000);
                s.invalidate_code(address, 1, 0, 0x200000);
                assert_eq!(s.pending[0].contains(&(address >> 12)), overlaps);
                assert!(s.pending[1].is_empty());
            }
        }
    }
    #[test]
    fn separately_translated_spans_keep_the_gap_writable() {
        for (address, overlaps) in [(0x203f, true), (0x2040, false), (0x207f, true)] {
            let mut s = State::new(vec![]);
            s.note_code(0, 0x203e, 2, 0x200000);
            s.note_code(0, 0x207f, 2, 0x200000);
            s.invalidate_code(address, 1, 0, 0x200000);
            assert_eq!(s.pending[0].contains(&2), overlaps);
        }
    }
    #[test]
    fn trace_ring_extend_equals_pushes() {
        for start in 0..40u64 {
            for count in 0..70u64 {
                let entries: Vec<Trace> = (0..count)
                    .map(|a| Trace { address: 1000 + a, raw: [a as u8; 4], size: 4 })
                    .collect();
                let (mut pushed, mut extended) = (TraceRing::default(), TraceRing::default());
                for a in 0..start {
                    let t = Trace { address: a, raw: [0; 4], size: 4 };
                    pushed.push(t);
                    extended.push(t);
                }
                entries.iter().for_each(|t| pushed.push(*t));
                extended.extend(&entries);
                assert_eq!(
                    pushed.iter().map(|t| t.address).collect::<Vec<_>>(),
                    extended.iter().map(|t| t.address).collect::<Vec<_>>()
                );
            }
        }
    }
    #[test]
    fn trace_ring_keeps_the_last_entries_oldest_first() {
        let mut ring = TraceRing::default();
        assert_eq!(ring.iter().count(), 0);
        for address in 0..TRACE_CAPACITY as u64 + 5 {
            ring.push(Trace {
                address,
                raw: [address as u8; 4],
                size: 4,
            });
        }
        let kept: Vec<u64> = ring.iter().map(|t| t.address).collect();
        assert_eq!(kept, (5..TRACE_CAPACITY as u64 + 5).collect::<Vec<_>>());
        ring.clear();
        assert_eq!(ring.iter().count(), 0);
        ring.push(Trace {
            address: 9,
            raw: [0; 4],
            size: 2,
        });
        assert_eq!(ring.iter().map(|t| t.address).collect::<Vec<_>>(), [9]);
    }
    #[test]
    fn front_slots_are_invalidated_with_the_decode_cache() {
        let mut s = State::new(vec![]);
        s.set_fast(true);
        let entry = classify(4, 0xe3a00001u32.to_le_bytes());
        for core in 0..2 {
            s.cache[core].insert(0x2000, entry);
            s.pages[core].entry(2).or_default().insert(0x2000);
            s.front[core][front_index(0x2000)] = Slot {
                address: 0x2000,
                entry,
            };
        }
        s.invalidate_code(0x2000, 4, 0, 0x200000);
        assert!(s.cache.iter().all(HashMap::is_empty));
        assert!(
            s.front
                .iter()
                .all(|f| f[front_index(0x2000)].address == u64::MAX)
        );
        s.front[0][front_index(0x2000)].address = 0x2000;
        s.clear_derived();
        assert!(s.front[0].iter().all(|slot| slot.address == u64::MAX));
        s.set_fast(false);
        assert!(s.front.iter().all(Vec::is_empty));
    }
    #[test]
    fn decode_masks_and_condition_codes() {
        assert!(classify(4, 0xe3a00001u32.to_le_bytes()).ordinary);
        for w in [
            0xeafffffeu32,
            0xe320f003,
            0xf57ff01f,
            0xe1920f9f,
            0xe1821f90,
            0xee120e10,
        ] {
            assert!(!classify(4, w.to_le_bytes()).ordinary);
        }
        for flags in 0..16 {
            let cpsr = flags << 28;
            assert!(condition_passes(14, cpsr));
            assert!(!condition_passes(15, cpsr));
            assert_ne!(condition_passes(0, cpsr), condition_passes(1, cpsr));
        }
    }
}
