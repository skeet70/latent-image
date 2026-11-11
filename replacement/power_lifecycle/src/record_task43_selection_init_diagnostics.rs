use super::boundary::*;

#[unsafe(export_name = "xm1_power_record_helper_f169ec")]
pub unsafe extern "C" fn apply_active_owner() {
    unsafe {
        let destination = buffer();
        if resource42(profile().0) == 1 { f48f54(destination.0); }
    }
}

#[unsafe(export_name = "xm1_power_record_helper_f16a24")]
pub unsafe extern "C" fn restore_owner_buffer() {
    unsafe { f48f1c(buffer().0); }
}

#[unsafe(export_name = "xm1_power_record_helper_f16a40")]
pub unsafe extern "C" fn apply_owner_setting() {
    unsafe {
        let destination = buffer();
        let value = owner_setting(profile().0);
        if property(0xa8) & 0x80 != 0 { f48fd8(destination.0, value); }
    }
}

#[unsafe(export_name = "xm1_power_record_helper_f16a88")]
pub unsafe extern "C" fn apply_resource_mode() {
    unsafe {
        let destination = buffer();
        if resource5a(profile().0).wrapping_sub(4) > 2 { return; }
        f48e74(destination.0);
        if resource42(Address(0x16c21fc).word()) == 1 { f47468(); }
    }
}

#[derive(Clone, Copy, Default)]
struct Average { first: u64, second: u64, count: u8 }
impl Average {
    fn add(&mut self, first: u32, second: u32) {
        self.first += u64::from(first);
        self.second += u64::from(second);
        self.count += 1;
    }
    fn values(self) -> (u32, u32) {
        if self.count == 0 { (0, 0) }
        else { ((self.first / u64::from(self.count)) as u32, (self.second / u64::from(self.count)) as u32) }
    }
}

#[unsafe(export_name = "xm1_power_record_helper_f16ae0")]
pub unsafe extern "C" fn export_diagnostics() {
    unsafe {
        let destination = buffer();
        let mut records = SharedRecords::default();
        read_records(profile().0, &mut records);
        destination.offset(0x6c3c0).set(0x53545253);
        destination.offset(0x6cd5c).set(0x4c415253);
        destination.offset(0x6c3c4).set(records.present);
        if records.present == 0 { return; }
        destination.offset(0x6c3c8).set(records.count & 0xff);
        for record in 0..5 {
            let source = Address(records.records).offset(record * 0x228);
            let column = destination.offset(0x6c3cc + record * 4);
            let present = source.word();
            column.set(present);
            if present == 0 { continue; }
            column.offset(0x884).set(source.offset(0x224).word());
            for (offset, field, signed) in [(0x898, 0x204, false), (0x8ac, 0x206, false), (0x8c0, 0x208, true), (0x8d4, 0x20a, true)] {
                let value = source.offset(field).half();
                column.offset(offset).set(if signed { value as i16 as i32 as u32 } else { u32::from(value) });
            }
            for (offset, field) in [(0x8e8, 0x210), (0x8fc, 0x20c), (0x910, 0x218), (0x924, 0x214), (0x938, 0x220), (0x94c, 0x21c)] {
                column.offset(offset).set(source.offset(field).word());
            }
            let mut averages = [Average::default(); 5];
            for row in 0u32..7 {
                for col in 0u32..7 {
                    let sample = row * 8 + col;
                    let first = source.offset(4 + sample * 4).word();
                    let second = source.offset(0x104 + sample * 4).word();
                    let output = (row * 7 + col) * 5 + record;
                    destination.offset(0x6c3e0 + output * 4).set(first);
                    destination.offset(0x6c7b4 + output * 4).set(second);
                    if (2..=5).contains(&row) && (1..=5).contains(&col) {
                        averages[4].add(first, second);
                        if row <= 4 && (2..=4).contains(&col) {
                            averages[3].add(first, second);
                            if row == 3 { averages[2].add(first, second); }
                            if col == 3 { averages[1].add(first, second); }
                            if row == 3 && col == 3 { averages[0].add(first, second); }
                        }
                    }
                }
            }
            for (index, average) in averages.into_iter().enumerate() {
                let output = destination.offset(0x6cb88 + record * 4 + index as u32 * 20);
                let (first, second) = average.values();
                output.set(first);
                output.offset(0x64).set(second);
            }
        }
        for offset in (0x6cd2c..0x6cd5c).step_by(4) { destination.offset(offset).set(0); }
    }
}
