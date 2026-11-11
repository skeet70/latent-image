use core::ptr::{read_unaligned, read_volatile, write_volatile};

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Address(u32);
impl Address {
    fn at(self, offset: u32) -> Self { Self(self.0.wrapping_add(offset)) }
    unsafe fn byte(self) -> u8 { unsafe { read_volatile(self.0 as *const u8) } }
    unsafe fn half(self) -> u16 { unsafe { read_unaligned(self.0 as *const u16) } }
    unsafe fn word(self) -> u32 { unsafe { read_unaligned(self.0 as *const u32) } }
}
#[derive(Clone, Copy)]
#[repr(transparent)]
struct Coordinate(i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct Scale(i16);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct Angle(i32);
#[derive(Clone, Copy)]
enum Component { Luma, Red, Derived, Blue }
impl Component {
    fn parse(value: i16) -> Self { match value { 0 => Self::Luma, 1 => Self::Red, 2 => Self::Derived, _ => Self::Blue } }
}
#[derive(Clone, Copy)]
enum FeatureKind { Gradient, Intensity, Horizontal, Vertical }
impl FeatureKind {
    fn parse(value: u16) -> Option<Self> { match value { 0 => Some(Self::Gradient), 1 => Some(Self::Intensity), 2 => Some(Self::Horizontal), 3 => Some(Self::Vertical), _ => None } }
}
#[derive(Clone, Copy)]
struct Feature { component: Component, kind: FeatureKind, x: i32, y: i32, distance: i32 }
#[derive(Clone, Copy)]
struct Image { pixels: Address, width: i32, height: i32, pair_stride: i32 }
impl Image {
    unsafe fn load(address: Address) -> Self {
        let height = unsafe { address.at(4).word() } as i32;
        let width = unsafe { address.at(8).word() } as i32;
        Self { pixels: Address(unsafe { address.word() }), width, height, pair_stride: width >> 1 }
    }
    unsafe fn pixel(self, x: i32, y: i32, component: Component) -> i32 {
        let pair = self.pixels.at(y.wrapping_mul(self.pair_stride).wrapping_add(x >> 1).wrapping_mul(4) as u32);
        match component {
            Component::Luma => (unsafe { pair.at((x & 1) as u32).byte() }) as i32,
            Component::Red => (unsafe { pair.at(3).byte() }) as i32,
            Component::Blue => (unsafe { pair.at(2).byte() }) as i32,
            Component::Derived => {
                let red = unsafe { pair.at(3).byte() } as i32;
                let luma = unsafe { pair.at((x & 1) as u32).byte() } as i32;
                let blue = unsafe { pair.at(2).byte() } as i32;
                ((198 * red - 98 * luma - 43 * blue) >> 8) + 99
            }
        }
    }
    unsafe fn strip(self, x: i32, y: i32, component: Component, dx: i32, dy: i32) -> i32 {
        unsafe { self.pixel(x-dx,y-dy,component) + self.pixel(x,y,component) + self.pixel(x+dx,y+dy,component) }
    }
}
#[derive(Clone, Copy)]
#[repr(C)]
struct Candidate { active: u32, x: i32, y: i32, scale: i16, final_score: i16, coarse_score: i16, fine_score: i16, score: i32 }
#[derive(Clone, Copy)]
struct Cascade { features: Address, thresholds: Address, scores: Address }
const COARSE: Cascade = Cascade { features: Address(0x107bd24), thresholds: Address(0x107c10c), scores: Address(0x107c204) };
const FINE: Cascade = Cascade { features: Address(0x107f404), thresholds: Address(0x107f7ec), scores: Address(0x107f8b4) };

unsafe extern "C" {
    fn xm1_buffers_configuration_byte(key: u32) -> u32;
    fn xm1_pipeline_image_redeye_divide(numerator: i32, denominator: i32) -> i32;
    fn xm1_pipeline_image_redeye_reset_records(list: *mut u8);
    fn xm1_pipeline_image_redeye_record_at(list: *const u8, index: i32) -> u32;
    fn xm1_pipeline_image_redeye_append_record(list: *mut u8, source: *const u8);
    fn xm1_pipeline_image_redeye_compact_records(list: *mut u8);
    fn xm1_pipeline_image_redeye_copy_records(source: *const u8, destination: *mut u8);
}
fn divide(n: i32, d: i32) -> i32 { unsafe { xm1_pipeline_image_redeye_divide(n,d) } }
fn trunc_shift(value: i32, bits: u32) -> i32 { value.wrapping_add(if value < 0 { (1i32 << bits)-1 } else { 0 }) >> bits }
unsafe fn interpolation(table: Address, index: i32, fraction: i32) -> i32 {
    let first = unsafe { table.at(index.wrapping_mul(2) as u32).half() } as i16 as i32;
    let second = unsafe { table.at(index.wrapping_add(1).wrapping_mul(2) as u32).half() } as i16 as i32;
    (1024-fraction).wrapping_mul(first).wrapping_add(fraction.wrapping_mul(second))
}
unsafe fn sine(angle: Angle) -> i32 {
    let normalized = (angle.0.wrapping_add(4096) as u32 & 8191) as i32 - 4096;
    let (position, table, negative) = match normalized {
        -4096..=-3073 => (normalized+4096, Address(0x10871a8), true),
        -3072..=-2049 => (-2048-normalized, Address(0x1086da0), true),
        -2048..=-1025 => (normalized+2048, Address(0x1086da0), true),
        -1024..=-1 => (-normalized, Address(0x10871a8), true),
        0..=1023 => (normalized, Address(0x10871a8), false),
        1024..=2047 => (2048-normalized, Address(0x1086da0), false),
        2048..=3071 => (normalized-2048, Address(0x1086da0), false),
        _ => (4096-normalized, Address(0x10871a8), false),
    };
    let sum = unsafe { interpolation(table, position >> 2, (position & 3)*256) };
    trunc_shift(if negative { sum.wrapping_neg() } else { sum },10)
}
unsafe fn angle(vertical: i32, horizontal: i32) -> Angle {
    if horizontal == 0 { return Angle(if vertical > 0 { 2048 } else if vertical < 0 { -2048 } else { 0 }); }
    if vertical == 0 { return Angle(if horizontal > 0 { 0 } else { -4096 }); }
    let x = horizontal.wrapping_abs();
    let y = vertical.wrapping_abs();
    let mut octant = if horizontal > 0 { 0 } else { 4 } + if vertical > 0 { 0 } else { 2 };
    let (small, large) = if x > y { (y,x) } else { octant += 1; (x,y) };
    let index = divide(small.wrapping_shl(8),large);
    let fraction = divide(small.wrapping_shl(18),large).wrapping_sub(index.wrapping_shl(10));
    let a = trunc_shift(unsafe { interpolation(Address(0x1086fa4), index, fraction) },10);
    Angle(match octant { 1 => 2048-a, 2 => -a, 3 => a-2048, 4 => 4096-a, 5 => a+2048, 6 => a-4096, 7 => -2048-a, _ => a })
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_gradient_angle(vertical: i32, horizontal: i32) -> i32 { unsafe { angle(vertical,horizontal).0 } }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_angle_sine(value: i32) -> i32 { unsafe { sine(Angle(value)) } }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_angle_cosine(value: i32) -> i32 { unsafe { sine(Angle(value.wrapping_add(2048))) } }

unsafe fn feature_at(address: Address) -> Option<Feature> {
    Some(Feature {
        component: Component::parse(unsafe { address.half() } as i16),
        kind: FeatureKind::parse(unsafe { address.at(2).half() })?,
        x: unsafe { address.at(4).half() } as i16 as i32,
        y: unsafe { address.at(6).half() } as i16 as i32,
        distance: unsafe { address.at(8).half() } as i16 as i32,
    })
}
unsafe fn bins(first: i32, second: i32, reverse: bool) -> u32 {
    let a = unsafe { Address(0x1083754).at(first.wrapping_add(255) as u32).byte() } as u32;
    let b = unsafe { Address(0x1083754).at(second.wrapping_add(255) as u32).byte() } as u32;
    (unsafe { Address(0x1083714).at(if reverse { a*8+b } else { a+b*8 }).byte() }) as u32
}
unsafe fn feature_value(image: Image, x: i32, y: i32, feature: Feature, smooth: bool) -> u32 {
    let x = x.wrapping_add(feature.x);
    let y = y.wrapping_add(feature.y);
    let c = feature.component;
    let d = feature.distance;
    match feature.kind {
        FeatureKind::Intensity => {
            let value = if smooth { unsafe { (image.strip(x,y-1,c,1,0)+image.strip(x,y,c,1,0)+image.strip(x,y+1,c,1,0))/9 } } else { unsafe { image.pixel(x,y,c) } };
            (value >> 2) as u32
        }
        FeatureKind::Gradient => {
            let (a,b) = if smooth {
                unsafe { ((image.strip(x-1,y,c,0,1)-image.strip(x+1,y,c,0,1))/3, (image.strip(x,y-1,c,1,0)-image.strip(x,y+1,c,1,0))/3) }
            } else {
                let center = unsafe { image.pixel(x,y,c) };
                unsafe { (center-image.pixel(x+d,y,c),center-image.pixel(x,y+d,c)) }
            };
            unsafe { bins(a,b,false) }
        }
        FeatureKind::Horizontal | FeatureKind::Vertical => {
            let horizontal = matches!(feature.kind, FeatureKind::Horizontal);
            let (dx,dy) = if horizontal { (d,0) } else { (0,d) };
            let (a,b) = if smooth {
                let (sx,sy) = if horizontal { (0,1) } else { (1,0) };
                let center = unsafe { image.strip(x+dx,y+dy,c,sx,sy) };
                unsafe { ((image.strip(x,y,c,sx,sy)-center)/3, (center-image.strip(x+2*dx,y+2*dy,c,sx,sy))/3) }
            } else {
                let center = unsafe { image.pixel(x+dx,y+dy,c) };
                unsafe { (image.pixel(x,y,c)-center, center-image.pixel(x+2*dx,y+2*dy,c)) }
            };
            unsafe { bins(a,b,true) }
        }
    }
}
enum CascadeResult { Rejected, Accepted(i16), InvalidFeature }
unsafe fn cascade(image: Image, x: i32, y: i32, cascade: Cascade, smooth: bool) -> CascadeResult {
    let mut score = 0i16;
    for stage in 0..100u32 {
        let Some(feature) = (unsafe { feature_at(cascade.features.at(stage*10)) }) else { return CascadeResult::InvalidFeature; };
        let bin = unsafe { feature_value(image,x,y,feature,smooth) };
        score = score.wrapping_add(unsafe { cascade.scores.at((stage*64+bin)*2).half() } as i16);
        if score < unsafe { cascade.thresholds.at(stage*2).half() } as i16 { return CascadeResult::Rejected; }
    }
    CascadeResult::Accepted(score)
}
unsafe fn gradient(image: Image, x: Coordinate, y: Coordinate, component: Component, orientation: u32) -> u32 {
    const DIRECTIONS: [(i32,i32);8] = [(1,0),(1,1),(0,1),(-1,1),(-1,0),(-1,-1),(0,-1),(1,-1)];
    let (dx,dy) = DIRECTIONS[orientation as usize];
    let target = Angle(trunc_shift(orientation.wrapping_mul(4096) as i32,2));
    let mut maximum = 0;
    let samples = if orientation & 1 == 0 { 4 } else { 2 };
    for distance in 1..=samples {
        let px = x.0+dx*distance;
        let py = y.0+dy*distance;
        let gx = unsafe { image.strip(px+1,py,component,0,1)-image.strip(px-1,py,component,0,1) };
        let gy = unsafe { image.strip(px,py+1,component,1,0)-image.strip(px,py-1,component,1,0) };
        if gx != 0 || gy != 0 {
            let measured = unsafe { angle(gy,gx) };
            let product = unsafe { sine(Angle(target.0+2048)).wrapping_mul(sine(Angle(measured.0+2048))).wrapping_add(sine(target).wrapping_mul(sine(measured))) };
            maximum = maximum.max(trunc_shift(product.wrapping_neg(),12));
        }
    }
    let normalized = if maximum <= 0 { 0 } else { trunc_shift(maximum.wrapping_shl(16),12) };
    ((normalized.wrapping_mul(255).wrapping_add(if normalized.wrapping_mul(255) < 0 { 65535 } else { 0 }) as u32) >> 16) & 255
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_gradient(pixels: u32, width: i32, height: i32, x: i32, y: i32, component: i32, orientation: u32) -> u32 {
    unsafe { gradient(Image { pixels: Address(pixels), width, height, pair_stride: width >> 1 },Coordinate(x),Coordinate(y),Component::parse(component as i16),orientation) }
}
unsafe fn final_score(image: Image, x: i32, y: i32) -> Option<i16> {
    let mut score = 0i16;
    for stage in 0..24 {
        let component = Component::parse(unsafe { Address(0x10836b4).at(stage*4).half() } as i16);
        let orientation = unsafe { Address(0x10836b6).at(stage*4).half() } as i16 as i32 as u32;
        let value = unsafe { gradient(image,Coordinate(x as i16 as i32),Coordinate(y as i16 as i32),component,orientation) };
        score = score.wrapping_add(unsafe { Address(0x1082ab4).at((stage*64+(value >> 2))*2).half() } as i16);
        if score < unsafe { Address(0x107c1d4).at(stage*2).half() } as i16 { return None; }
    }
    Some(score)
}
unsafe fn append(list: Address, candidate: &Candidate) {
    unsafe { xm1_pipeline_image_redeye_append_record(list.0 as *mut u8, (candidate as *const Candidate).cast()) };
}
unsafe fn record(list: Address, index: i32) -> *mut Candidate {
    (unsafe { xm1_pipeline_image_redeye_record_at(list.0 as *const u8,index) }) as *mut Candidate
}
unsafe fn count(list: Address) -> i32 { (unsafe { list.at(12).word() }) as i32 }
unsafe fn compact(list: Address) { unsafe { xm1_pipeline_image_redeye_compact_records(list.0 as *mut u8) }; }
unsafe fn distance_square(first: *const Candidate, second: *const Candidate) -> i32 {
    let dx = unsafe { (*first).x.wrapping_sub((*second).x) };
    let dy = unsafe { (*first).y.wrapping_sub((*second).y) };
    dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy))
}
unsafe fn remove_duplicates(list: Address) {
    for first in 0..unsafe { count(list) }-1 {
        let a = unsafe { record(list,first) };
        for second in first+1..unsafe { count(list) } {
            let b = unsafe { record(list,second) };
            if unsafe { (*a).x == (*b).x && (*a).y == (*b).y && read_unaligned(core::ptr::addr_of!((*a).scale).cast::<u32>()) == read_unaligned(core::ptr::addr_of!((*b).scale).cast::<u32>()) } {
                unsafe { (*b).active = 0 };
            }
        }
    }
    unsafe { compact(list) };
}
unsafe fn accumulate(list: Address) {
    for first in 0..unsafe { count(list) } {
        let a = unsafe { record(list,first) };
        for second in 0..unsafe { count(list) } {
            if first == second { continue; }
            let b = unsafe { record(list,second) };
            if unsafe { (*a).scale != (*b).scale } { continue; }
            let unit = divide(16384,unsafe { (*a).scale } as i32);
            if unsafe { distance_square(a,b) } <= unit.wrapping_mul(unit).wrapping_shl(3) {
                unsafe {
                    (*b).score = (*b).score.wrapping_add((*a).final_score as i32);
                    (*a).score = (*a).score.wrapping_add((*b).final_score as i32);
                }
            }
        }
    }
    unsafe { compact(list) };
}
unsafe fn remove_nearby(list: Address, radius: i32) {
    for first in 0..unsafe { count(list) }-1 {
        let a = unsafe { record(list,first) };
        for second in first+1..unsafe { count(list) } {
            let b = unsafe { record(list,second) };
            let sa = unsafe { (*a).scale } as i32;
            let sb = unsafe { (*b).scale } as i32;
            if sa > sb.wrapping_mul(2) || sb > sa.wrapping_mul(2) { continue; }
            let distance = unsafe { distance_square(a,b) };
            let limit = divide(radius.wrapping_shl(14),sa).wrapping_add(divide(radius.wrapping_shl(14),sb));
            let limit = limit.wrapping_mul(limit);
            if distance | limit < 0 { return; }
            if distance >= limit { continue; }
            unsafe {
                if (*a).score > (*b).score { (*b).active = 0; }
                else if (*a).score < (*b).score { (*a).active = 0; }
                else if (*a).final_score > (*b).final_score { (*b).active = 0; }
                else if (*a).final_score < (*b).final_score { (*a).active = 0; }
            }
        }
    }
    unsafe { compact(list) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_remove_duplicates(list: u32) { unsafe { remove_duplicates(Address(list)) }; }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_accumulate_scores(list: u32) { unsafe { accumulate(Address(list)) }; }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_remove_nearby(list: u32, radius: i32) { unsafe { remove_nearby(Address(list),radius) }; }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_filter_candidates(list: u32, _image: u32, radius: i32, _options: u32) {
    if unsafe { Address(0x14e213c).byte() } <= 2 { unsafe { remove_duplicates(Address(list)) }; }
    unsafe { accumulate(Address(list)); remove_nearby(Address(list),radius); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_detect(image: u32, destination: u32, detector: u32, _arg3: u32, _arg4: u32, options: u32, _arg6: u32, _arg7: u32, faces: u32, groups: u32) {
    let step = unsafe { xm1_buffers_configuration_byte(0xa446) } as u8;
    let first_scale = unsafe { xm1_buffers_configuration_byte(0xa447) } as u8;
    let minimum_scale = unsafe { xm1_buffers_configuration_byte(0xa448) } as u8;
    let scale_limit = unsafe { xm1_buffers_configuration_byte(0xa456) } as u8;
    unsafe {
        write_volatile(0x14e213c as *mut u8,step);
        write_volatile(0x14e213d as *mut u8,first_scale);
        write_volatile(0x14e213a as *mut u8,minimum_scale);
        write_volatile(0x14e213b as *mut u8,scale_limit);
    }
    let source = unsafe { Image::load(Address(image)) };
    let (limit_x,limit_y) = if source.height < source.width { (16384,12288) } else { (12288,16384) };
    let candidates = Address(detector).at(44);
    unsafe { xm1_pipeline_image_redeye_reset_records(candidates.0 as *mut u8) };
    for face_index in 0..unsafe { Address(faces).byte() } as u32 {
        let face = Address(faces).at(face_index*18);
        let face_scale = unsafe { face.at(14).half() } as i32;
        if unsafe { face.at(18).byte() } != 0 || (minimum_scale as i32) > face_scale*100/768 { continue; }
        let orientation = unsafe { face.at(19).byte() };
        for scale_index in first_scale..scale_limit {
            let view = Address(groups).at(face_index*52+scale_index as u32*12+8);
            let width = unsafe { view.at(4).half() } as i32;
            let height = unsafe { view.at(6).half() } as i32;
            let padding = unsafe { view.at(8).half() } as i32;
            let scaled = Image { pixels: Address(unsafe { view.word() }), width, height, pair_stride: (width+padding) >> 1 };
            let scale = Scale(divide(height << 10,face_scale) as i16);
            let (start_x,start_y,end_x,end_y) = match orientation {
                2..=4 => (width >> 1,0,width,height),
                5..=7 => (0,height >> 1,width,height),
                8..=10 => (0,0,width >> 1,height),
                _ => (0,0,width,height >> 1),
            };
            let mut y = start_y+1;
            while y < end_y-12 {
                let mut x = start_x+1;
                while x < end_x-12 {
                    let coarse = match unsafe { cascade(scaled,x,y,COARSE,false) } {
                        CascadeResult::InvalidFeature => return,
                        CascadeResult::Accepted(score) if score > 23 => score,
                        _ => { x += step as i32; continue; }
                    };
                    for dy in -1..=1 {
                        if step == 1 && dy != 0 { continue; }
                        for dx in -1..=1 {
                            if step == 1 && dx != 0 { continue; }
                            let fine = match unsafe { cascade(scaled,x+dx,y+dy,FINE,true) } {
                                CascadeResult::InvalidFeature => return,
                                CascadeResult::Accepted(score) if score >= 0 => score,
                                _ => continue,
                            };
                            let Some(final_score) = (unsafe { final_score(scaled,x+dx+5,y+dy+5) }) else { continue; };
                            if final_score <= -2160 { continue; }
                            let fx = (x+dx+10).wrapping_mul(16384);
                            let fy = (y+dy+10).wrapping_mul(16384);
                            if trunc_shift(fx,10)-8 >= limit_x || trunc_shift(fy,10)-8 >= limit_y { continue; }
                            let candidate = Candidate {
                                active: 0,
                                x: (unsafe { face.at(8).half() } as i32*16).wrapping_add(trunc_shift(divide(fx.wrapping_sub(90112),scale.0 as i32).wrapping_shl(10),10)).wrapping_sub(8),
                                y: (unsafe { face.at(10).half() } as i32*16).wrapping_add(trunc_shift(divide(fy.wrapping_sub(90112),scale.0 as i32).wrapping_shl(10),10)).wrapping_sub(8),
                                scale: scale.0, final_score, coarse_score: coarse, fine_score: fine, score: final_score as i32,
                            };
                            unsafe { append(candidates,&candidate) };
                        }
                    }
                    x += step as i32;
                }
                y += step as i32;
            }
        }
    }
    unsafe {
        xm1_pipeline_image_redeye_filter_candidates(candidates.0,image,11,options);
        xm1_pipeline_image_redeye_copy_records(candidates.0 as *const u8,destination as *mut u8);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_search_local(image: u32, destination: u32, scale: i32, _options: u32) {
    let source = unsafe { Image::load(Address(image)) };
    for y in 1..source.height-9 {
        for x in 1..source.width-9 {
            let coarse = match unsafe { cascade(source,x-1,y-1,COARSE,false) } {
                CascadeResult::InvalidFeature => return,
                CascadeResult::Accepted(score) if score > 23 => score,
                _ => continue,
            };
            let fine = match unsafe { cascade(source,x-1,y-1,FINE,true) } {
                CascadeResult::InvalidFeature => return,
                CascadeResult::Accepted(score) if score >= 0 => score,
                _ => continue,
            };
            let Some(final_score) = (unsafe { final_score(source,x+4,y+4) }) else { continue; };
            if final_score < 0 { continue; }
            let candidate = Candidate {
                active: 0,
                x: divide((x*16+72-((source.width-1)*16 >> 1)).wrapping_mul(1024),scale),
                y: divide((y*16+72-((source.height-1)*16 >> 1)).wrapping_mul(1024),scale),
                scale: scale as i16, final_score, coarse_score: coarse, fine_score: fine, score: final_score as i32,
            };
            unsafe { append(Address(destination),&candidate) };
        }
    }
}
