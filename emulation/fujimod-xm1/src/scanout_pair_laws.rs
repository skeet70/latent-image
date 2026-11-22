//! Display scanout cutover observations: the records one role writes, the laws each role's records must satisfy
//! and the stock/owned comparison. Laws come from reports/display-render-contracts/scanout.md (image-4 bytes).

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Retired stock bodies of image 4, half-open, at x70view 55436fc8 `function-ranges.json` extents.
pub const CARRIERS: [(u32, u32); 5] = [(0x6c6eb0, 0x6c75f0), (0x6c926c, 0x6c994c), (0x6c9cf4, 0x6ca04c), (0x6ca3e0, 0x6cb0ec), (0x6cb8b4, 0x6cb9cc)];
/// The engine-selection wrapper `0x6d03b8` that VGLIB's hand-off `0x6d76ec` calls first; its `bl 0x6c9cf4` returns to `0x6d0408`.
pub const VGLIB_PUBLICATION: (u32, u32) = (0x6d03b8, 0x6d040c);
pub const SELECTION: u32 = 0x955af8;
pub const FRONT: u32 = 0x8e21f8;
pub const STAGE_IMAGES: u32 = 0x8e2158;
pub const SCHEDULER: u32 = 0x917ed0;

pub const fn runtime(engine: u32) -> u32 { 0x9553cc + engine * 0x58 }
pub const fn saved(engine: u32) -> u32 { 0x96b9bc + engine * 0x1a4 }
pub const fn input(engine: u32) -> u32 { 0x9554b8 + engine * 0x23c }
pub const fn timing(engine: u32) -> u32 { 0x9429f4 + engine * 0x58 }
pub const fn scanout_bank(engine: u32) -> u32 { if engine == 0 { 0xfff6b000 } else { 0xfff6e000 } }

pub fn in_carriers(address: u32) -> bool { CARRIERS.iter().any(|(low, high)| *low <= address && address < *high) }

/// Core 0's ATCM window; its top is the first-level IRQ stack `0x40250` loads from `0x40370`.
pub const IRQ_STACK: (u32, u32) = (0x7000_0000, 0x7000_2000);

/// Words whose changes a call reports and whose entry values it is compared on: runtime, input, saved, timing and
/// scheduler records, the blanking selector, the engine selection and VGLIB's front byte and stage-0 images.
pub const REGIONS: [(u32, u32); 10] = [(runtime(0), 0xb0), (input(0), 0x478), (saved(0), 0x348), (timing(0), 0xb0), (SCHEDULER, 0x94),
    (0x916610, 4), (SELECTION + 0x10, 4), (SELECTION + 0x28, 4), (FRONT & !3, 4), (STAGE_IMAGES, 8)];

pub fn region_addresses() -> impl Iterator<Item = u32> {
    REGIONS.into_iter().flat_map(|(start, length)| (0..length / 4).map(move |word| start + 4 * word))
}

pub fn in_regions(address: u32) -> bool { REGIONS.iter().any(|(start, length)| *start <= address && address < start + length) }

/// The execution context a call ran in: on the IRQ stack, or on a task's stack.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Context { Interrupt, Task }

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Entry { IsrOne, IsrZero, SchedulerRegister, Capture, ItemConfiguration, RangePacket, RangeEnabled, GroupOptions, Commit, CommitZero, CommitOne, Blank }

impl Entry {
    pub const ALL: [Entry; 12] = [Self::IsrOne, Self::IsrZero, Self::SchedulerRegister, Self::Capture, Self::ItemConfiguration, Self::RangePacket,
        Self::RangeEnabled, Self::GroupOptions, Self::Commit, Self::CommitZero, Self::CommitOne, Self::Blank];
    pub const fn address(self) -> u32 {
        match self {
            Self::IsrOne => 0x6c6eb0, Self::IsrZero => 0x6c70e4, Self::SchedulerRegister => 0x6c7318, Self::Capture => 0x6c926c,
            Self::ItemConfiguration => 0x6c9cf4, Self::RangePacket => 0x6c9d44, Self::RangeEnabled => 0x6c9e7c, Self::GroupOptions => 0x6c9f70,
            Self::Commit => 0x6ca3e0, Self::CommitZero => 0x6cb0c4, Self::CommitOne => 0x6cb0d8, Self::Blank => 0x6cb8b4,
        }
    }
    pub fn from_address(address: u32) -> Option<Self> { Self::ALL.into_iter().find(|entry| entry.address() == address) }
    pub const fn arity(self) -> usize {
        match self {
            Self::IsrOne | Self::IsrZero | Self::CommitZero | Self::CommitOne => 0,
            Self::Commit => 1,
            Self::SchedulerRegister | Self::Capture | Self::Blank => 2,
            Self::GroupOptions => 3,
            Self::ItemConfiguration | Self::RangePacket | Self::RangeEnabled => 4,
        }
    }
    /// Entries whose stock r0 at return is a defined result.
    pub const fn returns(self) -> bool {
        matches!(self, Self::SchedulerRegister | Self::Capture | Self::ItemConfiguration | Self::RangePacket | Self::RangeEnabled | Self::GroupOptions)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Caller { Unit, Vglib, Retained }
impl Caller {
    /// `body` is the owned fixed-range image; a return into it or into a retired body is a call the unit made.
    pub fn of(return_address: u32, body: (u32, u32)) -> Self {
        if in_carriers(return_address) || (body.0..body.1).contains(&return_address) { Self::Unit }
        else if (VGLIB_PUBLICATION.0..VGLIB_PUBLICATION.1).contains(&return_address) { Self::Vglib }
        else { Self::Retained }
    }
}

impl Entry {
    pub fn committed_engine(self, arguments: &[u32]) -> Option<u32> {
        match self { Self::Commit => arguments.first().copied(), Self::CommitZero => Some(0), Self::CommitOne => Some(1), _ => None }
    }
    pub const fn drains(self) -> bool { matches!(self, Self::Commit | Self::CommitZero | Self::CommitOne | Self::IsrOne | Self::IsrZero) }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Bus { pub address: u32, pub write: bool, pub value: u32 }

/// One callback the unit body invoked: its target, the drain-list words when it was entered and when it returned.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Callback { pub target: u32, pub live: Vec<u32>, pub after: Option<Vec<u32>> }

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FrontState { pub front: u8, pub image_address: u32, pub shadow: u32 }

/// One entry into a retired stock address.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Call {
    pub entry: Entry,
    pub caller: Caller,
    /// The outermost of directly nested commit-family entries; it carries the commit's bus rows, drain and samples.
    pub head: bool,
    pub engine: Option<u32>,
    pub arguments: Vec<u32>,
    pub result: Option<u32>,
    /// `0x955af8` words `+0x10` and `+0x28` at entry.
    pub selection: [u32; 2],
    /// Drain lists at entry: a commit's first then second six slots, or the scheduler's one-shot then persistent callbacks.
    pub slots: Vec<u32>,
    /// Commit processing word `R(e)+0x0c` at entry.
    pub pending: u32,
    pub front: Option<FrontState>,
    /// A commit's saved image `S(e)` when its register writes ended.
    pub saved: Vec<u32>,
    /// Count of leading `bus` rows before the first callback.
    pub before_drain: usize,
    pub bus: Vec<Bus>,
    pub callbacks: Vec<Callback>,
    /// Changed words of the runtime, saved, input, timing and scheduler records: `[address, before, after]`.
    pub changes: Vec<[u32; 3]>,
    /// `0x6c9cf4`'s target shadow word before and after, when the item is valid.
    pub item: Option<[u32; 2]>,
    pub returned: bool,
    /// `0x6c7318`'s four request words at entry and its output word at return; its two arguments are caller stack addresses.
    #[serde(default)]
    pub request: Option<[u32; 4]>,
    #[serde(default)]
    pub output: Option<u32>,
    /// The caller's `sp` at entry.
    #[serde(default)]
    pub stack: u32,
    /// The `REGIONS` words at entry, in `region_addresses` order.
    #[serde(default)]
    pub entry_words: Vec<u32>,
    /// `REGIONS` word addresses the CPU read (`reads`) and wrote (`writes`) while this call was open in its own context on its core.
    #[serde(default)]
    pub reads: BTreeSet<u32>,
    #[serde(default)]
    pub writes: BTreeSet<u32>,
    /// The instruction counter at entry and at return.
    #[serde(default)]
    pub instructions: [u64; 2],
    /// The board tick at entry and at return.
    #[serde(default)]
    pub ticks: [u64; 2],
    /// Core 0's and core 1's instruction counters at entry.
    #[serde(default)]
    pub core_instructions: [u64; 2],
}

impl Call {
    pub fn context(&self) -> Context {
        if (IRQ_STACK.0..IRQ_STACK.1).contains(&self.stack) { Context::Interrupt } else { Context::Task }
    }
    /// Calls a unit body made into the unit's own entries are not compared unless they head a commit, which is how
    /// an ISR's persistent commit callback appears in both roles.
    pub fn compared(&self) -> bool { self.caller != Caller::Unit || self.head }
}

pub fn selected_engine(selection: [u32; 2]) -> u32 { u32::from(selection[1] == 0 && selection[0] == 1) }

/// The commit's saved-image to register map, in emission order: `(saved offset, register offset)`.
pub fn register_map(engine: u32) -> Vec<(u32, u32)> {
    let mut map: Vec<(u32, u32)> = (0..6).map(|n| (n * 4, n * 4)).collect();
    if engine == 1 { map.push((0x18, 0x18)); }
    map.extend([(0x1c, 0x1c), (0x20, 0x20), (0x24, 0x38), (0x28, 0x3c), (0x2c, 0x88), (0x30, 0x8c)]);
    map.extend([(0x34, 0x24), (0x38, 0x28), (0x3c, 0x2c), (0x40, 0x30), (0x44, 0xd4), (0x48, 0xd8), (0x4c, 0xdc)]);
    map.extend([(0x50, 0x34), (0x54, 0x40), (0x58, 0xe0), (0x9c, 0x84), (0xa0, 0x90), (0xa4, 0xe4)]);
    for n in 0..4 {
        map.extend([(0x5c + 16 * n, 0x44 + 4 * n), (0x60 + 16 * n, 0x54 + 4 * n), (0x64 + 16 * n, 0x64 + 4 * n), (0x68 + 16 * n, 0x74 + 4 * n)]);
        map.extend([(0xa8 + 16 * n, 0x94 + 4 * n), (0xac + 16 * n, 0xa4 + 4 * n), (0xb0 + 16 * n, 0xb4 + 4 * n), (0xb4 + 16 * n, 0xc4 + 4 * n)]);
    }
    map.extend([(0xec, 0x10c), (0xf0, 0x110), (0x174, 0x194), (0xf4, 0x114), (0xf8, 0x118), (0x178, 0x198)]);
    for n in 0..10 { map.extend([(0xfc + 8 * n, 0x11c + 8 * n), (0x100 + 8 * n, 0x120 + 8 * n)]); }
    map.extend([(0x14c, 0x16c), (0x150, 0x170), (0x154, 0x174)]);
    for n in 0..4 { map.push((0x158 + 4 * n, 0x178 + 4 * n)); }
    map.extend([(0x168, 0x188), (0x16c, 0x18c), (0x170, 0x190), (0xe8, 0x108)]);
    map.extend([(0x17c, 0xe8), (0x180, 0xec), (0x184, 0xf0), (0x188, 0xf4), (0x18c, 0xf8), (0x190, 0xfc), (0x194, 0x100), (0x198, 0x1bc), (0x19c, 0x9d0), (0x1a0, 0x9d4)]);
    map
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "law", rename_all = "kebab-case")]
pub enum Finding {
    /// A pending commit's writes before its drain differ from the saved image under the offset map.
    RegisterSequence { call: usize, position: usize, expected: Option<[u32; 2]>, observed: Option<[u32; 2]> },
    /// A commit wrote scanout registers while nothing was pending.
    UnpendingWrite { call: usize },
    /// The callbacks a commit invoked differ from its live drain lists, or a slot was still set when its callback ran.
    Drain { call: usize, callback: Option<usize>, detail: String },
    /// VGLIB's publication reached `0x6c9cf4` on an engine other than the `0x955af8` selection.
    Selection { call: usize, engine: u32, selected: u32 },
    /// `0x6c9cf4` did not keep bits 31:30 and replace the low 30 bits, or returned a wrong status.
    ItemWord { call: usize },
    /// At a commit of the selected engine, shadow `+0xb0` names neither the front buffer nor the buffer VGLIB published last.
    FrontStep { call: usize, front: FrontState },
}

const DRAIN_SLOTS: usize = 12;

fn register_law(index: usize, call: &Call, findings: &mut Vec<Finding>) {
    let Some(engine) = call.engine else { return };
    let bank = scanout_bank(engine);
    let writes: Vec<[u32; 2]> = call.bus[..call.before_drain.min(call.bus.len())].iter()
        .filter(|bus| bus.write && (bank..bank + 0x1000).contains(&bus.address)).map(|bus| [bus.address - bank, bus.value]).collect();
    if call.pending != 1 {
        if !writes.is_empty() { findings.push(Finding::UnpendingWrite { call: index }); }
        return;
    }
    let expected: Vec<[u32; 2]> = register_map(engine).into_iter()
        .map(|(source, register)| [register, call.saved.get(source as usize / 4).copied().unwrap_or(u32::MAX)]).collect();
    if let Some(position) = (0..expected.len().max(writes.len())).find(|position| expected.get(*position) != writes.get(*position)) {
        findings.push(Finding::RegisterSequence { call: index, position, expected: expected.get(position).copied(), observed: writes.get(position).copied() });
    }
}

/// Replays the drain over live slot words: the next set slot at or after the cursor must be the next callback,
/// cleared when it runs; after the last callback no set slot may remain ahead of the cursor.
fn drain_law(index: usize, call: &Call, findings: &mut Vec<Finding>) {
    let mut state = call.slots.clone();
    let mut cursor = 0;
    let mut fail = |callback: Option<usize>, detail: String| findings.push(Finding::Drain { call: index, callback, detail });
    if state.len() != DRAIN_SLOTS { return fail(None, "drain lists were not sampled at entry".into()); }
    for (number, callback) in call.callbacks.iter().enumerate() {
        let Some(slot) = (cursor..DRAIN_SLOTS).find(|slot| state[*slot] != 0) else {
            return fail(Some(number), format!("callback {:#x} has no set slot at or after {cursor}", callback.target));
        };
        if state[slot] != callback.target { return fail(Some(number), format!("slot {slot} holds {:#x}, callback ran {:#x}", state[slot], callback.target)); }
        if callback.live.get(slot) != Some(&0) { return fail(Some(number), format!("slot {slot} was still set when its callback ran")); }
        let Some(after) = &callback.after else { return fail(Some(number), "callback did not return".into()) };
        if after.len() != DRAIN_SLOTS { return fail(Some(number), "drain lists were not sampled at callback return".into()); }
        state.clone_from(after);
        cursor = slot + 1;
    }
    if let Some(slot) = (cursor..DRAIN_SLOTS).find(|slot| state[*slot] != 0) {
        fail(None, format!("slot {slot} still holds {:#x} after the drain", state[slot]));
    }
}

/// Role-local laws over one role's calls, in entry order.
pub fn laws(calls: &[Call]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut published: Option<u32> = None;
    for (index, call) in calls.iter().enumerate() {
        match call.entry {
            Entry::Commit | Entry::CommitZero | Entry::CommitOne if call.returned && call.head => {
                register_law(index, call, &mut findings);
                drain_law(index, call, &mut findings);
                if let (Some(front), Some(engine)) = (call.front, call.engine) {
                    let low = |word: u32| word & 0x3fff_ffff;
                    let explained = low(front.shadow) == low(front.image_address)
                        || published.map_or(low(front.shadow) == 0, |value| low(value) == low(front.shadow));
                    if engine == selected_engine(call.selection) && !explained { findings.push(Finding::FrontStep { call: index, front }); }
                }
            }
            Entry::ItemConfiguration if call.returned => {
                let [engine, group, item, value] = <[u32; 4]>::try_from(call.arguments.as_slice()).unwrap_or([u32::MAX; 4]);
                if call.caller == Caller::Vglib {
                    let selected = selected_engine(call.selection);
                    if engine != selected { findings.push(Finding::Selection { call: index, engine, selected }); }
                    if group == 1 && item == 0 { published = Some(value); }
                }
                let lawful = if item > 3 { call.result == Some((-30i32) as u32) }
                    else if engine > 1 || group > 1 { true }
                    else { matches!((call.item, call.result), (Some([before, after]), Some(0)) if after == (before & 0xc000_0000) | (value & 0x3fff_ffff)) };
                if !lawful { findings.push(Finding::ItemWord { call: index }); }
            }
            _ => {}
        }
    }
    findings
}

/// The fields of a boundary call that the stock/owned comparison requires equal; its record words compare through
/// `word_difference`.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub struct Projection<'a> {
    pub entry: Entry,
    pub arguments: &'a [u32],
    pub request: Option<[u32; 4]>,
    pub output: Option<u32>,
    pub result: Option<u32>,
    pub selection: [u32; 2],
    pub slots: &'a [u32],
    pub pending: u32,
    pub front: Option<FrontState>,
    pub bus: Vec<Bus>,
    pub callbacks: &'a [Callback],
    pub returned: bool,
}

/// `bus` with each run of identical consecutive reads kept once: a status poll's iteration count follows its loop's
/// instruction count, not the device.
pub fn collapsed_polls(bus: &[Bus]) -> Vec<Bus> {
    let mut rows: Vec<Bus> = Vec::with_capacity(bus.len());
    for row in bus {
        if !row.write && rows.last() == Some(row) { continue; }
        rows.push(row.clone());
    }
    rows
}

/// Cells holding handles the retained interrupt service `0x64e918` issues from its global counter: both ISR handle
/// cells and each engine's registration handle.
pub const HANDLE_CELLS: [u32; 4] = [SCHEDULER, SCHEDULER + 4, runtime(0) + 0x14, runtime(1) + 0x14];

/// One role's handle values numbered in order of first appearance; two roles compare equal exactly when one
/// consistent bijection maps one role's handles onto the other's.
#[derive(Default)]
pub struct HandleNames(std::collections::BTreeMap<u32, u32>);
impl HandleNames {
    fn name(&mut self, value: u32) -> u32 {
        if value == 0 { return 0; }
        let next = u32::try_from(self.0.len()).unwrap_or(u32::MAX).saturating_add(1);
        *self.0.entry(value).or_insert(next)
    }
}

pub fn projection(call: &Call) -> Projection<'_> {
    Projection {
        entry: call.entry, arguments: if call.entry == Entry::SchedulerRegister { &[] } else { &call.arguments },
        request: call.request, output: call.output, result: if call.entry.returns() { call.result } else { None },
        selection: call.selection, slots: &call.slots, pending: call.pending, front: call.front, bus: collapsed_polls(&call.bus),
        callbacks: &call.callbacks, returned: call.returned,
    }
}

/// One context's compared calls in entry order.
pub fn in_context(calls: &[Call], context: Context) -> Vec<&Call> {
    calls.iter().filter(|call| call.compared() && call.context() == context).collect()
}

/// The deepest owned entry below the stock entry of the same kind at the same position of its context, in bytes.
pub fn stack_excess(stock: &[Call], owned: &[Call]) -> i64 {
    [Context::Interrupt, Context::Task].into_iter().flat_map(|context| {
        in_context(stock, context).into_iter().zip(in_context(owned, context)).filter(|(stock, owned)| stock.entry == owned.entry)
            .map(|(stock, owned)| i64::from(stock.stack) - i64::from(owned.stack))
    }).max().unwrap_or(0)
}

/// How two paired calls' record words differ. Words are `[address, stock, owned]`; handle cells compare as names.
#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum WordMismatch {
    /// A call carries no entry words.
    Unsampled,
    /// `inputs`: entry words either call read. `outputs`: words either call wrote, at return. `unwritten`: words one
    /// call wrote and changed that the other did not write.
    Differ { inputs: Vec<[u32; 3]>, outputs: Vec<[u32; 3]>, unwritten: Vec<u32> },
}

fn word_difference(stock: &Call, owned: &Call, names: &mut (HandleNames, HandleNames)) -> Option<WordMismatch> {
    let count = region_addresses().count();
    if stock.entry_words.len() != count || owned.entry_words.len() != count { return Some(WordMismatch::Unsampled); }
    let mut equal = |address: u32, left: u32, right: u32| {
        if HANDLE_CELLS.contains(&address) { names.0.name(left) == names.1.name(right) } else { left == right }
    };
    let returned = |call: &Call, index: usize, address: u32| {
        call.changes.iter().find(|change| change[0] == address).map_or(call.entry_words[index], |change| change[2])
    };
    let changed = |call: &Call, address: u32| call.changes.iter().any(|change| change[0] == address);
    let (mut inputs, mut outputs, mut unwritten) = (Vec::new(), Vec::new(), Vec::new());
    for (index, address) in region_addresses().enumerate() {
        if (stock.reads.contains(&address) || owned.reads.contains(&address)) && !equal(address, stock.entry_words[index], owned.entry_words[index]) {
            inputs.push([address, stock.entry_words[index], owned.entry_words[index]]);
        }
        if stock.writes.contains(&address) || owned.writes.contains(&address) {
            let (left, right) = (returned(stock, index, address), returned(owned, index, address));
            if !equal(address, left, right) { outputs.push([address, left, right]); }
            let one_sided = |wrote: &Call, other: &Call| wrote.writes.contains(&address) && changed(wrote, address) && !other.writes.contains(&address);
            if one_sided(stock, owned) || one_sided(owned, stock) { unwritten.push(address); }
        }
    }
    (!(inputs.is_empty() && outputs.is_empty() && unwritten.is_empty())).then_some(WordMismatch::Differ { inputs, outputs, unwritten })
}

#[derive(Debug, Eq, PartialEq, Serialize)]
pub struct Difference {
    pub context: Context,
    /// Position within the context's compared calls.
    pub boundary_call: usize,
    pub stock: Option<serde_json::Value>,
    pub owned: Option<serde_json::Value>,
    pub words: Option<WordMismatch>,
}

/// The first paired call, interrupt context first, whose projection or record words differ. Each context's calls
/// pair in order, compared on their projections, the words they read at entry and the words they wrote, not on
/// changes another context made during them. The order between contexts follows interrupt arrival against task
/// progress and is not compared. One handle renaming per role spans the interrupt calls, then the task calls.
pub fn first_difference(stock: &[Call], owned: &[Call]) -> Option<Difference> {
    let mut names = (HandleNames::default(), HandleNames::default());
    for context in [Context::Interrupt, Context::Task] {
        let (left, right) = (in_context(stock, context), in_context(owned, context));
        for index in 0..left.len().max(right.len()) {
            let (stock_call, owned_call) = (left.get(index).copied(), right.get(index).copied());
            let (stock_projection, owned_projection) = (stock_call.map(projection), owned_call.map(projection));
            let words = match (stock_call, owned_call) {
                (Some(stock), Some(owned)) if stock_projection == owned_projection => word_difference(stock, owned, &mut names),
                _ => None,
            };
            if stock_projection != owned_projection || words.is_some() {
                return Some(Difference {
                    context, boundary_call: index, words,
                    stock: stock_projection.and_then(|value| serde_json::to_value(value).ok()),
                    owned: owned_projection.and_then(|value| serde_json::to_value(value).ok()),
                });
            }
        }
    }
    None
}

/// The first position where the two roles' compared calls, in entry order across both contexts, name different entries.
pub fn cross_context_order(stock: &[Call], owned: &[Call]) -> Option<usize> {
    let order = |calls: &[Call]| calls.iter().filter(|call| call.compared()).map(|call| (call.entry, call.context())).collect::<Vec<_>>();
    let (left, right) = (order(stock), order(owned));
    (0..left.len().max(right.len())).find(|index| left.get(*index) != right.get(*index))
}

/// Compared calls and the instructions between their entry and return, per entry.
pub fn instruction_spans(calls: &[Call]) -> std::collections::BTreeMap<Entry, [u64; 2]> {
    let mut spans = std::collections::BTreeMap::new();
    for call in calls.iter().filter(|call| call.compared() && call.returned) {
        let span: &mut [u64; 2] = spans.entry(call.entry).or_default();
        span[0] += 1;
        span[1] += call.instructions[1].saturating_sub(call.instructions[0]);
    }
    spans
}

#[cfg(test)]
#[path = "scanout_pair_laws_tests.rs"]
mod tests;
