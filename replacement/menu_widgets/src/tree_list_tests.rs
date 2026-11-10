extern crate std;
use super::render_commands::{DrawReport, FontId, NodeId, ResourceId, SideState};
use super::tree_list::{Child, DrawArgs, DrawMode, DrawServices, HookRecord, NestedChild, NestedGrid, ResourceDescriptor, Selection, Snapshot, SnapshotSource, Summary, tree_list};
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
    saved: [Snapshot; 2],
    nested: Vec<NestedChild>,
    nested_capacity: u32,
    resolved: Option<Selection>,
    details: Vec<(u32, u32, u32)>,
    footers: Vec<(u32, u32, u32)>,
    formats: Vec<bool>,
    special: u32,
}
impl DrawServices for Screen {
    fn initialize_hook(&mut self) -> HookRecord { HookRecord([0;16]) }
    fn panel_font(&self)->FontId { self.font }
    fn snapshot(&mut self,_:NodeId,source:SnapshotSource,out:&mut Snapshot) {
        *out=match source { SnapshotSource::Live=>self.snapshot,SnapshotSource::SavedSlot1=>self.saved[0],SnapshotSource::SavedSlot2=>self.saved[1] };
    }
    fn resolve_selection(&mut self,_:NodeId,out:&mut Selection)->bool {
        if let Some(selection)=self.resolved { *out=selection; true } else { false }
    }
    fn nested_summary(&mut self,_:NodeId,_:u32,out:&mut Summary) { *out=Summary {rows:self.nested.len() as u32,reserved:0,page_capacity:self.nested_capacity,retained:[0;3]}; }
    fn nested_child(&mut self,_:NodeId,_:u32,index:u32,out:&mut NestedChild) { *out=self.nested[index as usize]; }
    fn nested_label(&mut self,_:NodeId,_:u32,selection:u32)->ResourceId { ResourceId(300+selection) }
    fn selection_hook(&mut self,record:&mut HookRecord)->u32 {
        if record.0[11]!=0 { self.selected.push(record.0[2]); }
        self.hook_draws as u32
    }
    fn selected_detail(&mut self,node:NodeId,parent:u32,selection:u32) { self.details.push((node.0,parent,selection)); }
    fn footer(&mut self,node:NodeId,parent:u32,selection:u32) { self.footers.push((node.0,parent,selection)); }
    fn clear_report(&mut self,out:&mut DrawReport) { *out=DrawReport::zeroed(); }
    fn summarize(&mut self,_:NodeId,out:&mut Summary) { *out=Summary { rows:self.children.len() as u32,reserved:0,page_capacity:self.capacity,retained:[0;3] }; }
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
    fn special_mode(&self)->u32 { self.special }
    fn text_format(&mut self,enabled:bool) { self.formats.push(enabled); }
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
    Screen { snapshot:Snapshot([1,9,0,41,17,22,0,cursor,count,page,0]),children:states.iter().enumerate().map(|(i,state)| Child {record:90+i as u32,id:10+i as u32,selection:20+i as u32,variant:0,state:*state,reserved:0}).collect(),capacity:3,resources:Vec::new(),selected:Vec::new(),font:FontId(73),side:SideState {kind:99,alternate_font:false},hook_draws:false,saved:[Snapshot([0;11]);2],nested:Vec::new(),nested_capacity:3,resolved:None,details:Vec::new(),footers:Vec::new(),formats:Vec::new(),special:0 }
}
fn args()->DrawArgs { DrawArgs {node:NodeId(41),first:2,second:4,first_step:0,second_step:2,nested:NestedGrid {first:14,second:4,first_step:0,second_step:2},mode:DrawMode(0)} }
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

fn tree_screen()->Screen {
    let mut screen=screen(&[0,3,2,0,0],1,0,3);
    screen.snapshot=Snapshot([2,9,0,13,102,101,0,0,3,1,1]);
    screen.nested=(0..5).map(|i| NestedChild {selection:100+i,state:if i==1 {3} else {1},data:900+i}).collect();
    screen
}

#[test]
fn saved_snapshots_select_report_and_rows_without_using_live_state() {
    for (mode,slot) in [(5,0),(6,0),(11,0),(12,0),(7,1),(9,1),(13,1)] {
        let mut screen=screen(&[0,0,0],1,0,3);
        screen.saved[slot]=Snapshot([1,77,0,45,89,20,0,2,3,1,7]);
        let mut args=args();
        args.mode=DrawMode(mode);
        let mut report=DrawReport([0xa5;68]);
        tree_list(&mut screen,args,&mut report);
        assert_eq!(word(&report,4),77);
        assert_eq!(word(&report,8),45);
        assert_eq!(word(&report,0xc),12);
        assert_eq!(word(&report,0x10),89);
        assert_eq!(word(&report,0x30),7);
        assert_eq!(report.0[0x1d],0);
        assert_eq!(screen.font,FontId(73));
    }
}

#[test]
fn tree_reports_parent_page_and_scrolled_nested_selection() {
    let mut screen=tree_screen();
    let mut report=DrawReport([0xa5;68]);
    tree_list(&mut screen,args(),&mut report);
    assert_eq!(word(&report,0),2);
    assert_eq!(word(&report,8),93);
    assert_eq!(word(&report,0x10),23);
    assert_eq!(word(&report,0xc),13);
    assert_eq!(report.0[0x1f],1);
    assert_eq!(word(&report,0x14),0);
    assert_eq!(word(&report,0x18),0);
    assert_eq!(word(&report,0x24),5);
    assert_eq!(word(&report,0x28),3);
    assert_eq!(word(&report,0x2c),3);
    assert_eq!(word(&report,0x34),0);
    assert_eq!(report.0[0x3c],0);
    assert_eq!(report.0[0x3d],255);
    assert_eq!(word(&report,0x40),902);
    assert_eq!(&screen.resources[6..],[(402,260,72),(403,260,120),(404,260,168)]);
    assert_eq!(screen.details,[(41,13,102)]);
    assert_eq!(screen.footers,[(41,13,102)]);
    assert_eq!(screen.font,FontId(73));
}

#[test]
fn resolved_parent_selection_controls_page_and_preserves_report_fields() {
    let mut screen=tree_screen();
    screen.resolved=Some(Selection {record:700,selection:800,index:3});
    let mut report=DrawReport([0xa5;68]);
    tree_list(&mut screen,args(),&mut report);
    assert_eq!(word(&report,8),700);
    assert_eq!(word(&report,0x10),800);
    assert_eq!(report.0[0x1f],2);
    assert_eq!(word(&report,0xc),14);
    assert_eq!(report.0[0x1c],0);
    assert_eq!(word(&report,0x20),13);
    assert_eq!(word(&report,0x30),1);
    assert_eq!(word(&report,0x38),101);
}

#[test]
fn nested_selected_hook_redraw_and_footer_survive_formatting() {
    let mut screen=tree_screen();
    screen.hook_draws=true;
    screen.special=13;
    let mut report=DrawReport::zeroed();
    tree_list(&mut screen,args(),&mut report);
    assert!(screen.resources.is_empty());
    assert_eq!(screen.formats,[true,false]);
    assert_eq!(screen.footers,[(41,13,102)]);
    assert_eq!(screen.selected.iter().filter(|&&id|id==102).count(),1);
    assert_eq!(word(&report,0x40),902);
}

#[test]
fn unknown_snapshot_kind_does_not_draw_or_invent_flat_report_fields() {
    let mut screen=tree_screen();
    screen.snapshot.0[0]=9;
    let mut report=DrawReport([0xa5;68]);
    tree_list(&mut screen,args(),&mut report);
    assert_eq!(word(&report,0),9);
    assert_eq!(word(&report,8),0);
    assert_eq!(word(&report,0x18),0);
    assert!(screen.resources.is_empty());
    assert!(screen.footers.is_empty());
    assert_eq!(screen.side,SideState {kind:99,alternate_font:false});
    assert_eq!(screen.font,FontId(73));
}

#[test]
fn node_overrides_select_flat_behavior_from_tree_snapshot() {
    for (node,parent,selected) in [(0x25,0x27,0),(0x38,0x228,11)] {
        let mut screen=tree_screen();
        screen.snapshot.0[3]=parent;
        screen.snapshot.0[4]=selected;
        let mut args=args();
        args.node=NodeId(node);
        let mut report=DrawReport::zeroed();
        tree_list(&mut screen,args,&mut report);
        assert_eq!(word(&report,0),1);
        assert_eq!(word(&report,8),parent);
        assert_eq!(word(&report,0x10),selected);
        assert_eq!(word(&report,0x24),0);
        assert!(screen.footers.is_empty());
    }
}

#[test]
fn retained_summary_six_word_write_preserves_adjacent_caller_report() {
    #[repr(C)]
    struct CallerStorage { summary: Summary, report: DrawReport }
    let mut caller=CallerStorage {
        summary:unsafe { core::mem::zeroed() },
        report:DrawReport([0xa5;68]),
    };
    let retained_words=[7u32,3,4,0x11223344,0x55667788,0x99aabbcc];
    unsafe {
        core::ptr::copy_nonoverlapping(retained_words.as_ptr(), (&mut caller.summary as *mut Summary).cast::<u32>(), retained_words.len());
    }
    assert_eq!(caller.report.0,[0xa5;68]);
    assert_eq!(caller.summary.rows,7);
    assert_eq!(caller.summary.page_capacity,4);
}
