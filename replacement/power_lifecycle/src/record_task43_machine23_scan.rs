//! X-M1 1.50: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; id5 f3c17c..f3d3e0.

use core::ptr::{read_volatile, write_volatile};
use crate::record_address_space::GuestAddressSpace;

unsafe extern "C" {
    fn xm1_power_record_helper_69f8fc(key: u32) -> u32;
    fn xm1_power_record_helper_69fec8(key: u32) -> u32;
    fn xm1_power_record_helper_76da84() -> u32;
    fn xm1_power_record_helper_76d9c8() -> u32;
    fn xm1_power_record_helper_758098(resource: u32, record: *const u8);
    fn xm1_power_record_helper_7567b8(resource: u32, record: *const u8);
    fn xm1_power_record_helper_756b0c(resource: u32) -> u32;
    fn xm1_power_record_helper_756b2c(resource: u32, flags: u32);
    fn xm1_power_record_helper_74e5c0(resource: u32) -> u32;
    fn xm1_power_record_helper_f47468();
    fn xm1_power_record_helper_746054() -> u32;
    fn xm1_power_record_helper_faab3c(resource: u32, mode: u32, value: u32, profile: u32, count: u32, sample: u32, workspace: *mut u32) -> u32;
}

const BASE: usize = 0x016c0fb8;
unsafe fn r8(offset: usize) -> u8 { unsafe { read_volatile((BASE + offset) as *const u8) } }
unsafe fn r16(offset: usize) -> u16 { unsafe { read_volatile((BASE + offset) as *const u16) } }
unsafe fn r32(offset: usize) -> u32 { unsafe { read_volatile((BASE + offset) as *const u32) } }
unsafe fn w8(offset: usize, value: u8) { unsafe { write_volatile((BASE + offset) as *mut u8, value); } }
unsafe fn w16(offset: usize, value: u16) { unsafe { write_volatile((BASE + offset) as *mut u16, value); } }
unsafe fn w32(offset: usize, value: u32) { unsafe { write_volatile((BASE + offset) as *mut u32, value); } }
unsafe fn active_resource() -> u32 { unsafe { read_volatile(0x016c21fc as *const u32) } }
unsafe fn configuration(key: u32) -> u32 { unsafe { xm1_power_record_helper_69f8fc(key) } }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3c17c() {
    unsafe {
        write_volatile(0x016c2200 as *mut u32, 0);
        write_volatile(0x016c2204 as *mut u32, 0);
        w32(0, 0); w8(4, 0); w32(0x1c, 0); w16(0x2a, 0); w8(0x2d, 1); w8(0x2c, 1);
        let position = xm1_power_record_helper_76da84() as u16;
        w16(0x2e, position); w16(0x30, position); w16(0x34, position);
        for offset in [0x44, 0x48, 0x4c, 0x50] { w32(offset, 2); }
        w8(0xf5c, 0); w8(0xf5d, 0); w32(0xf4c, 0); w32(0x14, 0);
        w8(0x204, 1); w8(0x205, 1);
        for offset in [0x208, 0x20c] { w32(offset, 0); }
        w8(0x210, 0); w8(0x211, 0);
        for offset in [0x214, 0x218, 0x21c, 0x220] { w32(offset, 0); }
        for offset in [0x212, 0x240, 0x241, 0x1f2] { w8(offset, 0); }
        w32(0xf00, 0); w32(0xefc, 0); w16(0xf16, 0); w16(0xf18, 0);
        w8(0xf24, 0); w8(0xf25, 0); w16(0xfa4, 0); w16(0xfa6, 0);
        for offset in [0xf20, 0xf1c, 0xf28, 0xf9c, 0xfa0, 0xf2c, 0xfa8] { w32(offset, 0); }
        let previous = r32(0x38);
        if previous == xm1_power_record_helper_76d9c8() && r16(0x1f8) == 0 { w16(0x1f6, 255); w16(0x1f8, 0); }
        else { w16(0x1f6, 0); w16(0x1f8, 1); }
        if r32(0xc) == 17 {
            w32(8, 17); w8(0xef0, 1); w16(0xef8, xm1_power_record_helper_76da84() as u16); w32(0xf50, 1);
        } else { w32(8, 13); }
        let current = xm1_power_record_helper_76d9c8();
        w32(0x38, current); w32(0x3c, current);
        w32(0xfb0, 0); w32(0xfb4, 1); w32(0xfb8, 1);
        core::ptr::write_bytes((BASE + 0xfe0) as *mut u8, 0, 608);
        for offset in 0x121c..=0x1220 { w8(offset, 255); }
        let geometry = [0u32; 6];
        xm1_power_record_helper_758098(active_resource(), geometry.as_ptr().cast());
        let defaults = [0u32, 4, 0, 0, 0, 2, 0, 2];
        xm1_power_record_helper_7567b8(active_resource(), defaults.as_ptr().cast());
        let flags = xm1_power_record_helper_756b0c(active_resource());
        xm1_power_record_helper_756b2c(active_resource(), flags | 2);
        if xm1_power_record_helper_74e5c0(active_resource()) != 1 { xm1_power_record_helper_f47468(); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3c428() {
    unsafe {
        let mode = r32(8);
        if mode == 14 && r32(0x244) == 1 { return; }
        if r32(0xf70) == 14 && r32(0xf64) == 1 { return; }
        if mode == 17 && xm1_power_record_helper_746054() != 0 { return; }
        let position = r16(0x2e);
        let minimum = r16(0x20);
        let maximum = r16(0x22);
        let delta;
        if (position as i16) < (minimum as i16) {
            w8(0x2c, 1); delta = minimum.wrapping_sub(position);
        } else if (position as i16) > (maximum as i16) {
            w8(0x2c, 255); delta = position.wrapping_sub(maximum);
        } else {
            let distance = i32::from(r16(0x2a) as i16);
            match r8(0x2c) as i8 {
                -1 if i32::from(position as i16) - distance < i32::from(minimum as i16) => w16(0x2a, position.wrapping_sub(minimum)),
                1 if i32::from(position as i16) + distance > i32::from(maximum as i16) => w16(0x2a, maximum.wrapping_sub(position)),
                _ => {}
            }
            return;
        }
        let mut distance = u32::from(delta);
        if distance > configuration(0x8838) { distance = configuration(0x8838); }
        if (r16(0x2a) as i16) < (distance as i16) { w16(0x2a, distance as u16); }
    }
}

fn clear_step<M: crate::record_address_space::GuestAddressSpace, E>(context: &mut crate::record_address_space::RecordContext<M, E>) -> Result<(), crate::record_address_space::MemoryError> {
    use crate::record_address_space::GuestAddress;
    context.write_u16(GuestAddress::new(0x016c0fe2), 0)?;
    context.write_u8(GuestAddress::new(0x016c0fbc), 0)?;
    let direction = context.read_u8(GuestAddress::new(0x016c0fe5))?;
    context.write_u8(GuestAddress::new(0x016c0fe4), direction)
}

fn restart<M: crate::record_address_space::GuestAddressSpace, E>(context: &mut crate::record_address_space::RecordContext<M, E>) -> Result<(), crate::record_address_space::MemoryError> {
    use crate::record_address_space::GuestAddress;
    clear_step(context)?;
    context.write_u8(GuestAddress::new(0x016c1ea7), 2)?;
    let position = context.read_u16(GuestAddress::new(0x016c0fe0))?;
    context.write_u16(GuestAddress::new(0x016c1eb0), position)
}

pub fn helper_f3c5d0<M: crate::record_address_space::GuestAddressSpace, E>(context: &mut crate::record_address_space::RecordContext<M, E>) -> Result<i32, crate::record_address_space::MemoryError> {
    use crate::record_address_space::GuestAddress;
    if crate::record_configuration::helper_69f8fc(context, 0x8801)? != 1 {
        context.write_u32(GuestAddress::new(0x016c0fd4), 1)?;
        clear_step(context)?;
        Ok(17)
    } else {
        restart(context)?;
        Ok(18)
    }
}

pub fn helper_f3c64c<M: crate::record_address_space::GuestAddressSpace, E>(context: &mut crate::record_address_space::RecordContext<M, E>) -> Result<i32, crate::record_address_space::MemoryError> {
    use crate::record_address_space::GuestAddress;
    if crate::record_configuration::helper_69f8fc(context, 0x8801)? != 1 { return Ok(17); }
    let current = context.read_u32(GuestAddress::new(0x016c1f70))?;
    let previous = context.read_u32(GuestAddress::new(0x016c1f6c))?;
    if current != previous { restart(context)?; Ok(18) } else { Ok(17) }
}

fn update_counters<M: crate::record_address_space::GuestAddressSpace, E>(context: &mut crate::record_address_space::RecordContext<M, E>, direction: i8, value: u32, improved: bool) -> Result<(), crate::record_address_space::MemoryError> {
    use crate::record_address_space::GuestAddress;
    let rising = GuestAddress::new(0x016c11a6);
    let falling = GuestAddress::new(0x016c11a7);
    match direction {
        1 if improved => {
            context.write_u8(falling, 0)?;
            let count = context.read_u8(rising)?;
            context.write_u8(rising, count.wrapping_add(1))?;
        }
        1 => {
            let count = if value == 0 { 0 } else { context.read_u8(falling)?.wrapping_add(1) };
            context.write_u8(falling, count)?;
            context.write_u8(rising, 0)?;
        }
        -1 if improved => {
            let count = context.read_u8(falling)?;
            context.write_u8(falling, count.wrapping_add(1))?;
            context.write_u8(rising, 0)?;
        }
        -1 => {
            context.write_u8(falling, 0)?;
            let count = if value == 0 { 0 } else { context.read_u8(rising)?.wrapping_add(1) };
            context.write_u8(rising, count)?;
        }
        _ => { context.write_u8(falling, 0)?; context.write_u8(rising, 0)?; }
    }
    Ok(())
}

pub fn helper_f3c6c0<M: crate::record_address_space::GuestAddressSpace, E>(context: &mut crate::record_address_space::RecordContext<M, E>) -> Result<(), crate::record_address_space::MemoryError> {
    use crate::record_address_space::GuestAddress;
    let direction = context.read_u8(GuestAddress::new(0x016c11bc))? as i8;
    if direction == 1 || direction == -1 {
        let value = context.read_u32(GuestAddress::new(0x016c11dc))?;
        let previous = context.read_u32(GuestAddress::new(0x016c11e0))?;
        update_counters(context, direction, value, value < previous)?;
    } else { update_counters(context, direction, 0, false)?; }
    let direction = context.read_u8(GuestAddress::new(0x016c11bd))? as i8;
    if direction == 1 || direction == -1 {
        let value = context.read_u32(GuestAddress::new(0x016c11d0))?;
        let previous = context.read_u32(GuestAddress::new(0x016c11d4))?;
        update_counters(context, direction, value, value > previous)?;
    }
    if context.read_u32(GuestAddress::new(0x016c1188))? == 1 {
        context.write_u8(GuestAddress::new(0x016c11a7), 0)?;
        context.write_u8(GuestAddress::new(0x016c11a6), 0)?;
    }
    Ok(())
}

unsafe fn begin_peak() {
    unsafe { w8(0x27f, 0); w8(0x27e, 0); w16(0x27a, r16(0x2e)); w16(0x2a, 0); }
}

pub fn helper_f3c8d8<M: crate::record_address_space::GuestAddressSpace, E>(context: &mut crate::record_address_space::RecordContext<M, E>) -> Result<i32, crate::record_address_space::MemoryError> {
    use crate::record_address_space::GuestAddress;
    let first = u32::from(context.read_u8(GuestAddress::new(0x016c11a5))?);
    let first_reached = first >= crate::record_configuration::helper_69f8fc(context, 0x8811)?;
    let reached = if first_reached { true } else {
        let second = u32::from(context.read_u8(GuestAddress::new(0x016c11a4))?);
        second >= crate::record_configuration::helper_69f8fc(context, 0x8812)?
    };
    if reached {
        context.write_u8(GuestAddress::new(0x016c1237), 0)?;
        context.write_u8(GuestAddress::new(0x016c1236), 0)?;
        let position = context.read_u16(GuestAddress::new(0x016c0fe6))?;
        context.write_u16(GuestAddress::new(0x016c1232), position)?;
        context.write_u8(GuestAddress::new(0x016c0fbc), 0)?;
        let direction = if crate::record_configuration::helper_69f8fc(context, 0x880e)? & 2 != 0 { 1 } else { 255 };
        context.write_u8(GuestAddress::new(0x016c0fe4), direction)?;
        context.write_u16(GuestAddress::new(0x016c0fe2), 0)?;
        if first_reached { context.write_u8(GuestAddress::new(0x016c11ad), 1)?; }
        return Ok(14);
    }
    let direction = context.read_u8(GuestAddress::new(0x016c0fe5))?;
    context.write_u8(GuestAddress::new(0x016c0fe4), direction.wrapping_neg())?;
    context.write_u8(GuestAddress::new(0x016c11ad), 4)?;
    Ok(13)
}

unsafe fn select_counter_direction() {
    unsafe {
        if r8(0x1ef) != 0 { w8(0x2c, 1); }
        else if r8(0x1ee) != 0 { w8(0x2c, 255); }
    }
}

unsafe fn select_threshold_direction() {
    unsafe {
        if configuration(0x8819) == 1 {
            let first = u32::from(r8(0x1ef));
            if first >= configuration(0x8810) { w8(0x2c, 1); }
            else {
                let second = u32::from(r8(0x1ee));
                if second >= configuration(0x8810) { w8(0x2c, 255); }
            }
        }
    }
}

unsafe fn counters_reached() -> bool {
    unsafe {
        let first = u32::from(r8(0x1ef));
        if first >= configuration(0x8811) { true }
        else { let second = u32::from(r8(0x1ee)); second >= configuration(0x8811) }
    }
}

unsafe fn finish_classification(peak: bool, clear_direction: bool) -> i32 {
    unsafe {
        let result = if peak {
            begin_peak(); w8(0x1f5, 1); w8(0x1f1, 0); w8(0x1ed, 0); 14
        } else { w32(0xef4, 0); 16 };
        for offset in [0x1ef, 0x1ee, 4, 0x1ec, 0x1f0] { w8(offset, 0); }
        if clear_direction { w8(0x204, 0); }
        result
    }
}

unsafe fn fast_finish(step: i32) -> i32 {
    unsafe {
        let sample = r32(0x208);
        let threshold = configuration(0x8780).wrapping_mul(10);
        if sample < threshold {
            let first = r32(0x1a8);
            let second = r32(0x1ac);
            if first > second.wrapping_add(configuration(0x8788).wrapping_mul(10)) { return finish_classification(false, true); }
        }
        if read_volatile(0x016c0f4c as *const u32) == 1 { return finish_classification(false, true); }
        let position = i32::from(r16(0x2e) as i16);
        let maximum = i32::from(r16(0x22) as i16);
        if position > maximum.wrapping_sub((configuration(0x8835) as i32).wrapping_mul(step)) && r8(0x2c) as i8 == 1 { return finish_classification(false, true); }
        let position = i32::from(r16(0x2e) as i16);
        let minimum = i32::from(r16(0x20) as i16);
        if position < minimum.wrapping_add((configuration(0x8835) as i32).wrapping_mul(step)) && r8(0x2c) as i8 == -1 { return finish_classification(false, true); }
        if r32(0x208) > r32(0x224) { return finish_classification(false, true); }
        finish_classification(true, true)
    }
}

unsafe fn motion_small(offset: usize) -> bool {
    unsafe {
        let magnitude = (r32(offset) as i32).wrapping_abs();
        magnitude < (xm1_power_record_helper_69fec8(0x881a).wrapping_mul(10) as i32)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3c9f4() -> i32 {
    unsafe {
        w8(0x1f5, 4);
        let step = xm1_power_record_helper_faab3c(active_resource(),1,r32(0x1d8),r32(0xf8c),r32(0x1240),r32(0x74),(BASE + 0x88) as *mut u32);
        if r8(0x241) != 0 {
            let first = u32::from(r8(0x1ef));
            if first >= configuration(0x8811) { w8(0x1ef, 1); }
            else { let second = u32::from(r8(0x1ee)); if second >= configuration(0x8811) { w8(0x1ee, 1); } }
        }
        let primary = xm1_power_record_helper_74e5c0(active_resource()) == 1;
        let limit = if r8(0x241) != 0 && r32(0x1d0) == 0 {
            configuration(if primary { 0x8774 } else { 0x8773 })
        } else {
            let special = read_volatile(0x016c0f4c as *const u32) == 1;
            configuration(match (primary, special) { (true, true) => 0x8771, (true, false) => 0x8813, (false, true) => 0x8770, (false, false) => 0x8812 })
        };
        let fast = configuration(0x8778) == 1;
        if u32::from(r8(0x1ec)) >= limit {
            select_counter_direction();
            if fast { return fast_finish(i32::from(step as i16)); }
            let count = u32::from(r8(0x1f0));
            let reached = count >= configuration(0x881e);
            let peak = reached || configuration(0x8819) == 0;
            return finish_classification(peak, false);
        }
        if counters_reached() {
            select_counter_direction();
            if fast { return fast_finish(i32::from(step as i16)); }
            let peak = motion_small(0x230) || motion_small(0x234) || configuration(0x8819) != 1;
            return finish_classification(peak, false);
        }
        if fast {
            w8(0x2c, r8(0x2d).wrapping_neg());
            select_threshold_direction();
            let sample = r32(0x208);
            if sample >= configuration(0x877e) || r32(0x1d0) != 0 { return 13; }
            w8(0x1ec, r8(0x1ec).wrapping_add(limit as u8));
            return 13;
        }
        w8(0x2c, if r16(0x1fa) <= r16(0x1fc) { 255 } else { 1 });
        select_threshold_direction();
        if motion_small(0x230) && motion_small(0x234) { w8(0x1f0, r8(0x1f0).wrapping_add(1)); }
        else { w8(0x1f0, 0); }
        13
    }
}
