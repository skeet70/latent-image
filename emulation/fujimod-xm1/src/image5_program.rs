use super::{Catalog, CallTarget, Guest, SourceKind, StaticCall, decode_call};
use super::continuation_view::{ByteCount, Digest, FileBinding};
use super::image5_wire::{Artifacts, Binding, Role};
use fujimod_xm1::bridge::{read, write};
use serde::Deserialize;
use std::path::{Component, Path};

pub(super) const CARRIER: [u32; 2] = [0x6a96c0, 0x6a9844];
pub(super) const FILL_REQUEST: Guest = Guest(0x66609c);
pub(super) const FILL_CALLBACK: Guest = Guest(0x6a8bec);
pub(super) const FEED_CALLBACK: Guest = Guest(0x6ae500);
pub(super) const FILL_FLAG: Guest = Guest(0x89f8ac);
/// The marker writer's inner continuation in loader A.
pub(super) const STOCK_RETURN_LR: Guest = Guest(0x422b8);
const RETAINED_SERVICES: [u32; 9] = [0x42188, 0x42634, 0x657114, 0x657184, 0x66609c, 0x6a8f1c, 0x6ae360, 0x6ae3f4, 0x7221c8];

/// Reviewed owned-artifact identities.
struct OwnedIdentity { manifest: &'static str, linked: &'static str, symbols: &'static str, image: &'static str, update: &'static str }
const OWNED: OwnedIdentity = OwnedIdentity {
    manifest: "7de5562cd54a175b56ac31b1713d4cce2d3e68f710bc376da86c91107e894580",
    linked: "c4002c01c67a18647ffe5e0b2bc561af3e212c59c5ebcec0dac77000c6ea7703",
    symbols: "eebd59f77cf2a33eeb10e5b040e981df18c43099866e9626e21016a63d5952ce",
    image: "4808c1c321f3605fb36163614819d52efb11cf7f6ac6825f014b728dccb25f77",
    update: "c3c049a2e2c8ea833d44f42ae655e64d3084e1ad833257bc7d9035b6a6aabc9c",
};
const NOP: u32 = 0xe320f000;
const CANONICAL_IMAGE4: &str = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16";

#[derive(Deserialize)]
struct Call { pc: Guest, target: Guest }
#[derive(Deserialize)]
struct Producer { entry: Guest, rust_entry: Guest, replaced_extent: [Guest; 2], linked_bytes: ByteCount, linked_sha256: Digest, symbols_sha256: Digest, execution_pcs: Vec<Guest>, call_sites: Vec<Call>, return_pcs: Vec<Guest>, null_refusal_pc: Guest, scratch_low_from_entry_sp: i32, source_dat_image4_sha256: [Digest; 2], staged_image4_sha256: Digest, retained_calls: Vec<Guest>, fill_callback: Guest, feed_callback: Guest, fill_flag: Guest, return_lr: Guest }
#[derive(Deserialize)]
struct Manifest { schema: u32, candidate_dat_sha256: Digest, candidate_image4_sha256: Digest, image5_load: Producer }

struct Owned { rust_entry: Guest, return_pc: Guest, null_refusal: Guest, frame_bytes: u32, return_lr: Guest, execution: Vec<Guest>, update: &'static str }
pub(super) struct Program { binding: Binding, calls: Vec<StaticCall>, owned: Option<Owned> }
fn require(ok: bool, message: &str) -> Result<(), String> { if ok { Ok(()) } else { Err(message.into()) } }
fn bytes(root: &Path, binding: &FileBinding) -> Result<Vec<u8>, String> {
    require(!binding.path.as_os_str().is_empty() && binding.path.components().all(|part| matches!(part, Component::Normal(_))), "image5 artifact path is not relative")?;
    let value = read(&root.join(&binding.path))?;
    require(Digest::of(&value).as_str() == binding.sha256.as_str(), "image5 artifact bytes differ from binding")?;
    Ok(value)
}
fn carrier_offset(image: &super::SourceImage) -> Result<usize, String> { CARRIER[0].checked_sub(image.binding.start.0).map(|offset| offset as usize).ok_or_else(|| "image5 carrier precedes image4".to_owned()) }

impl Program {
    pub(super) fn from_staged(role: Role, stage: Option<&Path>, output: &Path, catalog: &mut Catalog) -> Result<Self, String> {
        if role == Role::Retained {
            require(stage.is_none(), "retained image5 witness takes no owned artifacts")?;
            return Self::from_binding(Binding { role, artifacts: None }, output, catalog);
        }
        let stage = stage.ok_or("owned image5 witness needs --image5-artifact")?;
        let retain = |source: &str, name: &str| -> Result<FileBinding, String> {
            let value = read(&stage.join(source))?;
            write(&output.join(name), &value)?;
            Ok(FileBinding { path: name.into(), sha256: Digest::of(&value) })
        };
        let manifest = retain("owned-image5-stage-v1-contract.json", "image5-contract.json")?;
        let linked = retain("image5-load-v1.bin", "image5-linked.bin")?;
        let symbols = retain("image5-load-v1.symbols", "image5-symbols.txt")?;
        let body = bytes(output, &linked)?;
        require(body.len() <= (CARRIER[1] - CARRIER[0]) as usize, "image5 linked producer exceeds its carrier")?;
        let source = catalog.0.iter().find(|image| image.binding.kind == SourceKind::Image4).ok_or("image5 source image4 absent")?;
        let low = carrier_offset(source)?;
        let mut image = source.bytes.clone();
        let carrier = image.get_mut(low..low + (CARRIER[1] - CARRIER[0]) as usize).ok_or("image5 carrier exceeds image4")?;
        for slot in carrier.chunks_exact_mut(4) { slot.copy_from_slice(&NOP.to_le_bytes()); }
        carrier[..body.len()].copy_from_slice(&body);
        write(&output.join("image5-image4.bin"), &image)?;
        let image4 = FileBinding { path: "image5-image4.bin".into(), sha256: Digest::of(&image) };
        Self::from_binding(Binding { role, artifacts: Some(Artifacts { manifest, linked, symbols, image4 }) }, output, catalog)
    }
    pub(super) fn from_binding(binding: Binding, root: &Path, catalog: &mut Catalog) -> Result<Self, String> {
        let mut calls = Vec::new();
        let owned = match (&binding.role, &binding.artifacts) {
            (Role::Retained, None) => {
                for pc in (CARRIER[0]..CARRIER[1]).step_by(4) {
                    if let Some(call) = decode_call(Guest(pc), catalog.word(Guest(pc))?.1)? { calls.push(call); }
                }
                None
            }
            (Role::Owned, Some(artifacts)) => {
                let reviewed = &OWNED;
                let raw = bytes(root, &artifacts.manifest)?;
                require(Digest::of(&raw).as_str() == reviewed.manifest, "image5 manifest is not the independently reviewed artifact")?;
                let manifest: Manifest = serde_json::from_slice(&raw).map_err(|error| error.to_string())?;
                let producer = manifest.image5_load;
                require(manifest.schema == 1 && manifest.candidate_dat_sha256.as_str() == reviewed.update && manifest.candidate_image4_sha256.as_str() == reviewed.image, "image5 candidate identity differs")?;
                let frame_bytes = u32::try_from(producer.scratch_low_from_entry_sp.checked_neg().ok_or("image5 frame low-water overflows")?).map_err(|_| "image5 frame low-water lies above the entry stack pointer")?;
                require(producer.entry == Guest(CARRIER[0]) && producer.rust_entry == Guest(0x6a96d4) && producer.replaced_extent == [Guest(CARRIER[0]), Guest(CARRIER[1])] && producer.return_pcs == [Guest(0x6a96d0)] && producer.null_refusal_pc == Guest(0x6a9790) && producer.scratch_low_from_entry_sp == -48 && frame_bytes == 48, "image5 emitted frame or boundaries differ")?;
                let mut services: Vec<Guest> = producer.retained_calls.clone();services.sort_unstable();services.dedup();
                require(services.iter().map(|target| target.0).eq(RETAINED_SERVICES) && producer.fill_callback == FILL_CALLBACK && producer.feed_callback == FEED_CALLBACK && producer.fill_flag == FILL_FLAG && producer.return_lr == STOCK_RETURN_LR, "image5 retained services or callback identities differ")?;
                require(producer.linked_sha256.as_str() == reviewed.linked && producer.symbols_sha256.as_str() == reviewed.symbols && producer.staged_image4_sha256.as_str() == reviewed.image && producer.source_dat_image4_sha256[0].as_str() == super::loader_orchestration_owned::SOURCE_UPDATE, "image5 source/artifact identity differs")?;
                let linked = bytes(root, &artifacts.linked)?;
                let symbols = bytes(root, &artifacts.symbols)?;
                let staged = bytes(root, &artifacts.image4)?;
                require(Digest::of(&linked).as_str() == reviewed.linked && linked.len() == producer.linked_bytes.0 as usize && Digest::of(&symbols).as_str() == reviewed.symbols && Digest::of(&staged).as_str() == reviewed.image, "image5 retained artifact identity differs")?;
                let image = catalog.0.iter_mut().find(|image| image.binding.kind == SourceKind::Image4).ok_or("image5 canonical image4 absent")?;
                let low = carrier_offset(image)?;
                let high = low + (CARRIER[1] - CARRIER[0]) as usize;
                let padded = staged.get(low + linked.len()..high).is_some_and(|tail| linked.len() % 4 == 0 && tail.chunks_exact(4).all(|word| word == NOP.to_le_bytes()));
                require(producer.source_dat_image4_sha256[1].as_str() == image.binding.sha256.as_str() && staged.len() == image.bytes.len() && staged[..low] == image.bytes[..low] && staged[high..] == image.bytes[high..] && staged.get(low..low + linked.len()) == Some(linked.as_slice()) && padded, "image5 image4 changes bytes outside reviewed carrier")?;
                let mut emitted = Vec::new();
                for (index, bytes) in linked.chunks_exact(4).enumerate() {
                    let pc = Guest(CARRIER[0] + 4 * index as u32);
                    let word = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                    require(word >> 25 != 0x7d, "image5 linked code contains an immediate BLX")?;
                    match decode_call(pc, word)? {
                        Some(StaticCall { target: CallTarget::Register(_), .. }) => return Err("image5 linked code contains a register-indirect call".into()),
                        Some(call) if matches!(call.target, CallTarget::Direct(target) if !(CARRIER[0]..CARRIER[1]).contains(&target.0)) => emitted.push(call),
                        _ => {}
                    }
                }
                require(emitted.len() == producer.call_sites.len() && emitted.iter().zip(&producer.call_sites).all(|(decoded, call)| decoded.site == call.pc && matches!(decoded.target, CallTarget::Direct(target) if target == call.target) && RETAINED_SERVICES.contains(&call.target.0)), "image5 emitted external calls differ from the manifest")?;
                calls = emitted;
                let inside = |pc: Guest| (CARRIER[0]..CARRIER[1]).contains(&pc.0) && pc.0 % 4 == 0;
                require(producer.execution_pcs.iter().copied().all(inside) && inside(producer.rust_entry) && inside(producer.return_pcs[0]) && inside(producer.null_refusal_pc) && producer.execution_pcs.contains(&producer.rust_entry) && producer.execution_pcs.contains(&producer.return_pcs[0]), "image5 emitted execution points differ")?;
                image.binding.sha256 = Digest::of(&staged);
                image.bytes = staged;
                Some(Owned { rust_entry: producer.rust_entry, return_pc: producer.return_pcs[0], null_refusal: producer.null_refusal_pc, frame_bytes, return_lr: producer.return_lr, execution: producer.execution_pcs, update: reviewed.update })
            }
            _ => return Err("image5 role and artifact binding disagree".into()),
        };
        Ok(Self { binding, calls, owned })
    }
    pub(super) fn binding(&self) -> Binding { self.binding.clone() }
    pub(super) fn role(&self) -> Role { self.binding.role }
    pub(super) fn owned(&self) -> bool { self.owned.is_some() }
    pub(super) fn update(&self) -> &'static str { self.owned.as_ref().map_or(super::loader_orchestration_owned::SOURCE_UPDATE, |owned| owned.update) }
    pub(super) fn image4(&self) -> &'static str { if self.owned() { OWNED.image } else { CANONICAL_IMAGE4 } }
    pub(super) fn call_sites(&self) -> Vec<StaticCall> { self.calls.clone() }
    pub(super) fn is_call_site(&self, pc: Guest) -> bool { self.calls.iter().any(|call| call.site == pc) }
    pub(super) fn return_pc(&self) -> Guest { self.owned.as_ref().map_or(Guest(0x6a9840), |owned| owned.return_pc) }
    pub(super) fn return_lr(&self) -> Guest { self.owned.as_ref().map_or(STOCK_RETURN_LR, |owned| owned.return_lr) }
    pub(super) fn rust_entry(&self) -> Option<Guest> { self.owned.as_ref().map(|owned| owned.rust_entry) }
    pub(super) fn null_refusal(&self) -> Guest { self.owned.as_ref().map_or(Guest(0x6a96dc), |owned| owned.null_refusal) }
    /// Bytes below the entry stack pointer that the producer may write.
    pub(super) fn frame_bytes(&self) -> u32 { self.owned.as_ref().map_or(40, |owned| owned.frame_bytes) }
    pub(super) fn fill_return_pc(&self) -> Option<Guest> {
        self.calls.iter().find(|call| matches!(call.target, CallTarget::Direct(target) if target == FILL_REQUEST)).map(|call| call.continuation)
    }
    pub(super) fn returned_target(&self, pc: Guest) -> Option<Guest> {
        self.calls.iter().find(|call| call.continuation == pc).and_then(|call| match call.target { CallTarget::Direct(target) => Some(target), _ => None })
    }
    pub(super) fn execution_allowed(&self, pc: Guest) -> bool {
        let physical = pc.0 & !0x40000000;
        if !(CARRIER[0]..CARRIER[1]).contains(&physical) { return true; }
        pc.0 == physical && match &self.owned { Some(owned) => owned.execution.contains(&pc), None => true }
    }
    /// Callee-saved words the producer's pushes store below the entry stack pointer, as (address, value at entry). The owned adapter pushes `{r11,lr}`; the reviewed Rust body pushes `{r2-r9,r11,lr}` below it, where `r2`/`r3` are padding and `[sp-48]` later carries the fifth storage-read argument.
    pub(super) fn saved_slots(&self, sp: u32, registers: &[u32; 13], lr: Guest) -> Vec<(u32, u32)> {
        if self.owned() { vec![(sp - 40, registers[4]), (sp - 36, registers[5]), (sp - 32, registers[6]), (sp - 28, registers[7]), (sp - 24, registers[8]), (sp - 20, registers[9]), (sp - 16, registers[11]), (sp - 12, CARRIER[0] + 8), (sp - 8, registers[11]), (sp - 4, lr.0)] }
        else { vec![(sp - 28, registers[4]), (sp - 24, registers[5]), (sp - 20, registers[6]), (sp - 16, registers[7]), (sp - 12, registers[8]), (sp - 8, registers[11]), (sp - 4, lr.0)] }
    }
}
