use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_METADATA: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CpuState {
    pub format: CpuFormat,
    #[serde(with = "binary")]
    pub context: Vec<u8>,
    pub identity_input: u32,
    #[serde(deserialize_with = "required_option")]
    pub clock: Option<(u64, u64, (u64, u64))>,
    pub clock_overshoot: u64,
    pub policy: Policy,
    pub pmu: Pmu,
    pub tcm: [Tcm; 2],
    pub maps: Vec<(u64, Option<u64>, usize, u32)>,
    pub mmio: Vec<(u64, usize)>,
    #[serde(with = "binary")]
    pub reservation: Vec<u8>,
    pub ring: Vec<u64>,
    pub budget: i64,
    pub head: u64,
    pub count: u64,
    pub cap: u64,
    pub synced: i64,
    pub settled_head: u64,
    pub thumb: bool,
    #[serde(deserialize_with = "required_option")]
    pub restarted: Option<u64>,
    pub user_stop: bool,
    pub irq: bool,
    #[serde(deserialize_with = "required_option")]
    pub hint: Option<u8>,
    #[serde(deserialize_with = "required_option")]
    pub exception: Option<(u8, u32)>,
    #[serde(deserialize_with = "required_option")]
    pub failure: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CpuFormat { #[serde(rename = "rprofile-r5f-v1")] RprofileR5fV1 }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Policy { Exact, Interactive }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pmu {
    pub control: u32, pub enabled: u32, pub interrupts: u32, pub overflow: u32,
    pub frozen: u64, pub start: u64, pub observed: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tcm {
    pub register: u32, pub len: usize,
    #[serde(with = "binary")]
    pub pages: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureDocument { pub schema: u32, pub architecture: Architecture }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum Architecture { CortexR5f(R5State) }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct R5State {
    pub identity: [u32; 6], pub cpsr: u32, pub registers: [u32; 16], pub scalar: [u32; 4],
    pub vectors: [[u8; 16]; 16], pub banks: [Bank; 7], pub cp15: [[u32; 2]; 16],
    pub mpu_selected: u32, pub mpu: [[u32; 3]; 16],
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bank {
    pub registers: [u32; 7],
    #[serde(deserialize_with = "required_option")]
    pub spsr: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reservation { pub address: u64, pub size: u64 }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuReservation { pub epoch: u128, pub reservation: Option<Reservation> }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExclusiveSlot { pub granule: u64, pub reservation: Option<Reservation> }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExclusiveState { pub epoch: u128, pub slots: [ExclusiveSlot; 2] }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page { pub index: u64, pub bytes: Vec<u8> }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageImage { pub len: u64, pub pages: Vec<Page> }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedCpu {
    pub state: CpuState, pub architecture: ArchitectureDocument,
    pub reservation: CpuReservation, pub tcm: [PageImage; 2],
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedArchive { pub cpu: [DecodedCpu; 2], pub exclusive: ExclusiveState }

fn required_option<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(deserializer)
}
mod binary {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> { serializer.serialize_bytes(bytes) }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> { Vec::<u8>::deserialize(deserializer) }
}
fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    if bytes.len() > MAX_METADATA { return Err("exact archive capacity exhausted".into()); }
    let mut input = bytes;
    let result = ciborium::from_reader(&mut input).map_err(|e| format!("exact archive decode: {e}"))?;
    if !input.is_empty() { return Err("exact archive trailing bytes".into()); }
    Ok(result)
}
fn word(bytes: &[u8], at: usize) -> Result<u64, String> {
    Ok(u64::from_le_bytes(bytes.get(at..at + 8).ok_or("truncated exact archive")?.try_into().unwrap()))
}
fn epoch(bytes: &[u8]) -> Result<u128, String> {
    Ok(u128::from_le_bytes(bytes.get(8..24).ok_or("truncated exact epoch")?.try_into().unwrap()))
}
fn reservation(address: u64, size: u64) -> Result<Option<Reservation>, String> {
    if (size == 0 && address != 0) || address.checked_add(size).is_none() { return Err("invalid exact reservation".into()); }
    Ok((size != 0).then_some(Reservation { address, size }))
}
pub fn decode_reservation(bytes: &[u8]) -> Result<CpuReservation, String> {
    if bytes.len() != 40 || bytes.get(..8) != Some(b"FJEXCL01") { return Err("unknown CPU exclusive archive".into()); }
    Ok(CpuReservation { epoch: epoch(bytes)?, reservation: reservation(word(bytes, 24)?, word(bytes, 32)?)? })
}
pub fn decode_exclusive(bytes: &[u8]) -> Result<ExclusiveState, String> {
    if bytes.len() != 80 || bytes.get(..8) != Some(b"FJMONI01") || word(bytes, 24)? != 2 { return Err("unknown two-core exclusive archive".into()); }
    let slot = |at| -> Result<ExclusiveSlot, String> {
        let granule = word(bytes, at)?;
        if !(4..=2048).contains(&granule) || !granule.is_power_of_two() { return Err("invalid exclusive granule".into()); }
        Ok(ExclusiveSlot { granule, reservation: reservation(word(bytes, at + 8)?, word(bytes, at + 16)?)? })
    };
    Ok(ExclusiveState { epoch: epoch(bytes)?, slots: [slot(32)?, slot(56)?] })
}
pub fn decode_pages(bytes: &[u8], len: usize) -> Result<PageImage, String> {
    if bytes.get(..8) != Some(b"FJMPAGE1") || word(bytes, 8)? != len as u64 { return Err("invalid TCM page archive extent".into()); }
    let count = usize::try_from(word(bytes, 16)?).map_err(|_| "page count overflow")?;
    if count > len.div_ceil(4096) || count > bytes.len() / 8 { return Err("invalid TCM page count".into()); }
    let mut at = 24;
    let mut pages: Vec<Page> = Vec::with_capacity(count);
    for _ in 0..count {
        let index = word(bytes, at)?;
        if index >= len.div_ceil(4096) as u64 || pages.last().is_some_and(|p| p.index >= index) { return Err("invalid TCM page index".into()); }
        at += 8;
        let size = 4096.min(len - index as usize * 4096);
        let page = bytes.get(at..at + size).ok_or("truncated TCM page")?;
        if page.iter().all(|b| *b == 0) { return Err("zero TCM page".into()); }
        pages.push(Page { index, bytes: page.to_vec() });
        at += size;
    }
    if at != bytes.len() { return Err("TCM archive trailing bytes".into()); }
    Ok(PageImage { len: len as u64, pages })
}
pub fn decode_cpu(bytes: &[u8]) -> Result<DecodedCpu, String> {
    let state: CpuState = decode(bytes)?;
    if state.ring.len() != 64 { return Err("CPU accounting ring length differs".into()); }
    let architecture: ArchitectureDocument = decode(&state.context)?;
    let Architecture::CortexR5f(registers) = &architecture.architecture;
    let modes = [0x10, 0x11, 0x12, 0x13, 0x17, 0x1b, 0x1f];
    let mode = modes.iter().position(|m| *m == registers.cpsr & 31).ok_or("invalid R5F processor mode")?;
    if registers.mpu_selected >= 16 { return Err("invalid R5F MPU selector".into()); }
    for (index, bank) in registers.banks.iter().enumerate() {
        if bank.spsr.is_none() != matches!(modes[index], 0x10 | 0x1f)
            || (modes[index] != 0x11 && bank.registers[..5] != registers.banks[0].registers[..5]) {
            return Err("inconsistent R5F register banks".into());
        }
    }
    if registers.banks[0] != registers.banks[6] || registers.registers[8..15] != registers.banks[mode].registers
        || registers.cp15[2].iter().any(|v| v & !0xf != 0)
        || registers.cp15[7].iter().any(|v| v & 31 != 0)
        || registers.cp15[15].iter().any(|v| v & !0xfffff6ff != 0) {
        return Err("unrepresentable R5F architectural state".into());
    }
    for bank in &state.tcm {
        let normalized = if bank.len == 0 {
            if bank.register & 1 != 0 { return Err("enabled absent TCM".into()); }
            bank.register & 0xfffff000
        } else {
            if !(0x1000..=0x800000).contains(&bank.len) || !bank.len.is_power_of_two() { return Err("invalid TCM geometry".into()); }
            (bank.register & !(bank.len as u32 - 1)) | ((bank.len.trailing_zeros() - 9) << 2) | (bank.register & 1)
        };
        if normalized != bank.register { return Err("unnormalized TCM register".into()); }
    }
    let [a, b] = &state.tcm;
    if a.register & 1 != 0 && b.register & 1 != 0 {
        let first = (a.register & 0xfffff000) as u64;
        let second = (b.register & 0xfffff000) as u64;
        if first < second + b.len as u64 && second < first + a.len as u64 { return Err("overlapping TCM banks".into()); }
    }
    if architecture.schema != 1 { return Err("unknown architecture schema".into()); }
    if state.hint.is_some_and(|h| h > 3) || state.exception.is_some_and(|(tag, value)| tag > 1 || (tag == 0 && value > 6)) { return Err("unknown CPU pending state".into()); }
    if let Some((_, phase, (n, d))) = state.clock {
        if n == 0 || d == 0 || phase >= n { return Err("invalid CPU clock".into()); }
    }
    let reservation = decode_reservation(&state.reservation)?;
    let tcm = [decode_pages(&state.tcm[0].pages, state.tcm[0].len)?, decode_pages(&state.tcm[1].pages, state.tcm[1].len)?];
    Ok(DecodedCpu { state, architecture, reservation, tcm })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactBlob {
    pub path: String, pub format: String, pub length: usize, pub sha256: String,
    pub component_sha256: String, pub bytes_hex: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactArchive {
    pub checkpoint_model_id: String,
    pub schema: u32, pub counts: [u64; 2], pub now: u64,
    pub model_definition_sha256: String, pub executable_sha256: String,
    pub cpu: [ExactBlob; 2], pub exclusive: ExactBlob,
}
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes { write!(&mut text, "{byte:02x}").expect("string write"); }
    text
}
pub fn unhex(text: &str) -> Result<Vec<u8>, String> {
    if text.len() > MAX_METADATA || text.len() % 2 != 0 { return Err("invalid exact archive hex length".into()); }
    text.as_bytes().chunks_exact(2).map(|pair| {
        let digit = |b: u8| match b { b'0'..=b'9' => Ok(b - b'0'), b'a'..=b'f' => Ok(b - b'a' + 10), _ => Err("invalid exact archive hex".to_owned()) };
        Ok(digit(pair[0])? * 16 + digit(pair[1])?)
    }).collect()
}
pub fn blob_component_digest(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    for field in [b"af-machine-components-v1".as_slice(), b"blob".as_slice()] { digest.update((field.len() as u64).to_le_bytes()); digest.update(field); }
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update((bytes.len().div_ceil(1048576) as u64).to_le_bytes());
    for chunk in bytes.chunks(1048576) { digest.update((chunk.len() as u64).to_le_bytes()); digest.update(Sha256::digest(chunk)); }
    hex(&digest.finalize())
}
pub fn authenticate(receipt: &serde_json::Value) -> Result<DecodedArchive, String> {
    serde_json::to_writer(MetadataSize(0), receipt).map_err(|e| e.to_string())?;
    let archive: ExactArchive = serde_json::from_value(receipt["exact_state"].clone()).map_err(|e| e.to_string())?;
    if archive.schema != 1 || serde_json::to_value(archive.counts).map_err(|e| e.to_string())? != receipt["counts"] || receipt["now"] != archive.now { return Err("exact archive capture binding differs".into()); }
    if archive.checkpoint_model_id != env!("XM1_CHECKPOINT_MODEL_ID") || unhex(&archive.model_definition_sha256)?.len() != 32 || unhex(&archive.executable_sha256)?.len() != 32 { return Err("exact archive immutable identity differs".into()); }
    let entries = receipt["components"]["entries"].as_array().ok_or("missing components")?;
    let verify = |blob: &ExactBlob, index: usize, format: &str| -> Result<Vec<u8>, String> {
        if blob.path != format!("$/children/0/blobs/{index}") || blob.format != format { return Err("exact blob path/format differs".into()); }
        let bytes = unhex(&blob.bytes_hex)?;
        if bytes.len() != blob.length || hex(&Sha256::digest(&bytes)) != blob.sha256 || blob_component_digest(&bytes) != blob.component_sha256 { return Err("exact blob digest differs".into()); }
        let matches: Vec<_> = entries.iter().filter(|entry| entry["path"] == blob.path).collect();
        if matches.len() != 1 || matches[0]["sha256"] != blob.component_sha256 || matches[0]["logical_bytes"] != blob.length || matches[0]["kind"] != "blob" || matches[0]["checkpoint_kind"] != "board-keyframe-v1" { return Err("exact component authentication differs".into()); }
        Ok(bytes)
    };
    let exact = [verify(&archive.cpu[0], 0, "rprofile-r5f-v1")?, verify(&archive.cpu[1], 1, "rprofile-r5f-v1")?, verify(&archive.exclusive, 2, "FJMONI01")?];
    authenticate_component_tree(receipt, &exact)?;
    let cpu = [decode_cpu(&exact[0])?, decode_cpu(&exact[1])?];
    let exclusive = decode_exclusive(&exact[2])?;
    for (core, slot) in cpu.iter().zip(&exclusive.slots) {
        if core.reservation.epoch != exclusive.epoch || core.reservation.reservation != slot.reservation { return Err("CPU/shared exclusive state differs".into()); }
    }
    Ok(DecodedArchive { cpu, exclusive })
}

pub(crate) fn retain(
    encoded: &fujimod_emulator_devices::checkpoint::Checkpoint,
    counts: [u64; 2], now: u64, definition: &serde_json::Value,
) -> Result<ExactArchive, String> {
    let board = encoded.children().first().ok_or("missing board checkpoint")?;
    if board.blobs().len() != 3 { return Err("exact archive requires two CPUs and shared exclusives".into()); }
    let total = board.blobs().iter().try_fold(0usize, |n, bytes| n.checked_add(bytes.len()).ok_or("exact archive size overflow"))?;
    if total > MAX_METADATA / 16 { return Err("exact archive metadata capacity exhausted".into()); }
    let retain = |index: usize, format: &str| {
        let bytes = &board.blobs()[index];
        ExactBlob { path: format!("$/children/0/blobs/{index}"), format: format.into(), length: bytes.len(),
            sha256: hex(&Sha256::digest(bytes)), component_sha256: blob_component_digest(bytes), bytes_hex: hex(bytes) }
    };
    let mut executable = std::fs::File::open("/proc/self/exe").map_err(|e| format!("executable identity: {e}"))?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        use std::io::Read;
        let count = executable.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 { break; }
        digest.update(&buffer[..count]);
    }
    let mut model_digest = IdentityHasher(Sha256::new());
    serde_json::to_writer(&mut model_digest, definition).map_err(|e| e.to_string())?;
    Ok(ExactArchive {
        schema: 1, checkpoint_model_id: env!("XM1_CHECKPOINT_MODEL_ID").into(), counts, now,
        model_definition_sha256: hex(&model_digest.0.finalize()),
        executable_sha256: hex(&digest.finalize()),
        cpu: [retain(0, "rprofile-r5f-v1"), retain(1, "rprofile-r5f-v1")], exclusive: retain(2, "FJMONI01"),
    })
}

struct MetadataSize(usize);
impl std::io::Write for MetadataSize {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.checked_add(bytes.len()).filter(|size| *size <= MAX_METADATA)
            .ok_or_else(|| std::io::Error::other("exact archive aggregate capacity exhausted"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sha256Identity(pub [u8; 32]);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointModelId(pub String);
pub struct ImmutableBindings {
    pub checkpoint_model_id: CheckpointModelId,
    pub model_definition: Sha256Identity,
    pub executable: Sha256Identity,
}
pub fn authenticate_bindings(receipt: &serde_json::Value, expected: &ImmutableBindings) -> Result<DecodedArchive, String> {
    if receipt["exact_state"]["model_definition_sha256"] != hex(&expected.model_definition.0)
        || receipt["exact_state"]["checkpoint_model_id"] != expected.checkpoint_model_id.0
        || receipt["exact_state"]["executable_sha256"] != hex(&expected.executable.0) {
        return Err("exact archive expected immutable identity differs".into());
    }
    authenticate(receipt)
}

struct IdentityHasher(Sha256);
impl std::io::Write for IdentityHasher {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum ComponentKind { Checkpoint, Header, Blob, Chunk }
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Component {
    path: String, kind: ComponentKind, checkpoint_kind: String,
    logical_bytes: usize, sha256: String, offset: Option<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentMetadata {
    schema: u32, algorithm: String, root_sha256: String,
    chunk_bytes: usize, entries: Vec<Component>,
}
struct ComponentTree {
    entries: std::collections::BTreeMap<String, Component>,
    visited: std::collections::BTreeSet<String>,
}
fn component_digest_start(domain: &[u8]) -> Sha256 {
    let mut digest = Sha256::new();
    digest_field(&mut digest, b"af-machine-components-v1");
    digest_field(&mut digest, domain);
    digest
}
fn digest_field(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
}
impl ComponentTree {
    fn take(&mut self, path: &str, kind: ComponentKind, checkpoint_kind: Option<&str>) -> Result<(usize, [u8; 32]), String> {
        let row = self.entries.get(path).ok_or_else(|| format!("missing component {path}"))?;
        if !self.visited.insert(path.into()) || row.kind != kind || row.offset.is_some()
            || checkpoint_kind.is_some_and(|expected| expected != row.checkpoint_kind) {
            return Err(format!("component geometry differs at {path}"));
        }
        let digest = unhex(&row.sha256)?.try_into().map_err(|_| "invalid component SHA-256")?;
        Ok((row.logical_bytes, digest))
    }
    fn children(&self, prefix: &str) -> Result<usize, String> {
        let mut indices = std::collections::BTreeSet::new();
        for path in self.entries.keys() {
            if let Some(suffix) = path.strip_prefix(prefix) {
                let index = suffix.split('/').next().ok_or("missing component index")?;
                let number: usize = index.parse().map_err(|_| "invalid component index")?;
                if number.to_string() != index { return Err("noncanonical component index".into()); }
                indices.insert(number);
            }
        }
        if indices.iter().copied().ne(0..indices.len()) { return Err("noncontiguous component indices".into()); }
        Ok(indices.len())
    }
    fn blob(&mut self, path: &str, checkpoint_kind: &str, raw: Option<&[u8]>) -> Result<(usize, [u8; 32]), String> {
        let (len, expected) = self.take(path, ComponentKind::Blob, Some(checkpoint_kind))?;
        let count = len.div_ceil(1048576);
        if self.children(&format!("{path}/chunks/"))? != count || raw.is_some_and(|bytes| bytes.len() != len) {
            return Err("component blob geometry differs".into());
        }
        let mut digest = component_digest_start(b"blob");
        digest.update((len as u64).to_le_bytes());
        digest.update((count as u64).to_le_bytes());
        for index in 0..count {
            let chunk_path = format!("{path}/chunks/{index}");
            let row = self.entries.get(&chunk_path).ok_or("missing component chunk")?;
            let offset = index * 1048576;
            let size = 1048576.min(len - offset);
            if !self.visited.insert(chunk_path) || row.kind != ComponentKind::Chunk
                || row.checkpoint_kind != checkpoint_kind || row.offset != Some(offset) || row.logical_bytes != size {
                return Err("component chunk geometry differs".into());
            }
            let chunk_digest: [u8; 32] = unhex(&row.sha256)?.try_into().map_err(|_| "invalid chunk SHA-256")?;
            if raw.is_some_and(|bytes| Sha256::digest(&bytes[offset..offset + size]).as_slice() != chunk_digest) {
                return Err("retained exact chunk digest differs".into());
            }
            digest.update((size as u64).to_le_bytes());
            digest.update(chunk_digest);
        }
        let digest: [u8; 32] = digest.finalize().into();
        if digest != expected { return Err("component blob digest differs".into()); }
        Ok((len, digest))
    }
    fn node(&mut self, path: &str, depth: usize, exact: &[Vec<u8>; 3]) -> Result<(usize, [u8; 32]), String> {
        if depth > 64 { return Err("component tree depth exhausted".into()); }
        let (expected_len, expected_digest) = self.take(path, ComponentKind::Checkpoint, None)?;
        let kind = self.entries.get(path).ok_or("missing checkpoint component")?.checkpoint_kind.clone();
        if path == "$" && kind != "emulator-session-v2"
            || path == "$/children/0" && kind != "board-keyframe-v1" {
            return Err("component session/board kind differs".into());
        }
        let (header_len, header_digest) = self.take(&format!("{path}/header"), ComponentKind::Header, Some(&kind))?;
        let mut digest = component_digest_start(b"checkpoint");
        digest_field(&mut digest, kind.as_bytes());
        digest.update((header_len as u64).to_le_bytes());
        digest.update(header_digest);
        let mut len = 32usize.checked_add(kind.len()).and_then(|n| n.checked_add(header_len)).ok_or("component length overflow")?;
        let blobs = self.children(&format!("{path}/blobs/"))?;
        let children = self.children(&format!("{path}/children/"))?;
        if path == "$" && (blobs != 0 || children != 2)
            || path == "$/children/0" && (blobs != 3 || children != 2) {
            return Err("component session/board geometry differs".into());
        }
        digest.update((blobs as u64).to_le_bytes());
        for index in 0..blobs {
            let raw = (path == "$/children/0").then(|| exact[index].as_slice());
            let (size, blob_digest) = self.blob(&format!("{path}/blobs/{index}"), &kind, raw)?;
            digest.update((size as u64).to_le_bytes());
            digest.update(blob_digest);
            len = len.checked_add(8).and_then(|n| n.checked_add(size)).ok_or("component length overflow")?;
        }
        digest.update((children as u64).to_le_bytes());
        for index in 0..children {
            let (size, child_digest) = self.node(&format!("{path}/children/{index}"), depth + 1, exact)?;
            digest.update((size as u64).to_le_bytes());
            digest.update(child_digest);
            len = len.checked_add(size).ok_or("component length overflow")?;
        }
        let digest: [u8; 32] = digest.finalize().into();
        if len != expected_len || digest != expected_digest { return Err(format!("component checkpoint authentication differs at {path}")); }
        Ok((len, digest))
    }
}
fn authenticate_component_tree(receipt: &serde_json::Value, exact: &[Vec<u8>; 3]) -> Result<(), String> {
    let metadata: ComponentMetadata = serde_json::from_value(receipt["components"].clone()).map_err(|e| e.to_string())?;
    if metadata.schema != 1 || metadata.algorithm != "sha256-hierarchical-v1" || metadata.chunk_bytes != 1048576 || metadata.entries.len() > 65536 {
        return Err("unsupported component metadata geometry".into());
    }
    let mut tree = ComponentTree { entries: Default::default(), visited: Default::default() };
    for row in metadata.entries {
        if tree.entries.insert(row.path.clone(), row).is_some() { return Err("duplicate component path".into()); }
    }
    let (len, digest) = tree.node("$", 0, exact)?;
    if tree.visited.len() != tree.entries.len() || hex(&digest) != metadata.root_sha256
        || receipt["logical_bytes"] != len.checked_add(8 + b"af-machine-fingerprint-v1".len()).ok_or("canonical length overflow")?
        || unhex(receipt["machine_sha256"].as_str().ok_or("missing machine SHA-256")?)?.len() != 32 {
        return Err("component root/coverage authentication differs".into());
    }
    Ok(())
}
