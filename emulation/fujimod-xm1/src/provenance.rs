//! Where each guest address of the run document comes from: an X-M1 1.50
//! word or A32 instruction that forms it, or FujiMod's EXR II catalog.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fmt, path::Path};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
pub struct Guest(pub u32);

impl fmt::Display for Guest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#010x}", self.0)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
pub struct ImageName(pub String);

/// A decoded X-M1 1.50 image under `private-data/analysis/x-m1-1.50`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageSource {
    pub name: ImageName,
    pub path: String,
    /// Guest address of the image's first byte.
    pub load: Guest,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Binding {
    /// The aligned word of `image` at `at` holds `value`.
    Word { image: ImageName, at: Guest, value: Guest },
    /// The A32 instruction of `image` at `at` forms `value` in a register or
    /// as a load/store address.
    Code { image: ImageName, at: Guest, value: Guest },
    /// FujiMod's EXR II catalog value; no listed image forms it.
    FujimodCatalogUnchecked,
    /// A register of the retained engine's device model; no listed image forms it.
    RetainedModelUnchecked,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowProvenance {
    pub node: String,
    pub base: Guest,
    pub size: u32,
    pub binding: Binding,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum RamFact {
    CurrentMenu,
    CalibrationPointer,
    ScannerRegistered,
    KeyTable,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RamProvenance {
    pub fact: RamFact,
    pub binding: Binding,
}

/// One register of a node's device model, at its exact address.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterProvenance {
    pub node: String,
    pub address: Guest,
    pub binding: Binding,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub images: Vec<ImageSource>,
    pub windows: Vec<WindowProvenance>,
    pub ram: Vec<RamProvenance>,
    pub registers: Vec<RegisterProvenance>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Window {
    pub base: Guest,
    pub size: u32,
}

impl Window {
    fn contains(self, value: Guest) -> bool {
        value.0.wrapping_sub(self.base.0) < self.size
    }
}

#[derive(Deserialize)]
struct PlanWindow {
    base: u64,
    size: u64,
}

#[derive(Deserialize)]
struct PlanNode {
    name: String,
    windows: Vec<PlanWindow>,
}

#[derive(Deserialize)]
struct PlanNodes {
    nodes: Vec<PlanNode>,
}

/// The `(node, window)` pairs of a FujiMod EXR II device plan.
pub fn plan_windows(devices: &str) -> Result<Vec<(String, Window)>, String> {
    let plan: PlanNodes = serde_json::from_str(devices).map_err(|e| format!("device plan windows: {e}"))?;
    let mut windows = Vec::new();
    for node in plan.nodes {
        for window in node.windows {
            let base = u32::try_from(window.base).map_err(|_| format!("{} window base exceeds 32 bits", node.name))?;
            let size = u32::try_from(window.size).map_err(|_| format!("{} window size exceeds 32 bits", node.name))?;
            windows.push((node.name.clone(), Window { base: Guest(base), size }));
        }
    }
    Ok(windows)
}

impl Provenance {
    /// Refuses provenance that does not name every plan window, RAM fact and
    /// modeled register exactly once, or a binding to an unlisted image or
    /// outside its target.
    pub fn validate(
        &self,
        windows: &[(String, Window)],
        ram: &[(RamFact, Guest)],
        registers: &[(&str, Guest)],
    ) -> Result<(), String> {
        let images: BTreeSet<&ImageName> = self.images.iter().map(|image| &image.name).collect();
        if images.len() != self.images.len() {
            return Err("provenance lists an image twice".into());
        }
        let declared: BTreeSet<(&str, Window)> = self
            .windows
            .iter()
            .map(|entry| (entry.node.as_str(), Window { base: entry.base, size: entry.size }))
            .collect();
        let planned: BTreeSet<(&str, Window)> = windows.iter().map(|(node, window)| (node.as_str(), *window)).collect();
        if declared.len() != self.windows.len() || declared != planned {
            return Err("provenance must name each plan window exactly once".into());
        }
        let facts: BTreeSet<RamFact> = self.ram.iter().map(|entry| entry.fact).collect();
        if facts.len() != self.ram.len() || facts != ram.iter().map(|(fact, _)| *fact).collect() {
            return Err("provenance must name each RAM fact exactly once".into());
        }
        let named: BTreeSet<(&str, Guest)> = self.registers.iter().map(|entry| (entry.node.as_str(), entry.address)).collect();
        if named.len() != self.registers.len() || named != registers.iter().copied().collect() {
            return Err("provenance must name each modeled register exactly once".into());
        }
        if let Some(entry) = self.registers.iter().find(|entry| {
            !windows.iter().any(|(node, window)| *node == entry.node && window.contains(entry.address))
        }) {
            return Err(format!("{} register {} lies in none of its windows", entry.node, entry.address));
        }
        let check = |binding: &Binding, target: &dyn Fn(Guest) -> bool, what: &str| match binding {
            Binding::Word { image, value, .. } | Binding::Code { image, value, .. } => {
                if !images.contains(image) {
                    Err(format!("{what} names unlisted image {}", image.0))
                } else if !target(*value) {
                    Err(format!("{what} binding value {value} is outside its target"))
                } else {
                    Ok(())
                }
            }
            Binding::FujimodCatalogUnchecked | Binding::RetainedModelUnchecked => Ok(()),
        };
        for entry in &self.windows {
            let window = Window { base: entry.base, size: entry.size };
            check(&entry.binding, &|value| window.contains(value), &format!("{} window {}", entry.node, entry.base))?;
        }
        for entry in &self.ram {
            let address = ram.iter().find(|(fact, _)| *fact == entry.fact).map(|(_, address)| *address);
            check(&entry.binding, &|value| Some(value) == address, &format!("{:?}", entry.fact))?;
        }
        for entry in &self.registers {
            check(&entry.binding, &|value| value == entry.address, &format!("{} register {}", entry.node, entry.address))?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Form {
    Word,
    Code,
}

/// A value an image holds as an aligned word or forms in A32 code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Reference {
    pub form: Form,
    pub at: Guest,
    pub value: Guest,
}

/// Rotated A32 modified immediate.
fn immediate(word: u32) -> u32 {
    (word & 0xff).rotate_right(2 * ((word >> 8) & 0xf))
}

fn register(word: u32, shift: u32) -> usize {
    ((word >> shift) & 0xf) as usize
}

struct Registers([Option<u32>; 16]);

impl Registers {
    fn clear(&mut self) {
        self.0 = [None; 16];
    }

    /// r0-r3, r12 and lr after a call.
    fn call(&mut self) {
        for index in [0, 1, 2, 3, 12, 14] {
            self.0[index] = None;
        }
    }

    fn get(&self, index: usize) -> Option<u32> {
        if index == 15 { None } else { self.0[index] }
    }

    /// A write by an instruction that may not execute keeps no constant.
    fn set(&mut self, index: usize, value: Option<u32>, always: bool) {
        if index == 15 {
            self.clear();
        } else {
            self.0[index] = value.filter(|_| always);
        }
    }
}

/// Linear A32 sweep keeping register constants formed by PC-relative literal
/// loads, MOVW/MOVT, MOV/MVN and ADD/SUB/ORR/BIC/EOR immediates. A load or
/// store forms its base, and its address when the offset is an immediate or
/// an unshifted register holding a constant. Constants are dropped at
/// prologues, calls, returns and unconditional branches, and on any other
/// write of their register. Returns the references `wanted` admits.
pub fn references(load: Guest, bytes: &[u8], wanted: &dyn Fn(Guest) -> bool) -> Vec<Reference> {
    let words: Vec<u32> =
        bytes.chunks_exact(4).map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])).collect();
    let word_at = |address: u32| -> Option<u32> {
        let offset = address.checked_sub(load.0)?;
        if offset % 4 != 0 {
            return None;
        }
        words.get(usize::try_from(offset / 4).ok()?).copied()
    };
    let mut found = Vec::new();
    let mut registers = Registers([None; 16]);
    for (index, &word) in words.iter().enumerate() {
        let at = load.0.wrapping_add(4 * index as u32);
        let mut note = |form: Form, value: u32| {
            if wanted(Guest(value)) {
                found.push(Reference { form, at: Guest(at), value: Guest(value) });
            }
        };
        note(Form::Word, word);
        let pc = at.wrapping_add(8);
        let condition = word >> 28;
        let always = condition == 0xe;
        let rd = register(word, 12);
        let rn = register(word, 16);
        if condition == 0xf {
            if word & 0x0e00_0000 == 0x0a00_0000 {
                registers.call();
            } else if word & 0x0e50_0000 == 0x0810_0000 {
                registers.clear();
            }
            continue;
        }
        if word & 0x0fff_ffd0 == 0x012f_ff10 {
            if word & 0x20 != 0 {
                registers.call();
            } else if always {
                registers.clear();
            }
        } else if word & 0x0e00_0000 == 0x0a00_0000 {
            if word & 0x0100_0000 != 0 {
                registers.call();
            } else if always {
                registers.clear();
            }
        } else if word & 0x0ff0_0000 == 0x0300_0000 {
            let value = ((word >> 4) & 0xf000) | (word & 0xfff);
            note(Form::Code, value);
            registers.set(rd, Some(value), always);
        } else if word & 0x0ff0_0000 == 0x0340_0000 {
            let value = registers.get(rd).map(|low| (low & 0xffff) | (((word >> 4) & 0xf000) | (word & 0xfff)) << 16);
            if let Some(value) = value {
                note(Form::Code, value);
            }
            registers.set(rd, value, always);
        } else if word & 0x0e00_0000 == 0x0200_0000 {
            let operand = immediate(word);
            let base = registers.get(rn);
            let value = match (word >> 21) & 0xf {
                0x1 => base.map(|base| base ^ operand),
                0x2 => base.map(|base| base.wrapping_sub(operand)),
                0x4 => base.map(|base| base.wrapping_add(operand)),
                0xc => base.map(|base| base | operand),
                0xd => Some(operand),
                0xe => base.map(|base| base & !operand),
                0xf => Some(!operand),
                0x8..=0xb => continue,
                _ => None,
            };
            if let Some(value) = value {
                note(Form::Code, value);
            }
            registers.set(rd, value, always);
        } else if word & 0x0fef_0ff0 == 0x01a0_0000 {
            let value = registers.get(register(word, 0));
            registers.set(rd, value, always);
        } else if word & 0x0c00_0000 == 0x0400_0000 {
            let load_bit = word & 0x0010_0000 != 0;
            let pre = word & 0x0100_0000 != 0;
            let up = word & 0x0080_0000 != 0;
            let writeback = !pre || word & 0x0020_0000 != 0;
            if word & 0x0200_0000 == 0 {
                let offset = word & 0xfff;
                let base = if rn == 15 { Some(pc) } else { registers.get(rn) };
                let moved = base.map(|base| if up { base.wrapping_add(offset) } else { base.wrapping_sub(offset) });
                let address = if pre { moved } else { base };
                if rn == 15 {
                    let literal = address.and_then(word_at).filter(|_| word & 0x0040_0000 == 0 && load_bit);
                    if let Some(literal) = literal {
                        note(Form::Code, literal);
                    }
                    registers.set(rd, literal, always);
                    continue;
                }
                if let Some(address) = address {
                    note(Form::Code, address);
                }
                if writeback {
                    registers.set(rn, moved, always);
                }
            } else {
                let base = registers.get(rn);
                if let Some(base) = base {
                    note(Form::Code, base);
                }
                let offset = registers.get(register(word, 0)).filter(|_| word & 0xff0 == 0);
                if let (Some(base), Some(offset), true) = (base, offset, pre) {
                    note(Form::Code, if up { base.wrapping_add(offset) } else { base.wrapping_sub(offset) });
                }
                if writeback {
                    registers.set(rn, None, true);
                }
            }
            if load_bit {
                registers.set(rd, None, true);
            }
        } else if word & 0x0e00_0090 == 0x0000_0090 && word & 0x60 != 0 {
            let pre = word & 0x0100_0000 != 0;
            let up = word & 0x0080_0000 != 0;
            let writeback = !pre || word & 0x0020_0000 != 0;
            let base = registers.get(rn);
            if word & 0x0040_0000 != 0 {
                let offset = ((word >> 4) & 0xf0) | (word & 0xf);
                let moved = base.map(|base| if up { base.wrapping_add(offset) } else { base.wrapping_sub(offset) });
                if let Some(address) = if pre { moved } else { base } {
                    note(Form::Code, address);
                }
                if writeback {
                    registers.set(rn, moved, always);
                }
            } else {
                if let Some(base) = base {
                    note(Form::Code, base);
                }
                if let (Some(base), Some(offset), true) = (base, registers.get(register(word, 0)), pre) {
                    note(Form::Code, if up { base.wrapping_add(offset) } else { base.wrapping_sub(offset) });
                }
                if writeback {
                    registers.set(rn, None, true);
                }
            }
            if word & 0x0010_0000 != 0 {
                registers.set(rd, None, true);
            } else if word & 0x60 == 0x40 {
                registers.set(rd, None, true);
                registers.set((rd + 1) & 0xf, None, true);
            }
        } else if word & 0x0e00_0000 == 0x0800_0000 {
            if let Some(base) = registers.get(rn) {
                note(Form::Code, base);
            }
            let list = word & 0xffff;
            let load_bit = word & 0x0010_0000 != 0;
            if !load_bit && rn == 13 && list & (1 << 14) != 0 {
                registers.clear();
                continue;
            }
            if word & 0x0020_0000 != 0 {
                registers.set(rn, None, true);
            }
            if load_bit {
                if list & (1 << 15) != 0 {
                    if always {
                        registers.clear();
                    }
                } else {
                    for index in (0..15).filter(|index| list & (1 << index) != 0) {
                        registers.set(index, None, true);
                    }
                }
            }
        } else {
            registers.set(rd, None, true);
            registers.set(rn, None, true);
        }
    }
    found
}

pub enum Target<'a> {
    Window { node: &'a str, window: Window },
    Ram { fact: RamFact, address: Guest },
    Register { node: &'a str, address: Guest },
}

impl Target<'_> {
    fn matches(&self, value: Guest) -> bool {
        match self {
            Target::Window { window, .. } => window.contains(value),
            Target::Ram { address, .. } | Target::Register { address, .. } => value == *address,
        }
    }
}

impl fmt::Display for Target<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::Window { node, window } => write!(f, "{node} window {} +{:#x}", window.base, window.size),
            Target::Ram { fact, address } => write!(f, "{fact:?} {address}"),
            Target::Register { node, address } => write!(f, "{node} register {address}"),
        }
    }
}

/// The references of each listed image, after checking its digest.
pub fn image_references(
    provenance: &Provenance,
    analysis: &Path,
    wanted: &dyn Fn(Guest) -> bool,
) -> Result<Vec<(ImageName, Vec<Reference>)>, String> {
    let mut all = Vec::new();
    for image in &provenance.images {
        let path = analysis.join(&image.path);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        if digest != image.sha256 {
            return Err(format!("{} has sha256 {digest}, not {}", path.display(), image.sha256));
        }
        all.push((image.name.clone(), references(image.load, &bytes, wanted)));
    }
    Ok(all)
}

fn matching<'a>(
    references: &'a [(ImageName, Vec<Reference>)],
    target: &'a Target<'a>,
    image: &'a ImageName,
) -> impl Iterator<Item = &'a Reference> + 'a {
    references
        .iter()
        .filter(move |(name, _)| name == image)
        .flat_map(|(_, found)| found.iter())
        .filter(move |reference| target.matches(reference.value))
}

/// A word or code binding must be one of its image's references; an
/// unchecked target must have none. A failing unchecked target names the
/// reference to bind: an exact base before an inner address, code before a word.
pub fn check(target: &Target, binding: &Binding, references: &[(ImageName, Vec<Reference>)]) -> Result<String, String> {
    match binding {
        Binding::Word { image, at, value } | Binding::Code { image, at, value } => {
            let form = if matches!(binding, Binding::Word { .. }) { Form::Word } else { Form::Code };
            let wanted = Reference { form, at: *at, value: *value };
            if matching(references, target, image).any(|reference| *reference == wanted) {
                let count = references.iter().map(|(name, _)| matching(references, target, name).count()).sum::<usize>();
                Ok(format!("{target}: {} {at} forms {value} ({count} references)", image.0))
            } else {
                Err(format!("{target}: {} {at} does not form {value}", image.0))
            }
        }
        Binding::FujimodCatalogUnchecked | Binding::RetainedModelUnchecked => {
            let exact = |value: Guest| match target {
                Target::Window { window, .. } => value == window.base,
                Target::Ram { .. } | Target::Register { .. } => true,
            };
            let best = references
                .iter()
                .enumerate()
                .flat_map(|(order, (name, _))| matching(references, target, name).map(move |reference| (order, name, *reference)))
                .min_by_key(|(order, _, reference)| (!exact(reference.value), reference.form == Form::Word, *order, reference.at));
            let source = if *binding == Binding::RetainedModelUnchecked { "retained model register" } else { "FujiMod catalog value" };
            match best {
                None => Ok(format!("{target}: {source}, no X-M1 reference")),
                Some((_, name, reference)) => Err(format!(
                    "{target} is marked unchecked, but {} {} forms {} ({:?})",
                    name.0, reference.at, reference.value, reference.form
                )),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|word| word.to_le_bytes()).collect()
    }

    fn code(words: &[u32]) -> Vec<(u32, u32)> {
        references(Guest(0x1000), &image(words), &|value| value.0 >= 0xfff0_0000)
            .into_iter()
            .filter(|reference| reference.form == Form::Code)
            .map(|reference| (reference.at.0, reference.value.0))
            .collect()
    }

    #[test]
    fn literal_base_and_store_offset_are_formed() {
        // ldr r4, [pc, #4]; str r0, [r4, #0x40]; bx lr; .word 0xfff43000
        assert_eq!(code(&[0xe59f_4004, 0xe584_0040, 0xe12f_ff1e, 0xfff4_3000]), vec![(0x1000, 0xfff4_3000), (0x1004, 0xfff4_3040)]);
    }

    #[test]
    fn mvn_base_with_negative_offset_is_formed() {
        // mvn r3, #0xbc000 (0xfff43fff); ldr r0, [r3, #-0xfff]
        assert_eq!(code(&[0xe3e0_3abc, 0xe513_0fff]), vec![(0x1000, 0xfff4_3fff), (0x1004, 0xfff4_3000)]);
    }

    #[test]
    fn movw_movt_pair_is_formed() {
        // movw r1, #0x3000; movt r1, #0xfff4
        assert_eq!(code(&[0xe303_1000, 0xe34f_1ff4]), vec![(0x1004, 0xfff4_3000)]);
    }

    #[test]
    fn calls_and_prologues_drop_scratch_constants() {
        // mvn r0, #0xbc000; bl .; str r1, [r0]
        assert_eq!(code(&[0xe3e0_0abc, 0xebff_fffe, 0xe580_1000]), vec![(0x1000, 0xfff4_3fff)]);
        // mvn r4, #0xbc000; bl .; str r1, [r4]
        assert_eq!(code(&[0xe3e0_4abc, 0xebff_fffe, 0xe584_1000]).len(), 2);
        // mvn r0, #0xbc000; push {r4, lr}; str r1, [r0]
        assert_eq!(code(&[0xe3e0_0abc, 0xe92d_4010, 0xe580_1000]), vec![(0x1000, 0xfff4_3fff)]);
    }

    #[test]
    fn conditional_writes_do_not_leave_constants() {
        // mvneq r0, #0xbc000; str r1, [r0]
        assert_eq!(code(&[0x03e0_0abc, 0xe580_1000]), vec![(0x1000, 0xfff4_3fff)]);
    }

    #[test]
    fn constant_register_offsets_form_their_address() {
        // mvn r2, #0xaf000 (0xfff50fff); mvn r3, #0xff0 (0xfffff00f); strh r0, [r2, r3]
        let expected = vec![(0x1000, 0xfff5_0fff), (0x1004, 0xffff_f00f), (0x1008, 0xfff5_0fff), (0x1008, 0xfff5_000e)];
        assert_eq!(code(&[0xe3e0_2aaf, 0xe3e0_3eff, 0xe182_00b3]), expected);
        // mvn r3, #0xaf000; mov r1, #0x24; ldr r0, [r3, -r1]
        assert_eq!(code(&[0xe3e0_3aaf, 0xe3a0_1024, 0xe713_0001]), vec![(0x1000, 0xfff5_0fff), (0x1008, 0xfff5_0fff), (0x1008, 0xfff5_0fdb)]);
        // mvn r3, #0xaf000; ldr r0, [r3, r1, lsl #2] with r1 unknown
        assert_eq!(code(&[0xe3e0_3aaf, 0xe793_0101]), vec![(0x1000, 0xfff5_0fff), (0x1004, 0xfff5_0fff)]);
    }

    fn provenance(binding: Binding) -> Provenance {
        Provenance {
            images: vec![ImageSource { name: ImageName("id4".into()), path: "id4.bin".into(), load: Guest(0x1000), sha256: String::new() }],
            windows: vec![WindowProvenance { node: "lz".into(), base: Guest(0xfff4_3000), size: 0x100, binding }],
            ram: Vec::new(),
            registers: vec![RegisterProvenance {
                node: "lz".into(),
                address: Guest(0xfff4_3040),
                binding: Binding::RetainedModelUnchecked,
            }],
        }
    }

    #[test]
    fn validation_requires_every_window_and_register_and_in_target_values() {
        let window = ("lz".to_owned(), Window { base: Guest(0xfff4_3000), size: 0x100 });
        let registers = [("lz", Guest(0xfff4_3040))];
        let bound = |value| Binding::Code { image: ImageName("id4".into()), at: Guest(0x1000), value: Guest(value) };
        assert!(provenance(bound(0xfff4_3040)).validate(std::slice::from_ref(&window), &[], &registers).is_ok());
        assert!(provenance(bound(0xfff4_3100)).validate(std::slice::from_ref(&window), &[], &registers).is_err());
        assert!(provenance(bound(0xfff4_3040)).validate(std::slice::from_ref(&window), &[], &[]).is_err());
        let mut misbound = provenance(bound(0xfff4_3040));
        misbound.registers[0].binding = bound(0xfff4_3044);
        assert!(misbound.validate(std::slice::from_ref(&window), &[], &registers).is_err());
        let other = ("lz".to_owned(), Window { base: Guest(0xfff4_4000), size: 0x100 });
        assert!(provenance(Binding::FujimodCatalogUnchecked).validate(&[window, other], &[], &registers).is_err());
    }

    #[test]
    fn unchecked_registers_fail_once_code_forms_them() {
        let target = Target::Register { node: "sd", address: Guest(0xfff5_000e) };
        let words = [0xe3e0_2aaf, 0xe3e0_3eff, 0xe182_00b3];
        let found = vec![(ImageName("id4".into()), references(Guest(0x1000), &image(&words), &|value| value.0 == 0xfff5_000e))];
        assert!(check(&target, &Binding::RetainedModelUnchecked, &found).is_err());
        let bound = Binding::Code { image: ImageName("id4".into()), at: Guest(0x1008), value: Guest(0xfff5_000e) };
        assert!(check(&target, &bound, &found).is_ok());
    }

    #[test]
    fn unchecked_targets_fail_once_code_forms_them() {
        let window = Window { base: Guest(0xfff4_3000), size: 0x100 };
        let target = Target::Window { node: "lz", window };
        let found = references(Guest(0x1000), &image(&[0xe3e0_3abc, 0xe513_0fff]), &|value| window.contains(value));
        let found = vec![(ImageName("id4".into()), found)];
        assert!(check(&target, &Binding::FujimodCatalogUnchecked, &found).is_err());
        let bound = Binding::Code { image: ImageName("id4".into()), at: Guest(0x1004), value: Guest(0xfff4_3000) };
        assert!(check(&target, &bound, &found).is_ok());
        let wrong = Binding::Code { image: ImageName("id4".into()), at: Guest(0x1000), value: Guest(0xfff4_3000) };
        assert!(check(&target, &wrong, &found).is_err());
    }
}
