use super::shared::{Address32, ByteOffset, ObjectGroup, ObjectKind, OperationCode, ServiceCompletion, StageContext, StagePlan, StageResult};
use crate::develop::framework;

#[derive(Clone, Copy)]
pub(super) enum Channel { Primary, Secondary }
impl Channel {
    fn index(self) -> u32 { match self { Self::Primary => 0, Self::Secondary => 1 } }
    fn parse(value: u32) -> Option<Self> { match value { 0 => Some(Self::Primary), 1 => Some(Self::Secondary), _ => None } }
}
#[derive(Clone, Copy)]
pub(super) enum ImageDomain { Capture, Auxiliary }
impl ImageDomain {
    fn index(self) -> u32 { match self { Self::Capture => 0, Self::Auxiliary => 1 } }
    fn parse(value: u32) -> Option<Self> { match value { 0 => Some(Self::Capture), 1 => Some(Self::Auxiliary), _ => None } }
}
#[derive(Clone, Copy)]
pub(super) struct OutputKind(u32);
impl OutputKind { pub(super) fn from_abi(value: u32) -> Self { Self(value) } }
#[derive(Clone, Copy)]
pub(super) enum Stage {
    PrepareCopy(Channel), CorrectPlanes, ReadComponents, ApplyLevels, FinalizeCapture(Channel),
    Calibrate(ImageDomain, Channel), PrepareSecondary(Channel), MergeAuxiliary,
    ConfigureOutputGeometry(OutputKind), DivideRaw, AddRaw, ResizeReview, ResizeDisplay, ResizeThumbnail, ResizeRawReview,
    SelectCrop, MatchPoints, MatchTriangles,
}

impl Stage {
    pub(super) fn from_command(command: OperationCode) -> Option<Self> {
        Some(match command.0 {
            0x00 => Self::PrepareCopy(Channel::Primary),
            0x01 => Self::PrepareCopy(Channel::Secondary),
            0x02 => Self::CorrectPlanes,
            0x03 => Self::ReadComponents,
            0x04 => Self::ApplyLevels,
            0x05 => Self::FinalizeCapture(Channel::Primary),
            0x06 => Self::FinalizeCapture(Channel::Secondary),
            0x07 => Self::Calibrate(ImageDomain::Capture, Channel::Primary),
            0x08 => Self::Calibrate(ImageDomain::Capture, Channel::Secondary),
            0x09 => Self::Calibrate(ImageDomain::Auxiliary, Channel::Primary),
            0x0a => Self::Calibrate(ImageDomain::Auxiliary, Channel::Secondary),
            0x0b => Self::PrepareSecondary(Channel::Primary),
            0x0c => Self::PrepareSecondary(Channel::Secondary),
            0x0d => Self::MergeAuxiliary,
            0x2a => Self::ConfigureOutputGeometry(OutputKind(2)),
            0x2b => Self::ConfigureOutputGeometry(OutputKind(4)),
            0x2c => Self::ConfigureOutputGeometry(OutputKind(1)),
            0x2d => Self::ConfigureOutputGeometry(OutputKind(0)),
            0x36 => Self::DivideRaw,
            0x37 => Self::AddRaw,
            0x4c => Self::ResizeRawReview,
            0x4d => Self::ResizeDisplay,
            0x4e => Self::ResizeReview,
            0x4f => Self::ResizeThumbnail,
            0x57 => Self::SelectCrop,
            0x5c => Self::MatchPoints,
            0x5d => Self::MatchTriangles,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy)]
struct Image(Address32);
impl Image {
    fn field(self, offset: u32) -> Address32 { self.0.offset(ByteOffset(offset)) }
    unsafe fn get(self, offset: u32) -> u32 { unsafe { self.field(offset).word() } }
    unsafe fn put(self, offset: u32, value: u32) { unsafe { self.field(offset).write_word(value) } }
}
unsafe fn frame(context: StageContext) -> Image { Image(unsafe { context.frame() }.0) }
unsafe fn field(context: StageContext, offset: u32) -> Address32 { unsafe { frame(context) }.field(offset) }
unsafe fn word(context: StageContext, offset: u32) -> u32 { unsafe { field(context, offset).word() } }
unsafe fn bypass(context: StageContext) -> bool { (unsafe { word(context, 0x27cc) }) != 0 }
unsafe fn object(context: StageContext, group: u32, kind: u32) -> Image {
    Image(unsafe { context.object(ObjectGroup(group), ObjectKind(kind as i32)) })
}
unsafe fn store(context: StageContext, state: u32) { unsafe { framework::xm1_develop_store(context.0, state) } }
unsafe fn callback(context: StageContext, state: u32) { unsafe { framework::xm1_develop_callback(context.0, state, 0, 0) } }
unsafe fn dispatch(context: StageContext, operation: u32, phase: u32) { unsafe { framework::xm1_develop_dispatch_objects(context.0, operation, phase, 1) } }
unsafe fn transition(context: StageContext, operation: u32) -> StageResult { StageResult(unsafe { framework::xm1_develop_transition(context.0, operation) } as u32) }
unsafe fn plan(operation: u32) -> StagePlan { unsafe { StagePlan::develop(OperationCode(operation)) } }
unsafe fn copy_image(destination: Image, source: Image) {
    unsafe {
        let a = source.get(0); let b = source.get(4); let c = source.get(8); let d = source.get(12);
        destination.put(0,a); destination.put(4,b); destination.put(8,c); destination.put(12,d);
        let e = source.get(16); let f = source.get(20); let g = source.get(24);
        destination.put(16,e); destination.put(20,f); destination.put(24,g);
    }
}
unsafe fn capture_allowed(context: StageContext) -> bool {
    unsafe { field(context,0x1680).half() == 1 || !matches!(word(context,0x4a8), 5 | 8) }
}
unsafe fn debug_image(context: StageContext, kind: u32, image: Image) {
    unsafe { outputs_debug_image(frame(context).0.0, kind, image.0.0, 0, 0) }
}

extern "C" {
    fn outputs_copy_request(job: u32, source: u32, auxiliary: u32, destination: u32, template: u32, channel: u32) -> u32;
    fn outputs_calibration_request(job: u32, image: u32, domain: u32, channel: u32) -> u32;
    fn outputs_secondary_request(job: u32, image: u32, primary: u32, secondary: u32, channel: u32) -> u32;
    fn outputs_component_request(job: u32, image: u32, primary: u32, secondary: u32, source: u32, levels: *mut u16, channel: u32) -> u32;
    fn outputs_store_levels(job: u32, levels: *const u16, channel: u32) -> u32;
    fn outputs_level_request(job: u32, mode: u32) -> u32;
    fn outputs_correct_request(job: u32, image: u32, source: u32) -> u32;
    fn outputs_debug_image(job: u32, kind: u32, image: u32, number: u32, name: u32);
    fn outputs_output_request(job: u32, selection: *const u32, template: u32, primary: u32, secondary: u32) -> u32;
    fn outputs_output_enabled(context: *mut u8) -> u32;
    fn outputs_prepare_geometry(context: *mut u8, image: u32, mode: u32, kind: u32) -> u32;
    fn outputs_select_source(context: *mut u8, image: *mut u32, direct: *mut u32);
    fn outputs_open_object(context: *mut u8, group: u32, kind: u32, enabled: u32);
    fn outputs_close_object(context: *mut u8, group: u32, kind: u32);
    fn outputs_expand_views(context: *mut u8, first: *mut u32, second: *mut u32, template: *const u32, count: u32, channel: u32);
    fn outputs_scale_request(job: u32, source: u32, destination: u32, mode: u32, packed: u32) -> u32;
    fn outputs_add_raw_request(job: u32, first: u32, second: u32, destination: u32, tenths: u32) -> u32;
    fn outputs_divide_raw_request(job: u32, source: u32, destination: u32, tenths: u32) -> u32;
    fn outputs_resize(source: u32, destination: u32) -> u32;
    fn outputs_rotate(job: u32, source: u32, destination: u32, rotation: u32) -> u32;
    fn outputs_copy_rows(job: u32, source: u32, destination: u32, mode: u32);
    fn outputs_prepare_display(result: *mut u8, image: u32, mode: u32, unused: u32, task: u32, flags: u32);
    fn outputs_match_points(job: u32, first: *const u32, first_count: u32, first_address: u32, second: *const u32, second_count: u32, second_address: u32, enabled: u32) -> u32;
    fn outputs_finish_points(job: u32, first: u32, second: u32) -> u32;
    fn outputs_match_triangles(job: u32, source: u32, destination: u32, first: u32, second: u32) -> u32;
    fn outputs_task() -> u32;
    fn outputs_text(task: u32, text: u32);
    fn outputs_notify(text: u32);
    fn outputs_trace(operation: u32);
    fn outputs_trace_end(operation: u32);
    fn outputs_signal();
    fn outputs_active(job: u32);
    fn outputs_config(key: u32) -> u32;
    fn outputs_width(kind: u32, ratio: u32) -> u32;
    fn outputs_height(kind: u32, ratio: u32) -> u32;
    fn outputs_write_file(kind: u32, address: u32, length: u32, number: u32, name: u32);
}
unsafe fn outputs_unsigned_decimal(address: u32, value: u32, width: u32, padding: u32) -> i32 {
    unsafe { super::debug_storage::xm1_pipeline_image_storage_decimal(Address32(address), value, width, padding) }
}
unsafe fn outputs_copy(destination: u32, source: u32, length: u32) {
    unsafe { super::debug_storage::copy(Address32(destination), Address32(source), length) }
}

unsafe fn outputs_format(address: u32, value: u32, signed: u32, width: u32, padding: u32) {
    unsafe {
        if signed != 0 && (value as i32) < 0 {
            if outputs_unsigned_decimal(address.wrapping_add(1),value.wrapping_neg(),width.wrapping_sub(1),padding) >= 0 { Address32(address).write_byte(b'-'); }
        } else { outputs_unsigned_decimal(address,value,width,padding); }
    }
}

unsafe fn prepare_copy(context: StageContext, channel: Channel) -> StageResult {
    unsafe {
        let index = channel.index();
        let source = Image(field(context,0x88 + index*28));
        let auxiliary = field(context,0xf8 + index*28);
        let step = plan(index);
        if word(context,0x640) != 2 || (index == 1 && field(context,0x19b4).byte() != 0) || !capture_allowed(context) { return step.skip(context); }
        debug_image(context,index,source);
        let result = step.begin(context);
        if result.0 != 0 { return result; }
        let template = object(context,if word(context,0x4b4) == 0x1d { 0x27 } else { 0x28 },0);
        let completion = if bypass(context) { 1 } else { outputs_copy_request(frame(context).0.0,source.0.0,auxiliary.0,source.0.0,template.0.0,index) };
        let result = step.complete(context,ServiceCompletion(completion));
        if completion != 1 { return result; }
        debug_image(context,6+index,source);
        if word(context,0x4b4) == 0x1a { copy_image(Image(field(context,0x168)),source); }
        result
    }
}
unsafe fn calibrate(context: StageContext, domain: ImageDomain, channel: Channel) -> StageResult {
    unsafe {
        let d = domain.index(); let c = channel.index();
        let source = Image(field(context,0x88 + d*0x70 + c*28));
        let step = plan(7+d*2+c);
        if c == 1 && field(context,0x19b4).byte() != 0 { return step.skip(context); }
        if !capture_allowed(context) { return step.skip(context); }
        if d != 0 || word(context,0x640) != 2 { debug_image(context,d*2+c,source); }
        let result = step.begin(context);
        if result.0 != 0 { return result; }
        let completion = if bypass(context) { 1 } else { outputs_calibration_request(frame(context).0.0,source.0.0,d,c) };
        let result = step.complete(context,ServiceCompletion(completion));
        if completion != 1 || d != 0 { return result; }
        debug_image(context,0x10+c,source);
        if (c == 0 && field(context,0x19b4).byte() != 0) || (c == 1 && field(context,0x19b4).byte() == 0) { outputs_trace(0x6000000); }
        copy_image(Image(field(context,0xc0+c*28)),source);
        result
    }
}
unsafe fn prepare_secondary(context: StageContext, channel: Channel) -> StageResult {
    unsafe {
        let c = channel.index(); let source = field(context,0xc0+c*28); let step = plan(11+c);
        if word(context,0x578) == 0 || field(context,0x19b4).byte() != 0 || word(context,0x63c) != 2 { return step.skip(context); }
        let result = step.begin(context); if result.0 != 0 { return result; }
        let kind = 10+c;
        let primary = object(context,0x14,kind);
        let primary = object(context,if primary.0.0 == 0 { 0x2b } else { 0x14 },kind);
        let secondary = object(context,0x28,0);
        let completion = if bypass(context) { 1 } else { outputs_secondary_request(frame(context).0.0,source.0,primary.0.0,secondary.0.0,c) };
        step.complete(context,ServiceCompletion(completion))
    }
}
unsafe fn merge_auxiliary(context: StageContext) -> StageResult {
    unsafe {
        let step = plan(13);
        if outputs_output_enabled(context.0) == 0 { return step.skip(context); }
        let result = step.begin(context); if result.0 != 0 { return result; }
        step.complete(context,ServiceCompletion(1))
    }
}
unsafe fn read_components(context: StageContext) -> StageResult {
    unsafe {
        let step = plan(3);
        if word(context,0x2740) == 0 && word(context,0x2744) == 0 { let result = step.skip(context); dispatch(context,3,2); return result; }
        let result = step.begin(context); if result.0 != 0 { return result; }
        let mut completion = 0;
        let mut levels = core::mem::MaybeUninit::<[u16;3]>::uninit();
        for channel in 0..2 {
            let job = frame(context);
            if job.get(0x2740+channel*4) == 1 {
                completion = if bypass(context) { 1 } else {
                    let primary = object(context,0x16,0); let secondary = object(context,0x18,0); let source = object(context,0x17,0);
                    let status = outputs_component_request(job.0.0,job.field(0x88).0,primary.0.0,secondary.0.0,source.0.0,levels.as_mut_ptr().cast(),channel);
                    outputs_store_levels(frame(context).0.0,levels.as_ptr().cast(),channel);
                    status
                };
            }
        }
        step.complete(context,ServiceCompletion(completion))
    }
}
unsafe fn correct_planes(context: StageContext) -> StageResult {
    unsafe {
        let step = plan(2);
        if word(context,0x2738) == 0 && word(context,0x273c) == 0 { return step.skip(context); }
        let result = step.begin(context); if result.0 != 0 { return result; }
        let job = frame(context);
        let completion = if bypass(context) { 1 } else { let source = object(context,0x17,0); outputs_correct_request(job.0.0,job.field(0x88).0,source.0.0) };
        step.complete(context,ServiceCompletion(completion))
    }
}
unsafe fn apply_levels(context: StageContext) -> StageResult {
    unsafe {
        let step = plan(4); let result = step.begin(context); if result.0 != 0 { return result; }
        let completion = if bypass(context) { 1 } else { outputs_level_request(frame(context).0.0,0) };
        step.complete(context,ServiceCompletion(completion))
    }
}
unsafe fn finalize_capture(context: StageContext, channel: Channel) -> StageResult {
    unsafe { plan(5+channel.index()).skip(context) }
}

#[derive(Clone, Copy)]
enum ReviewImage { Review, Display, Thumbnail, RawReview }
impl ReviewImage {
    fn operation(self) -> u32 { match self { Self::Review => 0x4e, Self::Display => 0x4d, Self::Thumbnail => 0x4f, Self::RawReview => 0x4c } }
    fn state(self) -> u32 { match self { Self::Review => 0x9c, Self::Display => 0x9a, Self::Thumbnail => 0x9e, Self::RawReview => 0x98 } }
    fn text(self) -> (u32,u32,u32) { match self { Self::Review => (0xea2078,0xea2090,0xea2098), Self::Display => (0xea20a0,0xea20bc,0xea20c4), Self::Thumbnail => (0xea1fc4,0xea1fe0,0xea1fec), Self::RawReview => (0xea1ff8,0xea2010,0xea2018) } }
}
unsafe fn log_start(text: u32, label: u32) { unsafe { outputs_text(outputs_task() & 255,text); outputs_notify(label); } }
unsafe fn transition_begin(context: StageContext, operation: u32, state: u32) -> StageResult {
    unsafe {
        let result = transition(context,operation); if result.0 != 0 { return result; }
        dispatch(context,operation,0);
        let result = transition(context,operation); if result.0 != 0 { return result; }
        dispatch(context,operation,1);
        let result = transition(context,operation);
        if result.0 != 0 { dispatch(context,operation,3); return result; }
        callback(context,state); store(context,state); result
    }
}
unsafe fn transition_finish(context: StageContext, operation: u32, state: u32, success: bool, label: u32) -> StageResult {
    unsafe {
        if success { store(context,state); dispatch(context,operation,2); callback(context,state); }
        let result = transition(context,operation); outputs_signal(); outputs_active(frame(context).0.0); outputs_notify(label); result
    }
}
unsafe fn rotate_review(context: StageContext, review: bool, direct: u32, supplied: Image) -> u32 {
    unsafe {
        let source = if review { Image(field(context,0x30c)) } else { supplied };
        let mut original = [0u32;7]; copy_image(Image(Address32(original.as_mut_ptr() as u32)),source);
        let mut rotated = [0u32;7]; let template = object(context,0x44,0);
        copy_image(Image(Address32(rotated.as_mut_ptr() as u32)),template);
        rotated[4] = if review { word(context,0x31c) } else { original[4] };
        rotated[3] = if review { word(context,0x318) } else { original[4] };
        rotated[5] = if review { word(context,0x320) } else { original[5] };
        rotated[2] = rotated[4].wrapping_mul(rotated[5]);
        let mut destination = [0u32;7]; copy_image(Image(Address32(destination.as_mut_ptr() as u32)),source);
        let orientation = word(context,0x30b8);
        let rotation = match orientation { 2..=4 => 3, 5..=7 => 2, 8..=10 => 1, _ => return 1 };
        if direct != 0 { outputs_close_object(context.0,14,0); outputs_open_object(context.0,1,0,1); }
        if !bypass(context) {
            outputs_rotate(frame(context).0.0,original.as_ptr() as u32,rotated.as_ptr() as u32,rotation);
            outputs_close_object(context.0,1,0); outputs_open_object(context.0,14,0,1);
            outputs_copy_rows(frame(context).0.0,rotated.as_ptr() as u32,destination.as_ptr() as u32,0);
            outputs_close_object(context.0,14,0);
        }
        if review { copy_image(Image(field(context,0x30c)),Image(Address32(rotated.as_ptr() as u32))); }
        1
    }
}
unsafe fn resize_review(context: StageContext, image: ReviewImage) -> StageResult {
    unsafe {
        let operation = image.operation(); let state = image.state(); let (text,start,end) = image.text();
        let is_display = matches!(image,ReviewImage::Display); let is_raw = matches!(image,ReviewImage::RawReview);
        if !is_raw {
            let mode = word(context,0x2770);
            if (mode == 0 && word(context,0x277c) == 0) || (!is_display && mode == 1 && word(context,0x309c) == 0)
                || (word(context,0x4ac) == 1 && field(context,0x1682).half() != 1) { store(context,state+1); return StageResult::SUCCESS; }
        }
        log_start(text,start);
        let result = transition_begin(context,operation,state); if result.0 != 0 { return result; }
        let (group,offset) = match image { ReviewImage::Review => (0x43,0x30c), ReviewImage::Display => (0x46,0x2f0), ReviewImage::Thumbnail => (0x45,0x328), ReviewImage::RawReview => (0x41,0x2b8) };
        let destination = field(context,offset); let template = object(context,group,0); copy_image(Image(destination),template);
        let mut source = core::mem::MaybeUninit::<[u32;7]>::uninit(); let mut direct = 0;
        if matches!(image,ReviewImage::Review) { outputs_select_source(context.0,source.as_mut_ptr().cast(),&mut direct); }
        if matches!(image,ReviewImage::Thumbnail) {
            let job = frame(context); job.put(0x338,120); job.put(0x334,120); job.put(0x33c,60); job.put(0x330,7200);
            copy_image(Image(field(context,0x328)),Image(job.field(0x328)));
        }
        let trace = match image { ReviewImage::Review => 0x6022400, ReviewImage::Display => 0x6022300, ReviewImage::Thumbnail => 0x6022500, ReviewImage::RawReview => 0x6022200 };
        outputs_trace(trace);
        let mut success = true;
        if !bypass(context) {
            let completion = match image {
                ReviewImage::RawReview => { let job = frame(context); outputs_scale_request(job.0.0,job.field(0x184).0,job.field(0x2b8).0,0,(job.get(0x19c)&0x2000000 == 0) as u32) },
                ReviewImage::Thumbnail => { let job = frame(context); outputs_resize(job.field(0x30c).0,job.field(0x328).0) },
                ReviewImage::Review => {
                    if direct == 1 { outputs_scale_request(frame(context).0.0,source.as_ptr() as u32,destination.0,0,(core::ptr::read_volatile(source.as_ptr().cast::<u32>().add(6))&0x2000000 == 0) as u32) }
                    else { dispatch(context,operation,3); outputs_open_object(context.0,1,0,1); outputs_resize(source.as_ptr() as u32,field(context,0x30c).0) }
                },
                ReviewImage::Display => {
                    let mut flag = 0u8; let job = frame(context);
                    outputs_prepare_display(&mut flag,job.field(0x2f0).0,0,0,context.address().offset(ByteOffset(0x34)).word(),context.address().offset(ByteOffset(0x3c)).word());
                    outputs_select_source(context.0,source.as_mut_ptr().cast(),&mut direct);
                    let size = outputs_config(0xa8f2); let x = outputs_config(0xa8f3); let y = outputs_config(0xa8f4);
                    let job = frame(context); let width = size.wrapping_mul(2); let stride = job.get(0x2fc);
                    let mut crop = [job.get(0x2f0),0,0,stride,width.wrapping_mul(4),width.wrapping_mul(2),job.get(0x308)];
                    crop[1] = stride.wrapping_mul(y.wrapping_mul(2).wrapping_sub(width)).wrapping_add(job.get(0x2f4)).wrapping_add((x.wrapping_mul(2).wrapping_sub(width)>>1).wrapping_mul(4)) & !7;
                    crop[2] = crop[4].wrapping_mul(crop[5]);
                    let result = outputs_resize(source.as_ptr() as u32,crop.as_ptr() as u32);
                    if result == 1 && word(context,0x2770) == 0 { rotate_review(context,false,0,Image(Address32(crop.as_mut_ptr() as u32))) } else { result }
                },
            };
            success = completion == 1;
            if success && matches!(image,ReviewImage::Review) && word(context,0x2770) == 0 { success = rotate_review(context,true,direct,Image(Address32(0))) == 1; }
        }
        outputs_trace_end(trace); dispatch(context,operation,3);
        transition_finish(context,operation,state+1,success,end)
    }
}
unsafe fn raw_arithmetic(context: StageContext, addition: bool) -> StageResult {
    unsafe {
        let original = frame(context);
        let (operation,state,text,start,end) = if addition { (0x37,0x6e,0xea2020,0xea2038,0xea2044) } else { (0x36,0x6c,0xea2050,0xea2060,0xea206c) };
        log_start(text,start);
        let other = context.address().offset(ByteOffset(0x14)).word();
        if (other == 0) == addition { store(context,state+1); return StageResult::SUCCESS; }
        let result = transition_begin(context,operation,state); if result.0 != 0 { return result; }
        let result = transition(context,operation);
        if result.0 != 0 { dispatch(context,operation,3); outputs_signal(); outputs_active(frame(context).0.0); return result; }
        let completion = if bypass(context) { 1 } else {
            let tenths = (context.address().offset(ByteOffset(0x18)).half() as u32).wrapping_mul(10) & 0xffff;
            if addition { let other = context.address().offset(ByteOffset(0x14)).word()+0x88; outputs_add_raw_request(frame(context).0.0,other,original.field(0x88).0,other,tenths) }
            else { outputs_divide_raw_request(frame(context).0.0,original.field(0x88).0,original.field(0x88).0,tenths & !1) }
        };
        dispatch(context,operation,3);
        transition_finish(context,operation,state+1,completion == 1,end)
    }
}
unsafe fn configure_output_geometry(context: StageContext, kind: OutputKind) -> StageResult {
    unsafe {
        let k = kind.0; let (operation,object_kind) = match k { 2 => (0x2a,0x10), 4 => (0x2b,0xe), 1 => (0x2c,0xe), _ => (0x2d,0xf) };
        let step = plan(operation);
        if framework::xm1_develop_precondition(context.0,k) == 0 { return step.skip(context); }
        let result = step.begin(context); if result.0 != 0 { return result; }
        let special = word(context,0x4b4) == 0x1d && k == 0;
        let template = object(context,if special { 0x39 } else { 0x21 },if special { 0 } else { object_kind });
        let job = frame(context); let mode = job.get(0x4b4);
        let image = Image(job.field(if (matches!(mode,0x19|0x1a) || job.get(0x710)&0x400 != 0) && k == 1 { 0x1a0 } else { 0x184 }));
        if matches!(mode,0x20|0x1d) || outputs_prepare_geometry(context.0,image.0.0,0,k) != 0 {
            field(context,0x1770).write_word(image.get(16)>>1); field(context,0x176c).write_word(image.get(20));
        } else {
            let job = frame(context); let width = outputs_width(job.get(0x644),job.get(0x4f0)); job.put(0x1770,width);
            let job = frame(context); let height = outputs_height(job.get(0x644),job.get(0x4f0)); job.put(0x176c,height);
        }
        if word(context,0x4b4) == 0x1d { field(context,0x279c).write_word((k == 0) as u32); }
        let job = frame(context); let mode = job.get(0x4b4);
        if matches!(mode,0x19|0x1a) || job.get(0x710)&0x400 != 0 {
            let value = Image(Address32(job.get(0x3098))).get(0xcc); job.put(0x2ff0,value);
            field(context,0x276c).write_word((k == 0 && word(context,0x2ff0) != 0) as u32);
        }
        let mut request = core::mem::MaybeUninit::<[u32;3]>::uninit();
        core::ptr::write(request.as_mut_ptr().cast::<u32>().add(1),k); core::ptr::write(request.as_mut_ptr().cast::<u32>().add(2),0);
        let job = frame(context);
        let completion = if bypass(context) { 1 } else { outputs_output_request(job.0.0,request.as_ptr().cast(),template.0.0,job.field(0xc0).0,job.field(0xdc).0) };
        step.complete(context,ServiceCompletion(completion))
    }
}

unsafe fn config_pair(low_key: u32) -> u32 { unsafe { let high = outputs_config(low_key+1); outputs_config(low_key) | high<<8 } }
unsafe fn crop_parameters(base: u32, alternate: bool) -> (u32,u32) {
    unsafe { let shift = if alternate { 4 } else { 0 }; (config_pair(base+4+shift),config_pair(base+12+shift)) }
}
unsafe fn select_crop(context: StageContext) -> StageResult {
    unsafe {
        let step = plan(0x57); let result = step.begin(context); if result.0 != 0 { return result; }
        let source = object(context,0x28,0); let mut saved = [0u32;7];
        copy_image(Image(Address32(saved.as_mut_ptr() as u32)),source);
        let address = source.get(4); let height = source.get(20); let stride = saved[3];
        let flags = outputs_config(0x9f6f);
        if flags&1 != 0 { publish_motion_debug(context); }
        if flags&4 != 0 { outputs_write_file(0x18,address,stride.wrapping_mul(height),0,0); }
        if word(context,0x3218) == 0 { return step.complete(context,ServiceCompletion(1)); }
        let records = Image(Address32(word(context,0x3150)));
        let mut left = records.get(0x10); let mut center = records.get(0x20); let mut pitch = records.get(0x48) as i32; let mut yaw = records.get(0x4c) as i32;
        let mut index = 0u32;
        loop {
            let key = match word(context,0x5cc) { 0 => 0x9f64, 1 => 0x9f65, _ => 0x9f66 };
            if index >= outputs_config(key) { break; }
            let record = Image(Address32(word(context,0x3150)).offset(ByteOffset(index.wrapping_mul(0x60))));
            if record.field(0x40).byte() == 0x10 { left = record.get(0x10); center = record.get(0x20); pitch = record.get(0x48) as i32; yaw = record.get(0x4c) as i32; }
            index = index.wrapping_add(1);
        }
        let alternate = !matches!(word(context,0x5d0),2|3);
        if (center as i32) < 0 { center = center.wrapping_add(7); }
        let extent = (center & !7).wrapping_add(left);
        let highest = match word(context,0x5cc) { 0 => 0, 1 => 1, _ => 4 };
        let mut margin = 0; let mut width = 0;
        for candidate in (0..=highest).rev() {
            let base = 0x9f10+candidate*16;
            let (first,second) = crop_parameters(base,alternate);
            if extent >= first.wrapping_add(second) {
                let (first,second) = crop_parameters(base,alternate); margin = first; width = second;
                field(context,0x3214).write_word(candidate); break;
            }
        }
        let candidate = match word(context,0x3214) { n @ 0..=3 => n, _ => 4 };
        let base = 0x9f10+candidate*16;
        let height = config_pair(base+if alternate { 18 } else { 14 });
        let mut top = config_pair(base+if alternate { 10 } else { 6 }).wrapping_add(8);
        let motion = config_pair(if alternate { 0x9f6d } else { 0x9f6b });
        let threshold = (motion.wrapping_mul(2) as i32)>>1;
        top = top.wrapping_add(if threshold < pitch { pitch as u32 } else if threshold < yaw.wrapping_abs() { motion.wrapping_mul(2).wrapping_add(yaw as u32) } else { threshold as u32 });
        if width != 0 {
            let destination = Image(field(context,0x184));
            for (index,value) in saved.into_iter().enumerate() { destination.put(index as u32*4,value); }
            destination.put(4,stride.wrapping_mul(top).wrapping_add(address).wrapping_add(margin.wrapping_mul(2)));
            destination.put(8,height.wrapping_mul(width).wrapping_mul(2)); destination.put(12,stride); destination.put(16,width.wrapping_mul(2)); destination.put(20,height);
            field(context,0x321c).write_word(extent);
        }
        let job = frame(context); let matching = Image(Address32(job.get(0x3228)));
        if matching.get(0x114) == 0 && job.get(0x5cc) == 4 {
            if job.get(0x3214) == 4 { copy_image(matching,Image(job.field(0x184))); } else { matching.put(0x110,1); }
        }
        step.complete(context,ServiceCompletion(1))
    }
}

unsafe fn matching_enabled(context: StageContext) -> bool {
    unsafe { if word(context,0x5cc) != 4 { return false; } let matching = Image(Address32(word(context,0x3228))); matching.get(0x110) != 1 && matching.get(0x114) != 1 && matching.get(0x10c) != 1 }
}
unsafe fn match_points(context: StageContext) -> StageResult {
    unsafe {
        let step = plan(0x5c); if !matching_enabled(context) { return step.skip(context); }
        let result = step.begin(context); if result.0 != 0 { return result; }
        let count = outputs_config(0x9fc5).wrapping_add(1);
        let first = object(context,0x2c,0).get(4); let second = object(context,0x2f,0).get(4);
        let matching = Image(Address32(word(context,0x3228)));
        let mut first_views = core::mem::MaybeUninit::<[[u32;7];7]>::uninit();
        let mut second_views = core::mem::MaybeUninit::<[[u32;7];7]>::uninit();
        copy_image(Image(Address32(first_views.as_mut_ptr() as u32)),Image(matching.field(0x54)));
        copy_image(Image(Address32(second_views.as_mut_ptr() as u32)),Image(matching.field(0x70)));
        let mut template = [0u32;7]; let object = object(context,0x3a,0);
        copy_image(Image(Address32(template.as_mut_ptr() as u32)),object);
        outputs_expand_views(context.0,first_views.as_mut_ptr().cast(),first_views.as_mut_ptr().cast(),template.as_ptr(),7,0);
        outputs_expand_views(context.0,second_views.as_mut_ptr().cast(),second_views.as_mut_ptr().cast(),template.as_ptr(),7,1);
        let first_pointers = core::array::from_fn::<u32,7,_>(|index| first_views.as_ptr().cast::<u32>().add(index*7) as u32);
        let second_pointers = core::array::from_fn::<u32,7,_>(|index| second_views.as_ptr().cast::<u32>().add(index*7) as u32);
        let status = outputs_match_points(frame(context).0.0,first_pointers.as_ptr(),count,first,second_pointers.as_ptr(),count,second,1);
        if status != 0 && outputs_finish_points(frame(context).0.0,first,second) == 0 { Image(Address32(word(context,0x3228))).put(0x110,1); }
        if outputs_config(0x9f6f)&0x10 != 0 { publish_point_debug(context); }
        step.complete(context,ServiceCompletion(status))
    }
}
unsafe fn match_triangles(context: StageContext) -> StageResult {
    unsafe {
        let step = plan(0x5d); if !matching_enabled(context) { return step.skip(context); }
        let result = step.begin(context); if result.0 != 0 { return result; }
        let matching = Image(Address32(word(context,0x3228))); let template = object(context,0x30,0);
        copy_image(Image(matching.field(0x8c)),template);
        let job = frame(context); let destination = Image(Address32(job.get(0x3228))).field(0x8c);
        let first = object(context,0x31,0); let second = object(context,0x32,0);
        let status = outputs_match_triangles(job.0.0,matching.field(0x1c).0,destination.0,first.0.0,second.0.0);
        let status = if status == 0 { Image(Address32(word(context,0x3228))).put(0x110,1); 1 } else { status };
        if outputs_config(0x9f6f)&0x20 != 0 { publish_triangle_debug(context); }
        step.complete(context,ServiceCompletion(status))
    }
}

unsafe fn csv_field(destination: u32, value: u32, signed: bool, width: u32) -> u32 {
    unsafe { if value == 0 { outputs_copy(destination,0xe49b80,1); 1 } else { outputs_format(destination,value,signed as u32,width,3); width } }
}
unsafe fn comma(destination: u32) { unsafe { outputs_copy(destination,0xea23cc,1) } }
unsafe fn newline(destination: u32) { unsafe { outputs_copy(destination,0xea23d0,2) } }
unsafe fn publish_motion_debug(context: StageContext) {
    unsafe {
        let buffer: u32 = 0x5210cf60; outputs_copy(buffer,0xea230c,0xc0); let mut length: u32 = 0xc0; let mut index = 0u32;
        while index < outputs_config(0x9f65) {
            let record = Image(Address32(word(context,0x3150)).offset(ByteOffset(index*0x60)));
            let mut values = [0u32;22];
            for (slot,value) in values.iter_mut().enumerate() {
                *value = if slot == 16 { record.field(0x40).byte() as u32 } else { record.get(slot as u32*4) };
            }
            outputs_format(buffer+length,index+1,1,5,3); length+=5;
            for slot in [0,1,17,2,3,4,5,6,7,8,9,18,19,10,11,12,13,14,15,16,20,21] {
                comma(buffer+length); length+=1; length+=csv_field(buffer+length,values[slot],slot != 16,5);
            }
            newline(buffer+length); length+=2; index = (index+1)&255;
        }
        outputs_write_file(0x33,buffer,length,0,0);
    }
}
unsafe fn publish_point_debug(context: StageContext) {
    unsafe {
        let buffer: u32 = 0x437cee00; outputs_copy(buffer,0xea23d4,0x1d); let mut length: u32 = 0x1d; let mut index = 0;
        for (slot,offset) in [0xfc,0x100,0x11c].into_iter().enumerate() {
            if slot != 0 { comma(buffer+length); length+=1; }
            length+=csv_field(buffer+length,Image(Address32(word(context,0x3228))).get(offset),true,5);
        }
        outputs_copy(buffer+length,0xea23f4,4);
        outputs_copy(buffer+length+4,0xea23f8,0x34); length+=0x38;
        while index < Image(Address32(word(context,0x3228))).get(0x118) {
            outputs_format(buffer+length,index+1,1,5,3); length+=5;
            for offset in [0x134+index*8,0x138+index*8,0x8134+index*8,0x8138+index*8,0x10134+index] {
                comma(buffer+length); length+=1;
                let matching = Image(Address32(word(context,0x3228)));
                let value = if offset == 0x10134+index { matching.field(offset).byte() as u32 } else { matching.get(offset) };
                length+=csv_field(buffer+length,value,true,5);
            }
            newline(buffer+length); length+=2; index+=1;
        }
        outputs_write_file(0x33,buffer,length,0,0);
    }
}
unsafe fn publish_triangle_debug(context: StageContext) {
    unsafe {
        let matching = Image(Address32(word(context,0x3228))); let a = matching.get(0x12c); let b = matching.get(0x128); let count = if (a as i32)<(b as i32) { b } else { a };
        let buffer: u32 = 0x52c91980; outputs_copy(buffer,0xea242c,0x65); let mut length: u32 = 0x65; let mut index = 0;
        while index != count {
            outputs_format(buffer+length,index+1,1,5,3); length+=5; comma(buffer+length); length+=1;
            if index < Image(Address32(word(context,0x3228))).get(0x12c) {
                for (slot,offset) in [0x12124+index*16,0x12128+index*16,0x1212c+index*16,0x12130+index*16].into_iter().enumerate() {
                    length+=csv_field(buffer+length,Image(Address32(word(context,0x3228))).get(offset),true,if slot == 0 { 11 } else { 5 }); comma(buffer+length); length+=1;
                }
            } else { outputs_copy(buffer+length,0xea2494,4); length+=4; }
            if index < Image(Address32(word(context,0x3228))).get(0x128) {
                for (slot,offset) in [0x11134+index*8,0x11138+index*8,0x1192c+index*8,0x11930+index*8].into_iter().enumerate() {
                    if slot != 0 { comma(buffer+length); length+=1; }
                    length+=csv_field(buffer+length,Image(Address32(word(context,0x3228))).get(offset),true,5);
                }
            } else { outputs_copy(buffer+length,0xea2494,4); length+=4; }
            newline(buffer+length); length+=2; index+=1;
        }
        outputs_write_file(0x33,buffer,length,0,0);
    }
}

pub(super) unsafe fn execute(context: StageContext, stage: Stage) -> StageResult {
    unsafe { match stage {
        Stage::PrepareCopy(channel) => prepare_copy(context,channel), Stage::CorrectPlanes => correct_planes(context), Stage::ReadComponents => read_components(context), Stage::ApplyLevels => apply_levels(context),
        Stage::FinalizeCapture(channel) => finalize_capture(context,channel), Stage::Calibrate(domain,channel) => calibrate(context,domain,channel), Stage::PrepareSecondary(channel) => prepare_secondary(context,channel), Stage::MergeAuxiliary => merge_auxiliary(context),
        Stage::ConfigureOutputGeometry(kind) => configure_output_geometry(context,kind), Stage::DivideRaw => raw_arithmetic(context,false), Stage::AddRaw => raw_arithmetic(context,true),
        Stage::ResizeReview => resize_review(context,ReviewImage::Review), Stage::ResizeDisplay => resize_review(context,ReviewImage::Display), Stage::ResizeThumbnail => resize_review(context,ReviewImage::Thumbnail), Stage::ResizeRawReview => resize_review(context,ReviewImage::RawReview),
        Stage::SelectCrop => select_crop(context), Stage::MatchPoints => match_points(context), Stage::MatchTriangles => match_triangles(context),
    } }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_prepare_copy(context: *mut u8, channel: u32) -> u32 { match Channel::parse(channel) { Some(channel) => unsafe { execute(StageContext(context),Stage::PrepareCopy(channel)).0 }, None => 0 } }
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_correct_planes(context: *mut u8) -> u32 { unsafe { execute(StageContext(context),Stage::CorrectPlanes).0 } }
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_read_components(context: *mut u8) -> u32 { unsafe { execute(StageContext(context),Stage::ReadComponents).0 } }
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_apply_levels(context: *mut u8) -> u32 { unsafe { execute(StageContext(context),Stage::ApplyLevels).0 } }
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_finalize_capture(context: *mut u8, channel: u32) -> u32 { match Channel::parse(channel) { Some(channel) => unsafe { execute(StageContext(context),Stage::FinalizeCapture(channel)).0 }, None => 0 } }
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_calibrate(context: *mut u8, domain: u32, channel: u32) -> u32 { match (ImageDomain::parse(domain),Channel::parse(channel)) { (Some(domain),Some(channel)) => unsafe { execute(StageContext(context),Stage::Calibrate(domain,channel)).0 }, _ => 0 } }
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_prepare_secondary(context: *mut u8, channel: u32) -> u32 { match Channel::parse(channel) { Some(channel) => unsafe { execute(StageContext(context),Stage::PrepareSecondary(channel)).0 }, None => 0 } }
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_merge_auxiliary(context: *mut u8) -> u32 { unsafe { execute(StageContext(context),Stage::MergeAuxiliary).0 } }
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_configure_output_geometry(context: *mut u8, kind: u32) -> u32 { unsafe { execute(StageContext(context),Stage::ConfigureOutputGeometry(OutputKind::from_abi(kind))).0 } }
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_divide_raw(context: *mut u8) -> u32 { unsafe { execute(StageContext(context),Stage::DivideRaw).0 } }
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_add_raw(context: *mut u8) -> u32 { unsafe { execute(StageContext(context),Stage::AddRaw).0 } }
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_resize_review(context: *mut u8) -> u32 { unsafe { execute(StageContext(context),Stage::ResizeReview).0 } }
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_resize_display(context: *mut u8) -> u32 { unsafe { execute(StageContext(context),Stage::ResizeDisplay).0 } }
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_resize_thumbnail(context: *mut u8) -> u32 { unsafe { execute(StageContext(context),Stage::ResizeThumbnail).0 } }
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_resize_raw_review(context: *mut u8) -> u32 { unsafe { execute(StageContext(context),Stage::ResizeRawReview).0 } }
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_select_crop(context: *mut u8) -> u32 { unsafe { execute(StageContext(context),Stage::SelectCrop).0 } }
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_match_points(context: *mut u8) -> u32 { unsafe { execute(StageContext(context),Stage::MatchPoints).0 } }
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_match_triangles(context: *mut u8) -> u32 { unsafe { execute(StageContext(context),Stage::MatchTriangles).0 } }
