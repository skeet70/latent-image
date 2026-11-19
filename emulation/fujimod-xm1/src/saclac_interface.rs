use super::{Address, Machine, Stop, parse_hex, read_u32, require};
use fujimod_emulator::board::observation::{WatchKinds, WatchRange};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const EVENT_LIMIT: usize = 100_000;
const PAYLOAD_LIMIT: usize = 4096;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EntryKind { Callback, ClientReply, ServiceRequest }

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetRegister { R3, R4, R5, R6, R12 }
impl TargetRegister {
    fn name(self) -> &'static str {
        match self { Self::R3 => "r3", Self::R4 => "r4", Self::R5 => "r5", Self::R6 => "r6", Self::R12 => "r12" }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PointerIdentity { Transient, Shared }

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ArgumentDecl { Word, Memory { bytes: usize, identity: PointerIdentity } }

struct BoundaryEntry {
    address: u32,
    kind: EntryKind,
    target: Option<TargetRegister>,
    arguments: Vec<ArgumentDecl>,
}

#[derive(Debug, Deserialize)]
pub struct Entry {
    address: String,
    kind: EntryKind,
    target: Option<TargetRegister>,
    #[serde(default)]
    arguments: Vec<ArgumentDecl>,
}

#[derive(Debug, Deserialize)]
pub struct OwnedPost {
    address: String,
    instruction: String,
}

/// A Root replacement of a declared stock callback, bound from one variant's link: its first 16
/// code bytes select the variant, and entering it is observed as entering the stock callback.
#[derive(Debug, Deserialize)]
pub struct OwnedCallback {
    address: String,
    code: String,
    stock_entry: String,
}

/// Root's client call veneer, bound from one variant's link: target in r0, argument words at r1
/// with their count in r2, and in r3 an optional record argument as `(index + 1) | bytes << 8`.
#[derive(Debug, Deserialize)]
pub struct OwnedClient {
    address: String,
    code: String,
}

#[derive(Debug, Deserialize)]
pub struct Shared {
    start: String,
    end: String,
}

#[derive(Debug, Deserialize)]
pub struct Declaration {
    post: String,
    /// One entry per Root image variant, each bound from that variant's link; the loaded word selects the variant.
    #[serde(default)]
    owned_posts: Vec<OwnedPost>,
    #[serde(default)]
    owned_callbacks: Vec<OwnedCallback>,
    #[serde(default)]
    owned_clients: Vec<OwnedClient>,
    entries: Vec<Entry>,
    shared: Vec<Shared>,
    #[serde(default)]
    callers: Vec<Shared>,
}

pub struct Plan {
    post: u32,
    owned_posts: Vec<(u32, u32)>,
    /// Address, first 16 code bytes, stock callback entry.
    owned_callbacks: Vec<(u32, [u8; 16], u32)>,
    /// Address, first 16 code bytes.
    owned_clients: Vec<(u32, [u8; 16])>,
    entries: Vec<BoundaryEntry>,
    shared: Vec<(u32, u32)>,
    callers: Vec<(u32, u32)>,
}

impl Declaration {
    pub fn validate(self) -> Result<Plan, String> {
        require(self.entries.len() <= 512 && self.shared.len() <= 64, "interface declaration exceeds bounds")?;
        let post = parse_hex(&self.post)?;
        let owned_posts = self.owned_posts.iter()
            .map(|owned| Ok((parse_hex(&owned.address)?, parse_hex(&owned.instruction)?)))
            .collect::<Result<Vec<_>, String>>()?;
        require(owned_posts.len() <= 8, "owned interface posts exceed bounds")?;
        let mut owned_addresses = BTreeSet::new();
        require(owned_posts.iter().all(|&(address, _)| address & 3 == 0 && address != post && owned_addresses.insert(address)),
            "owned interface post is unaligned, repeated or duplicates stock")?;
        let owned_callbacks = self.owned_callbacks.iter()
            .map(|owned| Ok((parse_hex(&owned.address)?, code(&owned.code)?, parse_hex(&owned.stock_entry)?)))
            .collect::<Result<Vec<_>, String>>()?;
        let owned_clients = self.owned_clients.iter()
            .map(|owned| Ok((parse_hex(&owned.address)?, code(&owned.code)?)))
            .collect::<Result<Vec<_>, String>>()?;
        require(owned_callbacks.len() <= 64 && owned_clients.len() <= 8, "owned interface entries exceed bounds")?;
        require(owned_callbacks.iter().map(|&(address, _, _)| address).chain(owned_clients.iter().map(|&(address, _)| address))
            .all(|address| address & 3 == 0 && address != post && owned_addresses.insert(address)),
            "owned interface entry is unaligned, repeated or duplicates another site")?;
        let mut addresses = BTreeSet::from([post]);
        let mut entries = Vec::new();
        for entry in self.entries {
            let address = parse_hex(&entry.address)?;
            require(address % 4 == 0 && addresses.insert(address), "interface entry is unaligned or repeated")?;
            require(matches!((entry.kind, entry.target), (EntryKind::Callback | EntryKind::ServiceRequest, None) | (EntryKind::ClientReply, Some(_))), "interface reply target register missing or unexpected")?;
            require(entry.arguments.len() <= 12 && entry.arguments.iter().all(|argument| match argument {
                ArgumentDecl::Word => true,
                ArgumentDecl::Memory { bytes, .. } => (1..=PAYLOAD_LIMIT).contains(bytes),
            }), "interface reply arguments exceed bounds")?;
            require(matches!(entry.kind, EntryKind::Callback) || !entry.arguments.is_empty(), "interface reply arguments missing")?;
            entries.push(BoundaryEntry { address, kind: entry.kind, target: entry.target, arguments: entry.arguments });
        }
        let mut shared = Vec::new();
        for span in self.shared {
            let (start, end) = (parse_hex(&span.start)?, parse_hex(&span.end)?);
            require(start < end && end - start <= 65536, "interface shared span exceeds bounds")?;
            shared.push((start, end));
        }
        shared.sort_unstable();
        require(shared.windows(2).all(|pair| pair[0].1 <= pair[1].0), "interface shared spans overlap")?;
        let callers = self.callers.into_iter().map(|span| {
            let (start, end) = (parse_hex(&span.start)?, parse_hex(&span.end)?);
            require(start < end, "interface caller span is empty")?;
            Ok((start, end))
        }).collect::<Result<Vec<_>, String>>()?;
        require(callers.len() <= 16 && (!entries.iter().any(|entry| matches!(entry.kind, EntryKind::ServiceRequest)) || !callers.is_empty()), "interface service callers missing or excessive")?;
        require(entries.iter().all(|entry| !owned_addresses.contains(&entry.address)), "owned interface site duplicates a stock entry")?;
        require(owned_callbacks.iter().all(|&(_, _, stock)| entries.iter().any(|entry| entry.address == stock && matches!(entry.kind, EntryKind::Callback))),
            "owned interface callback names no declared stock callback")?;
        Ok(Plan { post, owned_posts, owned_callbacks, owned_clients, entries, shared, callers })
    }
}

impl Plan {
    pub fn sites(&self) -> Vec<u32> {
        let mut sites = Vec::with_capacity(self.entries.len() + 1 + self.owned_posts.len() + self.owned_callbacks.len() + self.owned_clients.len());
        sites.push(self.post);
        sites.extend(self.entries.iter().map(|entry| entry.address));
        sites.extend(self.owned_posts.iter().map(|&(address, _)| address));
        sites.extend(self.owned_callbacks.iter().map(|&(address, _, _)| address));
        sites.extend(self.owned_clients.iter().map(|&(address, _)| address));
        sites
    }
}

/// Sixteen code bytes written as 32 hexadecimal digits.
fn code(text: &str) -> Result<[u8; 16], String> {
    let digits = text.strip_prefix("0x").unwrap_or(text);
    require(digits.len() == 32 && digits.bytes().all(|byte| byte.is_ascii_hexdigit()), "owned interface code is not 16 bytes of hexadecimal")?;
    let mut code = [0; 16];
    for (index, byte) in code.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&digits[2 * index..2 * index + 2], 16).map_err(|error| error.to_string())?;
    }
    Ok(code)
}

fn post_event(queue: u8, header: u32, payload: Vec<u8>) -> Option<Event> {
    let operation = header as u16;
    let sender = (header >> 16) as u8;
    if queue == 43 { Some(Event::QueueIn { operation, sender, payload }) }
    else if sender == 43 { Some(Event::PeerRequest { queue, operation, payload }) }
    else { None }
}

impl Plan {
    fn owned_post_instruction(&self, address: u32) -> Option<u32> {
        self.owned_posts.iter().find(|&&(owned, _)| owned == address).map(|&(_, instruction)| instruction)
    }

    fn owned_callback(&self, address: u32) -> Option<([u8; 16], u32)> {
        self.owned_callbacks.iter().find(|&&(owned, _, _)| owned == address).map(|&(_, code, stock)| (code, stock))
    }

    fn owned_client(&self, address: u32) -> Option<[u8; 16]> {
        self.owned_clients.iter().find(|&&(owned, _)| owned == address).map(|&(_, code)| code)
    }
}

impl Plan {
    pub fn classes(&self) -> impl Iterator<Item = WatchRange> + '_ {
        self.shared.iter().map(|&(start, end)| WatchRange {
            start: u64::from(start), end: u64::from(end), kinds: WatchKinds::CPU_WRITE,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Argument {
    Word { value: u32 },
    Memory { address: Option<Address>, bytes: Vec<u8> },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Event {
    QueueIn { operation: u16, sender: u8, payload: Vec<u8> },
    PeerRequest { queue: u8, operation: u16, payload: Vec<u8> },
    Callback { entry: Address },
    ClientReply { entry: Address, target: Address, arguments: Vec<Argument> },
    ServiceRequest { entry: Address, arguments: Vec<Argument> },
    Store { address: Address, bytes: Vec<u8> },
}

#[derive(Serialize)]
pub struct Observed {
    pub instructions: u64,
    pub core: usize,
    pub event: Event,
}

#[derive(Default, Serialize)]
pub struct Stream {
    pub events: Vec<Observed>,
    #[serde(skip)]
    cursor: usize,
}

fn bytes(machine: &Machine, core: usize, address: u32, length: usize) -> Result<Vec<u8>, String> {
    require(length <= PAYLOAD_LIMIT && address.checked_add(length as u32).is_some(), "interface payload exceeds bounds")?;
    let mut bytes = vec![0; length];
    if length != 0 { machine.session.board.read_memory(core, u64::from(address), &mut bytes)?; }
    Ok(bytes)
}

fn watch_violation(state: &serde_json::Value) -> Option<String> {
    let violation = state.get("violation").filter(|value| !value.is_null())?;
    let records = state.get("records").and_then(serde_json::Value::as_array);
    let rows = records.map_or(&[][..], Vec::as_slice);
    let last = rows.last();
    let field = |name| last.and_then(|row| row.get(name));
    let word = |value: Option<&serde_json::Value>| {
        value.and_then(serde_json::Value::as_u64).map_or_else(|| "?".to_owned(), |value| format!("{value:#x}"))
    };
    let source = field("source").and_then(serde_json::Value::as_str).unwrap_or("?");
    let last_count = last.map_or(0, |last| rows.iter().filter(|row| {
        row.get("source") == last.get("source") && row.get("address") == last.get("address") && row.get("pc") == last.get("pc")
    }).count());
    let source_count = |wanted| rows.iter().filter(|row| row.get("source").and_then(serde_json::Value::as_str) == Some(wanted)).count();
    Some(format!(
        "interface watch overflowed: violation={}; records={}; cpu_fetch={}; cpu_write={}; last_source={source}; last_address={}; last_pc={}; last_key_count={last_count}",
        violation.as_str().unwrap_or("?"),
        rows.len(),
        source_count("cpu-fetch"),
        source_count("cpu-write"),
        word(field("address")),
        word(field("pc")),
    ))
}

impl Stream {
    fn push(&mut self, observed: Observed) -> Result<(), String> {
        require(self.events.len() < EVENT_LIMIT, "interface event limit exceeded")?;
        self.events.push(observed);
        Ok(())
    }

    pub fn entry(&mut self, machine: &Machine, plan: &Plan, stop: &Stop) -> Result<(), String> {
        let register = |name: &str| stop.registers.get(name).ok_or_else(|| "interface register missing".to_owned()).and_then(|text| parse_hex(text));
        let event = if stop.address == plan.post {
            let mut descriptor = [0; 12];
            machine.session.board.read_memory(stop.core, u64::from(register("r1")?), &mut descriptor)?;
            let queue = u8::try_from(register("r0")?).map_err(|_| "interface queue exceeds byte")?;
            let payload = bytes(machine, stop.core, read_u32(&descriptor, 8), read_u32(&descriptor, 4) as usize)?;
            post_event(queue, read_u32(&descriptor, 0), payload)
        } else if let Some(expected) = plan.owned_post_instruction(stop.address) {
            if read_u32(&bytes(machine, stop.core, stop.address, 4)?, 0) != expected { return Ok(()); }
            let queue = u8::try_from(register("r1")?).map_err(|_| "owned interface queue exceeds byte")?;
            let sp = parse_hex(&stop.sp)?;
            let stack = bytes(machine, stop.core, sp, 8)?;
            let payload = bytes(machine, stop.core, read_u32(&stack, 0), read_u32(&stack, 4) as usize)?;
            post_event(queue, register("r3")?, payload)
        } else if let Some((code, stock)) = plan.owned_callback(stop.address) {
            if bytes(machine, stop.core, stop.address, 16)? != code { return Ok(()); }
            Some(Event::Callback { entry: Address(stock) })
        } else if let Some(code) = plan.owned_client(stop.address) {
            if bytes(machine, stop.core, stop.address, 16)? != code { return Ok(()); }
            let count = register("r2")? as usize;
            require(count <= 12, "owned client call exceeds its declared arguments")?;
            let words = bytes(machine, stop.core, register("r1")?, 4 * count)?;
            let record = register("r3")?;
            let arguments = (0..count).map(|index| {
                let word = read_u32(&words, 4 * index);
                Ok(if record != 0 && index + 1 == (record & 0xff) as usize {
                    Argument::Memory { address: None, bytes: bytes(machine, stop.core, word, (record >> 8) as usize)? }
                } else {
                    Argument::Word { value: word }
                })
            }).collect::<Result<Vec<_>, String>>()?;
            Some(Event::ClientReply { entry: Address(stop.address), target: Address(register("r0")?), arguments })
        } else {
            None
        };
        let event = if let Some(event) = event { event } else {
            match plan.entries.iter().find(|entry| entry.address == stop.address) {
                Some(entry) if matches!(entry.kind, EntryKind::Callback) => Event::Callback { entry: Address(stop.address) },
                Some(entry) => {
                    if matches!(entry.kind, EntryKind::ServiceRequest) {
                        let caller = parse_hex(&stop.lr)?;
                        if !plan.callers.iter().any(|&(start, end)| start <= caller && caller < end) { return Ok(()); }
                    }
                    let arguments = entry.arguments.iter().enumerate().map(|(index, declaration)| {
                        let word = if index < 4 {
                            register(["r0", "r1", "r2", "r3"][index])?
                        } else {
                            let offset = ((index - 4) * 4) as u32;
                            let address = parse_hex(&stop.sp)?.checked_add(offset).ok_or("reply stack address overflow")?;
                            let mut word = [0; 4];
                            machine.session.board.read_memory(stop.core, u64::from(address), &mut word)?;
                            u32::from_le_bytes(word)
                        };
                        Ok(match declaration {
                            ArgumentDecl::Word => Argument::Word { value: word },
                            ArgumentDecl::Memory { bytes: length, identity } => Argument::Memory {
                                address: match identity { PointerIdentity::Transient => None, PointerIdentity::Shared => Some(Address(word)) },
                                bytes: bytes(machine, stop.core, word, *length)?,
                            },
                        })
                    }).collect::<Result<Vec<_>, String>>()?;
                    match entry.kind {
                        EntryKind::ServiceRequest => Event::ServiceRequest { entry: Address(stop.address), arguments },
                        EntryKind::ClientReply => Event::ClientReply { entry: Address(stop.address), target: Address(register(entry.target.ok_or("interface reply target register missing")?.name())?), arguments },
                        EntryKind::Callback => Event::Callback { entry: Address(stop.address) },
                    }
                },
                None => return Ok(()),
            }
        };
        self.push(Observed { instructions: stop.instructions, core: stop.core, event })
    }

    pub fn stores(&mut self, plan: &Plan, state: &serde_json::Value) -> Result<(), String> {
        if let Some(problem) = watch_violation(state) { return Err(problem); }
        let records = state.get("records").and_then(serde_json::Value::as_array).ok_or("interface watch records missing")?;
        require(self.cursor <= records.len(), "interface watch cursor invalid")?;
        #[derive(Deserialize)]
        #[serde(rename_all = "kebab-case")]
        enum Source { CpuFetch, CpuWrite }
        #[derive(Deserialize)]
        struct Write { source: Source, address: u64, size: usize, value: Option<u64>, instructions: Option<u64>, core: Option<usize> }
        for record in &records[self.cursor..] {
            let row = Write::deserialize(record).map_err(|error| error.to_string())?;
            if !matches!(row.source, Source::CpuWrite) || !plan.shared.iter().any(|&(start, end)| u64::from(start) <= row.address && row.address < u64::from(end)) { continue; }
            require(row.size <= 8, "interface write is wider than its recorded value")?;
            let value = row.value.ok_or("interface write value missing")?;
            let address = u32::try_from(row.address).map_err(|_| "interface write exceeds guest address")?;
            require(plan.shared.iter().any(|&(start, end)| start <= address && u64::from(address) + row.size as u64 <= u64::from(end)), "interface write crosses shared span")?;
            self.push(Observed {
                instructions: row.instructions.ok_or("interface write clock missing")?,
                core: row.core.ok_or("interface write core missing")?,
                event: Event::Store { address: Address(address), bytes: value.to_le_bytes()[..row.size].to_vec() },
            })?;
        }
        self.cursor = records.len();
        Ok(())
    }

    pub fn restart(&mut self) { self.cursor = 0; }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> Plan {
        Declaration {
            post: "0x1000".into(),
            owned_posts: Vec::new(),
            owned_callbacks: Vec::new(),
            owned_clients: Vec::new(),
            entries: Vec::new(),
            shared: vec![Shared { start: "0x2000".into(), end: "0x2010".into() }],
            callers: Vec::new(),
        }.validate().unwrap()
    }

    #[test]
    fn writes_remain_ordered_across_entries_and_watch_epochs() {
        let mut stream = Stream::default();
        let first = serde_json::json!({"records":[
            {"source":"cpu-write","address":8192,"size":2,"value":4660,"instructions":10,"core":0}
        ],"violation":null});
        stream.stores(&plan(), &first).unwrap();
        stream.push(Observed { instructions: 11, core: 1, event: Event::Callback { entry: Address(0x1004) } }).unwrap();
        stream.stores(&plan(), &first).unwrap();
        stream.restart();
        let second = serde_json::json!({"records":[
            {"source":"cpu-write","address":8194,"size":1,"value":86,"instructions":12,"core":1}
        ],"violation":null});
        stream.stores(&plan(), &second).unwrap();
        assert_eq!(stream.events.iter().map(|row| &row.event).collect::<Vec<_>>(), vec![
            &Event::Store { address: Address(0x2000), bytes: vec![0x34, 0x12] },
            &Event::Callback { entry: Address(0x1004) },
            &Event::Store { address: Address(0x2002), bytes: vec![0x56] },
        ]);
    }

    #[test]
    fn owned_posts_are_armed_before_update_and_preserve_boundary_observations() {
        let owned = Declaration {
            post: "0x1000".into(),
            owned_posts: vec![
                OwnedPost { address: "0x3000".into(), instruction: "0xe92d4ff0".into() },
                OwnedPost { address: "0x300c".into(), instruction: "0xe92d4ff0".into() },
            ],
            owned_callbacks: Vec::new(),
            owned_clients: Vec::new(),
            entries: Vec::new(),
            shared: Vec::new(),
            callers: Vec::new(),
        }.validate().unwrap();
        assert_eq!(owned.sites(), vec![0x1000, 0x3000, 0x300c]);
        assert_eq!((owned.owned_post_instruction(0x300c), owned.owned_post_instruction(0x3004)), (Some(0xe92d4ff0), None));
        let repeated = Declaration {
            post: "0x1000".into(),
            owned_posts: vec![
                OwnedPost { address: "0x3000".into(), instruction: "0xe92d4ff0".into() },
                OwnedPost { address: "0x3000".into(), instruction: "0xe92d4ff0".into() },
            ],
            owned_callbacks: Vec::new(),
            owned_clients: Vec::new(),
            entries: Vec::new(),
            shared: Vec::new(),
            callers: Vec::new(),
        };
        assert!(repeated.validate().is_err());
        let payload = vec![1, 2, 3];
        assert_eq!(
            post_event(47, 0x2f2b3f26, payload.clone()),
            Some(Event::PeerRequest { queue: 47, operation: 0x3f26, payload: payload.clone() }),
        );
        assert_eq!(
            post_event(43, 0x2b2f3f13, payload.clone()),
            Some(Event::QueueIn { operation: 0x3f13, sender: 47, payload }),
        );
        assert_eq!(post_event(47, 0x2f2a3f26, Vec::new()), None);
    }

    #[test]
    fn owned_callbacks_and_clients_replace_declared_stock_sites_only() {
        const CODE: &str = "0xe92d48f0e28db014e24dd010e1a05000";
        let declaration = |stock: &str, callback: &str, client: &str, code: &str| Declaration {
            post: "0x1000".into(),
            owned_posts: Vec::new(),
            owned_callbacks: vec![OwnedCallback { address: callback.into(), code: code.into(), stock_entry: stock.into() }],
            owned_clients: vec![OwnedClient { address: client.into(), code: CODE.into() }],
            entries: vec![
                Entry { address: "0x4000".into(), kind: EntryKind::Callback, target: None, arguments: Vec::new() },
                Entry { address: "0x4100".into(), kind: EntryKind::ClientReply, target: Some(TargetRegister::R5), arguments: vec![ArgumentDecl::Word] },
            ],
            shared: Vec::new(),
            callers: Vec::new(),
        };
        let plan = declaration("0x4000", "0x5000", "0x6000", CODE).validate().unwrap();
        assert_eq!(plan.sites(), vec![0x1000, 0x4000, 0x4100, 0x5000, 0x6000]);
        assert_eq!(plan.owned_callback(0x5000).map(|(_, stock)| stock), Some(0x4000));
        assert!(plan.owned_client(0x6000).is_some() && plan.owned_client(0x5000).is_none());
        // A replacement of a client reply instead of a callback, one placed on a stock entry, a
        // client site repeating the callback's and a guard that is not 16 bytes are refused.
        assert!(declaration("0x4100", "0x5000", "0x6000", CODE).validate().is_err());
        assert!(declaration("0x4000", "0x4000", "0x6000", CODE).validate().is_err());
        assert!(declaration("0x4000", "0x5000", "0x5000", CODE).validate().is_err());
        assert!(declaration("0x4000", "0x5000", "0x6000", "0xe92d48f0").validate().is_err());
    }

    #[test]
    fn incomplete_write_evidence_is_refused() {
        for record in [
            serde_json::json!({"source":"cpu-write","address":8192,"size":4,"instructions":10,"core":0}),
            serde_json::json!({"source":"cpu-write","address":8207,"size":4,"value":0,"instructions":10,"core":0}),
        ] {
            assert!(Stream::default().stores(&plan(), &serde_json::json!({"records":[record],"violation":null})).is_err());
        }
        assert!(Stream::default().stores(&plan(), &serde_json::json!({"records":[],"violation":"overflow"})).is_err());
    }

    #[test]
    fn watch_overflow_reports_the_last_producer() {
        let state = serde_json::json!({"records":[
            {"source":"cpu-write","address":8192,"pc":4096},
            {"source":"cpu-fetch","address":12288,"pc":16384},
            {"source":"cpu-fetch","address":12288,"pc":16384}
        ],"violation":"record-limit"});
        assert_eq!(
            Stream::default().stores(&plan(), &state).unwrap_err(),
            "interface watch overflowed: violation=record-limit; records=3; cpu_fetch=2; cpu_write=1; last_source=cpu-fetch; last_address=0x3000; last_pc=0x4000; last_key_count=2",
        );
    }
}
