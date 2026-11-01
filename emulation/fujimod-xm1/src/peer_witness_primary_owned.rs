use super::*;

#[derive(Clone, Copy)]
enum SymbolName { Entry, ImageStart, TextStart, ImageEnd, Capacity, Publish, TextEnd, RodataStart, RodataEnd, PayloadEnd }
impl SymbolName {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "xm1_primary_initializer_entry" => Self::Entry,
            "__primary_initializer_image_start" => Self::ImageStart,
            "__primary_initializer_text_start" => Self::TextStart,
            "__primary_initializer_image_end" => Self::ImageEnd,
            "__primary_initializer_capacity" => Self::Capacity,
            "xm1_primary_initializer_publish" => Self::Publish,
            "__primary_initializer_text_end" => Self::TextEnd,
            "__primary_initializer_rodata_start" => Self::RodataStart,
            "__primary_initializer_rodata_end" => Self::RodataEnd,
            "__primary_initializer_payload_end" => Self::PayloadEnd,
            _ => return None,
        })
    }
}
#[derive(Clone, Copy)]
struct LinkedSymbol { address: Guest, bytes: SymbolBytes }
#[derive(Clone, Copy)]
struct SymbolBytes(u32);

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OwnedIdentity {
    pub(super) input_sha256: Sha256Digest,
    source_loader_a_sha256: Sha256Digest,
    image_sha256: Sha256Digest,
    symbols_sha256: Sha256Digest,
    publication_sha256: Sha256Digest,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Scratch { low: Guest, high: Guest, rust_bytes: u32, adapter_bytes: u32 }

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AccessKind { ScratchRead, ScratchWrite, ImageRead, PublicationWrite }

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "u8", into = "u8")]
struct EnteredRegister(u8);

impl TryFrom<u8> for EnteredRegister {
    type Error = &'static str;
    fn try_from(register: u8) -> Result<Self, Self::Error> {
        if register <= 12 { Ok(Self(register)) } else { Err("owned primary entered register exceeds r12") }
    }
}

impl From<EnteredRegister> for u8 {
    fn from(register: EnteredRegister) -> Self { register.0 }
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum ScratchValue {
    Constant { value: Word },
    EnteredRegister { register: EnteredRegister },
    EnteredLr,
    EnteredCpsr,
}

impl ScratchValue {
    fn expected(self, entered: &Context) -> Word {
        match self {
            Self::Constant { value } => value,
            Self::EnteredRegister { register } => entered.registers[usize::from(register.0)],
            Self::EnteredLr => entered.lr,
            Self::EnteredCpsr => entered.cpsr,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EmittedAccess { guest: Guest, pc: Guest, ordinal: Instructions, kind: AccessKind, scratch_value: Option<ScratchValue> }

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EmittedPublication { guest: Guest, value: Word, pc: Guest, ordinal: Instructions }

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OwnedProgram {
    pub(super) input_sha256: Sha256Digest,
    source_loader_a_sha256: Sha256Digest, image_sha256: Sha256Digest, symbols_sha256: Sha256Digest, publication_sha256: Sha256Digest,
    source_loader: Vec<u8>, image: Vec<u8>, symbols: Vec<u8>, scratch: Scratch,
    pub(super) instructions: Instructions,
    publications: Vec<EmittedPublication>, accesses: Vec<EmittedAccess>,
}

impl OwnedProgram {
    pub(super) fn identity(&self) -> OwnedIdentity {
        OwnedIdentity { input_sha256: self.input_sha256, source_loader_a_sha256: self.source_loader_a_sha256,
            image_sha256: self.image_sha256, symbols_sha256: self.symbols_sha256, publication_sha256: self.publication_sha256 }
    }

    pub(super) fn scratch_span(&self) -> (u64, u64) {
        (u64::from(self.scratch.low.0) - ZERO_GUEST, u64::from(self.scratch.high.0) - ZERO_GUEST)
    }

    pub(super) fn validate(&self, loader_sha: Sha256Digest) -> Result<(), String> {
        require(self.source_loader_a_sha256 == Sha256Digest::parse(LOADER_SHA)?
            && Sha256Digest::of(&self.source_loader) == self.source_loader_a_sha256
            && self.image.len() == (LEAF.1 - LEAF.0) as usize
            && Sha256Digest::of(&self.image) == self.image_sha256
            && self.symbols.len() <= 0x10000 && Sha256Digest::of(&self.symbols) == self.symbols_sha256,
            "owned primary source, image or symbols identity differs")?;
        let expected = source_publications(&self.source_loader)?;
        require(self.scratch.adapter_bytes == 8 && self.scratch.rust_bytes <= 0x400 && self.scratch.rust_bytes % 4 == 0
            && self.scratch.high.0 == STACK as u32
            && self.scratch.low.0 == STACK as u32 - 8 - self.scratch.rust_bytes
            && (17..=100017).contains(&self.instructions.0), "owned primary scratch or instruction extent differs")?;
        let mut hash = Sha256::new();
        for publication in &expected {
            hash.update((ZERO_GUEST as u32 + publication.physical.0 as u32).to_le_bytes());
            hash.update(publication.value.0.to_le_bytes());
        }
        require(Sha256Digest(hash.finalize().into()) == self.publication_sha256
            && self.publications.len() == expected.len(), "owned primary canonical publication identity differs")?;
        for (actual, source) in self.publications.iter().zip(&expected) {
            require(u64::from(actual.guest.0) == ZERO_GUEST + source.physical.0 && actual.value == source.value,
                "owned primary emitted publication differs from canonical source")?;
        }
        require((4..=WATCH_LIMIT).contains(&self.accesses.len()), "owned primary emitted access extent differs")?;
        let mut previous = 0;
        let mut publications = self.publications.iter();
        for (index, access) in self.accesses.iter().enumerate() {
            require(access.ordinal.0 < self.instructions.0 && (index == 0 || access.ordinal.0 >= previous)
                && (LEAF.0..LEAF.1).contains(&access.pc.0) && access.pc.0 % 4 == 0 && access.guest.0 % 4 == 0,
                "owned primary access chronology or instruction address differs")?;
            previous = access.ordinal.0;
            require(access.scratch_value.is_some() == (access.kind == AccessKind::ScratchWrite),
                "owned primary scratch value provenance role differs")?;
            match access.kind {
                AccessKind::ScratchRead | AccessKind::ScratchWrite => {
                    require(access.guest.0 >= self.scratch.low.0 && access.guest.0 <= self.scratch.high.0 - 4,
                        "owned primary access leaves scratch")?;
                }
                AccessKind::ImageRead => require(access.guest.0 >= LEAF.0 && access.guest.0 <= LEAF.1 - 4,
                    "owned primary read leaves image")?,
                AccessKind::PublicationWrite => {
                    let publication = publications.next().ok_or("owned primary excess publication access")?;
                    require(access.guest == publication.guest && access.pc == publication.pc
                        && access.ordinal.0 == publication.ordinal.0, "owned primary publication access site differs")?;
                }
            }
        }
        require(publications.next().is_none(), "owned primary missing publication")?;
        let adapter = |access: &EmittedAccess, guest, pc, ordinal, kind| access.guest.0 == guest
            && access.pc.0 == pc && access.ordinal.0 == ordinal && access.kind == kind;
        let n = self.accesses.len();
        require(adapter(&self.accesses[0], STACK as u32 - 8, LEAF.0, 0, AccessKind::ScratchWrite)
            && adapter(&self.accesses[1], STACK as u32 - 4, LEAF.0, 0, AccessKind::ScratchWrite)
            && adapter(&self.accesses[n - 2], STACK as u32 - 8, LEAF.0 + 24, self.instructions.0 - 11, AccessKind::ScratchRead)
            && adapter(&self.accesses[n - 1], STACK as u32 - 4, LEAF.0 + 24, self.instructions.0 - 11, AccessKind::ScratchRead)
            && self.accesses[0].scratch_value == Some(ScratchValue::EnteredRegister { register: EnteredRegister(4) })
            && self.accesses[1].scratch_value == Some(ScratchValue::EnteredLr),
            "owned primary adapter access sites differ")?;
        self.validate_symbols()?;
        let mut hash = Sha256::new();
        hash.update(&self.source_loader[..0x430]); hash.update(&self.image); hash.update(&self.source_loader[0x9ac..]);
        require(Sha256Digest(hash.finalize().into()) == loader_sha, "owned primary complete loader identity differs")
    }

    fn validate_symbols(&self) -> Result<(), String> {
        let text = std::str::from_utf8(&self.symbols).map_err(|error| error.to_string())?;
        let mut symbols = [None; 10];
        for line in text.lines().filter(|line| !line.is_empty()) {
            let mut row = line.split_whitespace();
            let number = |value: Option<&str>| -> Result<u32, String> {
                let value = value.ok_or("owned primary symbol row differs")?;
                u32::from_str_radix(value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")).unwrap_or(value), 16)
                    .map_err(|error| error.to_string())
            };
            let symbol = LinkedSymbol { address: Guest(number(row.next())?), bytes: SymbolBytes(number(row.next())?) };
            row.next().ok_or("owned primary symbol kind absent")?;
            let name = row.next().ok_or("owned primary symbol name absent")?;
            require(row.next().is_none(), "owned primary symbol row differs")?;
            if let Some(name) = SymbolName::parse(name) {
                require(symbols[name as usize].replace(symbol).is_none(), "owned primary duplicate required symbol")?;
            }
        }
        let at = |name: SymbolName| symbols[name as usize].ok_or("owned primary missing required symbol");
        let entry = at(SymbolName::Entry)?;
        require(entry.address.0 == LEAF.0 && entry.bytes.0 == 68
            && at(SymbolName::ImageStart)?.address.0 == LEAF.0
            && at(SymbolName::TextStart)?.address.0 == LEAF.0
            && at(SymbolName::ImageEnd)?.address.0 == LEAF.1
            && at(SymbolName::Capacity)?.address.0 == LEAF.1 - LEAF.0, "owned primary symbol placement differs")?;
        let symbol = at(SymbolName::Publish)?;
        let (rust, size) = (symbol.address.0, symbol.bytes.0);
        let text_end = at(SymbolName::TextEnd)?.address.0;
        let rodata = at(SymbolName::RodataStart)?.address.0;
        let rodata_end = at(SymbolName::RodataEnd)?.address.0;
        let end = at(SymbolName::PayloadEnd)?.address.0;
        require(rust >= LEAF.0 + 68 && rust % 4 == 0 && size > 0 && u64::from(rust) + u64::from(size) <= u64::from(text_end)
            && text_end <= rodata && rodata <= rodata_end && rodata_end <= end && end <= LEAF.1 && end % 4 == 0
            && self.image[(end - LEAF.0) as usize..].iter().all(|byte| *byte == 0), "owned primary linked text or padding differs")?;
        let call = 0xeb000000 | (((i64::from(rust) - i64::from(LEAF.0 + 16)) / 4) as u32 & 0xffffff);
        let words = [0xe92d4010,0xe10f4000,call,0xe3c4420f,0xe3844206,0xe128f004,0xe8bd4010,
            0xe30b0eac,0xe3440005,0xe30d1160,0xe3441005,0xe3012930,0xe3442006,0xe30e3f80,0xe3443004,0xe3a0c078,0xe12fff1e];
        require(self.image[..68].chunks_exact(4).zip(words).all(|(bytes, word)| bytes == u32::to_le_bytes(word)),
            "owned primary adapter bytes differ")
    }

    pub(super) fn validate_loader(&self, bytes: &[u8], digest: Sha256Digest) -> Result<(), String> {
        require(bytes.len() == self.source_loader.len() && Sha256Digest::of(bytes) == digest
            && bytes[..0x430] == self.source_loader[..0x430] && bytes[0x430..0x9ac] == self.image
            && bytes[0x9ac..] == self.source_loader[0x9ac..], "owned primary actual complete loader differs")
    }

    pub(super) fn admit_update(&self, bytes: &[u8], source: Sha256Digest) -> Result<(), String> {
        const OFFSET: usize = 0x8068c;
        let end = OFFSET + self.image.len();
        require(bytes.len() >= end
            && bytes[OFFSET..end].iter().zip(&self.image).all(|(actual, image)| *actual == !*image),
            "owned primary actual staged DAT or complemented carrier differs")?;
        let old_sum = self.source_loader[0x430..0x9ac].iter().fold(0_u32, |sum, byte| sum.wrapping_add(u32::from(*byte)));
        let new_sum = self.image.iter().fold(0_u32, |sum, byte| sum.wrapping_add(u32::from(*byte)));
        let checksum = 0x8f0ab18f_u32.wrapping_add(old_sum).wrapping_sub(new_sum);
        require(bytes[0x20c..0x210] == checksum.to_le_bytes(), "owned primary staged checksum differs")?;
        let mut hash = Sha256::new();
        hash.update(&bytes[..0x20c]); hash.update(0x8f0ab18f_u32.to_le_bytes()); hash.update(&bytes[0x210..OFFSET]);
        let original: [u8; 0x57c] = std::array::from_fn(|index| !self.source_loader[0x430 + index]);
        hash.update(original);
        hash.update(&bytes[end..]);
        require(Sha256Digest(hash.finalize().into()) == source, "owned primary restored canonical DAT identity differs")
    }

    pub(super) fn publications(&self) -> Result<Vec<ExpectedPublication>, String> {
        Ok(self.publications.iter().map(|publication| ExpectedPublication {
            physical: Address(u64::from(publication.guest.0) - ZERO_GUEST), value: publication.value, pc: publication.pc,
        }).collect())
    }

    pub(super) fn replay<'a>(&self, watch: &'a WatchState, entered: &Context, after: &Context,
        before: &Views, final_scratch: &Views, table_before: &Views, table_after: &Views) -> Result<Vec<&'a Event>, String> {
        require(watch.records.len() == self.accesses.len(), "owned primary raw access count differs")?;
        let mut scratch = before.physical.clone();
        let mut publications = Vec::with_capacity(self.publications.len());
        let mut last_tick = 0;
        let mut last_instruction = entered.counts.0.iter().sum::<u64>();
        let mut last_ordinal = 0;
        let mut last_core1 = entered.counts.0[1];
        for (event, access) in watch.records.iter().zip(&self.accesses) {
            let instructions = event.instructions.ok_or("owned primary access lacks instruction attribution")?;
            let tick = event.tick.as_ref().ok_or("owned primary access lacks tick attribution")?.0;
            let core0 = entered.counts.0[0].checked_add(access.ordinal.0 + 1).ok_or("owned primary access ordinal overflow")?;
            let core1 = instructions.checked_sub(core0).ok_or("owned primary access predates ordinal")?;
            let physical = u64::from(access.guest.0) - if access.guest.0 >= ZERO_GUEST as u32 { ZERO_GUEST } else { 0 };
            let write = matches!(access.kind, AccessKind::ScratchWrite | AccessKind::PublicationWrite);
            require(event.core == Some(0) && event.address == u64::from(access.guest.0) && event.physical == Some(physical)
                && event.size == 4 && event.pc == Some(u64::from(access.pc.0)) && event.tcm.is_none()
                && event.source == (if write { Source::CpuWrite } else { Source::CpuRead })
                && matches!(event.byte_order, Some(ByteOrder::Little)) && tick >= last_tick
                && instructions >= last_instruction && instructions < after.counts.0.iter().sum()
                && (access.ordinal.0 == last_ordinal || instructions > last_instruction)
                && (access.ordinal.0 != last_ordinal || instructions == last_instruction || last_instruction == entered.counts.0.iter().sum::<u64>())
                && core1 >= last_core1 && core1 <= after.counts.0[1],
                "owned primary raw access attribution or chronology differs")?;
            let value = event.value.ok_or("owned primary access lacks value")?;
            require(value <= u64::from(u32::MAX), "owned primary access value exceeds word")?;
            if access.kind == AccessKind::ScratchWrite {
                let expected = access.scratch_value.ok_or("owned primary scratch write lacks value provenance")?.expected(entered);
                require(value == u64::from(expected.0), "owned primary scratch write differs from entered value provenance")?;
            }
            let value = (value as u32).to_le_bytes();
            let preimage = |expected: &[u8]| -> Result<(), String> {
                let raw = event.before.as_ref().ok_or("owned primary write lacks preimage")?;
                require(raw.len == 4 && raw.bytes[..4] == *expected, "owned primary write preimage differs")
            };
            if !write { preimage(&value)?; }
            match access.kind {
                AccessKind::ScratchRead | AccessKind::ScratchWrite => {
                    let offset = (access.guest.0 - self.scratch.low.0) as usize;
                    if write { preimage(&scratch[offset..offset + 4])?; scratch[offset..offset + 4].copy_from_slice(&value); }
                    else { require(scratch[offset..offset + 4] == value, "owned primary scratch read differs")?; }
                }
                AccessKind::ImageRead => {
                    let offset = (access.guest.0 - LEAF.0) as usize;
                    require(self.image[offset..offset + 4] == value, "owned primary immutable image read differs")?;
                }
                AccessKind::PublicationWrite => {
                    let offset = (physical - PRIMARY_TABLE.0) as usize;
                    preimage(&table_before.physical[offset..offset + 4])?;
                    require(table_after.physical[offset..offset + 4] == value, "owned primary publication readback differs")?;
                    publications.push(event);
                }
            }
            last_tick = tick; last_instruction = instructions; last_ordinal = access.ordinal.0; last_core1 = core1;
        }
        require(scratch == final_scratch.physical, "owned primary complete scratch replay differs")?;
        Ok(publications)
    }
}
