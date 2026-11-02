use fujimod_emulator_devices::{checkpoint::Checkpoint,parts::nand::{Nand,Options}};
use fujimod_xm1::run::{NandBase,RunDocument};
use serde::{Deserialize,de::IgnoredAny};
use std::{borrow::Borrow,collections::BTreeMap};

pub(super) struct Reset {point:Checkpoint}

#[derive(Deserialize,PartialEq,Eq,PartialOrd,Ord)]
#[serde(transparent)]
struct ResourceName(String);
impl Borrow<str> for ResourceName {fn borrow(&self)->&str{&self.0}}
#[derive(Deserialize)]
#[serde(transparent)]
struct DeviceIndex(usize);
#[derive(Deserialize)]
struct SessionWire {nodes:BTreeMap<ResourceName,DeviceIndex>}
#[derive(Deserialize)]
#[serde(try_from="u64")]
struct Zero;
impl TryFrom<u64> for Zero {type Error=String;fn try_from(value:u64)->Result<Self,String>{if value==0{Ok(Self)}else{Err("reset counter is nonzero".into())}}}
#[derive(Deserialize)]
#[serde(try_from="usize")]
struct TwoCores;
impl TryFrom<usize> for TwoCores {type Error=String;fn try_from(value:usize)->Result<Self,String>{if value==2{Ok(Self)}else{Err("reset processor count differs".into())}}}
#[derive(Deserialize)]
#[serde(rename_all="kebab-case")]
enum ExactPolicy {Exact}
#[derive(Deserialize)]
struct TimelineWire {now:Zero,phase:Zero}
#[derive(Deserialize)]
struct RuntimeWire {execution_policy:ExactPolicy,timeline:TimelineWire,instructions:[Zero;2],accounted:[Zero;2],error:Option<String>}
#[derive(Deserialize)]
struct BoardWire {runtime:RuntimeWire,processors:TwoCores}
#[derive(Deserialize,PartialEq,Eq,PartialOrd,Ord)]
enum MediumName {#[serde(rename="nand")] Nand}
#[derive(Deserialize)]
struct SceneWire {nands:BTreeMap<MediumName,MediumSpec>}
#[derive(Deserialize)]
struct MediumSpec {options:Options}
#[derive(Deserialize)]
struct SensorWire {shared:bool}
#[derive(Deserialize)]
struct ResourcesWire {wires:Vec<ResourceName>,images:Vec<ResourceName>,statistics:Vec<ResourceName>,nands:Vec<MediumName>,sensors:Vec<SensorWire>}
#[derive(Deserialize,PartialEq,Eq,PartialOrd,Ord)]
#[serde(transparent)]
struct Page(u32);
#[derive(Deserialize)]
#[serde(transparent)]
struct PageBytes(Vec<u8>);
#[derive(Deserialize)]
#[serde(transparent)]
struct FlashOffset(u64);
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MediumWire {options:Options,overlays:Vec<FlashOffset>,spare:BTreeMap<Page,PageBytes>,pages:BTreeMap<Page,PageBytes>,written_spare:BTreeMap<Page,PageBytes>}

fn require(condition:bool,message:&str)->Result<(),String>{if condition{Ok(())}else{Err(message.into())}}
fn is_medium(point:&Checkpoint)->bool{point.decode::<IgnoredAny>("nand-medium").is_ok()}
fn medium(point:&Checkpoint,options:&Options,media:&NandBase)->Result<(),String>{
    let state:MediumWire=point.decode("nand-medium").map_err(|_|"reset NAND state differs")?;
    require(point.children().is_empty()&&point.blobs().len()==1,"reset NAND blob topology differs")?;
    require(state.options==*options,"reset NAND geometry differs")?;
    require(state.overlays.is_empty()&&state.spare.is_empty()&&state.pages.is_empty()&&state.written_spare.is_empty(),"reset NAND is not pristine")?;
    require(point.blobs()[0].as_ref()==media.as_ref(),"reset NAND media differs from bound update")?;
    Nand::from_checkpoint(point).map(|_|())
}
fn media_census(point:&Checkpoint)->usize{usize::from(is_medium(point))+point.children().iter().map(media_census).sum::<usize>()}

pub(super) fn admit(bytes:&[u8],document:&RunDocument,media:&NandBase)->Result<Reset,String>{
    let point=Checkpoint::from_bytes(bytes)?;
    let session:SessionWire=point.decode("emulator-session-v2").map_err(|_|"reset checkpoint session differs")?;
    require(point.blobs().is_empty()&&point.children().len()==2,"reset checkpoint topology differs")?;
    let board=&point.children()[0];
    let reset:BoardWire=board.decode("board-keyframe-v1").map_err(|_|"reset checkpoint is not at zero reset")?;
    require(reset.runtime.error.is_none(),"reset checkpoint is not at zero reset")?;
    require(board.blobs().len()==3&&board.children().len()==2,"reset checkpoint processor topology differs")?;
    let resources=&point.children()[1];
    let resource:ResourcesWire=resources.decode("session-resources-v1").map_err(|_|"reset checkpoint resources differ")?;
    require(resources.blobs().is_empty()&&resource.nands==[MediumName::Nand],"reset checkpoint NAND resources differ")?;
    let medium_index=resource.wires.len().checked_add(resource.images.len()).and_then(|count|count.checked_add(resource.statistics.len())).ok_or("reset resource extent overflows")?;
    let resource_count=resource.sensors.iter().try_fold(medium_index+2,|count,sensor|count.checked_add(if sensor.shared{1}else{2}).ok_or("reset resource extent overflows"))?;
    require(resources.children().len()==resource_count,"reset checkpoint resource topology differs")?;
    let scene:SceneWire=serde_json::from_str(document.scene.get()).map_err(|error|error.to_string())?;
    let options=&scene.nands.get(&MediumName::Nand).ok_or("canonical reset NAND specification absent")?.options;
    medium(&resources.children()[medium_index],options,media)?;
    let bus=&board.children()[1];
    bus.decode::<IgnoredAny>("board-bus-v1").map_err(|_|"reset checkpoint bus differs")?;
    require(bus.blobs().is_empty(),"reset checkpoint bus blob topology differs")?;
    let index=session.nodes.get(document.nand.0.as_str()).ok_or("reset checkpoint NAND controller absent")?;
    let controller=bus.children().get(index.0).ok_or("reset checkpoint NAND controller absent")?;
    controller.decode::<IgnoredAny>("nand-controller").map_err(|_|"reset checkpoint NAND controller differs")?;
    require(controller.blobs().is_empty()&&controller.children().len()==1,"reset checkpoint NAND controller topology differs")?;
    medium(&controller.children()[0],options,media)?;
    require(media_census(&point)==2,"reset NAND medium census differs")?;
    Ok(Reset {point})
}

fn header(point:&Checkpoint)->Result<Vec<u8>,String>{point.clone().with_blobs(Vec::new()).with_children(Vec::new()).to_bytes()}
fn equal_tree(retained:&Checkpoint,owned:&Checkpoint)->Result<(),String>{
    require(header(retained)?==header(owned)?,"paired reset nonmedia state differs")?;
    require(retained.children().len()==owned.children().len()&&retained.blobs().len()==owned.blobs().len(),"paired reset nonmedia topology differs")?;
    let media=is_medium(retained);
    require(media==is_medium(owned),"paired reset NAND topology differs")?;
    for (index,(before,after)) in retained.blobs().iter().zip(owned.blobs()).enumerate(){if !(media&&index==0){require(before==after,"paired reset nonmedia blob differs")?;}}
    for (before,after) in retained.children().iter().zip(owned.children()){equal_tree(before,after)?;}
    Ok(())
}
pub(super) fn compare(retained:&Reset,owned:&Reset)->Result<(),String>{equal_tree(&retained.point,&owned.point)}

#[cfg(test)]
mod tests {
    use super::{Reset,admit,compare};
    use fujimod_emulator_devices::{checkpoint::Checkpoint,parts::nand::{Nand,Options}};
    use fujimod_xm1::run::{NandBase,RunDocument,ValidatedUpdate,nand_base};
    use serde::Serialize;
    use std::{collections::BTreeMap,sync::Arc};

    #[derive(Clone,Copy,Serialize)]
    enum Marker {Original,Changed}
    #[derive(Serialize,PartialEq,Eq,PartialOrd,Ord)]
    enum SyntheticResource {#[serde(rename="nand")] Nand}
    #[derive(Serialize)]
    #[serde(transparent)]
    struct SyntheticDeviceIndex(usize);
    #[derive(Serialize)]
    enum SyntheticPolicy {#[serde(rename="exact")] Exact}
    #[derive(Serialize)]
    struct SyntheticSession {nodes:BTreeMap<SyntheticResource,SyntheticDeviceIndex>,marker:Marker}
    #[derive(Serialize)]
    struct SyntheticTimeline {now:u64,phase:u64}
    #[derive(Serialize)]
    struct SyntheticRuntime {execution_policy:SyntheticPolicy,timeline:SyntheticTimeline,instructions:[u64;2],accounted:[u64;2],error:Option<String>,marker:Marker}
    #[derive(Serialize)]
    struct SyntheticBoard {runtime:SyntheticRuntime,processors:usize}
    #[derive(Serialize)]
    struct SyntheticResources {wires:[u8;0],images:[u8;0],statistics:[u8;0],nands:[SyntheticResource;1],sensors:[u8;0]}
    #[derive(Clone,Copy)]
    enum Change {None,Session,Runtime,Cpu,Memory,MemoryTopology,Controller,DirtyNand,Geometry,NonzeroInstructions,NonzeroTime}

    fn document()->RunDocument{RunDocument::parse(include_bytes!("../data/xm1-150.json")).expect("canonical run document")}
    fn media(document:&RunDocument,flash:&[u8])->NandBase{
        let mut bytes=vec![0;document.update_flash_offset];bytes.extend_from_slice(flash);
        nand_base(document,&ValidatedUpdate(bytes)).expect("derive synthetic media")
    }
    fn point(media:&NandBase,change:Change)->Checkpoint{
        let options=Options {bytes:134217728,page_bytes:2048,pages_per_block:64,spare_bytes:512};
        let mut nand=Nand::new(options,Arc::from(media.as_ref()),Vec::new(),BTreeMap::new()).expect("synthetic NAND").checkpoint().expect("NAND checkpoint");
        if matches!(change,Change::DirtyNand|Change::Geometry){
            #[derive(Serialize)]
            struct ChangedMedium {options:Options,overlays:Vec<u64>,spare:BTreeMap<u32,Vec<u8>>,pages:BTreeMap<u32,Vec<u8>>,written_spare:BTreeMap<u32,Vec<u8>>}
            let state=ChangedMedium {options:Options {bytes:if matches!(change,Change::Geometry){268435456}else{134217728},page_bytes:2048,pages_per_block:64,spare_bytes:512},overlays:Vec::new(),spare:BTreeMap::new(),pages:if matches!(change,Change::DirtyNand){BTreeMap::from([(0,vec![0;2048])])}else{BTreeMap::new()},written_spare:BTreeMap::new()};
            nand=Checkpoint::encode("nand-medium",&state).expect("modified NAND").with_blobs(nand.blobs().to_vec());
        }
        let controller=Checkpoint::encode("nand-controller",&if matches!(change,Change::Controller){Marker::Changed}else{Marker::Original}).expect("controller").with_children(vec![nand.clone()]);
        let bus=Checkpoint::encode("board-bus-v1",&Marker::Original).expect("bus").with_children(vec![controller]);
        let mut memory_blobs=vec![Arc::from(if matches!(change,Change::Memory){&b"changed memory"[..]}else{&b"reset memory"[..]})];
        if matches!(change,Change::MemoryTopology){memory_blobs.push(Arc::from(&b"extra memory bank"[..]));}
        let memory=Checkpoint::encode("synthetic-memory",&Marker::Original).expect("memory").with_blobs(memory_blobs);
        let runtime=SyntheticRuntime {execution_policy:SyntheticPolicy::Exact,timeline:SyntheticTimeline {now:u64::from(matches!(change,Change::NonzeroTime)),phase:0},instructions:[u64::from(matches!(change,Change::NonzeroInstructions)),0],accounted:[0;2],error:None,marker:if matches!(change,Change::Runtime){Marker::Changed}else{Marker::Original}};
        let board=Checkpoint::encode("board-keyframe-v1",&SyntheticBoard {runtime,processors:2}).expect("board").with_blobs(vec![Arc::from(if matches!(change,Change::Cpu){&b"changed CPU"[..]}else{&b"reset CPU"[..]}),Arc::from(&b"peer CPU"[..]),Arc::from(&b"exclusive state"[..])]).with_children(vec![memory,bus]);
        let resources=Checkpoint::encode("session-resources-v1",&SyntheticResources {wires:[],images:[],statistics:[],nands:[SyntheticResource::Nand],sensors:[]}).expect("resources").with_children(vec![nand,Checkpoint::encode("resource-media",&Marker::Original).expect("other media")]);
        Checkpoint::encode("emulator-session-v2",&SyntheticSession {nodes:BTreeMap::from([(SyntheticResource::Nand,SyntheticDeviceIndex(0))]),marker:if matches!(change,Change::Session){Marker::Changed}else{Marker::Original}}).expect("session").with_children(vec![board,resources])
    }
    fn admitted(point:&Checkpoint,document:&RunDocument,media:&NandBase)->Reset{admit(&point.to_bytes().expect("export synthetic checkpoint"),document,media).expect("admit synthetic reset")}

    #[test]
    fn independently_bound_media_changes_preserve_reset_equivalence(){
        let document=document();let source=media(&document,&[0x11,0x22]);let candidate=media(&document,&[0x33,0x44]);
        assert_eq!(source.as_ref(),&[0xee,0xdd]);
        let before=admitted(&point(&source,Change::None),&document,&source);let after=admitted(&point(&candidate,Change::None),&document,&candidate);
        assert!(compare(&before,&after).is_ok());
    }
    #[test]
    fn an_unbound_nand_base_cannot_be_an_admitted_reset(){
        let document=document();let source=media(&document,&[0x11]);let other=media(&document,&[0x22]);
        assert!(admit(&point(&other,Change::None).to_bytes().expect("checkpoint"),&document,&source).is_err());
    }
    #[test]
    fn nonzero_runtime_and_mutated_nand_reset_state_are_refused(){
        let document=document();let source=media(&document,&[0x11]);
        for change in [Change::NonzeroInstructions,Change::NonzeroTime,Change::DirtyNand,Change::Geometry]{assert!(admit(&point(&source,change).to_bytes().expect("checkpoint"),&document,&source).is_err());}
    }
    #[test]
    fn media_admission_does_not_hide_other_checkpoint_differences(){
        let document=document();let source=media(&document,&[0x11]);let candidate=media(&document,&[0x22]);let before=admitted(&point(&source,Change::None),&document,&source);
        for change in [Change::Session,Change::Runtime,Change::Cpu,Change::Memory,Change::MemoryTopology,Change::Controller]{let after=admitted(&point(&candidate,change),&document,&candidate);assert!(compare(&before,&after).is_err());}
    }
    #[test]
    #[ignore="requires LOADER_RESET_CHECKPOINT and LOADER_RESET_UPDATE from the qualified retained observation"]
    fn preserved_qualified_reset_admits_its_bound_canonical_media_without_execution(){
        use fujimod_xm1::bridge::{read,sha256};
        use std::path::PathBuf;
        let checkpoint_path=PathBuf::from(std::env::var_os("LOADER_RESET_CHECKPOINT").expect("set qualified retained reset checkpoint"));
        let update_path=PathBuf::from(std::env::var_os("LOADER_RESET_UPDATE").expect("set canonical retained update"));
        let update=read(&update_path).expect("read canonical update");
        assert_eq!(sha256(&update),"43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490");
        assert_eq!(sha256(include_bytes!("../data/xm1-150.json")),"efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef");
        let document=document();let media=nand_base(&document,&ValidatedUpdate(update)).expect("derive canonical reset media");
        let checkpoint=read(&checkpoint_path).expect("read qualified reset checkpoint");
        assert_eq!(sha256(&checkpoint),"4e543a67ee02a63f8001ad7969d975e1d82aeab45f6658fde7c48b0df4727663");
        assert!(admit(&checkpoint,&document,&media).is_ok());
    }
}
