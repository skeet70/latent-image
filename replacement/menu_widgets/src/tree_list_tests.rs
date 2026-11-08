extern crate std;
use super::render_commands::{DrawReport, FontId, NodeId, ResourceId, SideState};
use super::tree_list::{Child, DrawArgs, DrawMode, DrawServices, HookRecord, ResourceDescriptor, Snapshot, Summary, tree_list};
use std::vec::Vec;

struct Screen {
    snapshot: Snapshot,
    children: Vec<Child>,
    capacity: u32,
    resources: Vec<(u32, i16, i16)>,
    selected: Vec<u32>,
    font: FontId,
    side: SideState,
    hook_draws: bool,
}
impl DrawServices for Screen {
    fn initialize_hook(&mut self) -> HookRecord { HookRecord([0;16]) }
    fn panel_font(&self)->FontId { self.font }
    fn live_snapshot(&mut self,out:&mut Snapshot) { *out=self.snapshot; }
    fn clear_report(&mut self,out:&mut DrawReport) { *out=DrawReport::zeroed(); }
    fn summarize(&mut self,_:NodeId,out:&mut Summary) { *out=Summary { rows:self.children.len() as u32,reserved:0,page_capacity:self.capacity }; }
    fn describe(&mut self,_:NodeId,index:u32,out:&mut Child) { *out=self.children[index as usize]; }
    fn divide(&mut self,n:u32,d:u32)->u32 { n/d }
    fn remainder(&mut self,n:u32,d:u32)->u32 { n%d }
    fn label(&mut self,_:NodeId,index:u32)->ResourceId { ResourceId(100+index) }
    fn font(&mut self,font:FontId) { self.font=font; }
    fn side_state(&mut self,state:SideState) { self.side=state; }
    fn selection_palette(&mut self,_:bool,_:u32) {}
    fn disable_format(&mut self) {}
    fn enable_format(&mut self) {}
    fn cell_box(&mut self,_:i32,_:i32,_:i32,_:i32,_:u32,_:u32,_:u32) {}
    fn label_hook(&mut self,record:&mut HookRecord)->u32 {
        if record.0[11]!=0 { self.selected.push(record.0[1]); }
        self.hook_draws as u32
    }
    fn resource(&mut self,d:&mut ResourceDescriptor) { self.resources.push((d.resource.0,d.x,d.y)); }
    fn special_mode(&self)->u32 { 0 }
    fn text_format(&mut self,_:bool) {}
    fn descriptor_highlight(&mut self,_:u32) {}
    fn grid_resource(&mut self,_:u32,_:i32,_:ResourceId) {}
    fn palette_override(&mut self,_:NodeId,_:u32)->u32 { 0 }
    fn primary_palette(&mut self,_:u32) {}
    fn value(&mut self,_:NodeId,child:u32,_:u32)->ResourceId { ResourceId(200+child) }
    fn value_hook(&mut self,_:&mut HookRecord)->u32 { self.hook_draws as u32 }
    fn palette_components(&mut self,_:u32,_:u32,_:u32,_:u32) {}
    fn grid_rectangle(&mut self,_:i32) {}
}
fn screen(states:&[u32],page:u32,cursor:u32,count:u32)->Screen {
    Screen { snapshot:Snapshot([1,9,0,41,17,22,0,cursor,count,page,0]),children:states.iter().enumerate().map(|(i,state)| Child {record:90+i as u32,id:10+i as u32,selection:20+i as u32,variant:0,state:*state,reserved:0}).collect(),capacity:3,resources:Vec::new(),selected:Vec::new(),font:FontId(73),side:SideState {kind:99,alternate_font:false},hook_draws:false }
}
fn args()->DrawArgs { DrawArgs {node:NodeId(41),first:2,second:4,first_step:0,second_step:2,mode:DrawMode(0)} }
fn word(report:&DrawReport,offset:usize)->u32 { u32::from_le_bytes(report.0[offset..offset+4].try_into().unwrap()) }

#[test]
fn second_page_filters_hidden_rows_without_moving_grid_origin() {
    let mut screen=screen(&[0,3,2,0,0,0,0],2,4,6);
    let mut report=DrawReport::zeroed();
    tree_list(&mut screen,args(),&mut report);
    assert_eq!(screen.resources,[(104,20,72),(214,-20,72),(105,20,120),(215,-20,120),(106,20,168),(216,-20,168)]);
    assert_eq!(screen.selected,[15]);
    assert_eq!(word(&report,0xc),15);
    assert_eq!(report.0[0x1c],1);
    assert_eq!(report.0[0x1d],255);
    assert_eq!(screen.font,FontId(73));
    assert_eq!(screen.side,SideState {kind:3,alternate_font:false});
}
#[test]
fn short_page_centers_rows_and_records_previous_selection() {
    let mut screen=screen(&[0,0],1,0,2);
    screen.capacity=6;
    screen.snapshot.0[5]=21;
    let mut report=DrawReport::zeroed();
    tree_list(&mut screen,args(),&mut report);
    assert_eq!(screen.resources,[(100,20,168),(210,-20,168),(101,20,216),(211,-20,216)]);
    assert_eq!(report.0[0x1e],2);
    assert_eq!(report.0[0x1d],1);
    assert_eq!(report.0[0x1c],0);
    assert_eq!(word(&report,0x18),6);
}
#[test]
fn hook_drawn_rows_do_not_emit_default_resources() {
    let mut screen=screen(&[0,2,0],1,2,3);
    screen.hook_draws=true;
    let mut report=DrawReport::zeroed();
    tree_list(&mut screen,args(),&mut report);
    assert!(screen.resources.is_empty());
    assert_eq!(screen.selected,[12]);
    assert_eq!(word(&report,0xc),12);
    assert_eq!(screen.font,FontId(73));
}
#[test]
fn scroll_top_is_reported_but_does_not_scroll_flat_rows() {
    let mut screen=screen(&[0,0,0],1,1,3);
    screen.snapshot.0[10]=2;
    let mut report=DrawReport::zeroed();
    tree_list(&mut screen,args(),&mut report);
    assert_eq!(screen.resources[0],(100,20,72));
    assert_eq!(word(&report,0x30),2);
    assert_eq!(report.0[0x3e],0);
}
#[test]
fn empty_list_preserves_side_state_and_restores_font() {
    let mut screen=screen(&[],1,0,0);
    let mut report=DrawReport::zeroed();
    tree_list(&mut screen,args(),&mut report);
    assert_eq!(screen.side,SideState {kind:99,alternate_font:false});
    assert_eq!(screen.font,FontId(73));
    assert!(screen.resources.is_empty());
}
