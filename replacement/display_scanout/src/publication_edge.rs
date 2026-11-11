use crate::{Engine,commit::*};
use crate::layout::{put_byte,put_word,runtime_base,saved_base,input_base};

pub unsafe fn captured(engine:Engine,state:&EngineState) {
    let s=saved_base(engine);let image=&state.saved;
    for(offset,value)in [(0,image.control),(4,image.geometry_control),(8,image.format),(0xc,image.timing),(0x10,image.extent),(0x14,image.sync)]{put_word(s+offset,value.0);}
    if engine==Engine::One{put_word(s+0x18,image.engine_one_control.0);}
    for(offset,value)in [(0x1c,image.window_start),(0x20,image.window_end),(0x24,image.output_start),(0x28,image.output_end),(0x2c,image.horizontal_scale),(0x30,image.vertical_scale),(0x34,image.origin),(0x38,image.crop_start),(0x3c,image.crop_end),(0x40,image.crop_step),(0x44,image.auxiliary_start),(0x48,image.auxiliary_end),(0x4c,image.auxiliary_step),(0x50,image.output_control),(0x54,image.plane_control),(0x58,image.auxiliary_control),(0x9c,image.primary_control),(0xa0,image.secondary_control),(0xa4,image.overlay_control)]{put_word(s+offset,value.0);}
    for index in 0..4{for(base,plane)in [(s+0x5c,image.primary_planes[index]),(s+0xa8,image.secondary_planes[index])]{let base=base+index*16;put_word(base,plane.origin.0);put_word(base+4,plane.layout.0);put_word(base+8,plane.stride.0);put_word(base+12,plane.address.0);}}
    for index in 0..2{put_word(s+0xec+index*8,image.two_rectangles[index].horizontal.0);put_word(s+0xf0+index*8,image.two_rectangles[index].vertical.0);put_word(s+0x174+index*4,image.two_extents[index].0);}
    for index in 0..10{put_word(s+0xfc+index*8,image.ten_rectangles[index].horizontal.0);put_word(s+0x100+index*8,image.ten_rectangles[index].vertical.0);}
    for index in 0..2{put_word(s+0x14c+index*4,image.default_levels[index].0);}put_word(s+0x154,image.level_selectors.0);for index in 0..4{put_word(s+0x158+index*4,image.default_colors[index].0);}
    for(offset,value)in [(0x168,image.color_selectors),(0x16c,image.default_opacities),(0x170,image.opacity_selectors),(0x17c,image.overlay_start),(0x180,image.overlay_end),(0x184,image.overlay_step),(0x188,image.overlay_format),(0x18c,image.overlay_address),(0x190,image.overlay_stride),(0x194,image.overlay_enable),(0x198,image.interrupt_control),(0x19c,image.interrupt_start),(0x1a0,image.interrupt_end)]{put_word(s+offset,value.0);}
}
pub unsafe fn prepared(engine:Engine,state:&EngineState){let s=saved_base(engine);let i=input_base(engine);let image=&state.saved;
    for index in 0..10 {put_word(i+0x58+index*8,state.input.ten_rectangles[index].horizontal.0);put_word(i+0x5c+index*8,state.input.ten_rectangles[index].vertical.0);put_byte(i+0xa8+index,state.input.ten_reverse[index]);}
    for index in 0..2 {put_word(i+0x44+index*8,state.input.two_rectangles[index].horizontal.0);put_word(i+0x48+index*8,state.input.two_rectangles[index].vertical.0);put_byte(i+0x54+index,state.input.two_reverse[index]);}
    for index in 0..2{put_word(s+0x14c+index*4,image.default_levels[index].0);put_word(s+0x174+index*4,image.two_extents[index].0);}
    for index in 0..4{put_word(s+0x158+index*4,image.default_colors[index].0);}
    for(offset,value)in [(0x16c,image.default_opacities),(0x154,image.level_selectors),(0x168,image.color_selectors),(0x170,image.opacity_selectors),(4,image.geometry_control)]{put_word(s+offset,value.0);}
    for index in 0..10{put_word(s+0xfc+index*8,image.ten_rectangles[index].horizontal.0);put_word(s+0x100+index*8,image.ten_rectangles[index].vertical.0);}
    for index in 0..2{put_word(s+0xec+index*8,image.two_rectangles[index].horizontal.0);put_word(s+0xf0+index*8,image.two_rectangles[index].vertical.0);}
    put_word(s+0x34,image.origin.0);
    for index in 0..4{put_word(s+0x5c+index*16,image.primary_planes[index].origin.0);put_word(s+0xa8+index*16,image.secondary_planes[index].origin.0);}
}
pub unsafe fn runtime_configuration(engine:Engine,state:&RuntimeState){let base=runtime_base(engine);put_word(base+4,state.mode.0);put_word(base,state.selector.0);put_word(base+8,state.auxiliary.0);}
pub unsafe fn clear_completion(engine:Engine,slot:CompletionSlot){let base=runtime_base(engine)+match slot.list{CompletionList::First=>0x24,CompletionList::Second=>0x3c};put_word(base+slot.index()*4,0);}
pub unsafe fn reset_runtime(engine:Engine,configuration:CaptureConfiguration){let base=runtime_base(engine);for index in 0..0x58/4{put_word(base+index*4,0);}put_word(base+4,if configuration.mode.0==0{if engine==Engine::One{1}else{3}}else{configuration.mode.0});put_word(base,configuration.selector.0);put_word(base+8,configuration.auxiliary.0);}
