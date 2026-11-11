use super::boundary::*;

type Notify = unsafe extern "C" fn(u32, u32, u32, u32, u32, u32, u32, u32, u32);

#[unsafe(export_name = "xm1_power_record_helper_f1667c")]
pub unsafe extern "C" fn initialize_profile_fields() {
    unsafe {
        if owner().0 != 0 {
            let (mut channel, mut first, mut second) = (0, 0, 0);
            eea030(&mut channel, &mut first, &mut second);
            let geometry = combined(u32::from(channel), f160d0()).wrapping_add(u32::from(first)) as i16;
            slot_geometry(profile().0, 0, geometry as i32 as u32);
            slot_channel(profile().0, 0, u32::from(channel));
            slot_first(profile().0, 0, first as i16 as i32 as u32);
            slot_second(profile().0, 0, second as i16 as i32 as u32);
            slot_a(profile().0, 0, 0);
            slot_b(profile().0, 0, 0);
            slot_c(profile().0, 0, 0);
            slot_d(profile().0, 0, 0);
            slot_e(profile().0, 0, 0);
            slot_f(profile().0, 0, 0);
            let item = profile();
            let scale = eea094();
            slot_scale(item.0, 0, scale);
            let enabled = u32::from(f615bc(profile().0, 0) != 6);
            slot_g(profile().0, 0, enabled);
            slot_h(profile().0, 0, enabled);
            let mut status = resource46(profile().0);
            if status == 0 {
                let index = slot_reset(profile().0, 0);
                status = match ef53d0(index) { 0 => 1, 1 => 2, _ => 3 };
            }
            slot_status(profile().0, 0, status);
            let mut snapshot = [0u32; 42];
            slot_snapshot(snapshot.as_mut_ptr(), profile().0, 0);
            let destination = owner().offset(0x20).pointee().word();
            copy_slot(destination, snapshot.as_ptr());
        }
        shared_a(profile().0, 900);
        shared_b(profile().0, 0);
        shared_c(profile().0, 700);
        shared_d(profile().0, 600);
        shared_e(profile().0, 0);
        shared_f(profile().0, 0);
        shared_g(profile().0, 0);
        shared_h(profile().0, 0);
        shared_i(profile().0, 1);
        shared_j(profile().0, 12);
        shared_k(profile().0, 0);
        shared_l(profile().0, 0);
        let current = owner();
        if current.0 != 0 && current.word() != 0 {
            notify(current, 3);
            notify(owner(), 4);
        }
    }
}

unsafe fn notify(current: Address, operation: u32) {
    unsafe {
        let callback: Notify = core::mem::transmute(current.word() as usize);
        let value = f160d0();
        let data = current.offset(0x34).word();
        let mode = f612dc(profile().0, 0);
        let flags = f61584(profile().0, 0);
        callback(operation, value, data, 0, 0, mode, flags, 0, 0);
    }
}
