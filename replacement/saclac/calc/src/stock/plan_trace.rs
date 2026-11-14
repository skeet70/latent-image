use crate::adjust::motion::drive::Planning;
use crate::adjust::motion::plan::DepthSettings;
use crate::adjust::motion::state::MotionState;
use crate::exposure::ports::{Properties, Property, StockMemory};
use crate::exposure::settings;
use xm1_saclac_interface::calc::SharedAddress;

#[derive(Clone, Copy)]
#[repr(u32)]
pub enum Trace {
    Start = 0x44534,
    End = 0x44538,
    Position = 0x4453c,
    Lead = 0x44578,
    Weights = 0x44b40,
    FocusStatus = 0x44ba8,
    MotionStatus = 0x44bac,
    Area = 0x44bb0,
    Class = 0x44bb4,
    Found = 0x44bb8,
    Searching = 0x44bbc,
    Kind = 0x44bc0,
    ToneScanned = 0x44bc4,
    Tone = 0x44bcc,
    ToneLow = 0x44bd0,
    ToneHigh = 0x44bd4,
    ToneFirst = 0x44bd8,
    ToneSecond = 0x44bdc,
    ToneDriven = 0x44be0,
    ToneHeld = 0x44be4,
    ResultDecision = 0x44be8,
    Fast = 0x44bec,
    Decision = 0x44bf0,
    NextWide = 0x44bf8,
    Layout = 0x44c78,
    PrimaryShare = 0x44c7c,
    SecondaryShare = 0x44c80,
    ConfirmationHigh = 0x45698,
    ConfirmationLow = 0x45694,
    FrameDelay = 0x4569c,
    ReuseReason = 0x45db0,
    ReuseLevelKind = 0x45db4,
    ReuseLevel = 0x45db8,
    ReuseReference = 0x45dbc,
    ReuseStep = 0x45dc0,
    SecondaryAreaShare = 0x46a04,
    ScanFollow = 0x46934,
    ScanResult = 0x46938,
}

pub const fn at(base: SharedAddress, offset: u32) -> SharedAddress {
    SharedAddress(base.0.wrapping_add(offset))
}
pub fn store(p: &mut impl StockMemory, section: SharedAddress, field: Trace, value: u32) {
    p.write_u32(at(section, field as u32), value);
}
pub fn load(p: &mut impl StockMemory, section: SharedAddress, field: Trace) -> u32 {
    p.read_u32(at(section, field as u32))
}

pub fn speed(p: &mut impl StockMemory, section: SharedAddress, selector: u8, values: [u16; 4]) {
    for (base, value) in [0x44544, 0x44550, 0x4455c, 0x44568].into_iter().zip(values) {
        p.write_u32(
            at(section, base + u32::from(selector) * 4),
            u32::from(value),
        );
    }
}

pub fn target(p: &mut impl StockMemory, results: SharedAddress, reason: u32) {
    p.write_u32(at(results, 0x974), reason);
}
pub fn target_details(p: &mut impl StockMemory, results: SharedAddress, values: [u32; 4]) {
    for (i, value) in values.into_iter().enumerate() {
        p.write_u32(at(results, 0x978 + i as u32 * 4), value);
    }
}

pub fn drive(p: &mut impl StockMemory, section: SharedAddress, start_time: i32, plan: Planning) {
    let values = [
        start_time as u32,
        plan.first_acceleration,
        plan.second_acceleration,
        plan.requested_speed,
        plan.first_speed,
        u32::from(plan.distance),
        u32::from(plan.first_distance),
        plan.first_duration,
        u32::from(plan.second_distance),
        plan.second_duration,
        u32::from(plan.cruise_distance),
        plan.cruise_duration,
    ];
    for (i, value) in values.into_iter().enumerate() {
        p.write_u32(at(section, 0x449f8 + i as u32 * 4), value);
    }
}

pub fn depth(p: &mut (impl Properties + StockMemory), section: SharedAddress, d: DepthSettings) {
    p.write_u32(at(section, 0x4683c), u32::from(d.halves[0]));
    for (offset, id, word, signed, factor) in [
        (0x46840, 0x9434, true, false, 1),
        (0x46844, 0x9444, true, false, 1),
        (0x46850, 0x942f, false, false, 1),
        (0x46854, 0x9430, true, true, 1),
        (0x46858, 0x9432, true, true, 1),
    ] {
        trace_property(p, section, offset, id, word, signed, factor);
    }
    p.write_u32(at(section, 0x4685c), u32::from(d.halves[1]));
    p.write_u32(at(section, 0x46860), u32::from(d.halves[2]));
    for (offset, id, word, signed, factor) in [
        (0x46864, 0x944d, false, false, 1),
        (0x46868, 0x944e, false, false, 1),
        (0x4686c, 0x944f, true, false, 1),
        (0x46870, 0x9451, false, false, 1),
        (0x46874, 0x9452, false, false, 1),
        (0x46878, 0x9428, false, false, 1),
        (0x4687c, 0x942a, false, false, 1),
        (0x46880, 0x944b, false, false, 1),
        (0x46884, 0x9455, true, false, 1),
        (0x46888, 0x9458, true, false, 1),
        (0x4688c, 0x945a, true, false, 1),
        (0x46890, 0x945e, true, false, 1),
        (0x46894, 0x945c, true, false, 1),
        (0x46898, 0x9463, false, false, 10),
        (0x4689c, 0x9464, false, false, 10),
        (0x468a0, 0x9465, false, false, 1),
        (0x468a4, 0x9466, false, false, 1),
        (0x46848, 0x92f7, true, false, 1),
        (0x4684c, 0x92f5, true, false, 1),
    ] {
        trace_property(p, section, offset, id, word, signed, factor);
    }
    p.fill(at(section, 0x459e4), 0x3cc, 0);
}

fn trace_property(
    p: &mut (impl Properties + StockMemory),
    section: SharedAddress,
    offset: u32,
    id: u32,
    word: bool,
    signed: bool,
    factor: u32,
) {
    let value = if word {
        let v = settings::word(p, Property(id));
        if signed {
            v as i16 as i32 as u32
        } else {
            u32::from(v)
        }
    } else {
        p.property(Property(id))
    };
    p.write_u32(at(section, offset), value.wrapping_mul(factor));
}

fn copy(
    p: &mut impl StockMemory,
    section: SharedAddress,
    destination: u32,
    source: u32,
    bytes: u32,
) {
    for offset in (0..bytes).step_by(4) {
        let value = p.read_u32(at(section, source + offset));
        p.write_u32(at(section, destination + offset), value);
    }
}

fn saved(
    p: &mut impl StockMemory,
    section: SharedAddress,
    destination: u32,
    source: u32,
    bytes: u32,
    sentinel: u8,
) {
    copy(p, section, destination, source, bytes);
    match sentinel {
        4 => p.write_u32(at(section, destination + bytes - 4), 0xfdfdfdfd),
        2 => p.write_u16(at(section, destination + bytes - 2), 0xfdfd),
        1 => p.write_u8(at(section, destination + bytes - 1), 0xfd),
        _ => {}
    }
}

pub fn history(p: &mut impl StockMemory, s: &MotionState) {
    let section = s.section;
    let pass = &s.plan;
    let values = [
        pass.result.position as i16 as i32 as u32,
        pass.profile.start as i32 as u32,
        pass.profile.end as i32 as u32,
        pass.layout.code(),
        pass.primary_share,
        pass.secondary_share,
        pass.result.decision,
        pass.result.fast,
        u32::from(pass.kind.0),
    ];
    for (i, value) in values.into_iter().enumerate() {
        p.write_u32(at(section, 0x45dc8 + i as u32 * 4), value);
    }
    let previous = p.read_u32(at(section, 0x44bf4));
    p.write_u32(at(section, 0x45dec), previous);
    p.write_u32(at(section, 0x45dc4), pass.result.kind);
    let values = [
        s.readings.request.at_10,
        pass.tone.words[1],
        pass.tone.words[0],
        pass.tone_limits[0],
        pass.tone_limits[1],
        pass.tone.words[11],
        pass.tone.words[12],
        pass.tone.words[13],
        pass.tone.words[14],
    ];
    for (i, value) in values.into_iter().enumerate() {
        p.write_u32(at(section, 0x46110 + i as u32 * 4), value);
    }
    for row in 0..5 {
        for column in 0..8 {
            copy(
                p,
                section,
                0x3a0c4 + column * 0x500 + row * 0x100,
                0x1d0a4 + column * 0x13b0 + row * 0x3f0,
                0x100,
            );
        }
        for column in 0..8 {
            p.write_u32(
                at(section, 0x3a1c0 + column * 0x500 + row * 0x100),
                0xfdfdfdfd,
            );
        }
    }
    for row in 0..9 {
        for column in 0..2 {
            copy(
                p,
                section,
                0x3c8c4 + row * 0x80 + column * 0x480,
                0x26e24 + row * 0x1f8 + column * 0x11b8,
                0x80,
            );
        }
        for column in 0..2 {
            p.write_u16(at(section, 0x3c942 + row * 0x80 + column * 0x480), 0xfdfd);
        }
    }
    for row in 0..9 {
        for column in 0..2 {
            copy(
                p,
                section,
                0x3d1c4 + row * 0x100 + column * 0x900,
                0x29194 + row * 0x3f0 + column * 0x2370,
                0x100,
            );
        }
        for column in 0..2 {
            p.write_u32(
                at(section, 0x3d2c0 + row * 0x100 + column * 0x900),
                0xfdfdfdfd,
            );
        }
        saved(
            p,
            section,
            0x3e3c4 + row * 0x80,
            0x2d874 + row * 0x1f8,
            0x80,
            2,
        );
    }
    for row in 0..14 {
        for column in 0..2 {
            copy(
                p,
                section,
                0x50014 + row * 0x100 + column * 0xe00,
                0x491a0 + row * 0x3f0 + column * 0x3720,
                0x100,
            );
        }
        for column in 0..2 {
            p.write_u32(
                at(section, 0x50110 + row * 0x100 + column * 0xe00),
                0xfdfdfdfd,
            );
        }
    }
    saved(p, section, 0x3e844, 0x2ea2c, 0x80, 2);
    saved(p, section, 0x3ebd4, 0x2ec24, 0x100, 4);
    for (destination, source, rows) in [(0x3ecd4, 0x2f014, 7), (0x3f3d4, 0x30ba4, 10)] {
        for row in 0..rows {
            saved(
                p,
                section,
                destination + row * 0x100,
                source + row * 0x3f0,
                0x100,
                4,
            );
        }
    }
    for row in 0..5 {
        saved(
            p,
            section,
            0x3fdd4 + row * 0x100,
            0x33304 + row * 0x3f0,
            0x100,
            4,
        );
    }
    for row in 5..9 {
        copy(
            p,
            section,
            0x3fdd4 + row * 0x100,
            0x33304 + row * 0x3f0,
            0x100,
        );
    }
    for row in 5..9 {
        p.write_u32(at(section, 0x3fed0 + row * 0x100), 0xfdfdfdfd);
    }
    for row in 0..12 {
        saved(
            p,
            section,
            0x43834 + row * 0x100,
            0x35984 + row * 0x3f0,
            0x100,
            4,
        );
    }
    for row in 0..4 {
        saved(
            p,
            section,
            0x44434 + row * 0x40,
            0x388c4 + row * 0x100,
            0x40,
            1,
        );
    }
    for row in 0..5 {
        for column in 0..2 {
            saved(
                p,
                section,
                0x42e34 + row * 0x100 + column * 0x500,
                0x406d4 + row * 0x3f0 + column * 0x13b0,
                0x100,
                4,
            );
        }
    }
    for row in 0..4 {
        copy(p, section, 0x3e8c4 + row * 0xc4, 0x35674 + row * 0xc4, 0xc4);
    }
    for row in 0..10 {
        for column in 0..2 {
            copy(
                p,
                section,
                0x38cc4 + row * 0x100 + column * 0xa00,
                0x181e4 + row * 0x3f0 + column * 0x2760,
                0x100,
            );
        }
        for column in 0..2 {
            p.write_u32(
                at(section, 0x38dc0 + row * 0x100 + column * 0xa00),
                0xfdfdfdfd,
            );
        }
    }
    copy(p, section, 0x46134, 0x456a8, 0x33c);
    copy(p, section, 0x46470, 0x459e4, 0x3cc);
    for column in 0..3 {
        for row in 0..4 {
            copy(
                p,
                section,
                0x45df0 + column * 4 + row * 12,
                0x44544 + column * 4 + row * 12,
                4,
            );
        }
    }
    for (destination, source, bytes) in [
        (0x46108, 0x44b98, 4),
        (0x4610c, 0x44b9c, 4),
        (0x45e20, 0x44578, 4),
        (0x45e24, 0x4457c, 4),
        (0x45e28, 0x44580, 0xc4),
        (0x45e44, 0x4459c, 0xc4),
        (0x45e60, 0x445b8, 0x28),
        (0x45e88, 0x445e0, 0x28),
    ] {
        copy(p, section, destination, source, bytes);
    }
    saved(p, section, 0x45eb0, 0x44608, 0x100, 4);
    for (destination, source, bytes) in [
        (0x45fb0, 0x449f8, 0x30),
        (0x45fe0, 0x44a28, 0x30),
        (0x46010, 0x44a58, 0x54),
        (0x46064, 0x44aac, 0x54),
    ] {
        copy(p, section, destination, source, bytes);
    }
    for row in 0..6 {
        copy(p, section, 0x460b8 + row * 12, 0x44b48 + row * 12, 12);
    }
    copy(p, section, 0x46100, 0x44b90, 8);
    copy(p, section, 0x51c14, 0x4ffe0, 0x34);
}
