use fujimod_emulator::board::factory::MemoryMap;
use fujimod_emulator_cpu::arm;
use fujimod_emulator::session::Session;
use fujimod_emulator_devices::Device;
use fujimod_emulator_exr2_system::boot::Block98000;
use fujimod_xm1::bridge::write;
use fujimod_xm1::provenance::Guest;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{fs::OpenOptions, io::Write, path::{Path, PathBuf}};

#[path = "memory_view.rs"]
mod memory_view;
use memory_view::{MemoryAddress as Address, MemoryCore as Core, TcmRegister, static_span, tcm_window};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub(super) struct Digest(String);
impl TryFrom<String> for Digest {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
            return Err("digest is not lowercase SHA-256".into());
        }
        Ok(Self(value))
    }
}
impl From<Digest> for String { fn from(value: Digest) -> Self { value.0 } }
impl Digest {
    pub(super) fn of(bytes: &[u8]) -> Self { Self(fujimod_xm1::bridge::sha256(bytes)) }
    pub(super) fn as_str(&self) -> &str { &self.0 }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub(super) struct EventId(pub usize);
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub(super) struct ByteCount(pub u32);
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum DramChoice { One, Two }

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Mapping {
    pub guest: Guest,
    pub physical: Guest,
    pub bytes: ByteCount,
    pub decoder: DramChoice,
    pub tcm: [[u32; 2]; 2],
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Words {
    pub mapping: Mapping,
    pub values: Vec<u32>,
    pub guest_equal: [bool; 2],
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Resource {
    LoaderA,
    LoaderB,
    Image5,
    Image5Bss,
    Plain,
    Plain0,
    Plain2,
    Plain3,
}

impl Resource {
    pub(super) fn extent(self) -> (u32, u32) {
        match self {
            Self::LoaderA => (0x409ac, 0x40b00),
            Self::LoaderB => (0xd09ac, 0xd0b00),
            Self::Image5 => (0xbf1a00, 0x10e3860),
            Self::Image5Bss => (0x1460000, 0x171be40),
            Self::Plain => (0x1100000, 0x1460000),
            Self::Plain0 => (0x1100000, 0x11ac5a0),
            Self::Plain2 => (0x11c0000, 0x11dcca0),
            Self::Plain3 => (0x11e0000, 0x144a5e0),
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::LoaderA => "loader-a-startup", Self::LoaderB => "loader-b-startup",
            Self::Image5 => "image5", Self::Image5Bss => "image5-bss", Self::Plain => "plain",
            Self::Plain0 => "plain-0", Self::Plain2 => "plain-2", Self::Plain3 => "plain-3",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct FileBinding {
    pub path: PathBuf,
    pub sha256: Digest,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Snapshot {
    pub event: EventId,
    pub resource: Resource,
    pub mapping: Mapping,
    pub files: [FileBinding; 3],
    pub guest_equal: [bool; 2],
    pub expected_equal: bool,
    pub first_difference: Option<Guest>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct RangeSnapshot {
    pub event: EventId,
    pub mapping: Mapping,
    pub alias_mapping: Mapping,
    pub files: [FileBinding; 5],
    pub guest_equal: [bool; 4],
    pub expected_equal: bool,
    pub first_difference: Option<Guest>,
}

pub(super) struct Reader<'a> {
    session: &'a Session,
    maps: &'a [MemoryMap],
    decoder: DramChoice,
    tcm: [[u32; 2]; 2],
}
impl<'a> Reader<'a> {
    pub(super) fn new(session: &'a Session, maps: &'a [MemoryMap]) -> Result<Self, String> {
        let mut tcm = [[0; 2]; 2];
        for (core, banks) in tcm.iter_mut().enumerate() {
            let cpu = session.board.cpu(core)?;
            for (bank, value) in banks.iter_mut().enumerate() {
                *value = u32::try_from(cpu.coprocessor(arm::Coprocessor { cp: 15, opc1: 0, crn: 9, crm: 1, opc2: bank as u32, secure: 0, wide: false }, None)?).map_err(|_| "TCM register exceeds word")?;
            }
        }
        #[derive(Deserialize)]
        struct State { registers: [u32; 5] }
        #[derive(Deserialize)]
        struct Fingerprint { state: State }
        let state: Fingerprint = serde_json::from_value(session.board.inspect_device::<Block98000, _>(session.node("block98000")?, |device| device.fingerprint())?).map_err(|error| error.to_string())?;
        let decoder = match state.state.registers[1] { 1 => DramChoice::One, 2 => DramChoice::Two, _ => return Err("projection DRAM decoder is not admitted".into()) };
        Ok(Self { session, maps, decoder, tcm })
    }
    fn mapping(&self, guest: Guest, bytes: usize) -> Result<Mapping, String> {
        if bytes == 0 { return Err("empty committed memory projection".into()); }
        let end = u64::from(guest.0).checked_add(bytes as u64).ok_or("projection extent overflow")?;
        let first = static_span(self.maps, Core(0), Address(u64::from(guest.0))..Address(end))?;
        let second = static_span(self.maps, Core(1), Address(u64::from(guest.0))..Address(end))?;
        if first.single.is_none() || second.single.is_none() || first.physical.0 != second.physical.0 {
            return Err("projection does not have one shared DRAM mapping".into());
        }
        for bank in self.tcm.into_iter().flatten() {
            if tcm_window(TcmRegister(bank))?.is_some_and(|(low, high)| low < end && u64::from(guest.0) < high) {
                return Err("projection overlaps an enabled TCM window".into());
            }
        }
        Ok(Mapping { guest, physical: Guest(u32::try_from(first.physical.0).map_err(|_| "physical address exceeds word")?), bytes: ByteCount(u32::try_from(bytes).map_err(|_| "projection length exceeds word")?), decoder: self.decoder, tcm: self.tcm })
    }
    pub(super) fn words(&self, guest: Guest, count: usize) -> Result<Words, String> {
        if guest.0 % 4 != 0 || count == 0 || count > 80 { return Err("word projection extent is not admitted".into()); }
        let length = count * 4;
        let mapping = self.mapping(guest, length)?;
        let mut physical = [0; 320];
        self.session.board.read_physical(u64::from(mapping.physical.0), &mut physical[..length])?;
        let mut equal = [false; 2];
        for (core, result) in equal.iter_mut().enumerate() {
            let mut guest_bytes = [0; 320];
            self.session.board.read_memory(core, u64::from(guest.0), &mut guest_bytes[..length])?;
            *result = guest_bytes[..length] == physical[..length];
        }
        if equal != [true; 2] { return Err("committed guest and physical words differ".into()); }
        Ok(Words { mapping, values: physical[..length].chunks_exact(4).map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]])).collect(), guest_equal: equal })
    }
}
pub(super) fn words(session: &Session, maps: &[MemoryMap], guest: Guest, count: usize) -> Result<Words, String> {
    Reader::new(session, maps)?.words(guest, count)
}
pub(super) fn snapshot(session: &Session, maps: &[MemoryMap], output: &Path, event: EventId, resource: Resource, expected: Option<&[u8]>) -> Result<Snapshot, String> {
    let (start, end) = resource.extent();
    let bytes = (end - start) as usize;
    if expected.is_some_and(|expected| expected.len() != bytes) { return Err("snapshot source extent differs".into()); }
    if expected.is_none() != matches!(resource, Resource::Image5Bss) { return Err("only the BSS projection has a zero expectation".into()); }
    let mapping = Reader::new(session, maps)?.mapping(Guest(start), bytes)?;
    let names: [String; 3] = std::array::from_fn(|view| format!("event-{:05}-{}-view-{view}.bin", event.0, resource.name()));
    let open = |name: &str| OpenOptions::new().write(true).create_new(true).open(output.join(name)).map_err(|error| error.to_string());
    let mut files = [open(&names[0])?, open(&names[1])?, open(&names[2])?];
    let (hashes, equal, difference) = capture_views(session, &mapping, [Guest(start); 2], &mut files, expected)?;
    let [a, b, c] = hashes;
    let [first, second, physical] = names;
    let files = [FileBinding {path:first.into(),sha256:a}, FileBinding {path:second.into(),sha256:b}, FileBinding {path:physical.into(),sha256:c}];
    let result = Snapshot { event, resource, mapping, files, guest_equal: equal, expected_equal: difference.is_none(), first_difference: difference };
    write(&output.join(format!("event-{:05}-{}-projection.json", event.0, resource.name())), &serde_json::to_vec_pretty(&result).map_err(|error| error.to_string())?)?;
    Ok(result)
}

pub(super) fn snapshot_range(session: &Session, maps: &[MemoryMap], output: &Path, event: EventId, start: Guest, length: ByteCount, label: &str, expected: Option<&[u8]>) -> Result<RangeSnapshot, String> {
    if label.is_empty() || !label.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-') { return Err("snapshot label is not a filename component".into()); }
    let bytes = length.0 as usize;
    if expected.is_some_and(|expected| expected.len() != bytes) { return Err("range snapshot source extent differs".into()); }
    if start.0 & 0x40000000 != 0 { return Err("range snapshot requires the low guest spelling".into()); }
    let reader = Reader::new(session, maps)?;
    let mapping = reader.mapping(start, bytes)?;
    let alias = Guest(start.0 | 0x40000000);
    let alias_mapping = reader.mapping(alias, bytes)?;
    if mapping.physical != alias_mapping.physical { return Err("range snapshot aliases have different physical backing".into()); }
    let names: [String; 5] = std::array::from_fn(|view| format!("event-{:05}-{label}-view-{view}.bin", event.0));
    let open = |name: &str| OpenOptions::new().write(true).create_new(true).open(output.join(name)).map_err(|error| error.to_string());
    let mut files = [open(&names[0])?, open(&names[1])?, open(&names[2])?, open(&names[3])?, open(&names[4])?];
    let (hashes, equal, difference) = capture_views(session, &mapping, [start,start,alias,alias], &mut files, expected)?;
    if equal != [true; 4] { return Err("plain range guest aliases differ from physical memory".into()); }
    let [a,b,c,d,e] = hashes;
    let [first,second,third,fourth,physical] = names;
    let files = [FileBinding {path:first.into(),sha256:a},FileBinding {path:second.into(),sha256:b},FileBinding {path:third.into(),sha256:c},FileBinding {path:fourth.into(),sha256:d},FileBinding {path:physical.into(),sha256:e}];
    Ok(RangeSnapshot {event,mapping,alias_mapping,files,guest_equal:equal,expected_equal:difference.is_none(),first_difference:difference})
}

fn capture_views<const V: usize, const F: usize>(session: &Session, mapping: &Mapping, guests: [Guest; V], files: &mut [std::fs::File; F], expected: Option<&[u8]>) -> Result<([Digest; F], [bool; V], Option<Guest>), String> {
    if F != V+1 { return Err("committed projection view census differs".into()); }
    let bytes = mapping.bytes.0 as usize;
    let mut hashes: [Sha256; F] = std::array::from_fn(|_| Sha256::new());
    let mut equal = [true; V];
    let mut difference = None;
    for offset in (0..bytes).step_by(4096) {
        let length = (bytes - offset).min(4096);
        let mut physical = [0; 4096];
        session.board.read_physical(u64::from(mapping.physical.0) + offset as u64, &mut physical[..length])?;
        for (view, address) in guests.iter().enumerate() {
            let mut guest = [0; 4096];
            session.board.read_memory(view % 2, u64::from(address.0) + offset as u64, &mut guest[..length])?;
            equal[view] &= guest[..length] == physical[..length];
            files[view].write_all(&guest[..length]).map_err(|error| error.to_string())?;
            hashes[view].update(&guest[..length]);
        }
        files[V].write_all(&physical[..length]).map_err(|error| error.to_string())?;
        hashes[V].update(&physical[..length]);
        if difference.is_none() {
            difference = physical[..length].iter().enumerate().find(|(index, value)| **value != expected.map_or(0, |expected| expected[offset + index])).map(|(index, _)| Guest(mapping.guest.0 + (offset + index) as u32));
        }
    }
    Ok((hashes.map(|hash| Digest(format!("{:x}", hash.finalize()))), equal, difference))
}
