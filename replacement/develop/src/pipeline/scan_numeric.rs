use core::ptr::{read_volatile, write_volatile};
use super::shared::Address32;

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Memory(Address32);
impl Memory {
    fn at(self, offset: u32) -> Self { Self(Address32(self.0.0.wrapping_add(offset))) }
    unsafe fn byte(self) -> u8 { unsafe { read_volatile(self.0.0 as *const u8) } }
    unsafe fn half(self) -> u16 { unsafe { read_volatile(self.0.0 as *const u16) } }
    unsafe fn word(self) -> u32 { unsafe { read_volatile(self.0.0 as *const u32) } }
    unsafe fn put_byte(self, value: u8) { unsafe { write_volatile(self.0.0 as *mut u8, value) } }
    unsafe fn put_half(self, value: u16) { unsafe { write_volatile(self.0.0 as *mut u16, value) } }
    unsafe fn put_word(self, value: u32) { unsafe { write_volatile(self.0.0 as *mut u32, value) } }
    unsafe fn pointer(self) -> Self { Self(Address32(unsafe { self.word() })) }
}

#[derive(Clone, Copy)]
struct ScanPlane(Memory);
#[derive(Clone, Copy)]
struct Region(Memory);
#[derive(Clone, Copy)]
struct Workspace(Memory);
#[derive(Clone, Copy)]
struct SampleScratch(Memory);
#[derive(Clone, Copy)]
struct ToneCurve(Memory);
#[derive(Clone, Copy)]
struct Levels(Memory);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct ConfigurationKey(u32);
#[derive(Clone, Copy)]
struct SampleCoordinate(u32);
#[derive(Clone, Copy)]
struct SampleIndex(u32);

#[derive(Clone, Copy)]
enum SensorPattern { RedFirst, BlueFirst, RedEven, BlueEven, GreenEven, GreenOdd, GreenEvenAlternate, GreenEvenReduced, SixBySix, Other(u32) }
impl SensorPattern {
    fn parse(value: u32) -> Self { match value { 0 => Self::RedFirst, 1 => Self::BlueFirst, 2 => Self::RedEven, 3 => Self::BlueEven, 4 => Self::GreenEven, 5 => Self::GreenOdd, 6 => Self::GreenEvenAlternate, 7 => Self::GreenEvenReduced, 8 => Self::SixBySix, value => Self::Other(value) } }
    fn code(self) -> u32 { match self { Self::RedFirst => 0, Self::BlueFirst => 1, Self::RedEven => 2, Self::BlueEven => 3, Self::GreenEven => 4, Self::GreenOdd => 5, Self::GreenEvenAlternate => 6, Self::GreenEvenReduced => 7, Self::SixBySix => 8, Self::Other(value) => value } }
}

#[derive(Clone, Copy)]
struct Rectangle { left: u32, top: u32, width: u32, height: u32 }
impl Region {
    unsafe fn rectangle(self, six_by_six: bool) -> Rectangle {
        let p = self.0.at(if six_by_six { 16 } else { 0 });
        unsafe { Rectangle { left: p.word(), top: p.at(4).word(), width: p.at(8).word(), height: p.at(12).word() } }
    }
    unsafe fn needs_measurement(self) -> bool {
        unsafe { self.0.at(0x22).half() == 0 || self.0.at(0x20).half() == 0 || self.0.at(0x24).word() as i32 >= configuration_byte(ConfigurationKey(0xa573)) as i32 || self.0.at(0x30).word() != 0 }
    }
}
impl ScanPlane {
    unsafe fn pattern(self) -> SensorPattern { SensorPattern::parse(unsafe { self.0.at(0x28).word() }) }
    unsafe fn sample(self, x: SampleCoordinate, y: SampleCoordinate) -> u32 {
        unsafe {
            let index = self.0.at(4).word().wrapping_mul(y.0).wrapping_add(x.0);
            let address = self.0.pointer();
            if self.0.at(0x20).word() == 0 { address.at(index.wrapping_mul(2)).half() as u32 }
            else { (packed_sensor_sample(address, SampleIndex(index)) as u32) << 2 }
        }
    }
    unsafe fn shading(self, x: SampleCoordinate) -> u16 {
        unsafe {
            let index = unsigned_divide(x.0.wrapping_mul(100),self.0.at(0x14).half() as u32);
            self.0.at(0x10).pointer().at(index.wrapping_mul(2)).half()
        }
    }
}

unsafe extern "C" {
    #[link_name = "xm1_buffers_configuration_byte"]
    fn configuration_byte(key: ConfigurationKey) -> u32;
    #[link_name = "xm1_buffers_configuration_half"]
    fn configuration_half(key: ConfigurationKey) -> u16;
    #[link_name = "xm1_pipeline_image_configuration_signed_byte"]
    fn configuration_signed_byte(key: ConfigurationKey) -> i32;
    #[link_name = "xm1_pipeline_image_configuration_pointer"]
    fn configuration_pointer(key: ConfigurationKey) -> Address32;
    fn scan_numeric_arithmetic_fault(value: ArithmeticFaultValue) -> !;
}

fn left_shift(value: u32, amount: u32) -> u32 { if amount >= 32 { 0 } else { value << amount } }
fn right_shift(value: u32, amount: u32) -> u32 { if amount >= 32 { 0 } else { value >> amount } }
fn signed_shift(value: i32, amount: u32) -> i32 { if amount >= 32 { value >> 31 } else { value >> amount } }
#[derive(Clone, Copy)]
#[repr(u32)]
enum ArithmeticFaultValue { Zero=0, Positive=0x7fffffff, Negative=0x80000000, Unsigned=0xffffffff }
fn unsigned_divide(value: u32, divisor: u32) -> u32 {
    if divisor == 0 { unsafe { scan_numeric_arithmetic_fault(if value == 0 { ArithmeticFaultValue::Zero } else { ArithmeticFaultValue::Unsigned }); } }
    value/divisor
}
fn signed_round_divide(value: i32, divisor: i32) -> i32 {
    if divisor == 0 { unsafe { scan_numeric_arithmetic_fault(if value > 0 { ArithmeticFaultValue::Positive } else if value < 0 { ArithmeticFaultValue::Negative } else { ArithmeticFaultValue::Zero }); } }
    value.wrapping_div(divisor)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_select_region(request: Address32, workspace: Address32) {
    unsafe {
        let request = Memory(request);
        let output = Memory(workspace).at(0xc28);
        let count = request.byte() as u32;
        let regions = request.at(4);
        let horizontal = request.at(0x298).pointer();
        let vertical = horizontal.at(400);
        let mut selected = 0u32;
        let mut brightest = 0u32;
        let mut tallest = 0u32;
        let mut maximum_brightness = 0u16;
        let mut maximum_height = 0u16;
        let mut valid = false;
        let mut brightness_choice = 3u8;
        let mut center_choice = 3u8;
        if count == 1 {
            valid = Region(regions).needs_measurement();
        } else {
            for index in 0..count {
                let region = Region(regions.at(index * 52));
                let rectangle = region.rectangle(false);
                if region.needs_measurement() {
                    let brightness = region_brightness(ScanPlane(request.at(0x258)), region, ToneCurve(request.at(0x254).pointer()), Levels(request.at(0x20c)));
                    let dx = rectangle.left.wrapping_add(rectangle.width >> 1).wrapping_sub(request.at(0x25c).word() >> 1);
                    let dy = rectangle.top.wrapping_add(rectangle.height >> 1).wrapping_sub(request.at(0x260).word() >> 1);
                    horizontal.at(index * 4).put_word(dx.wrapping_mul(dx));
                    vertical.at(index * 4).put_word(dy.wrapping_mul(dy));
                    if maximum_brightness < brightness { maximum_brightness = brightness; brightest = index; }
                    if (maximum_height as u32) < rectangle.height { maximum_height = rectangle.height as u16; tallest = index; }
                    valid = true;
                }
            }
            let width = request.at(0x25c).word() >> 1;
            let first = configuration_byte(ConfigurationKey(0xa577)).wrapping_mul(width) / 100;
            let second = configuration_byte(ConfigurationKey(0xa577)).wrapping_mul(width) / 100;
            let limit_x = first.wrapping_mul(second);
            let height = request.at(0x260).word() >> 1;
            let first = configuration_byte(ConfigurationKey(0xa577)).wrapping_mul(height) / 100;
            let second = configuration_byte(ConfigurationKey(0xa577)).wrapping_mul(height) / 100;
            let limit_y = first.wrapping_mul(second);
            let tall_quality = regions.at(tallest * 52 + 0x28).half() as i32;
            let bright_quality = regions.at(brightest * 52 + 0x28).half() as u32;
            let weighted = configuration_half(ConfigurationKey(0xa578)) as u32;
            let prefer_tall = (weighted.wrapping_mul(bright_quality) as i32) / 100 < tall_quality;
            let tall_outside = horizontal.at(tallest * 4).word() > limit_x || vertical.at(tallest * 4).word() > limit_y;
            let bright_outside = horizontal.at(brightest * 4).word() > limit_x || vertical.at(brightest * 4).word() > limit_y;
            brightness_choice = u8::from(prefer_tall);
            let choose_tall = if prefer_tall { !(tall_outside && !bright_outside) } else { bright_outside && !tall_outside };
            center_choice = u8::from(choose_tall);
            selected = if choose_tall { tallest } else { brightest };
        }
        output.put_half(if valid { selected as u16 } else { u16::MAX });
        output.at(2).put_half(maximum_brightness);
        output.at(4).put_word(brightest);
        output.at(8).put_word(tallest);
        output.at(12).put_byte(brightness_choice);
        output.at(13).put_byte(center_choice);
        if valid {
            let rectangle = Region(regions.at(selected * 52)).rectangle(false);
            output.at(16).put_word(rectangle.left);
            output.at(20).put_word(rectangle.top);
            output.at(24).put_word(rectangle.width);
            output.at(28).put_word(rectangle.height);
            output.at(32).put_word(rectangle.left.wrapping_add(((rectangle.width as i32) / 2) as u32));
            output.at(36).put_word(rectangle.top.wrapping_add(((rectangle.height as i32) / 2) as u32));
        }
    }
}

unsafe fn region_brightness(plane: ScanPlane, region: Region, curve: ToneCurve, levels: Levels) -> u16 {
    unsafe {
        let pattern = plane.pattern();
        let mut rectangle = region.rectangle(matches!(pattern, SensorPattern::SixBySix));
        let origin_x = plane.0.at(0x18).half() as u32;
        let origin_y = plane.0.at(0x1c).half() as u32;
        let mut x = rectangle.left.wrapping_sub(origin_x);
        let mut y = rectangle.top.wrapping_sub(origin_y);
        let mode = plane.0.at(0x2c).word();
        let (step_x, step_y) = if matches!(pattern, SensorPattern::SixBySix) { (3,3) }
            else if mode == 0 || (mode == 1 && !matches!(pattern, SensorPattern::RedEven | SensorPattern::BlueEven)) {
                if y & 1 != 0 { y = y.wrapping_add(1); rectangle.height = rectangle.height.wrapping_sub(1); }
                if x & 1 == 0 { x = x.wrapping_add(1); rectangle.width = rectangle.width.wrapping_sub(1); }
                (2,2)
            } else if matches!(pattern, SensorPattern::RedEven | SensorPattern::BlueEven) {
                if mode == 1 {
                    let delta = match y & 3 { 0 => 1, 2 => 3, 3 => 2, _ => 0 };
                    y = y.wrapping_add(delta); rectangle.height = rectangle.height.wrapping_sub(delta);
                    (1,4)
                } else {
                    if y & 7 == 0 { y = y.wrapping_add(1); rectangle.height = rectangle.height.wrapping_sub(1); }
                    else if y & 7 != 1 { y = (y & !7).wrapping_add(9); rectangle.height = rectangle.height.wrapping_sub(y & 7); }
                    (1,8)
                }
            } else {
                let delta = (4u32.wrapping_sub(y & 3)) & 3;
                y = y.wrapping_add(delta); rectangle.height = rectangle.height.wrapping_sub(delta);
                if x & 1 == 0 { x = x.wrapping_add(1); rectangle.width = rectangle.width.wrapping_sub(1); }
                (2,4)
            };
        let mut columns = rectangle.width / step_x;
        let mut rows = rectangle.height / step_y;
        let mut column_factor = 1u32;
        let mut row_factor = 1u32;
        while columns.wrapping_mul(rows) > configuration_half(ConfigurationKey(0xa574)) as u32 {
            if columns < rows { rows >>= 1; row_factor = row_factor.wrapping_mul(2); }
            else { columns >>= 1; column_factor = column_factor.wrapping_mul(2); }
        }
        x = x.wrapping_add(origin_x);
        y = y.wrapping_add(origin_y);
        let end_x = x.wrapping_add(rectangle.width);
        let end_y = y.wrapping_add(rectangle.height);
        let mut histogram = [0u16;256];
        let mut samples = 0u32;
        let adjustment = plane.0.at(12).byte().wrapping_sub(12) as i8;
        let mut row = y;
        while row < end_y {
            let mut column = x;
            while column < end_x {
                let shading = plane.shading(SampleCoordinate(column)) as u32;
                let raw = plane.sample(SampleCoordinate(column), SampleCoordinate(row));
                let shaded = raw.wrapping_sub(shading);
                let corrected = (levels.0.half() as u32).wrapping_mul(shaded).wrapping_add(128) as i32;
                let mut value = (corrected / 256).max(0) as u32;
                value = if adjustment < 0 { left_shift(value, adjustment.wrapping_neg() as u8 as u32) } else { signed_shift(value as i32, adjustment as u32) as u32 };
                if value as i32 >= 4096 { value = 4095; }
                let bin = curve.0.at(value).byte() as usize;
                histogram[bin] = histogram[bin].wrapping_add(1);
                samples = samples.wrapping_add(1);
                column = column.wrapping_add(column_factor.wrapping_mul(step_x));
            }
            row = row.wrapping_add(row_factor.wrapping_mul(step_y));
        }
        let threshold = samples.wrapping_mul(100u32.wrapping_sub(configuration_byte(ConfigurationKey(0xa576)))) / 100;
        let mut cumulative = 0u32;
        for index in (0..256).rev() {
            cumulative = cumulative.wrapping_add(histogram[index] as u32);
            if cumulative > threshold { return index as u16 + 1; }
        }
        u16::MAX
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_region_brightness(plane: Address32, region: Address32, curve: Address32, levels: Address32, output: Address32) {
    unsafe { Memory(output).put_half(region_brightness(ScanPlane(Memory(plane)),Region(Memory(region)),ToneCurve(Memory(curve)),Levels(Memory(levels)))); }
}

unsafe fn packed_sensor_sample(data: Memory, index: SampleIndex) -> u16 {
    let offset = index.0.wrapping_mul(3) >> 1;
    let pointer = data.at(offset);
    let value = unsafe { pointer.byte() as u16 | ((pointer.at(1).byte() as u16) << 8) };
    if offset % 3 == 0 { value & 0xfff } else { value >> 4 }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_read_packed_sample(data: Address32, index: u32) -> u16 {
    unsafe { packed_sensor_sample(Memory(data), SampleIndex(index)) }
}

#[derive(Clone, Copy)]
struct SamplingGrid { pitch_x: u32, pitch_y: u32, inset_x: u32, inset_y: u32, width: u32, height: u32, phase: [u8;2] }

unsafe fn sampling_grid(plane: ScanPlane, region: Region) -> SamplingGrid {
    unsafe {
        let six_by_six = matches!(plane.pattern(), SensorPattern::SixBySix);
        let divisions = configuration_byte(ConfigurationKey(0xa57a));
        let maximum = if six_by_six { 6 } else { configuration_byte(ConfigurationKey(0xa57b)) };
        let rectangle = region.rectangle(six_by_six);
        let phase = if six_by_six {
            [if unsigned_divide(rectangle.width,divisions.wrapping_mul(6)) & 1 == 0 { 3 } else { 0 },
             if unsigned_divide(rectangle.height,divisions.wrapping_mul(6)) & 1 == 0 { 3 } else { 0 }]
        } else { [0,0] };
        let pitch_x = unsigned_divide(rectangle.width,divisions);
        let pitch_y = unsigned_divide(rectangle.height,divisions);
        SamplingGrid { pitch_x, pitch_y, inset_x: if pitch_x > maximum { (pitch_x-maximum) >> 1 } else { 0 }, inset_y: if pitch_y > maximum { (pitch_y-maximum) >> 1 } else { 0 }, width: pitch_x.min(maximum), height: pitch_y.min(maximum), phase }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_sampling_grid(plane: Address32, region: Address32, output: Address32, phase: Address32) {
    unsafe {
        let grid = sampling_grid(ScanPlane(Memory(plane)), Region(Memory(region)));
        for (index,value) in [grid.pitch_x,grid.pitch_y,grid.inset_x,grid.inset_y,grid.width,grid.height].into_iter().enumerate() { Memory(output).at(index as u32*4).put_word(value); }
        Memory(phase).put_byte(grid.phase[0]);
        Memory(phase).at(1).put_byte(grid.phase[1]);
    }
}

fn saturation(bits: u8) -> u16 { match bits { 14 => 0x3fff, 12 => 0xfff, 10 => 0x3ff, _ => 0 } }

#[no_mangle]
pub extern "C" fn xm1_pipeline_image_scan_saturation(bits: u8) -> u16 { saturation(bits) }

unsafe fn sampling_period(plane: ScanPlane) -> u8 {
    unsafe {
        let pattern = plane.pattern().code();
        let mode = plane.0.at(0x2c).word();
        if pattern < 9 && (1u32 << pattern) & 0xf3 != 0 { match mode { 0 => 1, 1 => 2, _ => 4 } }
        else if pattern == 8 { 1 }
        else { match mode { 0 => 1, 1 => 4, _ => 8 } }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_sampling_period(plane: Address32) -> u8 { unsafe { sampling_period(ScanPlane(Memory(plane))) } }

#[derive(Clone, Copy)]
#[repr(usize)]
enum SampleChannel { Red = 0, Green = 1, Blue = 2, GreenFirst = 3, GreenSecond = 4 }
impl SampleChannel {
    fn offset(self) -> u32 { match self { Self::Red => 0x320, Self::Green => 0x720, Self::Blue => 0xb20, Self::GreenFirst => 0xf20, Self::GreenSecond => 0x1320 } }
}
struct SampleCounts(*mut u32);
impl SampleCounts {
    unsafe fn get(&self, channel: SampleChannel) -> u32 { unsafe { read_volatile(self.0.add(channel as usize)) } }
    unsafe fn increment(&mut self, channel: SampleChannel) { unsafe { write_volatile(self.0.add(channel as usize),self.get(channel).wrapping_add(1)); } }
}
impl SampleScratch {
    fn cell(self, channel: SampleChannel, index: SampleIndex) -> Memory { self.0.at(channel.offset().wrapping_add(index.0.wrapping_mul(4))) }
    unsafe fn clear_sample(self, index: SampleIndex) {
        for channel in [SampleChannel::Red,SampleChannel::Green,SampleChannel::Blue,SampleChannel::GreenFirst,SampleChannel::GreenSecond] { unsafe { self.cell(channel,index).put_word(0); } }
    }
}

unsafe fn mosaic_channel(phase: [u8;2]) -> u8 {
    unsafe { Memory(Address32(0xd6ca64)).at((phase[1] as u32).wrapping_mul(6).wrapping_add(phase[0] as u32)).byte() }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_mosaic_channel(phase: Address32) -> u8 {
    unsafe { mosaic_channel([Memory(phase).byte(), Memory(phase).at(1).byte()]) }
}

unsafe fn accumulate_sample(plane: ScanPlane, x: SampleCoordinate, y: SampleCoordinate, index: SampleIndex, limit: u16, valid: *mut u32, counts: &mut SampleCounts, scratch: SampleScratch, phase: *const u8) {
    unsafe {
        write_volatile(valid,1);
        let shading = plane.shading(x) as u32;
        let raw = plane.sample(x,y) as u16;
        if raw >= limit { write_volatile(valid,0); return; }
        let difference = raw as i32 - shading as i32;
        if difference <= 0 { return; }
        let column = x.0.wrapping_sub(plane.0.at(0x18).half() as u32);
        let row = y.0.wrapping_sub(plane.0.at(0x1c).half() as u32);
        let mut extra = None;
        let channel = match plane.pattern() {
            SensorPattern::RedFirst => match column & 3 { 0 => Some(SampleChannel::Red), 2 => Some(SampleChannel::Blue), _ => Some(SampleChannel::Green) },
            SensorPattern::BlueFirst => match column & 3 { 0 => Some(SampleChannel::Blue), 2 => Some(SampleChannel::Red), _ => Some(SampleChannel::Green) },
            SensorPattern::BlueEven => match row & 3 { 0 => Some(if column & 1 == 0 { SampleChannel::Blue } else { SampleChannel::Red }), 1 => Some(SampleChannel::Green), _ => None },
            SensorPattern::GreenEven | SensorPattern::GreenEvenAlternate | SensorPattern::GreenEvenReduced => match (row & 3,column & 1) {
                (0,0) => Some(SampleChannel::Red),
                (0,1) => { extra=Some(SampleChannel::GreenFirst); Some(SampleChannel::Green) },
                (1,0) => { extra=Some(SampleChannel::GreenSecond); Some(SampleChannel::Green) },
                (1,1) => Some(SampleChannel::Blue), _ => None
            },
            SensorPattern::GreenOdd => match (row & 1,column & 1) {
                (0,0) => Some(SampleChannel::Blue),
                (0,1) => { extra=Some(SampleChannel::GreenSecond); Some(SampleChannel::Green) },
                (1,0) => { extra=Some(SampleChannel::GreenFirst); Some(SampleChannel::Green) },
                _ => Some(SampleChannel::Red)
            },
            SensorPattern::SixBySix => Some(match mosaic_channel([read_volatile(phase),read_volatile(phase.add(1))]) { 0 => SampleChannel::Red, 1 | 3 => SampleChannel::Green, _ => SampleChannel::Blue }),
            SensorPattern::RedEven | SensorPattern::Other(_) => match row & 3 { 0 => Some(if column & 1 == 0 { SampleChannel::Red } else { SampleChannel::Blue }), 1 => Some(SampleChannel::Green), _ => None }
        };
        if let Some(channel) = channel {
            let cell=scratch.cell(channel,index);
            cell.put_word(cell.word().wrapping_add(difference as u32));
            if let Some(extra)=extra { let cell=scratch.cell(extra,index); cell.put_word(cell.word().wrapping_add(difference as u32)); }
            counts.increment(channel);
            if let Some(extra)=extra { counts.increment(extra); }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_accumulate_sample(plane: Address32, x: u32, y: u32, index: u32, limit: u16, valid: Address32, counts: Address32, scratch: Address32, phase: Address32) {
    unsafe {
        let mut sample_counts = SampleCounts(counts.0 as *mut u32);
        accumulate_sample(ScanPlane(Memory(plane)),SampleCoordinate(x),SampleCoordinate(y),SampleIndex(index),limit,valid.0 as *mut u32,&mut sample_counts,SampleScratch(Memory(scratch)),phase.0 as *const u8);
    }
}

unsafe fn normalize_sample(levels: Levels, counts: &SampleCounts, index: SampleIndex, valid: u32, limit: u16, scratch: SampleScratch) -> bool {
    unsafe {
        if valid == 0 || counts.get(SampleChannel::Red) == 0 || counts.get(SampleChannel::Green) == 0 || counts.get(SampleChannel::Blue) == 0 { scratch.clear_sample(index); return false; }
        let red = scratch.cell(SampleChannel::Red,index);
        let green = scratch.cell(SampleChannel::Green,index);
        let blue = scratch.cell(SampleChannel::Blue,index);
        for (cell,count) in [(red,counts.get(SampleChannel::Red)),(green,counts.get(SampleChannel::Green)),(blue,counts.get(SampleChannel::Blue))] { cell.put_word(cell.word().wrapping_add(count>>1)/count); }
        for (cell,gain) in [(red,levels.0.at(2).half()),(green,levels.0.half()),(blue,levels.0.at(4).half())] { cell.put_word(cell.word().wrapping_mul(gain as u32).wrapping_add(128)>>8); }
        if red.word() >= limit as u32 || green.word() >= limit as u32 || blue.word() >= limit as u32 { scratch.clear_sample(index); return false; }
        let first_ratio = scratch.0.at(0x1720u32.wrapping_add(index.0.wrapping_mul(4)));
        let second_ratio = scratch.0.at(0x1b20u32.wrapping_add(index.0.wrapping_mul(4)));
        first_ratio.put_word(unsigned_divide(red.word().wrapping_mul(100).wrapping_add(green.word()>>1),green.word()));
        second_ratio.put_word(unsigned_divide(blue.word().wrapping_mul(100).wrapping_add(green.word()>>1),green.word()));
        if first_ratio.word() >= 512 || second_ratio.word() >= 512 { scratch.clear_sample(index); return false; }
        for (offset,ratio) in [(0x1f20,first_ratio.word()),(0x2720,second_ratio.word())] { let cell = scratch.0.at(offset+ratio*4); cell.put_word(cell.word().wrapping_add(1)); }
        true
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_normalize_samples(levels: Address32, red_count: u32, green_count: u32, blue_count: u32, first_green_count: u32, second_green_count: u32, index: u32, valid: u32, limit: u16, scratch: Address32) -> i32 {
    let mut storage = [red_count,green_count,blue_count,first_green_count,second_green_count];
    unsafe { i32::from(normalize_sample(Levels(Memory(levels)),&SampleCounts(storage.as_mut_ptr()),SampleIndex(index),valid,limit,SampleScratch(Memory(scratch)))) }
}

unsafe fn analyze_samples(plane: ScanPlane, region: Region, levels: Levels, workspace: Workspace, scratch: SampleScratch) -> u16 {
    unsafe {
        let six_by_six = matches!(plane.pattern(),SensorPattern::SixBySix);
        let rectangle = region.rectangle(six_by_six);
        let grid = sampling_grid(plane,region);
        for index in 0..256 { scratch.clear_sample(SampleIndex(index)); }
        for index in 0..512 { scratch.0.at(0x1f20+index*4).put_word(0); scratch.0.at(0x2720+index*4).put_word(0); }
        let limit = saturation(plane.0.at(12).byte());
        let period = sampling_period(plane) as u32;
        let divisions = configuration_byte(ConfigurationKey(0xa57a));
        let mut accepted = 0u32;
        for grid_y in 0..divisions {
            for grid_x in 0..divisions {
                let left = grid_x.wrapping_mul(grid.pitch_x).wrapping_add(rectangle.left).wrapping_add(grid.inset_x);
                let top = grid_y.wrapping_mul(grid.pitch_y).wrapping_add(rectangle.top).wrapping_add(grid.inset_y);
                let right = left.wrapping_add(grid.width);
                let bottom = top.wrapping_add(grid.height);
                let mut valid = 1;
                let mut storage = [0u32;5];
                let mut counts = SampleCounts(storage.as_mut_ptr());
                let mut y = top;
                while y < bottom {
                    let selected = if six_by_six { true } else {
                        let remainder = y.wrapping_sub(plane.0.at(0x1c).half() as u32) % period;
                        remainder == 0 || (plane.pattern().code() > 1 && remainder == 1)
                    };
                    if selected {
                        let mut x = left;
                        while x < right {
                            let phase = if six_by_six { [((grid.phase[0] as u32).wrapping_add(x.wrapping_sub(right)).wrapping_add(6)%6) as u8,((grid.phase[1] as u32).wrapping_add(y.wrapping_sub(bottom)).wrapping_add(6)%6) as u8] } else { [0,0] };
                            accumulate_sample(plane,SampleCoordinate(x),SampleCoordinate(y),SampleIndex(accepted),limit,&mut valid,&mut counts,scratch,phase.as_ptr());
                            x = x.wrapping_add(1);
                        }
                        if valid == 0 { break; }
                    }
                    y = y.wrapping_add(1);
                }
                let accepted_cell = normalize_sample(levels,&counts,SampleIndex(accepted),valid,limit,scratch);
                let grid_index = divisions.wrapping_mul(grid_y).wrapping_add(grid_x);
                for (offset,channel) in [(20,SampleChannel::Green),(0x414,SampleChannel::Red),(0x814,SampleChannel::Blue)] { workspace.0.at(offset+grid_index*4).put_word(scratch.cell(channel,SampleIndex(accepted)).word()); }
                if accepted_cell { accepted = accepted.wrapping_add(1); }
            }
        }
        accepted as u16
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_analyze_samples(plane: Address32, region: Address32, levels: Address32, workspace: Address32, scratch: Address32, count: Address32) {
    unsafe { Memory(count).put_half(analyze_samples(ScanPlane(Memory(plane)),Region(Memory(region)),Levels(Memory(levels)),Workspace(Memory(workspace)),SampleScratch(Memory(scratch)))); }
}

#[derive(Clone, Copy)]
enum RatioAxis { Red, Blue }
#[derive(Clone, Copy, Default)]
#[repr(C)]
struct RatioWindow { blue_upper: u16, blue_lower: u16, red_lower: u16, red_upper: u16 }

unsafe fn histogram_window(scratch: SampleScratch, axis: RatioAxis, window: &mut RatioWindow, count: u16, red_source: Option<Memory>) {
    unsafe {
        let radius = configuration_byte(ConfigurationKey(0xa57c)) as u16;
        let threshold = configuration_byte(ConfigurationKey(match axis { RatioAxis::Red => 0xa57d, RatioAxis::Blue => 0xa57e })) as u16;
        let gap_limit = configuration_byte(ConfigurationKey(match axis { RatioAxis::Red => 0xa57f, RatioAxis::Blue => 0xa580 })) as u16;
        let (histogram,prefix,smooth) = match axis { RatioAxis::Red => (0x1f20,0x2f20,0x3720), RatioAxis::Blue => (0x2720,0x3320,0x3b20) };
        if matches!(axis,RatioAxis::Blue) && configuration_byte(ConfigurationKey(0xa581)) != 0 {
            for index in 0..count as u32 {
                let red = scratch.0.at(0x1720+index*4).word();
                let lower=red_source.map_or(window.red_lower,|pointer| pointer.at(4).half()) as u32;
                if red < lower || red > red_source.map_or(window.red_upper,|pointer| pointer.at(6).half()) as u32 {
                    let cell = scratch.0.at(histogram+scratch.0.at(0x1b20+index*4).word().wrapping_mul(4));
                    cell.put_word(cell.word().wrapping_sub(1));
                }
            }
        }
        scratch.0.at(prefix).put_half(scratch.0.at(histogram).word() as u16);
        for index in 1..512 { scratch.0.at(prefix+index*2).put_half((scratch.0.at(histogram+index*4).word() as u16).wrapping_add(scratch.0.at(prefix+(index-1)*2).half())); }
        for index in 0..512u32 {
            let start = if (index.wrapping_sub(radius as u32) as i32) <= 0 { 1 } else { index-radius as u32 };
            let end = (index+radius as u32).min(511);
            let difference = scratch.0.at(prefix+end*2).half().wrapping_sub(scratch.0.at(prefix+(start-1)*2).half()) as i16;
            let numerator = if difference == 0 { 1 } else { difference as i32 };
            let length = end.wrapping_sub(start).wrapping_add(1) as i32;
            scratch.0.at(smooth+index*2).put_half(signed_round_divide(numerator,if length == 0 { 1 } else { length }) as u16);
        }
        let mut peak = 0i32;
        let mut maximum = 0u16;
        for index in 0..512 { let value = scratch.0.at(smooth+index*2).half(); if maximum < value { maximum = value; peak = index as i32; } }
        let mut index = peak;
        while index >= 0 && threshold <= scratch.0.at(smooth+(index as u32)*2).half() { index -= 1; }
        let mut lower = (index+1) as u16;
        let mut gap = 0u16;
        index = lower as i32-1;
        while index >= 0 {
            gap = gap.wrapping_add(1);
            if gap > gap_limit { break; }
            if threshold <= scratch.0.at(smooth+index as u32*2).half() { lower = index as u16; gap = 0; }
            index -= 1;
        }
        index = peak;
        while index < 512 && threshold <= scratch.0.at(smooth+index as u32*2).half() { index += 1; }
        let mut upper = (index-1) as u16;
        gap = 0;
        index = upper as i32+1;
        while index < 512 {
            gap = gap.wrapping_add(1);
            if gap > gap_limit { break; }
            if threshold <= scratch.0.at(smooth+index as u32*2).half() { upper = index as u16; gap = 0; }
            index += 1;
        }
        match axis { RatioAxis::Red => { window.red_lower=lower; window.red_upper=upper; }, RatioAxis::Blue => { window.blue_lower=lower; window.blue_upper=upper; } }
    }
}


#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_red_ratio_window(scratch: Address32, window: Address32) {
    unsafe {
        let mut result = RatioWindow::default();
        histogram_window(SampleScratch(Memory(scratch)),RatioAxis::Red,&mut result,0,None);
        Memory(window).at(4).put_half(result.red_lower);
        Memory(window).at(6).put_half(result.red_upper);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_blue_ratio_window(scratch: Address32, window: Address32, count: u16) {
    unsafe {
        let mut result = RatioWindow::default();
        histogram_window(SampleScratch(Memory(scratch)),RatioAxis::Blue,&mut result,count,Some(Memory(window)));
        Memory(window).put_half(result.blue_upper);
        Memory(window).at(2).put_half(result.blue_lower);
    }
}

unsafe fn select_histogram_samples(plane: ScanPlane, curve: ToneCurve, scratch: SampleScratch, window: RatioWindow, count: u16) -> u16 {
    unsafe {
        let adjustment = plane.0.at(12).byte().wrapping_sub(12) as i8;
        for index in 0..256 { scratch.0.at(0x4520+index*2).put_half(0); }
        let mut selected = 0u16;
        for index in 0..count as u32 {
            let red = scratch.0.at(0x1720+index*4).word();
            let blue = scratch.0.at(0x1b20+index*4).word();
            if red >= window.red_lower as u32 && red <= window.red_upper as u32 && blue >= window.blue_lower as u32 && blue <= window.blue_upper as u32 {
                let green = scratch.0.at(0x720+index*4).word();
                let normalized = if adjustment < 0 { left_shift(green,adjustment.wrapping_neg() as u8 as u32) } else { right_shift(green,adjustment as u32) };
                let brightness = curve.0.at(normalized.min(4095)).byte() as u16;
                let bin = scratch.0.at(0x4520+brightness as u32*2);
                bin.put_half(bin.half().wrapping_add(1));
                scratch.0.at(0x3f20+selected as u32*2).put_half(red as u16);
                scratch.0.at(0x4120+selected as u32*2).put_half(blue as u16);
                scratch.0.at(0x4320+selected as u32*2).put_half(brightness);
                selected = selected.wrapping_add(1);
            }
        }
        selected
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_select_histogram_samples(plane: Address32, curve: Address32, scratch: Address32, blue_window: u32, red_window: u32, count: u16, selected: Address32) {
    unsafe { Memory(selected).put_half(select_histogram_samples(ScanPlane(Memory(plane)),ToneCurve(Memory(curve)),SampleScratch(Memory(scratch)),RatioWindow { blue_upper: blue_window as u16,blue_lower: (blue_window>>16) as u16,red_lower: red_window as u16,red_upper: (red_window>>16) as u16 },count)); }
}

#[derive(Clone, Copy, Default)]
struct SampleRatios { red: u32, blue: u32 }

unsafe fn weighted_sample_ratios(workspace: Workspace, scratch: SampleScratch, count: u16) -> Option<SampleRatios> {
    unsafe {
        let minimum_weight = configuration_byte(ConfigurationKey(0xa586)) as u16;
        let upper_percent = configuration_byte(ConfigurationKey(0xa582));
        let upper_count = count.wrapping_sub((upper_percent.wrapping_mul(count as u32).wrapping_add(50) as i32/100) as u16);
        let mut total = 0u32;
        let mut brightness = 0u16;
        for index in (0..256).rev() {
            total = total.wrapping_add(scratch.0.at(0x4520+index*2).half() as u32);
            if (upper_count as u32) < total { brightness = index as u16+1; break; }
        }
        let minimum = configuration_byte(ConfigurationKey(0xa583));
        let mut span = 0u16;
        let result = if (brightness as u32) < minimum {
            workspace.0.at(0xc14).put_half(0); workspace.0.at(0xc16).put_half(0);
            None
        } else {
            let low_count = ((configuration_byte(ConfigurationKey(0xa584)).wrapping_mul(count as u32).wrapping_add(50) as i32)/100) as u16;
            let mut low = 0u16;
            total = 0;
            for index in 0..256 {
                total = total.wrapping_add(scratch.0.at(0x4520+index*2).half() as u32);
                if total > low_count as u32 { low=index as u16; break; }
            }
            let high_count = ((configuration_byte(ConfigurationKey(0xa585)).wrapping_mul(count as u32).wrapping_add(50) as i32)/100) as u16;
            let mut high = 0u16;
            total = 0;
            for index in (0..256).rev() {
                total = total.wrapping_add(scratch.0.at(0x4520+index*2).half() as u32);
                if total > high_count as u32 { high=index as u16; break; }
            }
            span=high.wrapping_sub(low).wrapping_add(minimum_weight);
            let mut red_sum = 0u32;
            let mut blue_sum = 0u32;
            let mut weight_sum = 0i32;
            for index in 0..count as u32 {
                let weight = if low < high {
                    let cell=scratch.0.at(0x4320+index*2);
                    let value=cell.half();
                    if value < low { cell.put_half(low); } else if value > high { cell.put_half(high); }
                    let numerator=(cell.half() as u32).wrapping_sub(low as u32).wrapping_mul(span as u32-minimum_weight as u32).wrapping_add((high as i32-low as i32) as u32/2);
                    minimum_weight as i32+signed_round_divide(numerator as i32,high as i32-low as i32)
                } else { 1 };
                red_sum=red_sum.wrapping_add((weight as u32).wrapping_mul(scratch.0.at(0x3f20+index*2).half() as u32));
                blue_sum=blue_sum.wrapping_add((weight as u32).wrapping_mul(scratch.0.at(0x4120+index*2).half() as u32));
                weight_sum=weight_sum.wrapping_add(weight);
            }
            let ratios=if weight_sum == 0 { SampleRatios::default() } else { SampleRatios { red: red_sum.wrapping_add((weight_sum/2) as u32)/weight_sum as u32,blue: blue_sum.wrapping_add((weight_sum/2) as u32)/weight_sum as u32 } };
            workspace.0.at(0xc1c).put_word(weight_sum as u32);
            workspace.0.at(0xc20).put_half(high); workspace.0.at(0xc22).put_half(low);
            workspace.0.at(0xc18).put_half(ratios.red as u16); workspace.0.at(0xc1a).put_half(ratios.blue as u16);
            Some(ratios)
        };
        workspace.0.at(0xc5a).put_half(count); workspace.0.at(0xc5c).put_half(brightness); workspace.0.at(0xc5e).put_half(span);
        result
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_weighted_ratios(workspace: Address32, scratch: Address32, output: Address32, count: u16) {
    unsafe { if let Some(result)=weighted_sample_ratios(Workspace(Memory(workspace)),SampleScratch(Memory(scratch)),count) { Memory(output).put_word(result.red); Memory(output).at(4).put_word(result.blue); } }
}

unsafe fn constrain_sample_ratios(workspace: Workspace, ratios: &mut SampleRatios) {
    unsafe {
        let breakpoints: [u16;6] = core::array::from_fn(|i| configuration_half(ConfigurationKey(0xa587+i as u32*4)));
        let uppers: [i16;6] = core::array::from_fn(|i| configuration_byte(ConfigurationKey(0xa589+i as u32*4)) as i16);
        let lowers: [i16;6] = core::array::from_fn(|i| configuration_byte(ConfigurationKey(0xa58a+i as u32*4)) as i16);
        let (mut upper,mut lower)=(0u16,0u16);
        if ratios.red < breakpoints[0] as u32 || (breakpoints[5] as u32) < ratios.red { lower=512; }
        else {
            for index in 1..6 {
                if ratios.red <= breakpoints[index] as u32 {
                    let position=(ratios.red as u16).wrapping_sub(breakpoints[index-1]) as i16 as i32;
                    let width=breakpoints[index].wrapping_sub(breakpoints[index-1]) as i16 as i32;
                    upper=(uppers[index-1] as i32+signed_round_divide(position.wrapping_mul(uppers[index].wrapping_sub(uppers[index-1]) as i32),width)) as u16;
                    lower=(lowers[index-1] as i32+signed_round_divide(position.wrapping_mul(lowers[index].wrapping_sub(lowers[index-1]) as i32),width)) as u16;
                    break;
                }
            }
        }
        workspace.0.at(0xc24).put_half(upper); workspace.0.at(0xc26).put_half(lower);
        let accepted=upper as u32 >= ratios.blue && ratios.blue >= lower as u32;
        if !accepted { *ratios=SampleRatios::default(); }
        workspace.0.at(0xc60).put_word(u32::from(accepted));
        workspace.0.at(0xc14).put_half(ratios.red as u16); workspace.0.at(0xc16).put_half(ratios.blue as u16);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_constrain_ratios(workspace: Address32, _mode: u32, ratios: Address32) {
    unsafe {
        let mut result=SampleRatios { red: Memory(ratios).word(),blue: Memory(ratios).at(4).word() };
        constrain_sample_ratios(Workspace(Memory(workspace)),&mut result);
        Memory(ratios).put_word(result.red); Memory(ratios).at(4).put_word(result.blue);
    }
}

unsafe fn finish_measurement(plane: ScanPlane, curve: ToneCurve, workspace: Workspace, scratch: SampleScratch, count: u16) {
    unsafe {
        let mut window=RatioWindow::default();
        histogram_window(scratch,RatioAxis::Red,&mut window,count,None);
        histogram_window(scratch,RatioAxis::Blue,&mut window,count,None);
        let selected=select_histogram_samples(plane,curve,scratch,window,count);
        let mut ratios=weighted_sample_ratios(workspace,scratch,selected).unwrap_or_default();
        constrain_sample_ratios(workspace,&mut ratios);
        for (offset,value) in [(0xc50,count),(0xc52,window.red_lower),(0xc54,window.red_upper),(0xc56,window.blue_upper),(0xc58,window.blue_lower)] { workspace.0.at(offset).put_half(value); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_finish_measurement(plane: Address32, curve: Address32, _mode: u32, workspace: Address32, scratch: Address32, count: u16) {
    unsafe { finish_measurement(ScanPlane(Memory(plane)),ToneCurve(Memory(curve)),Workspace(Memory(workspace)),SampleScratch(Memory(scratch)),count); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_measure_region(plane: Address32, region: Address32, curve: Address32, levels: Address32, _mode: u32, workspace: Address32, scratch: Address32) -> i32 {
    unsafe {
        let count=analyze_samples(ScanPlane(Memory(plane)),Region(Memory(region)),Levels(Memory(levels)),Workspace(Memory(workspace)),SampleScratch(Memory(scratch)));
        if count == 0 { 0 } else { finish_measurement(ScanPlane(Memory(plane)),ToneCurve(Memory(curve)),Workspace(Memory(workspace)),SampleScratch(Memory(scratch)),count); 1 }
    }
}

#[derive(Clone, Copy)]
struct CurvePoint { red: u16, blue: u16 }

fn point_distance(point: CurvePoint, index: i32, value: u8) -> i32 {
    let dx=(point.red as i32).wrapping_sub(index);
    let dy=(point.blue as i32).wrapping_sub(value as i32);
    dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy))
}

unsafe fn walk_curve(point: CurvePoint, curve: Memory, start: u16, direction: i32, mut distance: i32) -> CurvePoint {
    unsafe {
        let mut selected=start;
        let mut previous=curve.at(start as i16 as i32 as u32).byte();
        let mut step=1i16;
        loop {
            let index=(start as i16 as i32).wrapping_add((step as i32).wrapping_mul(direction));
            let value=curve.at(index as u32).byte();
            let candidate=point_distance(point,index,value);
            if previous != value {
                if distance < candidate { return CurvePoint { red:selected,blue:curve.at(selected as i16 as i32 as u32).byte() as u16 }; }
                selected=index as u16; distance=candidate;
            }
            step=step.wrapping_add(1);
            previous=value;
        }
    }
}

unsafe fn project_curve(point: CurvePoint, curve: Memory) -> (bool,CurvePoint) {
    unsafe {
        let mut plateau=0i16;
        for index in 0..512i16 { if curve.at(index as u32).byte() <= curve.at(511).byte() { plateau=index; break; } }
        let mut crossing=512u16;
        for index in 0..512u16 { if curve.at(index as u32).byte() as u16 <= point.blue { crossing=index; break; } }
        let at_point=curve.at(point.red as i16 as i32 as u32).byte();
        let crossing_value=curve.at(crossing as i16 as i32 as u32).byte();
        let crossing_distance=point_distance(point,crossing as i16 as i32,crossing_value);
        let own_distance=point_distance(point,point.red as i16 as i32,at_point);
        let middle=if (point.red as i16) < (crossing as i16) {
            point.red.wrapping_add((((crossing as i16 as i32-point.red as i16 as i32)+1)/2) as u16)
        } else { crossing.wrapping_add((((point.red as i16 as i32-crossing as i16 as i32)+1)/2) as u16) };
        let middle_value=curve.at(middle as i16 as i32 as u32).byte();
        let middle_distance=point_distance(point,middle as i16 as i32,middle_value);
        let mut result=if point.red as i32 >= plateau as i32 { CurvePoint { red:point.red,blue:curve.at(point.red as u32).byte() as u16 } }
        else if crossing == 512 { walk_curve(point,curve,point.red,1,own_distance) }
        else if crossing_distance < middle_distance || own_distance < middle_distance {
            if crossing_distance < own_distance { walk_curve(point,curve,crossing,if (point.red as i16) <= (crossing as i16) { -1 } else { 1 },crossing_distance) }
            else if own_distance < crossing_distance { walk_curve(point,curve,point.red,if (point.red as i16) <= (crossing as i16) { 1 } else { -1 },own_distance) }
            else { CurvePoint { red:middle,blue:middle_value as u16 } }
        } else if middle_distance == crossing_distance || middle_distance == own_distance || crossing_distance == own_distance { CurvePoint { red:middle,blue:middle_value as u16 } }
        else {
            let left=(crossing_distance < own_distance && (crossing as i16) < (point.red as i16)) || (own_distance < crossing_distance && (point.red as i16) < (crossing as i16));
            walk_curve(point,curve,middle,if left { -1 } else { 1 },middle_distance)
        };
        if result.red == 0 || result.red > 511 || result.blue == 0 || result.blue > 511 { result=point; }
        ((at_point as u16) < point.blue,result)
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_project_curve(red: u16, blue: u16, output_red: Address32, output_blue: Address32, curve: Address32) -> i32 {
    unsafe {
        let (above,point)=project_curve(CurvePoint { red,blue },Memory(curve));
        Memory(output_red).put_half(point.red); Memory(output_blue).put_half(point.blue);
        i32::from(above)
    }
}

unsafe fn interpolate_curve(value: u16, points: Memory, responses: Memory, count: i16) -> u16 {
    unsafe {
        if value <= points.half() { return responses.byte() as u16; }
        let last=count.wrapping_sub(1) as i32 as u32;
        if value >= points.at(last.wrapping_mul(2)).half() { return responses.at(last).byte() as u16; }
        let mut index=1i16;
        while index < count {
            let upper=points.at(index as u32*2).half();
            if value <= upper {
                let lower=points.at((index as u32-1)*2).half();
                let first=responses.at(index as u32-1).byte();
                let second=responses.at(index as u32).byte();
                return (first as i32).wrapping_add(signed_round_divide((value.wrapping_sub(lower) as i16 as i32).wrapping_mul((second as u16).wrapping_sub(first as u16) as i16 as i32),upper.wrapping_sub(lower) as i16 as i32)) as u16;
            }
            index=index.wrapping_add(1);
        }
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_interpolate(value: u16, points: Address32, responses: Address32, count: i16) -> u16 {
    unsafe { interpolate_curve(value,Memory(points),Memory(responses),count) }
}

fn numeric_sqrt(value: i32) -> i32 {
    if value == 0 { return 0; }
    let mut estimate=1i32;
    let mut iterations=0i16;
    while estimate != 0 {
        estimate=value.wrapping_div(estimate).wrapping_add(estimate)/2;
        iterations=iterations.wrapping_add(1);
        let square=estimate.wrapping_mul(estimate);
        let next=estimate.wrapping_add(1).wrapping_mul(estimate.wrapping_add(1));
        if (square == value || square < value) && next != value && value <= next { return estimate; }
        if iterations > 50 { return estimate; }
    }
    -1
}

#[no_mangle]
pub extern "C" fn xm1_pipeline_image_scan_integer_sqrt(value: i32) -> i32 { numeric_sqrt(value) }

#[derive(Clone, Copy)]
struct CalibrationProfile(Memory);
#[derive(Clone, Copy)]
enum CalibrationKind { Camera, Reference, ReducedReference, Other }
impl CalibrationKind {
    fn parse(code: u32) -> Self { match code { 0=>Self::Camera,1=>Self::Reference,2=>Self::ReducedReference,_=>Self::Other } }
    fn is_reference(self) -> bool { matches!(self,Self::Reference|Self::ReducedReference) }
}
impl CalibrationProfile { unsafe fn kind(self) -> CalibrationKind { CalibrationKind::parse(unsafe { self.0.word() }) } }

unsafe fn configuration_memory(key: ConfigurationKey) -> Memory { Memory(unsafe { configuration_pointer(key) }) }
unsafe fn configuration_curve<const N: usize>(points: ConfigurationKey, responses: ConfigurationKey) -> ([u16;N],[u8;N]) {
    unsafe {
        let points=configuration_memory(points);
        let responses=configuration_memory(responses);
        (core::array::from_fn(|i| points.at(i as u32*2).byte() as u16 | ((points.at(i as u32*2+1).byte() as u16)<<8)),core::array::from_fn(|i| responses.at(i as u32).byte()))
    }
}
unsafe fn curve_from_arrays<const N: usize, const M: usize>(value: u16, points: &[u16;N], responses: &[u8;M]) -> u16 {
    unsafe { interpolate_curve(value,Memory(Address32(points.as_ptr() as u32)),Memory(Address32(responses.as_ptr() as u32)),N as i16) }
}

unsafe fn reference_tables(kind: CalibrationKind) -> (Memory,Memory) {
    unsafe { let (first,second)=if kind.is_reference() { (0x1b000,0x19000) } else if matches!(kind,CalibrationKind::Camera) { (0x1b800,0x1a000) } else { (0x1b400,0x1a000) }; (configuration_memory(ConfigurationKey(first)),configuration_memory(ConfigurationKey(second))) }
}
unsafe fn component_tables(kind: CalibrationKind) -> [Memory;3] {
    unsafe { let keys=if kind.is_reference() { [0x19400,0x19800,0x19c00] } else { [0x1a400,0x1a800,0x1ac00] }; keys.map(|key| configuration_memory(ConfigurationKey(key))) }
}
unsafe fn anchor_pair(kind: CalibrationKind, group: u32) -> CurvePoint {
    unsafe {
        let key=if kind.is_reference() { 0xa64c+group*2 } else { 0xa644+group*2 };
        CurvePoint { red:configuration_byte(ConfigurationKey(key)) as u16,blue:configuration_byte(ConfigurationKey(key+1)) as u16 }
    }
}
unsafe fn blue_attenuation(kind: CalibrationKind) -> ([u16;7],[u8;7]) {
    unsafe { let (x,y)=if kind.is_reference() { (0xa66f,0xa67d) } else { (0xa5a6,0xa5b4) }; configuration_curve(ConfigurationKey(x),ConfigurationKey(y)) }
}
unsafe fn correction_points(kind: CalibrationKind) -> [u16;9] {
    unsafe { let points=configuration_memory(ConfigurationKey(if kind.is_reference() { 0xa685 } else { 0xa5bb })); core::array::from_fn(|i| points.at(i as u32*2).byte() as u16 | ((points.at(i as u32*2+1).byte() as u16)<<8)) }
}
unsafe fn primary_responses(kind: CalibrationKind, alternate: bool) -> ([u8;9],[u8;3]) {
    unsafe {
        let (curve,values)=if kind.is_reference() { (0xa697,0xa6a0) } else if alternate { (0xa500,0xa509) } else { (0xa5cd,0xa5d6) };
        let curve=configuration_memory(ConfigurationKey(curve));
        let controls=core::array::from_fn(|i| configuration_byte(ConfigurationKey(values+i as u32)) as u8);
        (core::array::from_fn(|i| curve.at(i as u32).byte()),controls)
    }
}
unsafe fn primary_component_responses(kind: CalibrationKind, alternate: bool) -> ([u8;11],[u8;11]) {
    unsafe {
        let (red,blue)=if kind.is_reference() { (0xa6a3,0xa6a4) } else if alternate { (0xa50d,0xa50e) } else { (0xa5d9,0xa5da) };
        let red=configuration_memory(ConfigurationKey(red));
        let blue=configuration_memory(ConfigurationKey(blue));
        (core::array::from_fn(|i| red.at(i as u32*2).byte()),core::array::from_fn(|i| blue.at(i as u32*2).byte()))
    }
}
unsafe fn secondary_responses(kind: CalibrationKind, alternate: bool, selection: u32) -> ([u8;9],[u8;2]) {
    unsafe {
        let (curve,values)=if kind.is_reference() { (0xa6ba,0xa6c3) } else if alternate { (0xa524,0xa52d) } else {
            match selection { 2=>(0xa6c5,0xa6ce),3=>(0xa6d0,0xa6d9),4=>(0xa6db,0xa6e4),_=>(0xa5ef,0xa5f8) }
        };
        let curve=configuration_memory(ConfigurationKey(curve));
        let controls=[configuration_byte(ConfigurationKey(values)) as u8,configuration_byte(ConfigurationKey(values+1)) as u8];
        (core::array::from_fn(|i| curve.at(i as u32).byte()),controls)
    }
}
unsafe fn secondary_component_curve(kind: CalibrationKind, alternate: bool, blue: bool) -> ([u16;7],[u8;7]) {
    unsafe {
        let (x,y)=match (kind.is_reference(),alternate,blue) {
            (true,_,false)=>(0xa6e7,0xa6f5),(true,_,true)=>(0xa6fc,0xa70a),
            (false,true,false)=>(0xa52f,0xa53d),(false,true,true)=>(0xa544,0xa552),
            (false,false,false)=>(0xa5fa,0xa608),(false,false,true)=>(0xa60f,0xa61d)
        };
        configuration_curve(ConfigurationKey(x),ConfigurationKey(y))
    }
}

#[derive(Clone, Copy)]
struct OutputSelection(u32);
unsafe fn correction_limits(selection: OutputSelection) -> [u8;2] {
    let key=match selection.0 {
        2=>0xa71e,3=>0xa720,4=>0xa722,5=>0xa72e,7=>0xa730,8=>0xa732,9=>0xa736,10=>0xa734,
        11|12=>0xa72c,13=>0xa73e,14=>0xa73c,15=>0xa738,16=>0xa73a,17=>0xa740,18=>0xa744,19=>0xa742,
        20=>0xa746,21=>0xa55a,22=>0xa72a,23=>0xa728,24=>0xa748,25=>0xa74a,26=>0xa726,27..=31=>0xa724,40=>0xa74c,_=>0xa625
    };
    unsafe { [configuration_byte(ConfigurationKey(key)) as u8,configuration_byte(ConfigurationKey(key+1)) as u8] }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_reference_tables(kind: u32, first: Address32, second: Address32) {
    unsafe { let (a,b)=reference_tables(CalibrationKind::parse(kind)); Memory(first).put_word(a.0.0); Memory(second).put_word(b.0.0); }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_component_tables(kind: u32, first: Address32, second: Address32, third: Address32) {
    unsafe { let tables=component_tables(CalibrationKind::parse(kind)); for (target,table) in [first,second,third].into_iter().zip(tables) { Memory(target).put_word(table.0.0); } }
}
unsafe fn write_anchor(kind: u32, group: u32, red: Address32, blue: Address32) {
    unsafe { let point=anchor_pair(CalibrationKind::parse(kind),group); Memory(red).put_half(point.red); Memory(blue).put_half(point.blue); }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_first_anchor(kind: u32, red: Address32, blue: Address32) { unsafe { write_anchor(kind,0,red,blue); } }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_second_anchor(kind: u32, red: Address32, blue: Address32) { unsafe { write_anchor(kind,2,red,blue); } }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_target_anchor(kind: u32, red: Address32, blue: Address32) { unsafe { write_anchor(kind,1,red,blue); } }

unsafe fn write_curve<const N: usize>(points: Address32, responses: Address32, values: ([u16;N],[u8;N])) {
    unsafe { for index in 0..N { Memory(points).at(index as u32*2).put_half(values.0[index]); Memory(responses).at(index as u32).put_byte(values.1[index]); } }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_blue_attenuation_curve(kind: u32, points: Address32, responses: Address32) { unsafe { write_curve(points,responses,blue_attenuation(CalibrationKind::parse(kind))); } }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_correction_points(kind: u32, output: Address32) { unsafe { for (index,value) in correction_points(CalibrationKind::parse(kind)).into_iter().enumerate() { Memory(output).at(index as u32*2).put_half(value); } } }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_primary_responses(kind: u32, alternate: u32, output: Address32, first: Address32, second: Address32, third: Address32) {
    unsafe { let (curve,controls)=primary_responses(CalibrationKind::parse(kind),alternate==1); for (pointer,value) in [first,second,third].into_iter().zip(controls) { Memory(pointer).put_byte(value); } for (index,value) in curve.into_iter().enumerate() { Memory(output).at(index as u32).put_byte(value); } }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_primary_component_responses(kind: u32, alternate: u32, red: Address32, blue: Address32) {
    unsafe { let (first,second)=primary_component_responses(CalibrationKind::parse(kind),alternate==1); for index in 0..11 { Memory(red).at(index as u32).put_byte(first[index]); Memory(blue).at(index as u32).put_byte(second[index]); } }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_secondary_responses(kind: u32, alternate: u32, selection: u32, output: Address32, first: Address32, second: Address32) {
    unsafe { let (curve,controls)=secondary_responses(CalibrationKind::parse(kind),alternate==1,selection); for (index,value) in curve.into_iter().enumerate() { Memory(output).at(index as u32).put_byte(value); } Memory(first).put_byte(controls[0]); Memory(second).put_byte(controls[1]); }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_secondary_red_curve(kind: u32, alternate: u32, points: Address32, responses: Address32) { unsafe { write_curve(points,responses,secondary_component_curve(CalibrationKind::parse(kind),alternate==1,false)); } }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_secondary_blue_curve(kind: u32, alternate: u32, points: Address32, responses: Address32) { unsafe { write_curve(points,responses,secondary_component_curve(CalibrationKind::parse(kind),alternate==1,true)); } }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_correction_limits(selection: u32, lower: Address32, upper: Address32) -> i32 {
    unsafe { let values=correction_limits(OutputSelection(selection)); Memory(lower).put_byte(values[0]); Memory(upper).put_byte(values[1]); values[1] as i32 }
}

unsafe fn scale_region(plane: ScanPlane, dimensions: Memory, region: Region, target: Memory) -> Rectangle {
    unsafe {
        let width=dimensions.word().wrapping_sub(plane.0.at(0x18).half() as u32+plane.0.at(0x1a).half() as u32);
        let height=dimensions.at(4).word().wrapping_sub(plane.0.at(0x1c).half() as u32+plane.0.at(0x1e).half() as u32);
        let rectangle=region.rectangle(false);
        let target_width=target.word();
        let target_height=target.at(4).word();
        Rectangle {
            left:unsigned_divide(target_width.wrapping_mul(rectangle.left.wrapping_sub(plane.0.at(0x18).half() as u32)).wrapping_add(width>>1),width),
            top:unsigned_divide(target_height.wrapping_mul(rectangle.top.wrapping_sub(plane.0.at(0x1c).half() as u32)).wrapping_add(height>>1),height),
            width:unsigned_divide(target_width.wrapping_mul(rectangle.width).wrapping_add(width>>1),width),
            height:unsigned_divide(target_height.wrapping_mul(rectangle.height).wrapping_add(height>>1),height)
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_scale_region(plane: Address32, dimensions: Address32, region: Address32, target: Address32, output: Address32) {
    unsafe {
        let rectangle=scale_region(ScanPlane(Memory(plane)),Memory(dimensions),Region(Memory(region)),Memory(target));
        for (index,value) in [rectangle.left,rectangle.top,rectangle.width,rectangle.height].into_iter().enumerate() { Memory(output).at(index as u32*4).put_word(value); }
        Memory(output).at(0x22).put_half(Memory(region).at(0x22).half());
        Memory(output).at(0x20).put_half(Memory(region).at(0x20).half());
        Memory(output).at(0x24).put_word(Memory(region).at(0x24).word());
    }
}

#[derive(Clone, Copy)]
struct ExposureHundredths(i16);
unsafe fn exposure_gain(exposure: ExposureHundredths) -> u32 {
    let whole=exposure.0/100;
    let fraction=exposure.0%100+100;
    let value=unsafe { Memory(Address32(0xd6c99c)).at(fraction as i32 as u32).byte() as u32 };
    if whole < 0 { value.wrapping_mul(right_shift(65536,whole.wrapping_neg() as u16 as u32 & 255))>>16 }
    else { value.wrapping_mul(left_shift(1,whole as u16 as u32 & 255)) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_exposure_gain(exposure: i16) -> u32 { unsafe { exposure_gain(ExposureHundredths(exposure)) } }

#[derive(Clone, Copy)]
struct BlockEdge { index: i16, excluded: i32 }
fn block_edge(value: u32, origin: u32, pitch: u32, count: i16, end: bool) -> BlockEdge {
    let mut result=BlockEdge { index:0,excluded:0 };
    let mut index=1i16;
    while index <= count {
        let boundary=pitch.wrapping_mul(index as i32 as u32).wrapping_add(origin);
        if value < boundary {
            let difference=if end { boundary.wrapping_sub(value).wrapping_sub(1) } else { value.wrapping_sub(pitch.wrapping_mul(index.wrapping_sub(1) as i32 as u32).wrapping_add(origin)) };
            return BlockEdge { index:index.wrapping_sub(1),excluded:(difference as i32).max(0) };
        }
        result=BlockEdge { index:count.wrapping_sub(1),excluded:0 };
        index=index.wrapping_add(1);
    }
    result
}
fn included_blocks(first: BlockEdge, last: BlockEdge) -> (i16,i16) {
    let lower=if first.excluded > 0 { first.index.wrapping_add(1) } else { first.index };
    let upper=if last.excluded > 0 { last.index.wrapping_sub(1) } else { last.index };
    if upper >= lower { (lower,upper) }
    else { let selected=if last.excluded < first.excluded { last.index } else { first.index }; (selected,selected) }
}

unsafe fn spatial_correction(point: CurvePoint, region: Region, plane: ScanPlane, profile: CalibrationProfile, workspace: Workspace, curve: Memory) -> CurvePoint {
    unsafe {
        let rectangle=scale_region(plane,plane.0.at(4),region,profile.0.at(0x10));
        let grid=profile.0.at(0x28).half() as i16;
        let pitch_x=unsigned_divide(profile.0.at(0x18).word().wrapping_add((grid/2) as u32),grid as i32 as u32);
        let pitch_y=unsigned_divide(profile.0.at(0x1c).word().wrapping_add((grid/2) as u32),grid as i32 as u32);
        let origin_x=profile.0.at(0x20).word();
        let origin_y=profile.0.at(0x24).word();
        let operation=profile.0.at(0x30).word();
        let selection=profile.0.at(0x2c).word();
        let (limit,scale,offset,adjustment_key)=match operation {
            1=>(profile.0.at(14).half(),configuration_half(ConfigurationKey(0x8662)),configuration_half(ConfigurationKey(0x8664)) as i16,0x866e),
            2=>(profile.0.at(14).half(),configuration_half(ConfigurationKey(0x8668)),configuration_half(ConfigurationKey(0x866a)) as i16,0x8671),
            _=>(configuration_half(ConfigurationKey(0xa5a1)),configuration_half(ConfigurationKey(0xa5a3)),configuration_half(ConfigurationKey(0xa59f)) as i16,0xa750)
        };
        let adjustment=configuration_signed_byte(ConfigurationKey(adjustment_key+selection.min(2))) as i8;
        let left=block_edge(rectangle.left,origin_x,pitch_x,grid,false);
        let top=block_edge(rectangle.top,origin_y,pitch_y,grid,false);
        let right=block_edge(rectangle.left.wrapping_add(rectangle.width),origin_x,pitch_x,grid,true);
        let bottom=block_edge(rectangle.top.wrapping_add(rectangle.height),origin_y,pitch_y,grid,true);
        let (first_x,last_x)=included_blocks(left,right);
        let (first_y,last_y)=included_blocks(top,bottom);
        let red_grid=profile.0.at(4).pointer();
        let blue_grid=profile.0.at(8).pointer();
        let mut red_sum=0i64;
        let mut blue_sum=0i64;
        let mut count=0i16;
        let mut y=first_y;
        while y <= last_y {
            let mut x=first_x;
            while x <= last_x {
                let index=(y as i32).wrapping_mul(16).wrapping_add(x as i32) as u32;
                red_sum=red_sum.wrapping_add(red_grid.at(index.wrapping_mul(4)).word() as i32 as i64);
                blue_sum=blue_sum.wrapping_add(blue_grid.at(index.wrapping_mul(4)).word() as i32 as i64);
                count=count.wrapping_add(1);
                x=x.wrapping_add(1);
            }
            y=y.wrapping_add(1);
        }
        let (red,blue)=if count == 0 { (0i32,0i32) } else { ((red_sum/(count as i64)) as i32,(blue_sum/(count as i64)) as i32) };
        let gain=if red < blue {
            let mut exposure=(profile.0.at(12).half() as i16).wrapping_add(offset).wrapping_sub((adjustment as i16).wrapping_mul(10));
            if exposure as i32 > limit as i32 { exposure=limit as i16; }
            exposure_gain(ExposureHundredths(exposure))
        } else { 0 };
        let difference=(blue as i64).wrapping_sub(red as i64).wrapping_mul(gain as i64).wrapping_add(50)/100;
        let combined=difference.wrapping_add(red as i64);
        let result=if combined <= 0 { point } else {
            let percent=(difference.wrapping_mul(100).wrapping_add(combined/2)/combined) as u16;
            let scale=(1000i32).wrapping_add(((percent as i32).wrapping_mul(scale as i32-1000).wrapping_add(50))/100) as u16;
            let red=((scale as u32).wrapping_mul(point.red as u32).wrapping_add(500) as i32/1000) as u16;
            CurvePoint { red,blue:curve.at(red as u32).byte() as u16 }
        };
        workspace.0.at(0xc6c).put_word(red as u32);
        workspace.0.at(0xc70).put_word(blue as u32);
        workspace.0.at(0xc78).put_word(difference as u32);
        workspace.0.at(0xc7c).put_word((difference as u64>>32) as u32);
        result
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_spatial_correction(red: u16, blue: u16, output_red: Address32, output_blue: Address32, region: Address32, plane: Address32, profile: Address32, workspace: Address32, curve: Address32) {
    unsafe { let point=spatial_correction(CurvePoint { red,blue },Region(Memory(region)),ScanPlane(Memory(plane)),CalibrationProfile(Memory(profile)),Workspace(Memory(workspace)),Memory(curve)); Memory(output_red).put_half(point.red); Memory(output_blue).put_half(point.blue); }
}

unsafe fn neighborhood_weight(point: CurvePoint, scratch: SampleScratch, workspace: Workspace) {
    unsafe {
        let count=configuration_byte(ConfigurationKey(0xa627)) as usize;
        let first_points: [u16;5]=core::array::from_fn(|i| configuration_byte(ConfigurationKey(0xa628+i as u32)) as u16);
        let first_weights: [u8;5]=core::array::from_fn(|i| configuration_byte(ConfigurationKey(0xa62d+i as u32)) as u8);
        let second_points: [u16;5]=core::array::from_fn(|i| configuration_byte(ConfigurationKey(0xa632+i as u32)) as u16);
        let second_weights: [u8;5]=core::array::from_fn(|i| configuration_byte(ConfigurationKey(0xa637+i as u32)) as u8);
        let mut distances: [u16;64]=core::array::from_fn(|i| {
            let dx=(point.red as i32).wrapping_sub(scratch.0.at(0x4720+i as u32*2).half() as i32);
            let dy=(point.blue as i32).wrapping_sub(scratch.0.at(0x47a0+i as u32*2).half() as i32);
            numeric_sqrt(dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy))) as u16
        });
        let mut selected=[const {core::mem::MaybeUninit::<u16>::uninit()};64];
        let mut remaining=64usize;
        for index in 0..64 {
            let mut minimum=255u16;
            let mut selected_index=None;
            for position in 0..remaining { if distances[position] < minimum { minimum=distances[position]; selected_index=Some(position); } }
            selected[index].write(minimum);
            if let Some(position)=selected_index { distances[position]=distances[remaining-1]; }
            remaining-=1;
            if index as i32 >= count as i32-1 { break; }
        }
        let mut weighted=0i32;
        let mut sum=0i32;
        for selected in &selected[..count] {
            let distance=selected.assume_init();
            let weight=curve_from_arrays(distance,&first_points,&first_weights);
            weighted=weighted.wrapping_add((weight as i32).wrapping_mul(distance as i32));
            sum=sum.wrapping_add(weight as i32);
        }
        let distance=signed_round_divide(weighted.wrapping_add(sum/2),sum) as u16;
        let weight=curve_from_arrays(distance,&second_points,&second_weights).min(100);
        workspace.0.at(0xc94).put_half(weight);
        workspace.0.at(0xc90).put_word(distance as u32);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_neighborhood_weight(red: u16, blue: u16, scratch: Address32, workspace: Address32) {
    unsafe { neighborhood_weight(CurvePoint { red,blue },SampleScratch(Memory(scratch)),Workspace(Memory(workspace))); }
}

#[derive(Clone, Copy)]
enum AdjustmentPath { Spatial, ReferenceCurve, FirstAnchor, ComponentCurves, BlendFirst, BlendComponents, Unsupported }
impl AdjustmentPath {
    fn parse(code: i32) -> Self { match code {0=>Self::Spatial,1=>Self::ReferenceCurve,2=>Self::FirstAnchor,3=>Self::ComponentCurves,4=>Self::BlendFirst,5=>Self::BlendComponents,_=>Self::Unsupported} }
    fn code(self) -> i32 { match self {Self::Spatial=>0,Self::ReferenceCurve=>1,Self::FirstAnchor=>2,Self::ComponentCurves=>3,Self::BlendFirst=>4,Self::BlendComponents=>5,Self::Unsupported=>-1} }
}
fn blend_percent(first: u16, second: u16, first_weight: u16, second_weight: u16) -> u16 {
    let denominator=first_weight as u32+second_weight as u32;
    signed_round_divide((first_weight as u32).wrapping_mul(first as u32).wrapping_add((second_weight as u32).wrapping_mul(second as u32)).wrapping_add(denominator/2) as i32,denominator as i32) as u16
}
fn scale_component(value: u16, target: u16, reference: u16) -> u16 {
    signed_round_divide((value as u32).wrapping_mul(target as u32).wrapping_add((reference>>1) as u32) as i32,reference as i32) as u16
}
fn distance_between(first: CurvePoint, second: CurvePoint) -> u16 {
    let red=(first.red as i32).wrapping_sub(second.red as i32);
    let blue=(first.blue as i32).wrapping_sub(second.blue as i32);
    numeric_sqrt(red.wrapping_mul(red).wrapping_add(blue.wrapping_mul(blue))) as u16
}

unsafe fn choose_adjustment(region: Region, plane: ScanPlane, profile: CalibrationProfile, workspace: Workspace) -> AdjustmentPath {
    unsafe {
        let point=CurvePoint { red:workspace.0.at(0xc14).half(),blue:workspace.0.at(0xc16).half() };
        let kind=profile.kind();
        let (response_curve,reference_curve)=reference_tables(kind);
        let component=component_tables(kind);
        let (above,projection)=project_curve(point,reference_curve);
        let reference_distance=distance_between(point,projection);
        let first_anchor=anchor_pair(kind,0);
        let first_distance=distance_between(point,first_anchor);
        let second_anchor=anchor_pair(kind,2);
        let second_distance=distance_between(point,second_anchor);
        let threshold=configuration_byte(ConfigurationKey(0xa5a5)) as u8;
        let path=if matches!(kind,CalibrationKind::Camera) || matches!(profile.0.at(0x30).word(),1|2) { AdjustmentPath::Spatial }
            else if kind.is_reference() || above || reference_distance <= threshold as u16 { AdjustmentPath::ReferenceCurve }
            else if first_distance <= threshold as u16 { AdjustmentPath::FirstAnchor }
            else if second_distance <= threshold as u16 { AdjustmentPath::ComponentCurves }
            else if second_distance < first_distance { AdjustmentPath::BlendComponents } else { AdjustmentPath::BlendFirst };
        workspace.0.at(0xc84).put_half(match path {AdjustmentPath::FirstAnchor=>1,AdjustmentPath::ComponentCurves=>2,AdjustmentPath::BlendFirst|AdjustmentPath::BlendComponents=>3,_=>0});
        let target=anchor_pair(kind,1);
        let reference=CurvePoint { red:component[0].at(projection.red as u32*2).half(),blue:0 };
        let reference=CurvePoint { blue:response_curve.at(reference.red as u32).byte() as u16,..reference };
        let mut spatial=CurvePoint { red:0,blue:0 };
        let mut first_weight=0u16;
        let mut reference_weight=0u16;
        let adjusted=match path {
            AdjustmentPath::Spatial=>{
                spatial=spatial_correction(reference,region,plane,profile,workspace,response_curve);
                CurvePoint { red:scale_component(point.red,spatial.red,projection.red),blue:scale_component(point.blue,spatial.blue,projection.blue) }
            },
            AdjustmentPath::ReferenceCurve=>CurvePoint { red:scale_component(point.red,reference.red,projection.red),blue:scale_component(point.blue,reference.blue,projection.blue) },
            AdjustmentPath::FirstAnchor=>CurvePoint { red:scale_component(point.red,target.red,first_anchor.red),blue:scale_component(point.blue,target.blue,first_anchor.blue) },
            AdjustmentPath::ComponentCurves=>CurvePoint { red:component[1].at(point.red as u32*2).half(),blue:component[2].at(point.blue as u32*2).half() },
            AdjustmentPath::BlendFirst|AdjustmentPath::BlendComponents=>{
                let (candidate,distance)=if matches!(path,AdjustmentPath::BlendFirst) {
                    (CurvePoint {red:scale_component(point.red,target.red,first_anchor.red),blue:scale_component(point.blue,target.blue,first_anchor.blue)},first_distance)
                } else { (CurvePoint {red:component[1].at(point.red as u32*2).half(),blue:component[2].at(point.blue as u32*2).half()},second_distance) };
                let nearest=CurvePoint {red:scale_component(point.red,reference.red,projection.red),blue:scale_component(point.blue,reference.blue,projection.blue)};
                let sum=reference_distance as i32-threshold as i32+distance as i32-threshold as i32;
                first_weight=signed_round_divide((reference_distance as i32-threshold as i32).wrapping_mul(100).wrapping_add(sum/2),sum) as u16;
                reference_weight=signed_round_divide((distance as i32-threshold as i32).wrapping_mul(100).wrapping_add(sum/2),sum) as u16;
                CurvePoint {red:blend_percent(nearest.red,candidate.red,reference_weight,first_weight),blue:blend_percent(nearest.blue,candidate.blue,reference_weight,first_weight)}
            },
            AdjustmentPath::Unsupported=>return path
        };
        if adjusted.red == 0 || adjusted.red > 511 || adjusted.blue == 0 || adjusted.blue > 511 { return AdjustmentPath::Unsupported; }
        let (attenuation_points,attenuation_values)=blue_attenuation(kind);
        let attenuation=curve_from_arrays(projection.red,&attenuation_points,&attenuation_values);
        workspace.0.at(0xc68).put_half(projection.red); workspace.0.at(0xc6a).put_half(projection.blue);
        workspace.0.at(0xca2).put_half(reference.red); workspace.0.at(0xca4).put_half(reference.blue);
        workspace.0.at(0xc86).put_half(spatial.red); workspace.0.at(0xc88).put_half(spatial.blue);
        workspace.0.at(0xc80).put_half(adjusted.red); workspace.0.at(0xc82).put_half(adjusted.blue);
        workspace.0.at(0xca6).put_half((100i32).wrapping_sub(((attenuation as u32).wrapping_mul(100u32.wrapping_sub(adjusted.blue as u32)).wrapping_add(50) as i32)/100) as u16);
        workspace.0.at(0xc8a).put_half(first_weight); workspace.0.at(0xc8c).put_half(reference_weight);
        path
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_choose_adjustment(region: Address32, plane: Address32, profile: Address32, workspace: Address32) -> i32 {
    unsafe { choose_adjustment(Region(Memory(region)),ScanPlane(Memory(plane)),CalibrationProfile(Memory(profile)),Workspace(Memory(workspace))).code() }
}

fn mix_level(original: u16, target: u16, amount: u16) -> u16 {
    let difference=target as i32-original as i32;
    let numerator=(amount as i32).wrapping_mul(difference).wrapping_add(if difference < 0 {-50} else {50});
    (original as i32).wrapping_add(numerator/100) as u16
}

unsafe fn primary_adjustment(path: AdjustmentPath, alternate: bool, _fallback: Levels, levels: Levels, scratch: SampleScratch, profile: CalibrationProfile, workspace: Workspace) {
    unsafe {
        let kind=profile.kind();
        let points=correction_points(kind);
        let (responses,controls)=primary_responses(kind,alternate);
        let (red_responses,blue_responses)=primary_component_responses(kind,alternate);
        let reference=workspace.0.at(0xca2).half();
        let spatial=workspace.0.at(0xc86).half();
        let target_red=workspace.0.at(0xc80).half();
        let target_blue=workspace.0.at(0xca6).half();
        let mut first_weight=workspace.0.at(0xc8a).half();
        let mut reference_weight=workspace.0.at(0xc8c).half();
        let amount=match path {
            AdjustmentPath::Spatial=>curve_from_arrays(spatial,&points,&responses),
            AdjustmentPath::ReferenceCurve=>curve_from_arrays(reference,&points,&responses),
            AdjustmentPath::FirstAnchor=>controls[0] as u16,
            AdjustmentPath::ComponentCurves=>controls[1] as u16,
            AdjustmentPath::BlendFirst|AdjustmentPath::BlendComponents=>{
                let control=if matches!(path,AdjustmentPath::BlendFirst) {controls[0]} else {controls[1]} as u16;
                let curve=curve_from_arrays(reference,&points,&responses);
                if (controls[2] as u16) < first_weight {
                    let remaining=100-controls[2] as i32;
                    first_weight=signed_round_divide((first_weight as i32-controls[2] as i32).wrapping_mul(100).wrapping_add(remaining/2),remaining) as u16;
                    reference_weight=100u16.wrapping_sub(first_weight);
                } else { first_weight=0; reference_weight=100; }
                blend_percent(curve,control,reference_weight,first_weight)
            },
            AdjustmentPath::Unsupported=>0
        }.min(100);
        let (red,blue)=match path {
            AdjustmentPath::Spatial=>(curve_from_arrays(spatial,&points,&red_responses),curve_from_arrays(spatial,&points,&blue_responses)),
            AdjustmentPath::ReferenceCurve=>(curve_from_arrays(reference,&points,&red_responses),curve_from_arrays(reference,&points,&blue_responses)),
            AdjustmentPath::FirstAnchor=>(red_responses[9] as u16,blue_responses[9] as u16),
            AdjustmentPath::ComponentCurves=>(red_responses[10] as u16,blue_responses[10] as u16),
            AdjustmentPath::BlendFirst|AdjustmentPath::BlendComponents=>{
                let index=if matches!(path,AdjustmentPath::BlendFirst) {9} else {10};
                (blend_percent(curve_from_arrays(reference,&points,&red_responses),red_responses[index] as u16,reference_weight,first_weight),blend_percent(curve_from_arrays(reference,&points,&blue_responses),blue_responses[index] as u16,reference_weight,first_weight))
            },
            AdjustmentPath::Unsupported=>(0,0)
        };
        let old_green=levels.0.half();
        let old_red=levels.0.at(2).half();
        let old_blue=levels.0.at(4).half();
        let new_red=unsigned_divide((red as u32).wrapping_mul(old_red as u32).wrapping_add((target_red>>1) as u32),target_red as u32) as u16;
        let new_blue=unsigned_divide((blue as u32).wrapping_mul(old_blue as u32).wrapping_add((target_blue>>1) as u32),target_blue as u32) as u16;
        let adjusted_red=mix_level(old_red,new_red,amount);
        let adjusted_blue=mix_level(old_blue,new_blue,amount);
        neighborhood_weight(CurvePoint {red:target_red,blue:target_blue},scratch,workspace);
        workspace.0.at(0xca8).put_half(red); workspace.0.at(0xcaa).put_half(blue);
        workspace.0.at(6).put_half(old_green); workspace.0.at(8).put_half(adjusted_red); workspace.0.at(10).put_half(adjusted_blue);
        workspace.0.at(12).put_half(old_green); workspace.0.at(14).put_half(new_red); workspace.0.at(16).put_half(new_blue);
        workspace.0.at(0xc96).put_half(amount); workspace.0.at(0xc8a).put_half(first_weight); workspace.0.at(0xc8c).put_half(reference_weight);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_primary_adjustment(path: i32, alternate: u32, fallback: Address32, levels: Address32, scratch: Address32, profile: Address32, workspace: Address32) {
    unsafe { primary_adjustment(AdjustmentPath::parse(path),alternate==1,Levels(Memory(fallback)),Levels(Memory(levels)),SampleScratch(Memory(scratch)),CalibrationProfile(Memory(profile)),Workspace(Memory(workspace))); }
}

unsafe fn secondary_adjustment(path: AdjustmentPath, alternate: bool, selection: u32, fallback: Levels, settings: Memory, profile: CalibrationProfile, operation: OutputSelection, region: Region, workspace: Workspace) {
    unsafe {
        let kind=profile.kind();
        let points=correction_points(kind);
        let (responses,controls)=secondary_responses(kind,alternate,selection);
        let (red_points,red_responses)=secondary_component_curve(kind,alternate,false);
        let (blue_points,blue_responses)=secondary_component_curve(kind,alternate,true);
        let limits=correction_limits(operation);
        let first_weight=workspace.0.at(0xc8a).half();
        let reference_weight=workspace.0.at(0xc8c).half();
        let amount=match path {
            AdjustmentPath::Spatial|AdjustmentPath::ReferenceCurve=>{
                let key=if alternate {0xa559} else {0xa624};
                let position=if configuration_byte(ConfigurationKey(key)) as u8 == 1 {
                    workspace.0.at(if matches!(path,AdjustmentPath::Spatial) {0xc86} else {0xca2}).half()
                } else {settings.word() as u16};
                curve_from_arrays(position,&points,&responses)
            },
            AdjustmentPath::FirstAnchor=>controls[0] as u16,
            AdjustmentPath::ComponentCurves=>controls[1] as u16,
            AdjustmentPath::BlendFirst|AdjustmentPath::BlendComponents=>blend_percent(curve_from_arrays(workspace.0.at(0xca2).half(),&points,&responses),controls[usize::from(matches!(path,AdjustmentPath::BlendComponents))] as u16,reference_weight,first_weight),
            AdjustmentPath::Unsupported=>0
        };
        let current_red=workspace.0.at(8).half();
        let current_blue=workspace.0.at(10).half();
        let original_red=fallback.0.at(2).half();
        let original_blue=fallback.0.at(4).half();
        let red_difference=signed_round_divide((original_red as u32).wrapping_mul(1000).wrapping_add((current_red>>1) as u32) as i32,current_red as i32).wrapping_sub(1000) as i16;
        let red=if (red_points[0] as i32) < red_difference as i32 {curve_from_arrays(red_difference as u16,&red_points,&red_responses)} else {red_responses[0] as u16};
        let blue_difference=signed_round_divide((original_blue as u32).wrapping_mul(1000).wrapping_add((current_blue>>1) as u32) as i32,current_blue as i32).wrapping_sub(1000) as i16;
        let blue=if (blue_points[0] as i32) < blue_difference as i32 {curve_from_arrays(blue_difference as u16,&blue_points,&blue_responses)} else {blue_responses[0] as u16};
        let mut strength=100u16.wrapping_sub(((red.max(blue) as u32).wrapping_mul(amount as u32).wrapping_add(50) as i32/100) as u16);
        if strength < limits[0] as u16 {strength=limits[0] as u16;} else if strength > limits[1] as u16 {strength=limits[1] as u16;}
        let scale=if region.0.at(0x2c).word() == 0 {100} else {configuration_byte(ConfigurationKey(0xa74f)) as u16};
        let combined=(scale as u32).wrapping_mul(workspace.0.at(0xc94).half() as i16 as i32 as u32).wrapping_mul(strength as u32).wrapping_add(5000)/10000;
        let red_level=mix_level(original_red,current_red,combined as u16);
        let blue_level=mix_level(original_blue,current_blue,combined as u16);
        workspace.0.at(0xc98).put_half(amount); workspace.0.at(0xc9a).put_half(red); workspace.0.at(0xc9c).put_half(blue);
        workspace.0.at(0xc9e).put_half(combined as u16); workspace.0.at(0xca0).put_half(scale);
        workspace.0.at(2).put_half(red_level); workspace.0.put_half(fallback.0.half()); workspace.0.at(4).put_half(blue_level);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_secondary_adjustment(path: i32, alternate: u32, selection: u32, fallback: Address32, settings: Address32, profile: Address32, operation: u32, region: Address32, workspace: Address32) {
    unsafe { secondary_adjustment(AdjustmentPath::parse(path),alternate==1,selection,Levels(Memory(fallback)),Memory(settings),CalibrationProfile(Memory(profile)),OutputSelection(operation),Region(Memory(region)),Workspace(Memory(workspace))); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scan_adjust_components(plane: Address32, region: Address32, profile: Address32, levels: Address32, workspace: Address32, fallback: Address32, alternate: u32, settings: Address32, selection: u32, operation: u32, scratch: Address32) -> i32 {
    unsafe {
        let profile=CalibrationProfile(Memory(profile));
        let workspace=Workspace(Memory(workspace));
        let region=Region(Memory(region));
        let path=choose_adjustment(region,ScanPlane(Memory(plane)),profile,workspace);
        if matches!(path,AdjustmentPath::Unsupported) {return 0;}
        primary_adjustment(path,alternate==1,Levels(Memory(fallback)),Levels(Memory(levels)),SampleScratch(Memory(scratch)),profile,workspace);
        secondary_adjustment(path,alternate==1,selection,Levels(Memory(fallback)),Memory(settings),profile,OutputSelection(operation),region,workspace);
        1
    }
}
