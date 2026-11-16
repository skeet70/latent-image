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

#[derive(Debug, Deserialize)]
pub struct Shared {
    start: String,
    end: String,
}

#[derive(Debug, Deserialize)]
pub struct Declaration {
    post: String,
    #[serde(default)]
    owned_post: Option<OwnedPost>,
    entries: Vec<Entry>,
    shared: Vec<Shared>,
    #[serde(default)]
    callers: Vec<Shared>,
}

pub struct Plan {
    post: u32,
    owned_post: Option<(u32, u32)>,
    entries: Vec<BoundaryEntry>,
    shared: Vec<(u32, u32)>,
    callers: Vec<(u32, u32)>,
}

impl Declaration {
    pub fn validate(self) -> Result<Plan, String> {
        require(self.entries.len() <= 512 && self.shared.len() <= 64, "interface declaration exceeds bounds")?;
        let post = parse_hex(&self.post)?;
        let owned_post = match self.owned_post {
            Some(owned) => Some((parse_hex(&owned.address)?, parse_hex(&owned.instruction)?)),
            None => None,
        };
        require(owned_post.is_none_or(|(address, _)| address & 3 == 0 && address != post), "owned interface post is unaligned or duplicates stock")?;
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
        Ok(Plan { post, owned_post, entries, shared, callers })
    }
}

impl Plan {
    pub fn sites(&self) -> Vec<u32> {
        let mut sites = Vec::with_capacity(self.entries.len() + 2);
        sites.push(self.post);
        sites.extend(self.entries.iter().map(|entry| entry.address));
        if let Some((address, _)) = self.owned_post { sites.push(address); }
        sites
    }
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
        self.owned_post.and_then(|(owned, instruction)| (owned == address).then_some(instruction))
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
            owned_post: None,
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
    fn owned_post_is_armed_before_update_and_preserves_boundary_observations() {
        let owned = Declaration {
            post: "0x1000".into(),
            owned_post: Some(OwnedPost { address: "0x3000".into(), instruction: "0xe92d4ff0".into() }),
            entries: Vec::new(),
            shared: Vec::new(),
            callers: Vec::new(),
        }.validate().unwrap();
        assert_eq!(owned.sites(), vec![0x1000, 0x3000]);
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
