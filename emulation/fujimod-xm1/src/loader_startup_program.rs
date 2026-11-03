use super::{Catalog, Guest, SourceKind, Side, Program};
use super::continuation_view::{Digest, FileBinding};
use fujimod_xm1::bridge::{read, write};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub(super) const FILES: [&str; 17] = [
    "canonical.dat", "canonical-image4.bin", "canonical-loader-a.bin", "canonical-loader-b.bin",
    "candidate.dat", "candidate-image4.bin", "candidate-loader-a.bin", "candidate-loader-b.bin",
    "loader-a.bin", "loader-a.symbols", "loader-b.bin", "loader-b.symbols",
    "dispatcher.bin", "dispatcher.symbols", "installer.bin", "installer.symbols", "contract.json",
];
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Bindings { pub(super) files: Vec<FileBinding> }
#[derive(Deserialize)]
struct Source { dat_sha256: Digest, image4_sha256: Digest, loader_a_sha256: Digest, loader_b_sha256: Digest }
#[derive(Deserialize)]
struct Native { image_sha256: Digest, symbols_sha256: Digest }
#[derive(Deserialize)]
struct Natives { loader_a: Native, loader_b: Native, dispatcher: Native, installer: Native }
#[derive(Deserialize, PartialEq)]
enum Qualification { #[serde(rename="conditional-loader-startup-composition")] Conditional }
#[derive(Deserialize, PartialEq)]
enum ProviderRole { OwnedJoined }
#[derive(Deserialize)]
struct Provider { role: ProviderRole, entry: Guest }
#[derive(Clone, Copy, Deserialize, Serialize)]
pub(super) struct Sites {
    pub(super) completion_store: Guest, pub(super) completion_before_dsb: Guest,
    pub(super) completion_after_dsb: Guest, pub(super) peer_poll: Guest,
    pub(super) load_destination_register: u8, pub(super) peer_poll_postload: Guest,
    pub(super) peer_poll_branch: Guest, pub(super) initializer_call: Guest,
    pub(super) initializer_entry: Guest, pub(super) initializer_return: Guest,
}
#[derive(Deserialize)]
struct Loader { side: Side, linked_bytes: usize, compiler_entry: Guest, body_entry: Guest,
    patch: [Guest; 2], execution_pcs: Vec<Guest>, sites: Sites }
#[derive(Deserialize)]
struct Stack { loader_frame_low: i32, dispatcher_low: i32, installer_low_from_entry: i32, combined_low_from_dispatcher_entry: i32 }
#[derive(Deserialize)]
struct Composition { schema: u32, qualification: Qualification, source: Source, native_inputs: Natives,
    dispatcher_provider: Provider, loaders: [Loader; 2], owned_stack: Stack,
    dispatcher_calls:Vec<super::shared_dispatch::shared::CallFact> }
#[derive(Deserialize)]
struct Contract { schema: u32, candidate_dat_sha256: Digest, candidate_image4_sha256: Digest,
    candidate_loader_a_sha256: Digest, candidate_loader_b_sha256: Digest, composition: Composition }
pub(super) struct JoinedProgram {
    pub(super) binding: Bindings, pub(super) update: Digest, loaders: [Loader; 2],
    pub(super) dispatcher_calls:Vec<super::shared_dispatch::shared::CallFact>,
}
fn require(ok: bool, message: &str) -> Result<(), String> { if ok { Ok(()) } else { Err(message.into()) } }
fn patch(source: &[u8], native: &[u8], low: usize, high: usize) -> Result<Vec<u8>, String> {
    require(!native.is_empty() && native.len()%4==0 && native.len()<=high-low, "native patch extent differs")?;
    let mut result=source.to_vec();
    let window=result.get_mut(low..high).ok_or("native patch exceeds source")?;
    for word in window.chunks_exact_mut(4) { word.copy_from_slice(&0xe320f000u32.to_le_bytes()); }
    window[..native.len()].copy_from_slice(native);
    Ok(result)
}
fn source_word(bytes: &[u8], base: Guest, pc: Guest) -> Result<u32, String> {
    let offset=pc.0.checked_sub(base.0).ok_or("site precedes admitted loader")? as usize;
    Ok(u32::from_le_bytes(bytes.get(offset..offset+4).ok_or("site exceeds admitted loader")?.try_into().map_err(|_|"site word extent")?))
}
fn check_sites(loader: &Loader, bytes: &[u8]) -> Result<(), String> {
    let sites=loader.sites;
    let word=|pc|source_word(bytes,loader.side.base(),pc);
    require(sites.load_destination_register<13 && sites.peer_poll.0.checked_add(4)==Some(sites.peer_poll_postload.0)
        && sites.peer_poll.0.checked_add(8)==Some(sites.peer_poll_branch.0) && sites.initializer_call.0.checked_add(4)==Some(sites.initializer_return.0),
        "loader source boundary adjacency differs")?;
    require(word(sites.completion_before_dsb)?==0xf57ff04f && sites.completion_before_dsb.0.checked_add(4)==Some(sites.completion_after_dsb.0)
        && word(sites.completion_store)?&0x0c100000==0x04000000,
        "completion store/barrier sites differ from emitted source")?;
    let load=word(sites.peer_poll)?;
    require(load&0x0c100000==0x04100000 && ((load>>12)&15)==u32::from(sites.load_destination_register),
        "poll site is not the admitted source load")?;
    let call=super::decode_call(sites.initializer_call,word(sites.initializer_call)?)?.ok_or("initializer site is not an emitted call")?;
    require(matches!(call.target,super::CallTarget::Direct(target) if target==sites.initializer_entry)
        && sites.initializer_entry.0==loader.side.base().0+0x430, "initializer call target differs")?;
    for pc in [sites.completion_store,sites.completion_before_dsb,sites.completion_after_dsb,
        sites.peer_poll,sites.peer_poll_postload,sites.peer_poll_branch,sites.initializer_call,sites.initializer_return] {
        require(loader.execution_pcs.binary_search(&pc).is_ok(), "boundary is not executable admitted source")?;
    }
    Ok(())
}
struct StagedFiles<'a> {root:&'a Path,prefix:&'a str}
impl StagedFiles<'_> {fn join(&self,name:impl AsRef<Path>)->PathBuf {self.root.join(format!("{}{}",self.prefix,name.as_ref().to_string_lossy()))}}
impl JoinedProgram {
    pub(super) fn execution_allowed(&self, side: Side, pc: Guest) -> bool {
        self.loaders[usize::from(side.core())].execution_pcs.binary_search(&pc).is_ok()
    }
    pub(super) fn sites(&self, side: Side) -> Sites { self.loaders[usize::from(side.core())].sites }
    pub(super) fn open(stage: &Path, output: &Path, catalog: &mut Catalog) -> Result<(Program, Bindings), String> {
        let files=FILES.into_iter().map(|name| {
            let bytes=read(&stage.join(name))?;
            let destination=PathBuf::from(format!("staged-{name}"));
            write(&output.join(&destination),&bytes)?;
            Ok(FileBinding { path: destination, sha256: Digest::of(&bytes) })
        }).collect::<Result<Vec<_>,String>>()?;
        let binding=Bindings { files };
        Self::admit(&StagedFiles {root:stage,prefix:""},binding,catalog)
    }
    pub(super) fn from_binding(binding:Bindings,root:&Path,catalog:&mut Catalog)->Result<(Program,Bindings),String>{
        require(binding.files.len()==FILES.len(),"shared artifact file census differs")?;
        for (name,file) in FILES.into_iter().zip(&binding.files) {
            require(file.path==PathBuf::from(format!("staged-{name}")),"shared artifact filename differs")?;
            require(Digest::of(&read(&root.join(&file.path))?).as_str()==file.sha256.as_str(),"shared artifact file identity differs")?;
        }
        Self::admit(&StagedFiles {root,prefix:"staged-"},binding,catalog)
    }
    fn admit(stage:&StagedFiles<'_>,binding:Bindings,catalog:&mut Catalog)->Result<(Program,Bindings),String>{
        let contract:Contract=serde_json::from_slice(&read(&stage.join("contract.json"))?).map_err(|error|error.to_string())?;
        let composition=contract.composition;
        require(contract.schema==1 && composition.schema==1 && composition.qualification==Qualification::Conditional,
            "joined contract schema or qualification differs")?;
        require(composition.dispatcher_provider.role==ProviderRole::OwnedJoined && composition.dispatcher_provider.entry==Guest(0x69cdf8),
            "joined dispatcher provider differs")?;
        let stack=composition.owned_stack;
        require(stack.loader_frame_low == -16 && stack.dispatcher_low == -72 && stack.installer_low_from_entry == -40
            && stack.combined_low_from_dispatcher_entry == -104, "composed stack contract differs")?;
        let canonical=[("canonical-image4.bin",SourceKind::Image4,&composition.source.image4_sha256),
            ("canonical-loader-a.bin",SourceKind::LoaderA,&composition.source.loader_a_sha256),
            ("canonical-loader-b.bin",SourceKind::LoaderB,&composition.source.loader_b_sha256)];
        for (name,kind,identity) in canonical {
            let image=catalog.0.iter().find(|image|image.binding.kind==kind).ok_or("canonical catalog entry absent")?;
            let bytes=read(&stage.join(name))?;
            require(bytes==image.bytes && Digest::of(&bytes).as_str()==identity.as_str(),"joined canonical source differs")?;
        }
        require(composition.source.dat_sha256.as_str()==super::loader_orchestration_owned::SOURCE_UPDATE
            && Digest::of(&read(&stage.join("canonical.dat"))?).as_str()==composition.source.dat_sha256.as_str(), "joined canonical DAT differs")?;
        for (name,native) in [("loader-a",&composition.native_inputs.loader_a),("loader-b",&composition.native_inputs.loader_b),
            ("dispatcher",&composition.native_inputs.dispatcher),("installer",&composition.native_inputs.installer)] {
            require(Digest::of(&read(&stage.join(format!("{name}.bin")))?).as_str()==native.image_sha256.as_str()
                && Digest::of(&read(&stage.join(format!("{name}.symbols")))?).as_str()==native.symbols_sha256.as_str(), "joined native input differs")?;
        }
        for (index,side) in [Side::LoaderA,Side::LoaderB].into_iter().enumerate() {
            let name=if side==Side::LoaderA {"loader-a"} else {"loader-b"};
            let native=read(&stage.join(format!("{name}.bin")))?;
            let symbols=read(&stage.join(format!("{name}.symbols")))?;
            require(Digest::of(&native).as_str()==side.linked() && Digest::of(&symbols).as_str()==side.symbols(),"loader native proof identity differs")?;
            let loader=&composition.loaders[index];
            require(loader.side==side && loader.linked_bytes==native.len() && native.len()==280
                && loader.patch==[Guest(side.base().0+0x9ac),Guest(side.base().0+0xb00)]
                && loader.compiler_entry==Guest(side.base().0+0x9b8) && loader.body_entry==Guest(side.base().0+0x9c0)
                && loader.execution_pcs.len()==65 && loader.execution_pcs.windows(2).all(|pair|pair[0]<pair[1]),"joined loader source facts differ")?;
            let image=catalog.0.iter_mut().find(|image|image.binding.kind==side.source()).ok_or("loader source absent")?;
            let carried=patch(&image.bytes,&native,0x9ac,0xb00)?;
            require(carried==read(&stage.join(format!("candidate-{name}.bin")))?,"carried loader is not its native patch")?;
            let expected=if side==Side::LoaderA {&contract.candidate_loader_a_sha256} else {&contract.candidate_loader_b_sha256};
            require(Digest::of(&carried).as_str()==expected.as_str(),"carried loader binding differs")?;
            check_sites(loader,&carried)?;
            image.binding.sha256=Digest::of(&carried); image.bytes=carried;
        }
        let image=catalog.0.iter_mut().find(|image|image.binding.kind==SourceKind::Image4).ok_or("image4 source absent")?;
        let dispatcher=read(&stage.join("dispatcher.bin"))?;
        let installer=read(&stage.join("installer.bin"))?;
        let carried=patch(&patch(&image.bytes,&dispatcher,0x69cdf8-0x61c400,0x69cf84-0x61c400)?,&installer,0x725a04-0x61c400,0x725b54-0x61c400)?;
        require(carried==read(&stage.join("candidate-image4.bin"))? && Digest::of(&carried).as_str()==contract.candidate_image4_sha256.as_str(),"carried image4 is not both native patches")?;
        require(Digest::of(&read(&stage.join("candidate.dat"))?).as_str()==contract.candidate_dat_sha256.as_str(),"candidate DAT binding differs")?;
        require(!composition.dispatcher_calls.is_empty(),"dispatcher live-call census absent")?;
        for call in &composition.dispatcher_calls {call.validate(&carried)?;}
        image.binding.sha256=Digest::of(&carried); image.bytes=carried;
        let joined=Self { binding:binding.clone(),update:contract.candidate_dat_sha256,loaders:composition.loaders,dispatcher_calls:composition.dispatcher_calls };
        Ok((Program::Joined(joined),binding))
    }
}

