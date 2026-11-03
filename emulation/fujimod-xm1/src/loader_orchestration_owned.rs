use super::{Catalog, Core, Guest, SourceKind, CallTarget, Gpr, decode_call};
use super::continuation_view::{ByteCount, Digest, FileBinding};
use fujimod_xm1::bridge::{read, write};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

pub(super) const SOURCE_UPDATE: &str = "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490";
pub(super) const OWNED_UPDATE: &str = "4ea1a92b8f8193d9352db00ea1c772349a1aa04a3965d186d671ad6fd27ad627";
const MANIFEST: &str = "57c3e70297d5ed1a94b01aef9c8a0d611d7e00abad39f34323535eb2457e59d2";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) enum Side { LoaderA, LoaderB }
impl Side {
    pub(super) fn base(self)->Guest {Guest(match self {Self::LoaderA=>0x40000,Self::LoaderB=>0xd0000})}
    pub(super) fn core(self)->Core {match self {Self::LoaderA=>Core::Primary,Self::LoaderB=>Core::Secondary}}
    pub(super) fn frame(self)->Guest {Guest(match self {Self::LoaderA=>0x40063450,Self::LoaderB=>0x400f3410})}
    pub(super) fn source(self)->SourceKind {match self {Self::LoaderA=>SourceKind::LoaderA,Self::LoaderB=>SourceKind::LoaderB}}
    pub(super) fn linked(self)->&'static str {match self {Self::LoaderA=>"32e1ef56b5f1eddd4e88b46aa1258c0cd65e5fe533c047886b6a7bd2c9c25795",Self::LoaderB=>"19bb8cde6bf0a43c0933e9be10fa441415a66eb0a5fc981aed339c05c52dd0d4"}}
    pub(super) fn symbols(self)->&'static str {match self {Self::LoaderA=>"c4976afcb03a7f9278d4a93c60fc21b12209270720bd1bd630b3098e706ed2f9",Self::LoaderB=>"72be77fae5b394ad90aa0d4ea8224578dbec894a0b03718f6f2d77ecddd23a26"}}
    fn staged(self)->&'static str {match self {Self::LoaderA=>"1b331b0792fd0dc99483326ed34b25541ecf09ae26da3878a492004379139e43",Self::LoaderB=>"3ec88f9e7224e8e8694410940e297e52f77a427bbfc629e3ee285d638a902454"}}
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) enum Service { Identity,ClearShared,PrivateInitialize,Log,Prepare1,Prepare2,Prepare3,Prepare4,PrimaryContinue,PeerContinue,Continue1,Continue2,Continue3,Continue4,Continue5,Continue6,Continue7,PublishCallback,AutoReady,ActivateTimers,Schedule }
impl Service {
    pub(super) fn address(self,side:Side)->Guest {
        let variant=|a,b|match side {Side::LoaderA=>a,Side::LoaderB=>b};
        Guest(side.base().0+match self {
            Self::Identity=>0x27b8,Self::ClearShared=>0x34f0,Self::PrivateInitialize=>0x430,Self::Log=>0x1354,
            Self::Prepare1=>0x3834,Self::Prepare2=>variant(0x3a3c,0x39f0),Self::Prepare3=>0x24f8,Self::Prepare4=>0xd98,
            Self::PrimaryContinue=>0x386c,Self::PeerContinue=>0x38a8,Self::Continue1=>variant(0x3a58,0x3a0c),Self::Continue2=>variant(0x4a50,0x4a10),
            Self::Continue3=>variant(0xcea0,0xce60),Self::Continue4=>variant(0xce78,0xce38),Self::Continue5=>0x3934,Self::Continue6=>variant(0x3ac0,0x3a7c),Self::Continue7=>0xb5c,
            Self::PublishCallback=>0xc6c,Self::AutoReady=>variant(0x3f9c,0x3f58),Self::ActivateTimers=>variant(0xbf3c,0xbefc),Self::Schedule=>0x3688,
        })
    }
    pub(super) fn live(self)->&'static [Gpr] {match self {Self::ClearShared=>&[Gpr(0),Gpr(1)],Self::Log|Self::PublishCallback=>&[Gpr(0)],_=>&[]}}
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
enum Qualification { #[serde(rename="conditional-source-only")] ConditionalSourceOnly }
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
enum Provider { CanonicalRetained }
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
struct StackOffset(i32);
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
struct FileOffset(u32);
#[derive(Deserialize)]
struct Checksum {offset:FileOffset,bytes:ByteCount}
#[derive(Deserialize)]
struct Dispatcher {role:Provider,entry:Guest,source_image4_sha256:Digest,local_stack_low_from_service_sp:StackOffset,local_stack_low_from_carrier_entry:StackOffset,caller_live_stack:[StackOffset;2],caller_live_scratch_below_service_sp:Vec<StackOffset>}
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
enum BootRole { Primary,Peer }
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
enum Completion { InitiallyPublished,PublishedAfterDelay,NeverPublished }
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
enum Setup { SetupReturns,SetupNonreturn }
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
enum Terminal { EnteredScheduler,SetupDidNotReturn,WaitingForCompletion }
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
enum Phase { Setup,Root,Diagnostic,Idle0,PeerTasks }
#[derive(Deserialize, PartialEq, Eq)]
struct ProvenPath {role:BootRole,completion:Completion,setup:Setup,terminal:Terminal,phases:Vec<Phase>}
#[derive(Deserialize)]
#[serde(tag="kind")]
enum Boundary {Retained {pc:Guest,service:Service,target:Guest},Dispatcher {pc:Guest,target:Guest}}
impl Boundary {
    fn pc(&self)->Guest {match *self {Self::Retained {pc,..}|Self::Dispatcher {pc,..}=>pc}}
}
#[derive(Deserialize)]
struct Carrier {side:Side,patch:[Guest;2],linked_sha256:Digest,symbols_sha256:Digest,linked_bytes:ByteCount,staged_loader_sha256:Digest,compiler_entry:Guest,body_entry:Guest,frame_bytes:ByteCount,service_sp_from_entry:StackOffset,service_fp_from_entry:StackOffset,execution_pcs:Vec<Guest>,calls:Vec<Boundary>,paths:[ProvenPath;5],source_loader_sha256:Digest}
#[derive(Deserialize)]
struct Manifest {schema:u32,qualification:Qualification,source_dat_sha256:Digest,source_image4_sha256:Digest,source_loader_a_sha256:Digest,source_loader_b_sha256:Digest,candidate_dat_sha256:Digest,allowed_dat_checksum:Checksum,dispatcher_provider:Dispatcher,loaders:[Carrier;2]}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct OwnedBindings {manifest:FileBinding,loaders:[FileBinding;2]}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag="role",content="artifacts",rename_all="kebab-case")]
pub(super) enum ProgramBinding {Retained,Owned(OwnedBindings),Joined(super::loader_startup_program::Bindings)}
pub(super) struct OwnedProgram {binding:OwnedBindings,manifest:Manifest}
pub(super) enum Program {Retained,Owned(OwnedProgram),Joined(super::loader_startup_program::JoinedProgram)}

fn require(premise:bool,message:&str)->Result<(),String>{if premise {Ok(())} else {Err(message.into())}}
fn source(catalog:&Catalog,kind:SourceKind)->Result<&super::SourceImage,String>{catalog.0.iter().find(|image|image.binding.kind==kind).ok_or_else(||"artifact source image absent".into())}
fn bytes_at(root:&Path,binding:&FileBinding)->Result<Vec<u8>,String>{
    let mut parts=binding.path.components();require(matches!(parts.next(),Some(Component::Normal(_)))&&parts.next().is_none(),"owned artifact is not a single relative filename")?;
    let bytes=read(&root.join(&binding.path))?;require(Digest::of(&bytes).as_str()==binding.sha256.as_str(),"owned artifact file identity differs")?;Ok(bytes)
}
fn paths_match(observed:&[ProvenPath;5])->bool {
    let matches=|path:&ProvenPath,role,completion,setup,terminal,phases:&[Phase]|path.role==role&&path.completion==completion&&path.setup==setup&&path.terminal==terminal&&path.phases==phases;
    matches(&observed[0],BootRole::Primary,Completion::InitiallyPublished,Setup::SetupReturns,Terminal::EnteredScheduler,&[Phase::Setup,Phase::Root,Phase::Idle0,Phase::Diagnostic])
        &&matches(&observed[1],BootRole::Primary,Completion::InitiallyPublished,Setup::SetupNonreturn,Terminal::SetupDidNotReturn,&[Phase::Setup])
        &&matches(&observed[2],BootRole::Peer,Completion::InitiallyPublished,Setup::SetupReturns,Terminal::EnteredScheduler,&[Phase::PeerTasks,Phase::Diagnostic])
        &&matches(&observed[3],BootRole::Peer,Completion::PublishedAfterDelay,Setup::SetupReturns,Terminal::EnteredScheduler,&[Phase::PeerTasks,Phase::Diagnostic])
        &&matches(&observed[4],BootRole::Peer,Completion::NeverPublished,Setup::SetupReturns,Terminal::WaitingForCompletion,&[])
}
impl Program {
    pub(super) fn binding(&self)->ProgramBinding {match self {Self::Retained=>ProgramBinding::Retained,Self::Owned(owned)=>ProgramBinding::Owned(owned.binding.clone()),Self::Joined(joined)=>ProgramBinding::Joined(joined.binding.clone())}}
    pub(super) fn update(&self)->&str {match self {Self::Retained=>SOURCE_UPDATE,Self::Owned(_)=>OWNED_UPDATE,Self::Joined(joined)=>joined.update.as_str()}}
    pub(super) fn owned(&self)->bool {!matches!(self,Self::Retained)}
    pub(super) fn from_staged(stage:&Path,output:&Path,catalog:&mut Catalog)->Result<Self,String>{
        let file=|name:&str|->Result<FileBinding,String>{let bytes=read(&stage.join(name))?;let destination=format!("owned-{name}");write(&output.join(&destination),&bytes)?;Ok(FileBinding {path:PathBuf::from(destination),sha256:Digest::of(&bytes)})};
        let binding=OwnedBindings {manifest:file("contract.json")?,loaders:[file("loader-a.bin")?,file("loader-b.bin")?]};
        Self::from_binding(ProgramBinding::Owned(binding),output,catalog)
    }
    pub(super) fn from_binding(binding:ProgramBinding,root:&Path,catalog:&mut Catalog)->Result<Self,String>{
        if matches!(binding,ProgramBinding::Joined(_)){return Err("shared program requires independent joined-capture replay".into());}
        let ProgramBinding::Owned(binding)=binding else {return Ok(Self::Retained)};
        let bytes=bytes_at(root,&binding.manifest)?;require(Digest::of(&bytes).as_str()==MANIFEST,"owned contract is not the reviewed staged artifact")?;
        let manifest:Manifest=serde_json::from_slice(&bytes).map_err(|error|error.to_string())?;
        require(manifest.schema==1&&manifest.qualification==Qualification::ConditionalSourceOnly&&manifest.source_dat_sha256.as_str()==SOURCE_UPDATE&&manifest.candidate_dat_sha256.as_str()==OWNED_UPDATE,"owned contract qualification or DAT identities differ")?;
        let image4=source(catalog,SourceKind::Image4)?.binding.sha256.as_str();
        require(manifest.source_image4_sha256.as_str()==image4&&manifest.source_loader_a_sha256.as_str()==source(catalog,SourceKind::LoaderA)?.binding.sha256.as_str()&&manifest.source_loader_b_sha256.as_str()==source(catalog,SourceKind::LoaderB)?.binding.sha256.as_str(),"owned contract canonical inputs differ")?;
        let provider=&manifest.dispatcher_provider;
        require(provider.role==Provider::CanonicalRetained&&provider.entry==Guest(0x69cdf8)&&provider.source_image4_sha256.as_str()==image4&&provider.local_stack_low_from_service_sp==StackOffset(-48)&&provider.local_stack_low_from_carrier_entry==StackOffset(-64)&&provider.caller_live_stack==[StackOffset(-16),StackOffset(0)]&&provider.caller_live_scratch_below_service_sp.is_empty(),"actual dispatcher provider or caller-live frame differs")?;
        require(manifest.allowed_dat_checksum.offset==FileOffset(0x20c)&&manifest.allowed_dat_checksum.bytes.0==4,"container checksum exception differs")?;
        for (index,side) in [Side::LoaderA,Side::LoaderB].into_iter().enumerate(){
            let carrier=&manifest.loaders[index];let staged=bytes_at(root,&binding.loaders[index])?;
            require(carrier.side==side&&carrier.patch==[Guest(side.base().0+0x9ac),Guest(side.base().0+0xb00)]&&carrier.linked_bytes.0==280&&carrier.compiler_entry==Guest(side.base().0+0x9b8)&&carrier.body_entry==Guest(side.base().0+0x9c0)&&carrier.frame_bytes.0==16&&carrier.service_sp_from_entry==StackOffset(-16)&&carrier.service_fp_from_entry==StackOffset(-4),"owned placement or frame contract differs")?;
            require(carrier.linked_sha256.as_str()==side.linked()&&carrier.symbols_sha256.as_str()==side.symbols()&&carrier.staged_loader_sha256.as_str()==side.staged()&&Digest::of(&staged).as_str()==side.staged(),"owned linked/staged identity differs")?;
            let original=source(catalog,side.source())?;
            require(staged.len()==original.bytes.len()&&carrier.source_loader_sha256.as_str()==original.binding.sha256.as_str()&&staged[..0x9ac]==original.bytes[..0x9ac]&&staged[0xb00..]==original.bytes[0xb00..],"staged loader changes bytes outside its carrier")?;
            require(Digest::of(&staged[0x9ac..0xac4]).as_str()==side.linked()&&staged[0xac4..0xb00].chunks_exact(4).all(|word|word==0xe320f000u32.to_le_bytes()),"owned image or NOP padding differs")?;
            require(paths_match(&carrier.paths)&&carrier.execution_pcs.len()==65&&carrier.execution_pcs.windows(2).all(|pair|pair[0].0<pair[1].0),"owned path or execution-point census differs")?;
            require(carrier.execution_pcs.iter().all(|pc|pc.0>=side.base().0+0x9ac&&pc.0<=side.base().0+0xab4&&pc.0%4==0&&!(side.base().0+0x9b8..side.base().0+0x9c0).contains(&pc.0)),"owned execution set includes compiler prologue or data")?;
            require(carrier.calls.len()==26,"owned service-boundary census differs")?;
            for call in &carrier.calls {
                let pc=call.pc();let offset=pc.0.checked_sub(side.base().0).ok_or("owned call precedes loader")? as usize;
                let raw=u32::from_le_bytes(staged.get(offset..offset+4).ok_or("owned call extent")?.try_into().map_err(|_|"owned call word")?);
                let decoded=decode_call(pc,raw)?.ok_or("owned boundary is not an emitted call")?;
                match call {Boundary::Retained {service,target,..}=>require(*target==service.address(side)&&matches!(decoded.target,CallTarget::Direct(actual) if actual==*target),"owned retained call differs")?,Boundary::Dispatcher {target,..}=>require(*target==Guest(0x69cdf8)&&matches!(decoded.target,CallTarget::Register(Gpr(4))),"owned dispatcher call differs")?,}
            }
            let image=catalog.0.iter_mut().find(|image|image.binding.kind==side.source()).ok_or("owned loader catalog entry absent")?;
            image.binding.sha256=Digest::of(&staged);image.bytes=staged;
        }
        Ok(Self::Owned(OwnedProgram {binding,manifest}))
    }
    pub(super) fn execution_allowed(&self,pc:Guest)->bool {
        let physical=pc.0&!0x40000000;
        for side in [Side::LoaderA,Side::LoaderB] {
            if (side.base().0+0x9ac..side.base().0+0xb00).contains(&physical) {
                if pc.0!=physical {return false;}
                return match self {Self::Retained=>physical<side.base().0+0xaf4,Self::Owned(owned)=>owned.manifest.loaders[usize::from(side.core())].execution_pcs.binary_search(&pc).is_ok(),Self::Joined(joined)=>joined.execution_allowed(side,pc)};
            }
        }
        true
    }
    pub(super) fn refusal_sites(&self)->impl Iterator<Item=Guest>+'_ {
        [Side::LoaderA,Side::LoaderB].into_iter().flat_map(|side|(side.base().0+0x9ac..side.base().0+0xb00).step_by(4)).flat_map(|pc|[Guest(pc),Guest(pc|0x40000000)]).filter(move|pc|!self.execution_allowed(*pc))
    }
}
