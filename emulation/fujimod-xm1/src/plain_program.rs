use super::{Catalog, CallTarget, Guest, SourceKind, StaticCall, decode_call};
use super::continuation_view::{ByteCount, Digest, FileBinding};
use super::plain_wire::{Artifacts, Binding, Role};
use fujimod_xm1::bridge::{read, write};
use serde::Deserialize;
use std::path::{Component, Path};

const MANIFEST: &str = "3cbcc9c1d3b9f4aeb8fb44fec36ea6d5394aa0959ae3ab0dce17aa01c7bfe42c";
const LINKED: &str = "4ad7c644908af1ccb1df1fedf70d123b70fd77e25762619fb01c234757117540";
const SYMBOLS: &str = "3a5509acacc25ed4fe12612818db10fe35af9126b423402e7da2ddd5705d66e4";
const IMAGE: &str = "796538ad3e48509f5032501721f6399abc685d710b6b9407b960ab28c272e01a";
const OWNED_UPDATE: &str = "b9a3f1a1c74940bdbcbadb97acf65091f19dad2bc83c24adac7eda9df2eb0af3";
#[derive(Deserialize)]
struct Call { pc: Guest, target: Guest }
#[derive(Deserialize)]
struct Producer { entry: Guest, rust_entry: Guest, replaced_extent: [Guest; 2], linked_bytes: ByteCount, linked_sha256: Digest, symbols_sha256: Digest, execution_pcs: Vec<Guest>, call_sites: Vec<Call>, return_pcs: Vec<Guest>, null_refusal_pc: Guest, scratch_low_from_entry_sp: i32, source_dat_image4_sha256: [Digest; 2], staged_image4_sha256: Digest }
#[derive(Deserialize)]
struct Manifest { schema: u32, candidate_dat_sha256: Digest, candidate_image4_sha256: Digest, plain_load: Producer }
pub(super) struct Program { binding: Binding, calls: Vec<StaticCall>, execution: Vec<Guest> }
fn require(ok: bool, message: &str) -> Result<(), String> { if ok { Ok(()) } else { Err(message.into()) } }
fn bytes(root: &Path, binding: &FileBinding) -> Result<Vec<u8>, String> {
    require(!binding.path.as_os_str().is_empty() && binding.path.components().all(|part| matches!(part, Component::Normal(_))), "plain artifact path is not relative")?;
    let value = read(&root.join(&binding.path))?;
    require(Digest::of(&value).as_str() == binding.sha256.as_str(), "plain artifact bytes differ from binding")?;
    Ok(value)
}
impl Program {
    pub(super) fn from_staged(role: Role, stage: Option<&Path>, output: &Path, catalog: &mut Catalog) -> Result<Self, String> {
        if role == Role::Retained {
            require(stage.is_none(), "retained plain witness takes no owned artifacts")?;
            return Self::from_binding(Binding { role, artifacts: None }, output, catalog);
        }
        let stage = stage.ok_or("owned plain witness needs --plain-artifact")?;
        let retain = |source: &str, name: &str| -> Result<FileBinding, String> {
            let value = read(&stage.join(source))?;
            write(&output.join(name), &value)?;
            Ok(FileBinding { path: name.into(), sha256: Digest::of(&value) })
        };
        let manifest = retain("owned-plain-stage-v1-contract.json", "plain-contract.json")?;
        let linked = retain("plain-load-v1.bin", "plain-linked.bin")?;
        let symbols = retain("plain-load-v1.symbols", "plain-symbols.txt")?;
        let body = bytes(output, &linked)?;
        require(body.len() == 244 && Digest::of(&body).as_str() == LINKED, "plain linked producer differs")?;
        let mut image = catalog.0.iter().find(|image| image.binding.kind == SourceKind::Image4).ok_or("plain source image4 absent")?.bytes.clone();
        image.get_mut(0x6a9844-0x61c400..0x6a9938-0x61c400).ok_or("plain carrier exceeds image4")?.copy_from_slice(&body);
        write(&output.join("plain-image4.bin"), &image)?;
        let image4 = FileBinding { path: "plain-image4.bin".into(), sha256: Digest::of(&image) };
        Self::from_binding(Binding { role, artifacts: Some(Artifacts { manifest, linked, symbols, image4 }) }, output, catalog)
    }
    pub(super) fn from_binding(binding: Binding, root: &Path, catalog: &mut Catalog) -> Result<Self, String> {
        let mut calls = Vec::new();
        let mut execution = Vec::new();
        match (&binding.role, &binding.artifacts) {
            (Role::Retained, None) => {
                for pc in (0x6a9844..0x6a9934).step_by(4) {
                    if let Some(call) = decode_call(Guest(pc), catalog.word(Guest(pc))?.1)? { calls.push(call); }
                }
            }
            (Role::Owned, Some(artifacts)) => {
                let raw = bytes(root, &artifacts.manifest)?;
                require(Digest::of(&raw).as_str() == MANIFEST, "plain manifest is not the independently reviewed artifact")?;
                let manifest: Manifest = serde_json::from_slice(&raw).map_err(|error| error.to_string())?;
                let producer = manifest.plain_load;
                require(manifest.schema == 1 && manifest.candidate_dat_sha256.as_str() == OWNED_UPDATE && manifest.candidate_image4_sha256.as_str() == IMAGE, "plain candidate identity differs")?;
                require(producer.entry == Guest(0x6a9844) && producer.rust_entry == Guest(0x6a9854) && producer.replaced_extent == [Guest(0x6a9844), Guest(0x6a9938)] && producer.linked_bytes.0 == 244 && producer.return_pcs == [Guest(0x6a9850)] && producer.null_refusal_pc == Guest(0x6a98d4) && producer.scratch_low_from_entry_sp == -40, "plain emitted frame or boundaries differ")?;
                require(producer.linked_sha256.as_str() == LINKED && producer.symbols_sha256.as_str() == SYMBOLS && producer.staged_image4_sha256.as_str() == IMAGE && producer.source_dat_image4_sha256[0].as_str() == super::loader_orchestration_owned::SOURCE_UPDATE, "plain source/artifact identity differs")?;
                let linked = bytes(root, &artifacts.linked)?;
                let symbols = bytes(root, &artifacts.symbols)?;
                let staged = bytes(root, &artifacts.image4)?;
                require(Digest::of(&linked).as_str() == LINKED && linked.len() == 244 && Digest::of(&symbols).as_str() == SYMBOLS && Digest::of(&staged).as_str() == IMAGE, "plain retained artifact identity differs")?;
                let image = catalog.0.iter_mut().find(|image| image.binding.kind == SourceKind::Image4).ok_or("plain canonical image4 absent")?;
                let low = 0x6a9844 - image.binding.start.0 as usize;
                let high = low + linked.len();
                require(producer.source_dat_image4_sha256[1].as_str() == image.binding.sha256.as_str() && staged.len() == image.bytes.len() && staged[..low] == image.bytes[..low] && staged[high..] == image.bytes[high..] && staged[low..high] == linked, "plain image4 changes bytes outside reviewed carrier")?;
                for call in &producer.call_sites {
                    let offset = (call.pc.0 - 0x6a9844) as usize;
                    let word = u32::from_le_bytes(linked.get(offset..offset+4).ok_or("plain call outside carrier")?.try_into().map_err(|_| "plain call word")?);
                    let decoded = decode_call(call.pc, word)?.ok_or("plain metadata call is not emitted")?;
                    require(matches!(decoded.target, CallTarget::Direct(target) if target == call.target), "plain emitted call target differs")?;
                    calls.push(decoded);
                }
                execution = producer.execution_pcs;
                image.binding.sha256 = Digest::of(&staged);
                image.bytes = staged;
            }
            _ => return Err("plain role and artifact binding disagree".into()),
        }
        Ok(Self { binding, calls, execution })
    }
    pub(super) fn binding(&self) -> Binding { self.binding.clone() }
    pub(super) fn owned(&self) -> bool { self.binding.role == Role::Owned }
    pub(super) fn update(&self) -> &'static str { if self.owned() { OWNED_UPDATE } else { super::loader_orchestration_owned::SOURCE_UPDATE } }
    pub(super) fn call_sites(&self) -> Vec<StaticCall> { self.calls.clone() }
    pub(super) fn return_pc(&self) -> Guest { Guest(if self.owned() { 0x6a9850 } else { 0x6a9934 }) }
    pub(super) fn returned_target(&self, pc: Guest) -> Option<Guest> {
        self.calls.iter().find(|call| call.continuation == pc).and_then(|call| match call.target { CallTarget::Direct(target) => Some(target), _ => None })
    }
    pub(super) fn execution_allowed(&self, pc: Guest) -> bool {
        let physical = pc.0 & !0x40000000;
        if !(0x6a9844..0x6a9938).contains(&physical) { return true; }
        pc.0 == physical && if self.owned() { self.execution.contains(&pc) } else { physical <= 0x6a9934 }
    }
}
