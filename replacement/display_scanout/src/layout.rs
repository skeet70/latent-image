use core::ptr::{read_volatile,write_volatile};
use crate::{Engine,commit::*,shadow,timing};

pub unsafe fn byte(address:usize)->u8 {read_volatile(address as *const u8)}
pub unsafe fn half(address:usize)->u16 {read_volatile(address as *const u16)}
pub unsafe fn word(address:usize)->u32 {read_volatile(address as *const u32)}
pub unsafe fn put_byte(address:usize,value:u8){write_volatile(address as *mut u8,value);}
pub unsafe fn put_half(address:usize,value:u16){write_volatile(address as *mut u16,value);}
pub unsafe fn put_word(address:usize,value:u32){write_volatile(address as *mut u32,value);}
pub const fn runtime_base(engine:Engine)->usize{0x9553cc+engine.index()*0x58}
pub const fn saved_base(engine:Engine)->usize{0x96b9bc+engine.index()*0x1a4}
pub const fn input_base(engine:Engine)->usize{0x9554b8+engine.index()*0x23c}
pub const fn timing_base(engine:Engine)->usize{0x9429f4+engine.index()*0x58}
pub const fn formatter_base(engine:Engine)->usize{0x917c10+engine.index()*0x128}

pub struct StockShadow(pub Engine);
#[derive(Clone,Copy)]
enum Width {Byte,Half,Word}
impl StockShadow {
    fn location(&self,field:shadow::Field)->(usize,Width) {
        use shadow::{Field::*,MaskStorage,ColorComponent,Group};
        let saved=saved_base(self.0);let input=input_base(self.0);
        match field {
            Initialized=>(runtime_base(self.0)+0x54,Width::Byte),
            ItemConfiguration(group,item)=>(saved+0x64+group.index()*0x4c+item.index()*16,Width::Word),
            RangeOrigin(group,item)=>(input+0x14+group.index()*16+item.index()*4,Width::Word),
            FirstDimension(group,item)=>(saved+0x62+group.index()*0x4c+item.index()*16,Width::Half),
            SecondDimension(group,item)=>(saved+0x60+group.index()*0x4c+item.index()*16,Width::Half),
            Padding(group,item)=>(saved+0x68+group.index()*0x4c+item.index()*16,Width::Half),
            Opacity(group,item)=>(saved+0x58+group.index()*0x4c+item.index(),Width::Byte),
            RangeMask(group,MaskStorage::Saved)=>(saved+0x54+group.index()*0x4c,Width::Half),
            RangeMask(group,MaskStorage::Alternate)=>(input+4+group.index()*4,Width::Half),
            GroupControl(group)=>(saved+0x50+group.index()*0x4c,Width::Byte),
            GroupFormat(Group::First)=>(saved+2,Width::Byte),
            GroupFormat(Group::Second)=>(saved+3,Width::Byte),
            PreparationFlags=>(input+0x3c,Width::Byte),
            GroupSelectors(group)=>(saved+8+group.index(),Width::Byte),
            PrimaryFlags(MaskStorage::Saved)=>(saved,Width::Byte),
            PrimaryFlags(MaskStorage::Alternate)=>(input,Width::Byte),
            BackingMask(MaskStorage::Saved)=>(saved+0xe8,Width::Half),
            BackingMask(MaskStorage::Alternate)=>(input+0x40,Width::Half),
            BlankColor(component)=>(saved+0x180+match component{ColorComponent::Red=>2,ColorComponent::Green=>1,ColorComponent::Blue=>0},Width::Byte),
            AlternateSelector=>(0x916610,Width::Byte),
        }
    }
}
impl shadow::Storage for StockShadow {
    fn read(&self,field:shadow::Field)->shadow::FieldBits {let(address,width)=self.location(field);shadow::FieldBits(unsafe{match width{Width::Byte=>u32::from(byte(address)),Width::Half=>u32::from(half(address)),Width::Word=>word(address)}})}
    fn write(&mut self,field:shadow::Field,value:shadow::FieldBits){let(address,width)=self.location(field);unsafe{match width{Width::Byte=>put_byte(address,value.0 as u8),Width::Half=>put_half(address,value.0 as u16),Width::Word=>put_word(address,value.0)}}}
}

pub unsafe fn load_runtime(engine:Engine)->RuntimeState {
    let mut state=RuntimeState::zero();let r=runtime_base(engine);
    state.selector=ConfigurationSelector(word(r));state.mode=CaptureMode(word(r+4));state.auxiliary=ConfigurationAuxiliary(word(r+8));state.pending=CommitPending(word(r+12));state.registration=RegistrationId(word(r+0x14));state.capture_count=byte(r+0x54);
    for index in 0..6 {let first=word(r+0x24+index*4);let second=word(r+0x3c+index*4);state.completion_callbacks[index]=if first==0{None}else{Some(CallbackId(first))};state.secondary_callbacks[index]=if second==0{None}else{Some(CallbackId(second))};}
    state
}
pub unsafe fn load_engine(engine:Engine)->EngineState {
    let mut state=EngineState::zero();let s=saved_base(engine);let i=input_base(engine);state.runtime=load_runtime(engine);
    let saved=&mut state.saved;
    saved.control=RegisterWord(word(s));saved.geometry_control=RegisterWord(word(s+4));saved.format=RegisterWord(word(s+8));saved.timing=RegisterWord(word(s+0xc));saved.extent=RegisterWord(word(s+0x10));saved.sync=RegisterWord(word(s+0x14));saved.engine_one_control=RegisterWord(word(s+0x18));
    saved.window_start=RegisterWord(word(s+0x1c));saved.window_end=RegisterWord(word(s+0x20));saved.output_start=RegisterWord(word(s+0x24));saved.output_end=RegisterWord(word(s+0x28));saved.horizontal_scale=RegisterWord(word(s+0x2c));saved.vertical_scale=RegisterWord(word(s+0x30));saved.origin=RegisterWord(word(s+0x34));saved.crop_start=RegisterWord(word(s+0x38));saved.crop_end=RegisterWord(word(s+0x3c));saved.crop_step=RegisterWord(word(s+0x40));saved.auxiliary_start=RegisterWord(word(s+0x44));saved.auxiliary_end=RegisterWord(word(s+0x48));saved.auxiliary_step=RegisterWord(word(s+0x4c));saved.output_control=RegisterWord(word(s+0x50));saved.plane_control=RegisterWord(word(s+0x54));saved.auxiliary_control=RegisterWord(word(s+0x58));
    for index in 0..4 {for (base,plane) in [(s+0x5c,&mut saved.primary_planes[index]),(s+0xa8,&mut saved.secondary_planes[index])]{let base=base+index*16;plane.origin=RegisterWord(word(base));plane.layout=RegisterWord(word(base+4));plane.stride=RegisterWord(word(base+8));plane.address=RegisterWord(word(base+12));}}
    saved.primary_control=RegisterWord(word(s+0x9c));saved.secondary_control=RegisterWord(word(s+0xa0));saved.overlay_control=RegisterWord(word(s+0xa4));saved.rectangle_mask=RegisterWord(word(s+0xe8));
    for index in 0..2 {saved.two_rectangles[index].horizontal=RegisterWord(word(s+0xec+index*8));saved.two_rectangles[index].vertical=RegisterWord(word(s+0xf0+index*8));saved.two_extents[index]=RegisterWord(word(s+0x174+index*4));saved.default_levels[index]=RegisterWord(word(s+0x14c+index*4));}
    for index in 0..10 {saved.ten_rectangles[index].horizontal=RegisterWord(word(s+0xfc+index*8));saved.ten_rectangles[index].vertical=RegisterWord(word(s+0x100+index*8));}
    saved.level_selectors=RegisterWord(word(s+0x154));for index in 0..4 {saved.default_colors[index]=RegisterWord(word(s+0x158+index*4));}saved.color_selectors=RegisterWord(word(s+0x168));saved.default_opacities=RegisterWord(word(s+0x16c));saved.opacity_selectors=RegisterWord(word(s+0x170));
    saved.overlay_start=RegisterWord(word(s+0x17c));saved.overlay_end=RegisterWord(word(s+0x180));saved.overlay_step=RegisterWord(word(s+0x184));saved.overlay_format=RegisterWord(word(s+0x188));saved.overlay_address=RegisterWord(word(s+0x18c));saved.overlay_stride=RegisterWord(word(s+0x190));saved.overlay_enable=RegisterWord(word(s+0x194));saved.interrupt_control=RegisterWord(word(s+0x198));saved.interrupt_start=RegisterWord(word(s+0x19c));saved.interrupt_end=RegisterWord(word(s+0x1a0));
    let input=&mut state.input;
    input.origin=RegisterWord(word(i+0xc));for index in 0..4 {input.primary_origins[index]=RegisterWord(word(i+0x14+index*4));input.secondary_origins[index]=RegisterWord(word(i+0x24+index*4));}
    input.vertical_offset=CoordinateOffset(word(i+0x34));input.horizontal_offset=CoordinateOffset(word(i+0x38));input.geometry_control=RegisterWord(word(i+0x3c));input.rectangle_mask=RegisterWord(word(i+0x40));
    for index in 0..2 {input.two_rectangles[index].horizontal=RegisterWord(word(i+0x44+index*8));input.two_rectangles[index].vertical=RegisterWord(word(i+0x48+index*8));input.two_reverse[index]=byte(i+0x54+index);load_record(i+0xb4+index*0x20,&mut input.two_records[index]);}
    for index in 0..10 {input.ten_rectangles[index].horizontal=RegisterWord(word(i+0x58+index*8));input.ten_rectangles[index].vertical=RegisterWord(word(i+0x5c+index*8));input.ten_reverse[index]=byte(i+0xa8+index);load_record(i+0xf4+index*0x20,&mut input.ten_records[index]);}
    state
}
unsafe fn load_record(base:usize,record:&mut RectangleInput) {for index in 0..4 {record.coordinates[index]=FloatBits(word(base+index*4));}record.extent.width=half(base+0x10) as i16;record.extent.height=half(base+0x12) as i16;record.level=FloatBits(word(base+0x14));record.color=ColorWord(word(base+0x18));record.opacity=Opacity(byte(base+0x1c));record.reverse=byte(base+0x1d);}

pub unsafe fn load_timing(engine:Engine)->timing::TimingState {
    let base=timing_base(engine);let mut state=timing::TimingState::zero();
    state.pending=timing::PendingUpdate(word(base+0x10));state.force=timing::ForceUpdate(word(base+0x14));state.defaults=timing::DefaultSelection(byte(base+0x1c));let registers=&mut state.registers;
    registers.control=word(base+0x20);registers.dimensions=word(base+0x24);registers.mode_pulse=word(base+0x28);registers.mode_interval=word(base+0x2c);registers.route_pulse=word(base+0x30);registers.route_interval=word(base+0x34);registers.mode_delay=word(base+0x38);registers.output_pulse=word(base+0x3c);registers.output_interval=word(base+0x40);registers.secondary_output_pulse=word(base+0x44);registers.secondary_output_interval=word(base+0x48);registers.threshold=word(base+0x4c);registers.defaults=word(base+0x50);registers.route_adjustment=word(base+0x54);state
}
pub unsafe fn store_staged_timing(engine:Engine,state:&timing::TimingState){let base=timing_base(engine);let registers=&state.registers;
    for(offset,value)in [(0x20,registers.control),(0x24,registers.dimensions),(0x28,registers.mode_pulse),(0x2c,registers.mode_interval),(0x30,registers.route_pulse),(0x34,registers.route_interval),(0x38,registers.mode_delay),(0x3c,registers.output_pulse),(0x40,registers.output_interval),(0x44,registers.secondary_output_pulse),(0x48,registers.secondary_output_interval),(0x4c,registers.threshold),(0x50,registers.defaults),(0x54,registers.route_adjustment)]{put_word(base+offset,value);}put_word(base+0x10,state.pending.0);
}
pub unsafe fn load_formatter(engine:Engine)->timing::FormatterState {let base=formatter_base(engine);let mut state=timing::FormatterState::zero();state.pending=timing::PendingUpdate(word(base+0x34));let registers=&mut state.registers;registers.control=word(base+0x38);registers.input_control=word(base+0x3c);registers.dimensions=word(base+0x58);registers.output_control=word(base+0x100);registers.mode=word(base+0x6c);for index in 0..5{registers.parameters_38[index]=word(base+0x70+index*4);registers.parameters_64[index]=word(base+0x9c+index*4);}registers.channel_control=word(base+0xb0);for group in 0..3{for index in 0..6{registers.channel_parameters[group][index]=word(base+0xb4+group*24+index*4);}}registers.final_control=word(base+0xfc);state}
