use super::*;

pub(super) const SENTINEL: u32 = 0x89d4f8;
pub(super) const CARRIERS: [(u32, u32, u8); 5] = [
    (0x89cda4, 0x89d4f8, 4), (0x89d4fc, 0x89d908, 4),
    (0x89da3c, 0x89dfc0, 4), (0x89e464, 0x89e7c4, 4), (0xbf1c1c, 0xbf1cd4, 5),
];
pub(super) const ENTRY_ADDRESSES: [u32; 9] = [0x89cda4, 0x89cdc4, 0x89cde4, 0x89da3c, 0x89da64, 0x89e464, 0x89e560, 0xbf1c1c, 0xbf1c44];
/// Stock recursive lock, unlock and memmove.
const RETAINED_CALLEES: [u32; 3] = [0x72b164, 0x72b1cc, 0x89d908];

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct Interval { pub low: u32, pub high: u32, pub image: u8 }
#[derive(Deserialize)]
struct Canonical { dat_sha256: Digest, image4_sha256: Digest, image5_sha256: Digest }
#[derive(Deserialize)]
struct Staged { path: PathBuf, sha256: Digest }
#[derive(Deserialize)]
struct Sentinel { address: u32, initial: u32 }
#[derive(Deserialize)]
struct Frame { entry: u32 }
#[derive(Deserialize)]
struct Literal { address: u32, value: u32 }
#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum Qualification { ConditionalSourceOnly }
#[derive(Clone, Deserialize, Eq, PartialEq)]
struct Geometry {
    first_level_bins: u32, second_level_bins: u32, granularity: u32, stock_prefix_bytes: u32,
    stock_alignment: u32, first_level_bitmap_bits: u32, second_level_bitmap_bits: u32,
}
#[derive(Deserialize)]
struct Arena { base: u32, end: u32, guard_bytes: u32, state_bytes: u32, pool_start: u32, pool_end: u32 }
#[derive(Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
struct SymbolName(String);
impl std::borrow::Borrow<str> for SymbolName {
    fn borrow(&self) -> &str { &self.0 }
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Redesign { CheckedCallocProduct, OversizedResizeRejectedBeforeLock, ZeroRequestMinimumAllocation, TrimReturnsZero }
#[derive(Deserialize)]
struct RedesignItem { id: Redesign, stock: String, owned: String, why: String, dependence_signal: String }
#[derive(Deserialize)]
struct Contract {
    schema: u32, qualification: Qualification, canonical_source: Canonical,
    staged_image4: Staged, staged_image5: Staged, candidate_dat_sha256: Digest,
    carrier_ranges: Vec<Interval>, immutable_intervals: Vec<Interval>, sentinel: Sentinel,
    symbols: BTreeMap<SymbolName, u32>, function_symbols: BTreeMap<SymbolName, u32>,
    owned_instruction_pcs: Vec<u32>, literal_words: Vec<Literal>,
    frame_metadata: Vec<Frame>, conditional_premises: Vec<String>, backend_geometry: Geometry, arena: Arena,
    redesign_items: Vec<RedesignItem>,
}

pub(super) struct Admitted {
    pub contract_digest: Digest,
    pub candidate: ValidatedUpdate,
    pub images: Vec<Image>,
    pub owned: BTreeSet<u32>,
    pub premises: Vec<String>,
    arena: Arena,
}

fn metadata(images: &[Image], address: u32) -> Result<[u32; 14], String> {
    let mut words = [0; 14];
    for (index, word) in words.iter_mut().enumerate() {
        *word = expected(images, address.checked_add(index as u32 * 4).ok_or("metadata address overflow")?)?;
    }
    Ok(words)
}

fn geometry(images: &[Image], address: u32, declared: &Geometry, arena: &Arena) -> Result<(), String> {
    geometry_words(metadata(images, address)?, declared, arena)
}

fn geometry_words(words: [u32; 14], declared: &Geometry, arena: &Arena) -> Result<(), String> {
    let actual = Geometry { first_level_bins: words[7], second_level_bins: words[8], granularity: words[9],
        stock_prefix_bytes: words[10], stock_alignment: words[11], first_level_bitmap_bits: words[12], second_level_bitmap_bits: words[13] };
    let wanted = Geometry { first_level_bins: 17, second_level_bins: 16, granularity: 16, stock_prefix_bytes: 16,
        stock_alignment: 8, first_level_bitmap_bits: 32, second_level_bitmap_bits: 16 };
    require(words[0] == 0x48454150 && words[1] == 1 && actual == wanted && *declared == actual, "owned backend size-class geometry differs")?;
    require(words[2] == HEAP_START && [HEAP_END, 0xbe1a00, fujimod_xm1::fixed_image_layout::IMAGE_BASE].contains(&words[3]) && words[6] == 32
        && words[4] >= 17 * 16 * 4 && words[5] == HEAP_START.checked_add(words[4]).and_then(|end| end.checked_add(15)).map(|end| end & !15).ok_or("charged state overflow")?
        && words[5] & 15 == 0 && words[5] < words[3] - 32
        && (arena.base, arena.end, arena.state_bytes, arena.pool_start, arena.guard_bytes, arena.pool_end)
            == (words[2], words[3], words[4], words[5], words[6], words[3] - 32), "owned charged arena geometry differs")
}

pub(super) fn admit_pointer(admitted: &Admitted, pointer: Address, bytes: u64) -> Result<(), String> {
    let low = u64::from(pointer.0 & !0x40000000);
    require(pointer.0 & 7 == 0 && low >= u64::from(admitted.arena.pool_start)
        && low.checked_add(bytes).is_some_and(|high| high <= u64::from(admitted.arena.pool_end)),
        "owned payload escapes charged pool")
}

fn carrier(pc: u32) -> bool { pc & 3 == 0 && CARRIERS.iter().any(|(low, high, _)| *low <= pc && pc.checked_add(4).is_some_and(|end| end <= *high)) }
fn immediate(word: u32) -> u32 { (word & 255).rotate_right(((word >> 8) & 15) * 2) }

/// Nonreturning roots are the greatest fixpoint: every root starts nonreturning, and explored paths only grow as roots leave the set.
fn derive(images: &[Image], roots: &BTreeSet<u32>) -> Result<(BTreeSet<u32>, BTreeMap<u32, u32>), String> {
    let mut noreturn = roots.clone();
    loop {
        let mut pcs = BTreeSet::new();
        let mut literals = BTreeMap::new();
        let mut remaining = BTreeSet::new();
        for root in roots {
            if !walk(images, roots, &noreturn, *root, &mut pcs, &mut literals)? { remaining.insert(*root); }
        }
        if remaining == noreturn {
            require(pcs.is_disjoint(&literals.keys().copied().collect()), "owned literal is executable")?;
            return Ok((pcs, literals));
        }
        noreturn = remaining;
    }
}

/// Walks one root's frame and reports whether a return is reachable. Each path carries the last evaluated
/// condition and its outcome until an instruction may write the flags; a call to a nonreturning root has no fallthrough.
fn walk(images: &[Image], roots: &BTreeSet<u32>, noreturn: &BTreeSet<u32>, root: u32,
    pcs: &mut BTreeSet<u32>, literals: &mut BTreeMap<u32, u32>) -> Result<bool, String> {
    let mut pending = vec![(root, 0i32, None::<i32>, None::<(u32, bool)>)];
    let mut seen = BTreeSet::new();
    let mut returns = false;
    while let Some(state @ (pc, sp, fp, known)) = pending.pop() {
        if !seen.insert(state) { continue; }
        require(seen.len() <= 100_000 && carrier(pc), "owned CFG leaves charged carriers")?;
        let word = expected(images, pc)?;
        let condition = word >> 28;
        require(condition < 15, "unconditional extension instruction is not admitted")?;
        pcs.insert(pc);
        let outcome = if condition == 14 { Some(true) } else {
            match known {
                Some((previous, held)) if previous == condition => Some(held),
                Some((previous, held)) if previous ^ 1 == condition => Some(!held),
                _ => None,
            }
        };
        let fallthrough = pc.checked_add(4).ok_or("CFG PC overflow")?;
        if outcome == Some(false) {
            pending.push((fallthrough, sp, fp, known));
            continue;
        }
        let mut next = Some((fallthrough, sp, fp));
        let mut writes_flags = false;
        let mut terminal_call = false;
        if word & 0x0e000000 == 0x0a000000 {
            let displacement = (((word & 0xffffff) << 8) as i32) >> 6;
            let target = pc.wrapping_add(8).wrapping_add_signed(displacement);
            require(pc >= 0xbf1a00 || target < 0xbf1a00, "image4 transfer depends on unloaded image5")?;
            if word & 0x01000000 != 0 {
                require(sp & 7 == 0 && (roots.contains(&target) || RETAINED_CALLEES.contains(&target)), "owned call target or stack is not admitted")?;
                writes_flags = true;
                if noreturn.contains(&target) {
                    terminal_call = true;
                    next = None;
                }
            } else if RETAINED_CALLEES.contains(&target) {
                require(sp == 0, "owned tail call has a live frame")?;
                next = None;
            } else {
                require(carrier(target), "owned branch leaves carriers")?;
                next = Some((target, sp, fp));
            }
        } else if word & 0x0ffffff0 == 0x012fff10 {
            require(word & 15 == 14 && sp == 0, "owned computed transfer or unbalanced return")?;
            next = None;
        } else if word & 0x0e000000 == 0x08000000 {
            let rn = (word >> 16) & 15;
            let list = word & 0xffff;
            let count = list.count_ones() as i32 * 4;
            let load = word & (1 << 20) != 0;
            require(word & (1 << 22) == 0, "owned multiple transfer changes processor state")?;
            if rn == 13 && word & (1 << 21) != 0 {
                let offset = if !load && word & 0x01800000 == 0x01000000 { sp - count }
                    else if load && word & 0x01800000 == 0x00800000 { sp + count }
                    else { return Err("owned stack multiple transfer is not admitted".into()); };
                require((-65536..=0).contains(&offset) && offset & 3 == 0 && list & (1 << 13) == 0, "owned stack multiple transfer escapes frame")?;
                if load && list & (1 << 15) != 0 {
                    require(offset == 0, "owned return retains stack frame")?;
                    next = None;
                } else { next = Some((pc + 4, offset, if load && list & (1 << 11) != 0 { None } else { fp })); }
            } else {
                require((!load || list & 0xa000 == 0) && !(rn == 15 && word & (1 << 21) != 0), "owned load multiple or writeback has a computed transfer")?;
                if load && list & (1 << 11) != 0 || rn == 11 && word & (1 << 21) != 0 { next = Some((pc + 4, sp, None)); }
            }
        } else if word & 0x0e000010 == 0x06000010 {
            let admitted = [0x06ef0070, 0x06ff0070, 0x06af0070, 0x06bf0070].contains(&(word & 0x0fff03f0))
                || [0x06e00070, 0x06f00070, 0x06a00070, 0x06b00070].contains(&(word & 0x0ff003f0))
                || [0x06bf0f30, 0x06bf0fb0, 0x06ff0f30].contains(&(word & 0x0fff0ff0))
                || word & 0x0fa00070 == 0x07a00050 || word & 0x0fe00070 == 0x07c00010;
            require(admitted && ![13, 15].contains(&((word >> 12) & 15)), "owned media instruction is not admitted")?;
            if (word >> 12) & 15 == 11 { next = Some((pc + 4, sp, None)); }
        } else if word & 0x0e4f0000 == 0x040d0000 && word & 0x01200000 != 0x01000000 {
            let rd = (word >> 12) & 15;
            let load = word & (1 << 20) != 0;
            require(word & 0x01200000 != 0x00200000 && (!load || rd != 13), "owned indexed stack transfer is not admitted")?;
            let amount = (word & 4095) as i32 * if word & (1 << 23) != 0 { 1 } else { -1 };
            let offset = sp + amount;
            require((-65536..=0).contains(&offset) && offset & 3 == 0, "owned indexed stack transfer escapes frame")?;
            if load && rd == 15 {
                require(offset == 0, "owned indexed return retains frame")?;
                next = None;
            } else { next = Some((pc + 4, offset, if load && rd == 11 { None } else { fp })); }
        } else if word & 0x0c000000 == 0x04000000 {
            let rn = (word >> 16) & 15;
            let rd = (word >> 12) & 15;
            let load = word & (1 << 20) != 0;
            require(!load || ![13, 15].contains(&rd), "owned load changes PC or SP")?;
            require(word & 0x01200000 != 0x00200000 && (rn != 13 || word & 0x01200000 == 0x01000000), "owned indexed stack or unprivileged transfer is not admitted")?;
            if load && rd == 11 || rn == 11 && word & 0x01200000 != 0x01000000 { next = Some((pc + 4, sp, None)); }
            if rn == 15 {
                require(load && word & 0x03600000 == 0x01000000, "owned PC-relative transfer is not a word literal load")?;
                let displacement = word & 4095;
                let address = if word & (1 << 23) != 0 { pc.checked_add(8).and_then(|pc| pc.checked_add(displacement)) }
                    else { pc.checked_add(8).and_then(|pc| pc.checked_sub(displacement)) }.ok_or("literal address overflow")?;
                require(carrier(address) && (pc >= 0xbf1a00 || address < 0xbf1a00), "owned literal leaves carrier or reads unloaded image5")?;
                literals.insert(address, expected(images, address)?);
            }
        } else if word & 0x0ff00000 == 0x03000000 || word & 0x0ff00000 == 0x03400000
            || word & 0x0fff0ff0 == 0x016f0f10 {
            require(![13, 15].contains(&((word >> 12) & 15)), "owned constant or CLZ changes PC or SP")?;
            if (word >> 12) & 15 == 11 { next = Some((pc + 4, sp, None)); }
        } else if word & 0x0fffff00 == 0x0320f000 {
            require(word & 255 <= 1, "owned wait or event instruction is not admitted")?;
        } else if word & 0x0e0000f0 == 0x00000090 {
            require(word & 0x01000000 == 0 && word & 0x00100000 == 0, "owned exclusive instruction is not admitted")?;
            let first = (word >> 16) & 15;
            let second = (word >> 12) & 15;
            let dual = word & 0x00800000 != 0 || word & 0x00600000 == 0x00400000;
            require(![13, 15].contains(&first) && (!dual || ![13, 15].contains(&second)), "owned multiply changes PC or SP")?;
            if first == 11 || dual && second == 11 { next = Some((pc + 4, sp, None)); }
        } else if word & 0x0e000090 == 0x00000090 {
            let rd = (word >> 12) & 15;
            let rn = (word >> 16) & 15;
            let op = (word >> 5) & 3;
            let load = word & (1 << 20) != 0 || op == 2;
            let dual = op == 2 && word & (1 << 20) == 0;
            require(!dual || rd & 1 == 0 && rd + 1 < 13, "owned dual load changes PC or SP or uses an invalid pair")?;
            if load && (rd == 11 || dual && rd + 1 == 11) || rn == 11 && word & 0x01200000 != 0x01000000 { next = Some((pc + 4, sp, None)); }
            require(op != 0 && word & 0x01200000 != 0x00200000
                && (!load || ![13, 15].contains(&rd)) && (rn != 13 || word & 0x01200000 == 0x01000000),
                "owned extra transfer changes PC or SP")?;
            if rn == 15 {
                require(load && op == 2 && word & 0x01700000 == 0x01400000, "owned extra PC-relative transfer is not a dual literal")?;
                let displacement = ((word >> 4) & 0xf0) | (word & 15);
                let address = if word & (1 << 23) != 0 { pc.checked_add(8).and_then(|pc| pc.checked_add(displacement)) }
                    else { pc.checked_add(8).and_then(|pc| pc.checked_sub(displacement)) }.ok_or("dual literal address overflow")?;
                for address in [address, address.checked_add(4).ok_or("dual literal overflow")?] {
                    require(carrier(address) && (pc >= 0xbf1a00 || address < 0xbf1a00), "owned dual literal leaves carrier or reads unloaded image5")?;
                    literals.insert(address, expected(images, address)?);
                }
            }
        } else if word & 0x0c000000 == 0 {
            let rd = (word >> 12) & 15;
            let rn = (word >> 16) & 15;
            let op = (word >> 21) & 15;
            let imm = word & (1 << 25) != 0;
            let tests = [8, 9, 10, 11].contains(&op);
            require((word & 0x01800000 != 0x01000000 || word & (1 << 20) != 0)
                && (!tests || word & (1 << 20) != 0) && (rd != 15 || word & (1 << 20) == 0 || tests),
                "owned status or exception instruction is not admitted")?;
            if rd == 13 {
                let offset = if imm && rn == 13 && [2, 4].contains(&op) {
                    let amount = i32::try_from(immediate(word)).map_err(|_| "owned stack immediate exceeds signed range")?;
                    if op == 4 { sp.checked_add(amount) } else { sp.checked_sub(amount) }
                } else if imm && rn == 11 && op == 2 { fp.and_then(|fp| i32::try_from(immediate(word)).ok().and_then(|amount| fp.checked_sub(amount))) }
                else if !imm && op == 13 && word & 0xfff == 11 { fp }
                else { None }.ok_or("owned SP assignment is not statically bounded")?;
                require((-65536..=0).contains(&offset) && offset & 3 == 0, "owned SP leaves frame")?;
                next = Some((pc + 4, offset, fp));
            } else if rd == 15 && ![8, 9, 10, 11].contains(&op) {
                require(word & 0x0fffffff == 0x01a0f00e && sp == 0, "owned data processing computes PC")?;
                next = None;
            } else if rd == 11 && rn == 13 && imm && op == 4 {
                let amount = i32::try_from(immediate(word)).map_err(|_| "owned frame immediate exceeds signed range")?;
                next = Some((pc + 4, sp, Some(sp.checked_add(amount).ok_or("owned frame pointer overflow")?)));
            } else if rd == 11 && !tests { next = Some((pc + 4, sp, None)); }
            require(word & 0x0f9000f0 != 0x01000090, "owned exclusive instruction is not admitted")?;
            writes_flags = word & (1 << 20) != 0;
        } else { return Err(format!("owned instruction {word:#x} at {pc:#x} has no admitted decoder")); }
        if next.is_none() && !terminal_call { returns = true; }
        let executed = if writes_flags { None } else if outcome.is_none() { Some((condition, true)) } else { known };
        if let Some((target, offset, frame)) = next { pending.push((target, offset, frame, executed)); }
        if outcome.is_none() { pending.push((fallthrough, sp, fp, Some((condition, false)))); }
    }
    Ok(returns)
}

pub(super) fn admit(contract_path: &Path, canonical_dat: &Path, candidate_dat: &Path, analysis: &Path) -> Result<Admitted, String> {
    let bytes = bounded_read(contract_path, 16 * 1024 * 1024)?;
    let contract: Contract = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    require(contract.schema == 1 && contract.qualification == Qualification::ConditionalSourceOnly, "artifact schema or qualification differs")?;
    let (mut images, _) = source_images(analysis)?;
    let source = bounded_read(canonical_dat, 128 * 1024 * 1024)?;
    let candidate = bounded_read(candidate_dat, 128 * 1024 * 1024)?;
    require(sha256(&source) == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"
        && contract.canonical_source.dat_sha256.0 == sha256(&source)
        && contract.candidate_dat_sha256.0 == sha256(&candidate), "artifact DAT identity differs")?;
    require(contract.carrier_ranges == CARRIERS.iter().map(|(low, high, image)| Interval { low: *low, high: *high, image: *image }).collect::<Vec<_>>()
        && contract.immutable_intervals == contract.carrier_ranges, "artifact carrier or immutable intervals differ")?;
    require(contract.sentinel.address == SENTINEL && contract.sentinel.initial == 0, "artifact sentinel differs")?;
    let root = contract_path.parent().ok_or("artifact has no parent directory")?;
    for (base, image_number, staged, canonical) in [
        (0x61c400, 4u8, &contract.staged_image4, &contract.canonical_source.image4_sha256),
        (0xbf1a00, 5u8, &contract.staged_image5, &contract.canonical_source.image5_sha256),
    ] {
        let image = images.iter_mut().find(|image| image.base == base).ok_or("canonical image absent")?;
        require(canonical.0 == sha256(&image.bytes), "artifact canonical image identity differs")?;
        let staged_bytes = bounded_read(&root.join(&staged.path), 32 * 1024 * 1024)?;
        require(staged.sha256.0 == sha256(&staged_bytes) && image.bytes.len() == staged_bytes.len(), "staged image identity or length differs")?;
        for (offset, (old, new)) in image.bytes.iter().zip(&staged_bytes).enumerate() {
            let address = base + offset as u32;
            require(old == new || CARRIERS.iter().any(|(low, high, number)| *number == image_number && *low <= address && address < *high)
                || (image_number == 4 && (SENTINEL..SENTINEL + 4).contains(&address)), "staged image changes retained bytes")?;
        }
        image.bytes = staged_bytes;
    }
    require(expected(&images, SENTINEL)? == 0, "staged sentinel is not zero")?;
    let roots: BTreeSet<_> = contract.function_symbols.values().copied().collect();
    require(roots == contract.frame_metadata.iter().map(|frame| frame.entry).collect(), "artifact function roots differ from frame inventory")?;
    require(ENTRY_ADDRESSES.iter().all(|entry| roots.contains(entry))
        && roots.iter().all(|entry| contract.symbols.values().any(|value| value == entry)), "owned roots are not bound to entry symbols")?;
    for name in ["xm1_heap_malloc_r", "xm1_heap_free_r", "xm1_heap_realloc_r", "xm1_heap_calloc_r", "xm1_heap_trim_r"] {
        require(contract.symbols.get(name).is_some_and(|entry| roots.contains(entry)), "core export root absent")?;
    }
    let (owned, literals) = derive(&images, &roots)?;
    require(contract.owned_instruction_pcs == owned.iter().copied().collect::<Vec<_>>()
        && contract.literal_words.iter().map(|literal| (literal.address, literal.value)).collect::<BTreeMap<_, _>>() == literals,
        "artifact instruction or literal inventory differs from reconstructed bytes")?;
    require(!contract.conditional_premises.is_empty(), "artifact conditional scope absent")?;
    let redesigns: BTreeSet<_> = contract.redesign_items.iter().map(|item| item.id).collect();
    require(redesigns.len() == contract.redesign_items.len()
        && redesigns == [Redesign::CheckedCallocProduct, Redesign::OversizedResizeRejectedBeforeLock, Redesign::ZeroRequestMinimumAllocation, Redesign::TrimReturnsZero].into_iter().collect()
        && contract.redesign_items.iter().all(|item| [&item.stock, &item.owned, &item.why, &item.dependence_signal].iter().all(|text| !text.is_empty())),
        "artifact redesign items are not exactly the four approved differences")?;
    let metadata_address = *contract.symbols.get("XM1_HEAP_METADATA").ok_or("owned backend metadata symbol absent")?;
    geometry(&images, metadata_address, &contract.backend_geometry, &contract.arena)?;
    Ok(Admitted { contract_digest: Digest(sha256(&bytes)), candidate: ValidatedUpdate(candidate), images, owned, premises: contract.conditional_premises,
        arena: contract.arena })
}
