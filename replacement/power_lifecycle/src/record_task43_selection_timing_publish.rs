use super::*;

#[cfg(target_arch = "arm")]
core::arch::global_asm!(include_str!("record_task43_selection_timing_residue.S"), options(raw));

unsafe extern "C" {
    pub(super) fn xm1_power_record_helper_f19634();
}

#[derive(Clone, Copy)]
struct Publication {
    owner: Address,
    item_records: Address,
    count: u32,
    mode: u32,
    active: bool,
    prepared_mode: i32,
    request_first: i32,
    request_second: i32,
}

unsafe fn state() -> Publication {
    if word(JOB + 0x126c) == 1 {
        let owner = h75a58c(selected().0);
        let item_records = h75a61c(selected().0);
        let prepared_mode = f612dc(selected().0, word(JOB + 0x47c));
        Publication { owner: Address(owner as u32 as usize), item_records: Address(item_records as u32 as usize), count: 0,
            mode: 0, active: true, prepared_mode, request_first: 0, request_second: 0 }
    } else {
        let r = request().0;
        let mode = word(r + 4) as u32;
        let owner = Address(address(address(r + 0x2c)));
        let count = word(address(r + 0x18)) as u32;
        let item_records = Address(address(r + 0x20));
        let request_first = word(r + 0x3c);
        let request_second = word(r + 0x38);
        let active = word(r + 0x10) & 2 != 0;
        let prepared_mode = f612dc(selected().0, word(JOB + 0x47c));
        Publication { owner, item_records, count, mode, active, prepared_mode, request_first, request_second }
    }
}

unsafe fn publish_mode_five() {
    let mut record = [0i32; 19];
    h7588e8(record.as_mut_ptr() as i32);
    let at = record.as_mut_ptr() as usize;
    put_half(at, h750efc(selected().0) as i16);
    put_half(at + 2, h751914(selected().0) as i16);
    put_half(at + 4, h7519ac(selected().0) as i16);
    let category = if word(PROFILE + 0x6410) == 0 { 4 } else { i32::from(byte(PROFILE + 0x6414)) };
    let pair = f1b050(7, category, 0) as u32 as usize;
    let offset = usize::from(byte(pair)) * 0xcc + usize::from(byte(pair + 1)) * 0x44;
    record[9] = if word(PROFILE + 0x6410) != 0 && category <= 2 {
        word(0x016b343c + offset + category as usize * 0x264)
    } else { word(0x016b31d8 + offset) };
    h75890c(selected().0, record.as_mut_ptr() as i32);
}

unsafe fn save_pair(index: i32, first: i32, second: i32) {
    let pair = [first, second];
    h75a464(selected().0, index, pair.as_ptr() as i32);
}

unsafe fn publish_saved_geometry() {
    let category = i32::from(byte(PROFILE + 0x6414));
    let pair = f1b050(7, category, 0) as u32 as usize;
    let offset = usize::from(byte(pair)) * 0xfc + usize::from(byte(pair + 1)) * 0x54;
    let p = 0x016aead4 + offset;
    copy(p, 0x016b2760 + category as usize * 0x2f4 + offset, 0x54);
    h756fb8(selected().0, i32::from(half(p)));
    h757004(selected().0, word(p + 4));
    h757050(selected().0, i32::from(half(p + 10)));
    h7570a0(selected().0, i32::from(half(p + 12)));
    h7570f0(selected().0, i32::from(half(p + 8)));
    h757140(selected().0, i32::from(half(p + 16)));
    h757190(selected().0, i32::from(half(p + 22)));
    h7571e0(selected().0, i32::from(half(p + 26)));
    h75722c(selected().0, word(p + 28));
    h757274(selected().0, word(p + 52));
    h757360(selected().0, half(p + 64) as u16 as i32);
    h7573ac(selected().0, i32::from(byte(p + 66)));
    h7572c0(selected().0, i32::from(half(p + 60)));
    h7573f4(selected().0, word(LIST + 0x1eec));
}

#[derive(Clone, Copy)]
struct CallbackArguments {
    parameter: i32,
    aperture: i32,
    target: i32,
    timing: i32,
    flags: i32,
    status: i32,
    low: i32,
    high: i32,
    step: i32,
    saved: i32,
    geometry: i32,
}

impl CallbackArguments {
    unsafe fn read(item: Item) -> Self {
        let parameter = h7536a0(item.0, 0);
        let aperture = h752ba0(item.0, 0);
        let target = h752a40(item.0, 0);
        let timing = h753750(item.0, 0);
        let flags = h753800(item.0, 0);
        let status = h7538b0(item.0, 0);
        let low = h752c50(item.0, 0);
        let high = h752d00(item.0, 0);
        let step = h752f10(item.0, 0);
        let saved = h753120(item.0, 0);
        let record = TimingRecord::read(item);
        Self { parameter, aperture, target, timing, flags, status, low, high, step, saved, geometry: record.words[11] }
    }
    unsafe fn invoke(self, callback: usize) {
        let call: unsafe extern "C" fn(i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32) = core::mem::transmute(callback);
        call(self.parameter, self.aperture, self.target, self.timing, self.flags, self.status, 0, 1,
            self.low, self.high, self.step, self.saved, self.geometry);
    }
    unsafe fn store(self, callback: usize, payload: Address) {
        let p = payload.0;
        put(p, callback as i32);
        put(p + 4, self.parameter);
        put_half(p + 8, self.aperture as i16);
        put(p + 12, self.target);
        put_half(p + 16, self.timing as i16);
        put(p + 20, self.flags);
        put(p + 24, self.status);
        put(p + 28, 0);
        put(p + 32, 1);
        put_half(p + 36, self.low as i16);
        put_half(p + 38, self.high as i16);
        put_half(p + 40, self.step as i16);
        put_half(p + 42, self.saved as i16);
        put(p + 44, self.geometry);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn xm1_power_record_timing_publish_cpu(payload: *mut u8) {
    if f16664() == 0 { return; }
    let state = state();
    if state.mode == 5 { publish_mode_five(); }
    let mut copied = false;
    let mut run_defaults = true;
    if word(PROFILE + 0x6410) != 0 {
        if word(PROFILE + 0x6568) != 0 && configuration_byte(0x5256) & 3 != 0 {
            let mut snapshot = [0i32; 10];
            h7582c0(snapshot.as_mut_ptr() as i32);
            let value = h75060c(selected().0);
            let property = h7503ec(selected().0);
            f1c6b4(1, snapshot.as_mut_ptr() as i32, value, state.prepared_mode, property);
        }
        if state.active || state.mode <= 2 {
            records::xm1_power_record_helper_f18b94(selected().0, 1, state.mode as i32, word(LIST + 0x1f14), word(JOB + 0x126c));
            copied = true;
        } else {
            if word(PROFILE + 0x6568) != 0 {
                records::xm1_power_record_helper_f19204(selected().0, 1, state.mode as i32, word(LIST + 0x1f14), word(JOB + 0x126c));
                copied = true;
            }
            run_defaults = false;
        }
    }
    if run_defaults && state.mode != 7 && state.active && word(JOB + 0x126c) == 0 {
        let mut record = TimingRecord::read(selected());
        let value = if word(JOB + 0x1268) == 1 { h75a440(selected().0) } else { h74f5ec(selected().0) };
        put_half(record.address() + 40, value as i16);
        record.save(selected());
        h75a414(selected().0, value as i16 as i32);
        for index in 0..5 {
            let first = h751b74(selected().0);
            let second = h752a40(selected().0, 0);
            save_pair(index, first, second);
        }
        if matches!(state.prepared_mode, 0x2e | 0x2f | 0x36 | 0x38) {
            save_pair(2, state.request_first, state.request_second);
        }
        let current = selected();
        let active = word(PROFILE + 0x6410) != 0;
        let category = word(PROFILE + 0x6414) as u32 as usize;
        let value = h752990(current.0, 0);
        let p = if active { 0x016aedc8 + category * 0x8dc } else { 0x016ad7a0 };
        let saved = half(p);
        h75a324(current.0, value);
        h75a374(current.0, i32::from(saved));
    }
    if matches!(state.mode, 0 | 6) && word(JOB + 0x126c) == 0 {
        let first = h751b74(selected().0);
        let second = h752a40(selected().0, 0);
        save_pair(0, first, second);
    }
    let saved_geometry = (state.active || state.mode <= 2) && word(PROFILE + 0x6410) != 0;
    if saved_geometry { publish_saved_geometry(); }
    if copied { f602ec(state.owner.0 as i32, PROFILE as i32, 0x016b4f24, 0x016b3e00, 0x016a026c); }
    if saved_geometry { f6030c(state.owner.0 as i32, PROFILE as i32, 0x016a026c); }
    if !state.active && state.mode < 3 && state.count != 0 {
        let mut slot = Slot(0);
        loop {
            let mut record = [0i32; 42];
            h752928(record.as_mut_ptr() as i32, selected().0, i32::from(slot.0));
            h75ae78(word(state.item_records.0 + usize::from(slot.0) * 4), record.as_mut_ptr() as i32);
            slot.0 = slot.0.wrapping_add(1);
            if u32::from(slot.0) >= state.count { break; }
        }
    }
    let mut record = [0i32; 42];
    h752928(record.as_mut_ptr() as i32, selected().0, 0);
    put(0x016ad738, record[1]);
    put(TRACE + 12, record[1]);
    if state.mode == 7 {
        if (word(PROFILE + 0x65f0) as u32) < 2 {
            let record = TimingRecord::read(selected());
            let first = record.words[11];
            let second = h752a40(selected().0, 0);
            save_pair(4, first, second);
        }
        let current = selected();
        let args = CallbackArguments::read(current);
        if word(PROFILE + 0x65f0) == 0 { save_pair(2, args.geometry, args.target); }
        save_pair(1, args.geometry, args.target);
        let callback = h756b9c(current.0) as u32 as usize;
        if callback != 0 {
            args.invoke(callback);
            args.store(callback, Address(payload as usize));
            task_lock();
            copy(0x016a04a8, payload as usize, 0x30);
            task_unlock();
        }
    } else if state.active && word(JOB + 0x126c) == 0 { efa3f8(selected().0, 4); }
}
