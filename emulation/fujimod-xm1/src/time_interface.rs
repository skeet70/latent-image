use fujimod_emulator::board::observation::{MemoryWatch, ViewPolicy, WatchKinds, WatchRange};
use fujimod_emulator::board::factory::MemoryMap;
use fujimod_xm1::{machine::Machine, run, trace::{Hit, TaskContext}};
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, BTreeSet, VecDeque}, fs::File, io::{BufWriter, Write}, path::Path};

const ROW_LIMIT: usize = 131_072;
const FRAME_LIMIT: usize = 1024;
const SITE_LIMIT: usize = 4096;
const EVENT_LIMIT: usize = 500_000;

fn require(condition: bool, message: &str) -> Result<(), String> { if condition { Ok(()) } else { Err(message.into()) } }
fn address(text: &str) -> Result<u32, String> {
    u32::from_str_radix(text.strip_prefix("0x").ok_or("address lacks 0x prefix")?, 16).map_err(|error| error.to_string())
}
fn register(hit: &Hit, index: u8) -> Result<u32, String> {
    const NAMES: [&str; 13] = ["r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11", "r12"];
    address(hit.registers.get(*NAMES.get(usize::from(index)).ok_or("invalid interface register")?).ok_or("interface register missing")?)
}
#[derive(Clone, Deserialize, Serialize)]
struct ReadSpec { register: u8, offsets: Vec<u32>, #[serde(default)] code_offsets: Vec<u32> }
#[derive(Clone, Deserialize, Serialize)]
struct InlinePayload { register: u8, length_offset: u32, data_offset: u32, maximum: u32 }
#[derive(Clone, Deserialize, Serialize)]
struct Entry {
    address: String, label: String, arguments: Vec<u8>,
    #[serde(default)] symbol: Option<String>,
    #[serde(default)] interface_label: Option<String>,
    #[serde(default)] code_arguments: Vec<usize>,
    #[serde(default)] returns: bool,
    #[serde(default)] outputs: Vec<u8>,
    #[serde(default)] reads: Vec<ReadSpec>,
    #[serde(default)] payload: Option<InlinePayload>,
    #[serde(default)] track: bool,
}
#[derive(Clone, Deserialize, Serialize)]
struct Callback {
    address: String, label: String, register: u8,
    #[serde(default)] symbol: Option<String>,
    #[serde(default)] owned_only: bool,
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ViewKind { Counter, Message, Interrupts }
#[derive(Clone, Deserialize, Serialize)]
struct View { address: String, bytes: u32, kind: ViewKind, #[serde(default)] queue: Option<u16> }
#[derive(Clone, Deserialize, Serialize)]
struct Slot {
    offset: u32, label: String, arguments: Vec<u8>,
    #[serde(default)] queues: Vec<u16>,
    #[serde(default)] reads: Vec<ReadSpec>,
    #[serde(default)] payload: Option<InlinePayload>,
    #[serde(default)] block_output: Option<u8>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum MessageField { Operation, Sender, Destination, Length, PayloadPointer }
#[derive(Deserialize, Serialize)]
struct MessageDeclaration {
    queue: u16, operation: u16, sender_writes: Vec<MessageField>, receiver_reads: Vec<MessageField>,
    sender_extent: [String; 2], receiver_extent: [String; 2],
}
#[derive(Deserialize, Serialize)]
struct Normalization { owned: u32, stock: u32 }
#[derive(Deserialize, Serialize)]
struct Contract {
    schema: u32, extents: Vec<[u32; 2]>, entries: Vec<Entry>, callbacks: Vec<Callback>, shared: Vec<View>,
    #[serde(default)] retired_extents: Vec<[u32; 2]>,
    #[serde(default)] retired_entries: Vec<u32>,
    #[serde(default)] imports: Vec<Entry>,
    #[serde(default)] effect_extents: Vec<[u32; 2]>,
    #[serde(default)] kernel_root_table: Option<String>,
    #[serde(default)] slots: Vec<Slot>,
    #[serde(default)] normalization: Vec<Normalization>,
    messages: Vec<MessageDeclaration>,
}
#[derive(Clone)]
enum Site { Boundary(Entry), Callback(Callback), Request(Entry), Slot(Slot) }
impl Site {
    fn raw(&self) -> (String, Vec<u8>) {
        match self {
            Self::Boundary(entry) | Self::Request(entry) => (entry.label.clone(), entry.arguments.clone()),
            Self::Callback(callback) => (callback.label.clone(), vec![callback.register]),
            Self::Slot(slot) => (slot.label.clone(), slot.arguments.clone()),
        }
    }
}
#[derive(Clone, Copy, Eq, PartialEq, Serialize)]
struct ContextKey { core: usize, task: Option<u16>, depth: u32, mode: u32 }
fn context(hit: &Hit) -> ContextKey {
    let (task, depth) = match hit.task {
        Some(TaskContext::Available { task_id, context_depth, .. }) => (Some(task_id.get()), context_depth.get()),
        _ => (None, 0),
    };
    ContextKey { core: hit.core, task, depth, mode: hit.cpsr & 0x1f }
}
#[derive(Clone, Serialize)]
struct Message { header: u32, payload: Vec<u8> }
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum Event {
    Call { actor: ContextKey, entry: String, arguments: Vec<u32>, payload: Vec<u8> },
    Return { actor: ContextKey, entry: String, status: u32, outputs: Vec<u32>, message: Option<Message> },
    Callback { actor: ContextKey, target: u32 },
    Store { view: u32, offset: u32, value: u32 },
    Message { queue: u16, header: u32, length: u32, payload: Vec<u8> },
}
#[derive(Clone, Serialize)]
struct Frame {
    key: ContextKey, sp: u32, resume: u32, label: Option<String>, track: bool,
    outputs: Vec<u32>, block_output: Option<u32>, queue: Option<u16>,
}

#[derive(Serialize)]
pub struct Boundary {
    event_count: usize,
    pending_calls: Vec<Frame>,
}
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Access { CpuFetch, CpuWrite }
#[derive(Deserialize)]
struct Row { source: Access, address: u64, size: u64, value: Option<u64>, core: Option<usize>, pc: Option<u64>, instructions: Option<u64> }
#[derive(Deserialize)]
struct Batch { enabled: bool, violation: Option<String>, records: Vec<Row> }
#[derive(Serialize)]
pub struct FetchReceipt { start: u32, end: u32, fetches: u64 }
#[derive(Serialize)]
pub struct Coverage {
    pub fetch_receipts: Vec<FetchReceipt>,
    pub visited: BTreeMap<u32, u64>,
    pub retired_body_fetches: BTreeMap<u32, u64>,
    pub events: usize,
    pub pending: usize,
}

pub struct Recorder {
    contract: Contract,
    sites: BTreeMap<u32, Site>,
    return_sites: BTreeSet<u32>,
    normalized: BTreeMap<u32, u32>,
    frames: Vec<Frame>,
    views: Vec<(u32, View)>,
    bytes: BTreeMap<u32, u8>,
    messages: BTreeMap<u16, Message>,
    writer: BufWriter<File>,
    watch: Option<Box<MemoryWatch>>,
    maps: Option<Vec<MemoryMap>>,
    kernel_bound: bool,
    rows: VecDeque<Row>,
    visited: BTreeMap<u32, u64>,
    retired_body_fetches: BTreeMap<u32, u64>,
    events: usize,
}
impl Recorder {
    pub fn boundary(&self) -> Boundary {
        Boundary { event_count: self.events, pending_calls: self.frames.clone() }
    }

    pub fn new(bytes: &[u8], output: &Path) -> Result<Self, String> {
        let contract: Contract = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        require(contract.schema == 1, "unsupported time interface schema")?;
        require(!contract.extents.is_empty() && contract.extents.len() <= 16, "time extent bound")?;
        require(contract.extents.iter().chain(&contract.retired_extents).chain(&contract.effect_extents).all(|[start, end]| start < end && start & 3 == 0 && end & 3 == 0), "invalid time extent")?;
        let mut sites = BTreeMap::new();
        for entry in &contract.entries { require(sites.insert(address(&entry.address)?, Site::Boundary(entry.clone())).is_none(), "duplicate time entry")?; }
        for entry in &contract.imports { require(sites.insert(address(&entry.address)?, Site::Request(entry.clone())).is_none(), "duplicate time request")?; }
        for callback in &contract.callbacks {
            if callback.owned_only { continue; }
            require(sites.insert(address(&callback.address)?, Site::Callback(callback.clone())).is_none(), "duplicate callback boundary")?;
        }
        require(sites.len() < SITE_LIMIT, "time static site bound")?;
        let normalized = contract.normalization.iter().map(|row| (row.owned, row.stock)).collect();
        let views = contract.shared.iter().map(|view| Ok((address(&view.address)?, view.clone()))).collect::<Result<Vec<_>, String>>()?;
        require(views.len() <= 16 && views.iter().all(|(start, view)| view.bytes <= 4096 && start.checked_add(view.bytes).is_some()), "time view bound")?;
        Ok(Self { contract, sites, return_sites: BTreeSet::new(), normalized, frames: Vec::new(), views,
            bytes: BTreeMap::new(), messages: BTreeMap::new(), writer: BufWriter::new(File::create(output).map_err(|error| error.to_string())?),
            watch: None, maps: None, kernel_bound: false, rows: VecDeque::new(), visited: BTreeMap::new(), retired_body_fetches: BTreeMap::new(), events: 0 })
    }
    pub fn raw_sites(&self) -> BTreeMap<u32, (String, Vec<u8>)> { self.sites.iter().map(|(pc, site)| (*pc, site.raw())).collect() }
    pub fn site_addresses(&self) -> impl Iterator<Item = u32> + '_ { self.sites.keys().copied() }
    pub fn require_owned(&self) -> Result<(), String> { require(self.retired_body_fetches.is_empty(), "retired time service body executed") }
    fn contains(ranges: &[[u32; 2]], pc: u32) -> bool { ranges.iter().any(|[start, end]| *start <= pc && pc < *end) }
    fn normalized(&self, word: u32) -> u32 { self.normalized.get(&word).copied().unwrap_or(word) }
    fn internal_target(&self, target: u32) -> bool {
        Self::contains(if self.contract.retired_extents.is_empty() { &self.contract.extents } else { &self.contract.retired_extents }, self.normalized(target))
    }
    fn emit(&mut self, event: Event) -> Result<(), String> {
        require(self.events < EVENT_LIMIT, "ordered time interface event bound")?;
        serde_json::to_writer(&mut self.writer, &event).map_err(|error| error.to_string())?;
        self.writer.write_all(b"\n").map_err(|error| error.to_string())?;
        self.events += 1;
        Ok(())
    }
    fn read(machine: &Machine, core: usize, address: u32, bytes: usize) -> Result<Vec<u8>, String> {
        require(bytes <= 4096 && address.checked_add(bytes as u32).is_some(), "interface read bound")?;
        let mut value = vec![0; bytes];
        machine.session.board.read_memory(core, u64::from(address), &mut value)?;
        Ok(value)
    }
    fn word(machine: &Machine, core: usize, address: u32) -> Result<u32, String> {
        let mut bytes = [0; 4];
        machine.session.board.read_memory(core, u64::from(address), &mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }
    fn arguments(&self, machine: &Machine, hit: &Hit, indices: &[u8], codes: &[usize], reads: &[ReadSpec], payload: Option<&InlinePayload>) -> Result<(Vec<u32>, Vec<u8>), String> {
        let mut words = indices.iter().enumerate().map(|(index, register_index)| {
            register(hit, *register_index).map(|word| if codes.contains(&index) { self.normalized(word) } else { word })
        }).collect::<Result<Vec<_>, _>>()?;
        for read in reads {
            let base = register(hit, read.register)?;
            for offset in &read.offsets {
                let word = Self::word(machine, hit.core, base.checked_add(*offset).ok_or("interface pointer overflow")?)?;
                words.push(if read.code_offsets.contains(offset) { self.normalized(word) } else { word });
            }
        }
        let data = match payload {
            Some(payload) => {
                let base = register(hit, payload.register)?;
                let length = Self::word(machine, hit.core, base.checked_add(payload.length_offset).ok_or("payload length overflow")?)?;
                require(length <= payload.maximum && length <= 4096, "interface payload bound")?;
                Self::read(machine, hit.core, base.checked_add(payload.data_offset).ok_or("payload data overflow")?, length as usize)?
            }
            None => Vec::new(),
        };
        Ok((words, data))
    }
    fn from_time(&self, hit: &Hit) -> Result<bool, String> {
        let key = context(hit);
        Ok(Self::contains(&self.contract.extents, address(&hit.lr)?) || self.frames.iter().any(|frame| frame.track && frame.key == key))
    }
    pub fn observe(&mut self, machine: &Machine, hit: &Hit) -> Result<BTreeMap<u32, String>, String> {
        let pc = address(&hit.address)?;
        let sp = address(&hit.sp)?;
        let key = context(hit);
        let mut added = BTreeMap::new();
        while let Some(index) = self.frames.iter().rposition(|frame| frame.key == key && frame.sp == sp && frame.resume == pc) {
            let frame = self.frames.remove(index);
            if let Some(label) = frame.label {
                let status = register(hit, 0)?;
                let outputs = if status == 0 {
                    frame.outputs.iter().map(|pointer| Self::word(machine, hit.core, *pointer)).collect::<Result<Vec<_>, _>>()?
                } else { Vec::new() };
                let message = if status == 0 {
                    if let Some(pointer) = frame.block_output {
                        let block = Self::word(machine, hit.core, pointer)?;
                        let header = Self::word(machine, hit.core, block.checked_add(4).ok_or("message header overflow")?)?;
                        let length = Self::word(machine, hit.core, block.checked_add(8).ok_or("message length overflow")?)?;
                        require(length <= 4, "time queue message exceeds its four-byte payload")?;
                        let message = Message { header, payload: Self::read(machine, hit.core, block.checked_add(12).ok_or("message payload overflow")?, length as usize)? };
                        if let Some(queue) = frame.queue { self.messages.insert(queue, message.clone()); }
                        Some(message)
                    } else { None }
                } else { None };
                self.emit(Event::Return { actor: key, entry: label, status, outputs, message })?;
            }
        }
        if let Some(site) = self.sites.get(&pc).cloned() {
            let mut frame = None;
            match site {
                Site::Callback(callback) => {
                    let target = register(hit, callback.register)?;
                    if !self.internal_target(target) { self.emit(Event::Callback { actor: key, target: self.normalized(target) })?; }
                }
                Site::Boundary(entry) | Site::Request(entry) => {
                    let boundary = matches!(self.sites.get(&pc), Some(Site::Boundary(_)));
                    let admitted = if boundary { !Self::contains(&self.contract.extents, address(&hit.lr)?) } else { self.from_time(hit)? };
                    if admitted {
                        let label = entry.interface_label.clone().unwrap_or(entry.label);
                        if !entry.track {
                            let (arguments, payload) = self.arguments(machine, hit, &entry.arguments, &entry.code_arguments, &entry.reads, entry.payload.as_ref())?;
                            self.emit(Event::Call { actor: key, entry: label.clone(), arguments, payload })?;
                        }
                        if entry.returns || entry.track {
                            frame = Some(Frame { key, sp, resume: address(&hit.lr)?, label: if entry.track { None } else { Some(label) }, track: true,
                                outputs: entry.outputs.iter().map(|index| register(hit, *index)).collect::<Result<Vec<_>, _>>()?, block_output: None, queue: None });
                        }
                    }
                }
                Site::Slot(slot) => {
                    let queue = (register(hit, 0)? & 0xffff) as u16;
                    let admitted = if slot.queues.is_empty() { self.from_time(hit)? } else { slot.queues.contains(&queue) };
                    if admitted {
                        let (arguments, payload) = self.arguments(machine, hit, &slot.arguments, &[], &slot.reads, slot.payload.as_ref())?;
                        self.emit(Event::Call { actor: key, entry: slot.label.clone(), arguments, payload })?;
                        frame = Some(Frame { key, sp, resume: address(&hit.lr)?, label: Some(slot.label), track: false, outputs: Vec::new(),
                            block_output: slot.block_output.map(|index| register(hit, index)).transpose()?, queue: Some(queue) });
                    }
                }
            }
            if let Some(frame) = frame {
                require(self.frames.len() < FRAME_LIMIT, "time interface invocation bound")?;
                let resume = frame.resume;
                require(resume & 3 == 0, "time interface return is not A32")?;
                self.frames.push(frame);
                if !self.sites.contains_key(&resume) && self.return_sites.insert(resume) {
                    require(self.sites.len() + self.return_sites.len() < SITE_LIMIT, "time return site bound")?;
                    added.insert(resume, "time-return".into());
                }
            }
        }
        added.extend(self.kernel_sites(machine)?);
        Ok(added)
    }
    fn kernel_sites(&mut self, machine: &Machine) -> Result<BTreeMap<u32, String>, String> {
        let mut added = BTreeMap::new();
        if self.kernel_bound { return Ok(added); }
        let Some(root) = &self.contract.kernel_root_table else { return Ok(added); };
        let root = address(root)?;
        let mut ready = true;
        for core in 0..2 {
            let block = Self::word(machine, core, root + 4 * core as u32)?;
            if block == 0 { ready = false; continue; }
            for slot in &self.contract.slots {
                let pc = Self::word(machine, core, block.checked_add(slot.offset).ok_or("kernel slot overflow")?)?;
                require(pc != 0 && pc & 3 == 0, "invalid kernel interface slot")?;
                if !self.sites.contains_key(&pc) {
                    require(!self.return_sites.contains(&pc), "kernel slot overlaps a return point")?;
                    self.sites.insert(pc, Site::Slot(slot.clone()));
                    added.insert(pc, "time-interface".into());
                }
            }
        }
        self.kernel_bound = ready;
        Ok(added)
    }
    pub fn begin_watch(&mut self, machine: &Machine) -> Result<(), String> {
        if self.bytes.is_empty() {
            for (base, view) in &self.views {
                for (offset, byte) in Self::read(machine, 0, *base, view.bytes as usize)?.into_iter().enumerate() { self.bytes.insert(*base + offset as u32, byte); }
            }
        }
        let mut ranges = self.contract.extents.iter().map(|[start, end]| WatchRange { start: u64::from(*start), end: u64::from(*end), kinds: WatchKinds::CPU_FETCH }).collect::<Vec<_>>();
        ranges.extend(self.contract.retired_extents.iter().map(|[start, end]| WatchRange { start: u64::from(*start), end: u64::from(*end), kinds: WatchKinds::CPU_FETCH }));
        ranges.extend(self.views.iter().map(|(base, view)| WatchRange { start: u64::from(*base), end: u64::from(*base + view.bytes), kinds: WatchKinds::CPU_WRITE }));
        if self.maps.is_none() { self.maps = Some(run::memory_maps(&machine.document)?); }
        self.watch = Some(machine.session.board.memory_watch_begin_classified(ranges, ROW_LIMIT, self.maps.as_ref().ok_or("missing time memory maps")?.clone(), ViewPolicy::Observe)?);
        Ok(())
    }
    fn stored_word(&self, address: u32) -> Result<u32, String> {
        let mut bytes = [0u8; 4];
        for (offset, byte) in bytes.iter_mut().enumerate() { *byte = *self.bytes.get(&(address + offset as u32)).ok_or("missing shared view byte")?; }
        Ok(u32::from_le_bytes(bytes))
    }
    pub fn end_watch(&mut self, machine: &Machine) -> Result<(), String> {
        let Some(mut watch) = self.watch.take() else { return Err("time watch is not active".into()); };
        let batch = serde_json::from_value::<Batch>(watch.records.borrow().state()).map_err(|error| error.to_string());
        let ended = machine.session.board.memory_watch_end(&mut watch);
        let batch = batch?;
        ended?;
        require(batch.enabled && batch.violation.is_none() && batch.records.len() <= ROW_LIMIT, "time interface watch overflowed or refused")?;
        require(self.rows.is_empty() && batch.records.iter().all(|row| row.instructions.is_some()), "time watch rows lack instruction order")?;
        require(batch.records.windows(2).all(|pair| pair[0].instructions <= pair[1].instructions), "time watch instruction order decreased")?;
        self.rows.extend(batch.records);
        Ok(())
    }
    pub fn before_hit(&mut self, instruction: u64) -> Result<(), String> {
        while self.rows.front().is_some_and(|row| row.instructions.is_some_and(|clock| clock <= instruction)) {
            let row = self.rows.pop_front().ok_or("time watch row disappeared")?;
            self.apply_row(row)?;
        }
        Ok(())
    }
    pub fn after_hits(&mut self) -> Result<(), String> {
        while let Some(row) = self.rows.pop_front() { self.apply_row(row)?; }
        Ok(())
    }
    fn apply_row(&mut self, row: Row) -> Result<(), String> {
        let at = u32::try_from(row.address).map_err(|_| "time watch address exceeds 32 bits")?;
        match row.source {
            Access::CpuFetch => {
                *self.visited.entry(at).or_default() += 1;
                if Self::contains(&self.contract.retired_extents, at) && !self.contract.retired_entries.contains(&at) { *self.retired_body_fetches.entry(at).or_default() += 1; }
            }
            Access::CpuWrite => {
                let pc = u32::try_from(row.pc.ok_or("time store lacks writer PC")?).map_err(|_| "time writer exceeds 32 bits")?;
                if !(Self::contains(&self.contract.extents, pc) || Self::contains(&self.contract.effect_extents, pc)) { return Ok(()); }
                require(row.core.is_some() && row.size <= 8, "time store attribution or width missing")?;
                let value = row.value.ok_or("time store lacks value")?;
                let end = at.checked_add(row.size as u32).ok_or("time store address overflow")?;
                let Some((base, view)) = self.views.iter().find(|(base, view)| *base <= at && end <= *base + view.bytes).cloned() else { return Err("store leaves declared time views".into()); };
                for offset in 0..row.size { self.bytes.insert(at + offset as u32, (value >> (8 * offset)) as u8); }
                let offset = at - base;
                match view.kind {
                    ViewKind::Counter | ViewKind::Interrupts => {
                        require(row.size == 4 && offset & 3 == 0, "time scalar store width differs")?;
                        self.emit(Event::Store { view: base, offset, value: value as u32 })?;
                    }
                    ViewKind::Message if offset == 4 && row.size == 4 => {
                        let queue = view.queue.ok_or("message view lacks queue")?;
                        let header = self.stored_word(base)?;
                        let length = self.stored_word(base + 4)?;
                        let payload = self.messages.get(&queue).map(|message| message.payload.clone()).unwrap_or_default();
                        require(length as usize == payload.len(), "message publication lacks captured receive payload")?;
                        self.emit(Event::Message { queue, header, length, payload })?;
                    }
                    ViewKind::Message if offset >= 12 => {
                        require(row.size == 4 && offset & 3 == 0, "time control store width differs")?;
                        let value = if offset >= 16 { self.normalized(value as u32) } else { value as u32 };
                        self.emit(Event::Store { view: base, offset, value })?;
                    }
                    ViewKind::Message => {}
                }
            }
        }
        Ok(())
    }
    pub fn finish(&mut self) -> Result<Coverage, String> {
        self.after_hits()?;
        self.writer.flush().map_err(|error| error.to_string())?;
        Ok(Coverage { fetch_receipts: self.contract.extents.iter().map(|[start, end]| FetchReceipt { start: *start, end: *end,
            fetches: self.visited.range(*start..*end).map(|(_, count)| *count).sum() }).collect(), visited: self.visited.clone(), retired_body_fetches: self.retired_body_fetches.clone(), events: self.events, pending: self.frames.len() })
    }
}

pub fn owned_plan(contract_path: &Path, symbols_path: &Path, output: &Path) -> Result<(), String> {
    for path in [contract_path, symbols_path] {
        require(std::fs::metadata(path).map_err(|error| error.to_string())?.len() <= 4 * 1024 * 1024, "time plan input bound")?;
    }
    let mut contract: Contract = serde_json::from_slice(&fujimod_xm1::bridge::read(contract_path)?).map_err(|error| error.to_string())?;
    let bytes = fujimod_xm1::bridge::read(symbols_path)?;
    let text = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
    let mut symbols = BTreeMap::new();
    for line in text.lines() {
        let words = line.split_whitespace().collect::<Vec<_>>();
        if words.len() == 4 && (words[3].starts_with("xm1_time_") || words[3].starts_with("__fixed_body_")) {
            let pc = u32::from_str_radix(words[0], 16).map_err(|error| error.to_string())?;
            require(symbols.insert(words[3].to_owned(), pc).is_none(), "duplicate time symbol")?;
        }
    }
    let symbol = |name: &str| symbols.get(name).copied().ok_or_else(|| format!("missing time symbol {name}"));
    contract.retired_extents = contract.extents;
    contract.extents = vec![[symbol("__fixed_body_start")?, symbol("__fixed_body_end")?]];
    contract.retired_entries = contract.entries.iter().map(|entry| address(&entry.address)).collect::<Result<Vec<_>, _>>()?;
    let mut normalized = BTreeMap::new();
    let mut entries = BTreeSet::new();
    for entry in &mut contract.entries {
        let pc = symbol(entry.symbol.as_deref().ok_or("time entry lacks owned symbol")?)?;
        normalized.entry(pc).or_insert(address(&entry.address)?);
        entry.address = format!("0x{pc:08x}");
        entry.label = entry.interface_label.clone().unwrap_or_else(|| entry.label.clone());
    }
    contract.entries.retain(|entry| entries.insert(entry.address.clone()));
    let mut callbacks = BTreeSet::new();
    for callback in &mut contract.callbacks {
        let pc = symbol(callback.symbol.as_deref().ok_or("callback lacks owned symbol")?)?;
        callback.address = format!("0x{pc:08x}");
        callback.label = "callback".into();
        callback.register = 0;
        callback.owned_only = false;
    }
    contract.callbacks.retain(|callback| callbacks.insert(callback.address.clone()));
    contract.normalization = normalized.into_iter().map(|(owned, stock)| Normalization { owned, stock }).collect();
    fujimod_xm1::bridge::write(output, &serde_json::to_vec_pretty(&contract).map_err(|error| error.to_string())?)
}
