//! X-M1 1.50 actual73964c projection through the shared admitted address space.

use crate::capture_profile_bindings::*;
use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

#[derive(Clone, Copy)]
pub struct CaptureSettings { pub(crate) address: GuestAddress }

impl CaptureSettings {
    pub fn new(address: GuestAddress) -> Result<Self, MemoryError> {
        address.checked_add(999)?;
        Ok(Self { address })
    }
    pub(crate) fn word<M: GuestAddressSpace, E>(&self, context: &mut RecordContext<M, E>, index: u32) -> Result<u32, MemoryError> {
        context.read_u32(self.address.checked_add(index * 4)?)
    }
    fn byte<M: GuestAddressSpace, E>(&self, context: &mut RecordContext<M, E>, offset: u32) -> Result<u32, MemoryError> {
        Ok(context.read_u8(self.address.checked_add(offset)?)? as u32)
    }
    fn signed_byte<M: GuestAddressSpace, E>(&self, context: &mut RecordContext<M, E>, offset: u32) -> Result<u32, MemoryError> {
        Ok(self.byte(context, offset)? as u8 as i8 as i32 as u32)
    }
}

#[derive(Clone, Copy)]
pub struct PrimaryScratch {
    pub values: GuestAddress,
    pub comparison: GuestAddress,
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ConversionCode(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ProfileOption(pub u32);

pub struct PrimaryConfiguration {
    pub settings: CaptureSettings,
    pub outputs: Option<GuestAddress>,
    pub conversion: ConversionCode,
    pub burst: ProfileOption,
    pub option: ProfileOption,
    pub state: ProfileOption,
    pub config65c: ProfileOption,
    pub config678: ProfileOption,
    pub config_extra: ProfileOption,
}

pub fn suppress_numeric_selection(category: u32, config65c: ProfileOption, config678: ProfileOption) -> bool {
    matches!(category, 0x1d | 1 | 0x1a | 0x19 | 0x20 | 5 | 0x18 | 8 | 0xb | 0xd | 0xc | 0x12 | 0x13 | 0x11 | 0x10 | 0x14 | 0x16 | 0x15 | 0x17 | 0x2c)
        || config65c.0 == 1 || config678.0 == 1
}

pub fn project_primary<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: GuestAddress, configuration: PrimaryConfiguration, scratch: PrimaryScratch) -> Result<u32, MemoryError> {
    let p = profile;
    let s = configuration.settings;
    let a = configuration.config65c.0;
    let b = configuration.config678.0;
    macro_rules! call {
        ($helper:ident, ) => {{  $helper(context)? }};
        ($helper:ident, $a0:expr) => {{ let a0 = $a0; $helper(context, a0)? }};
        ($helper:ident, $a0:expr, $a1:expr) => {{ let a0 = $a0; let a1 = $a1; $helper(context, a0, a1)? }};
        ($helper:ident, $a0:expr, $a1:expr, $a2:expr) => {{ let a0 = $a0; let a1 = $a1; let a2 = $a2; $helper(context, a0, a1, a2)? }};
        ($helper:ident, $a0:expr, $a1:expr, $a2:expr, $a3:expr) => {{ let a0 = $a0; let a1 = $a1; let a2 = $a2; let a3 = $a3; $helper(context, a0, a1, a2, a3)? }};
        ($helper:ident, $a0:expr, $a1:expr, $a2:expr, $a3:expr, $a4:expr) => {{ let a0 = $a0; let a1 = $a1; let a2 = $a2; let a3 = $a3; let a4 = $a4; $helper(context, a0, a1, a2, a3, a4)? }};
    }
    macro_rules! update {
        ($get:ident, $set:ident, $value:expr) => {{
            let previous = $get(context, p)?;
            let value = $value;
            if previous != value { $set(context, p, value)?; }
        }};
    }
    macro_rules! repeat {
        ($get:ident, $set:ident, $value:expr) => {{
            let previous = $get(context, p)?;
            if previous != $value { let value = $value; $set(context, p, value)?; }
        }};
    }
    update!(c75b054, c75b06c, call!(c736178, s.word(context, 0)?, b));
    update!(c75b0bc, c75b0d4, call!(c736694, configuration.config_extra.0, s.word(context, 0)?, a, b));
    update!(c75b088, c75b0a0, call!(c7362dc, s.word(context, 0)?, s.word(context, 0x24)?, a, configuration.burst.0, s.word(context, 1)?));
    update!(c75b0f0, c75b108, call!(c7366e8, s.word(context, 3)?, s.word(context, 0)?));
    update!(c75b1c0, c75b1d8, call!(c736770, s.word(context, 0)?));
    update!(c75b1f4, c75b20c, call!(c736790, s.word(context, 0x25)?));
    update!(c75b228, c75b240, call!(c7367b0, s.word(context, 0)?, s.word(context, 0x51)?, s.word(context, 0x15)?, b));
    repeat!(c75b25c, c75b274, {
        let category = s.word(context, 0)?;
        let index = if call!(c72dd38, category, b) == 0 { 0x15 } else { 0x14 };
        call!(c7367ec, category, s.word(context, index)?, s.word(context, 0x19)?, a, b)
    });
    call!(c7368c4, scratch.values, s.word(context, 0x18)?);
    call!(c735d54, p, scratch.values);
    update!(c75bd0c, c75bd24, call!(c73693c, s.word(context, 0x19)?));
    update!(c75b31c, c75b334, call!(c736960, s.word(context, 0x22)?));
    update!(c75b350, c75b368, call!(c7369c4, s.word(context, 0x1a)?));
    update!(c75b384, c75b39c, call!(c736a1c, s.word(context, 0x1d)?));
    update!(c75b3b8, c75b3d0, call!(c736a48, s.word(context, 0x1e)?));
    update!(c75b454, c75b46c, call!(c736a74, s.word(context, 0x20)?));
    repeat!(c75b488, c75b4a0, call!(c736adc, call!(c72d210, s.word(context, 0x21)?, s.word(context, 3)?), s.word(context, 0)?, b));
    update!(c75b4f0, c75b508, call!(c736bc4, s.word(context, 5)?, s.word(context, 8)?, s.word(context, 6)?));
    update!(c75b5f4, c75b60c, call!(c736d18, s.word(context, 5)?));
    update!(c75b58c, c75b5a4, call!(c736d5c, s.word(context, 7)?));
    update!(c75b5c0, c75b5d8, call!(c736d3c, s.word(context, 6)?));
    update!(c75b524, c75b53c, call!(c736d7c, s.word(context, 9)?));
    update!(c75b558, c75b570, call!(c736e3c, ));
    repeat!(c75b628, c75b640, call!(c736e54, s.word(context, 10)?));
    update!(c75b65c, c75b674, call!(c736eb0, s.word(context, 0x23)?));
    update!(c75b690, c75b6a8, call!(c736f70, s.signed_byte(context, 0x98)?));
    repeat!(c75b6c4, c75b6dc, call!(c736f84, s.word(context, 0x27)?));
    if call!(c731b90, s.word(context, 0)?, s.word(context, 1)?) == 1 {
        repeat!(c75b6f8, c75b710, call!(c736f98, configuration.conversion.0));
    } else {
        let previous = call!(c75b6f8, p);
        if call!(c75b6f8, p) != previous { call!(c75b710, p, previous); }
    }
    repeat!(c75b760, c75b778, call!(c736fac, s.word(context, 0x1f)?));
    if s.word(context, 3)? == 8 && s.word(context, 0xcf)? == 8 {
        repeat!(c75b794, c75b7ac, call!(c736fac, 10));
    } else {
        repeat!(c75b794, c75b7ac, call!(c736fac, s.word(context, 0xcf)?));
    }
    update!(c75ba68, c75ba80, call!(c737154, s.word(context, 0x1f)?));
    update!(c75b7c8, c75b7e0, call!(c7371b0, s.word(context, 4)?));
    update!(c75b7fc, c75b814, call!(c7371dc, s.word(context, 0x13)?));
    update!(c75b8e4, c75b8fc, call!(c737830, s.word(context, 0x13)?, configuration.option.0));
    if matches!(s.word(context, 0x13)?, 5 | 2) {
        call!(c737278, scratch.values, s.signed_byte(context, 0xb6)?, s.signed_byte(context, 0xb7)?);
    } else {
        call!(c75b830, scratch.values, p);
    }
    call!(c735dac, p, scratch.values, scratch.comparison);
    update!(c75b87c, c75b894, call!(c73729c, s.word(context, 0x45)?));
    update!(c75b8b0, c75b8c8, call!(c7372bc, s.word(context, 0x46)?));
    update!(c75b94c, c75b964, call!(c7372d8, s.word(context, 0xb)?));
    let color_index = match s.word(context, 0xb)? { 8 => 0xf, 9 => 0x11, _ => 0xd };
    update!(c75b980, c75b998, call!(c737378, s.word(context, color_index)?));
    update!(c75b9b4, c75b9cc, call!(c737378, s.word(context, color_index + 1)?));
    if let Some(outputs) = configuration.outputs {
        context.copy_bytes(GuestAddress::new(0x0091_34dc), outputs, ByteCount(12))?;
    }
    call!(c73747c, scratch.values, context.read_u16(GuestAddress::new(0x0091_34dc))? as u32, context.read_u16(GuestAddress::new(0x0091_34de))? as u32, context.read_u16(GuestAddress::new(0x0091_34e0))? as u32);
    call!(c735df8, p, scratch.values, scratch.comparison);
    update!(c75ba9c, c75bab4, call!(c7375c0, s.word(context, 0x4a)?));
    update!(c75bb38, c75bb50, { let _ = s.word(context, 0x4d)?; call!(c7375ec, ) });
    update!(c75bb6c, c75bb84, call!(c737604, s.word(context, 0x53)?));
    update!(c75bad0, c75bae8, call!(c737620, s.word(context, 0x4c)?));
    update!(c75bb04, c75bb1c, call!(c737640, s.word(context, 0x57)?));
    update!(c75bbd4, c75bbec, call!(c737660, s.word(context, 0)?, a));
    update!(c75bca4, c75bcbc, call!(c7377d0, s.word(context, 0)?, a, b));
    repeat!(c75bba0, c75bbb8, {
        let category = s.word(context, 0)?;
        let index = if call!(c72dd38, category, b) == 0 { 0x15 } else { 0x14 };
        call!(c7377e8, category, s.word(context, index)?, s.word(context, 0x51)?, s.word(context, 0xb8)?, b)
    });
    update!(c75b158, c75b170, call!(c737df8, s.word(context, 0)?, s.word(context, 3)?, s.word(context, 0xbf)?));
    update!(c75b124, c75b13c, call!(c737ed8, s.word(context, 0)?, s.word(context, 3)?, s.word(context, 0xbe)?));
    update!(c75bc70, c75bc88, call!(c737ff0, s.word(context, 0xc1)?));
    update!(c75bd40, c75bd58, (s.word(context, 199)? == 1 || s.word(context, 0)? == 0x1d) as u32);
    update!(c75bda8, c75bdc0, call!(c738010, s.word(context, 0xc4)?));
    update!(c75b3ec, c75b404, call!(c738030, s.word(context, 0xc2)?));
    repeat!(c75b918, c75b930, call!(c738090, s.word(context, 0x2e)?));
    update!(c75b18c, c75b1a4, call!(c7380fc, s.word(context, 0x1c)?));
    update!(c75b2e8, c75b300, call!(c738134, s.word(context, 0xc6)?));
    update!(c75bcd8, c75bcf0, call!(c738154, s.word(context, 0x59)?));
    update!(c75b420, c75b438, call!(c7381ac, s.word(context, 0xc3)?));
    if s.word(context, 0xb)? == 10 { update!(c75ba34, c75ba4c, call!(c738204, s.word(context, 0xc)?)); }
    if suppress_numeric_selection(s.word(context, 0)?, configuration.config65c, configuration.config678) {
        if call!(c75b72c, p) != 0x7fff { call!(c75b744, p, 0x7fff); }
    } else {
        repeat!(c75b72c, c75b744, call!(c738394, s.word(context, 0x28)?));
    }
    update!(c75be10, c75be28, call!(c73843c, s.word(context, 0x13)?, s.byte(context, 0x338)?) & 0xff);
    call!(c73845c, scratch.values, s.signed_byte(context, 0x339)?, s.signed_byte(context, 0x33a)?);
    call!(c73611c, p, scratch.values);
    update!(c75bed4, c75beec, call!(c738480, s.byte(context, 0x338)?));
    update!(c75dfe0, c75dff8, call!(c737258, s.word(context, 0x13)?, s.byte(context, 0x338)?) & 0xff);
    update!(c75bddc, c75bdf4, call!(c73849c, s.word(context, 0x35)?, s.word(context, 0xec)?, s.word(context, 0x3f)?, s.word(context, 0x13)?, s.word(context, 0xee)?));
    update!(c75bd74, c75bd8c, call!(c7384b4, s.word(context, 0)?));
    update!(c75bc08, c75bc20, call!(c7384d4, s.word(context, 0x4b)?, s.byte(context, 0x338)?, s.word(context, 0)?, s.word(context, 0xf7)?));
    update!(c75be44, c75be5c, call!(c738510, s.word(context, 0x38)?));
    update!(c75bc3c, c75bc54, { let _ = s.word(context, 0xd2)?; call!(c738530, ) });
    update!(c75bf08, c75bf20, { let _ = s.word(context, 0xd0)?; call!(c738548, ) });
    update!(c75bf3c, c75bf54, { let _ = s.word(context, 0xd3)?; call!(c738560, ) });
    update!(c75b4bc, c75b4d4, call!(c736b5c, s.word(context, 0xd4)?));
    update!(c75bfa4, c75bfbc, call!(c738578, s.word(context, 0x4f)?));
    update!(c75e07c, c75e094, call!(c73859c, s.word(context, 0xe4)?));
    update!(c75e0b0, c75e0c8, call!(c7385c8, s.word(context, 0xe5)?));
    update!(c75e014, c75e02c, call!(c7385e8, s.word(context, 0xc5)?));
    update!(c75bf70, c75bf88, call!(c738608, s.word(context, 0xe3)?));
    update!(c75e048, c75e060, { let _ = (s.word(context, 0xec)?, s.word(context, 0xed)?, s.word(context, 0)?, b); call!(c738638, ) });
    update!(c75deac, c75dec4, call!(c738650, s.word(context, 0xe7)?));
    update!(c75dee0, c75def8, call!(c738670, s.word(context, 0xe8)?));
    call!(c7386e0, p, s.address, scratch.values);
    let previous = call!(c75dfac, p);
    let value = s.signed_byte(context, 0x3ac)?;
    Ok(if previous != value { call!(c75dfc4, p, value) } else { previous })
}

pub fn helper_73964c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: GuestAddress, settings: GuestAddress, outputs: GuestAddress, conversion: u32, burst: u32, option: u32, state: u32, config65c: u32, config678: u32, config_extra: u32, scratch: PrimaryScratch) -> Result<u32, MemoryError> {
    project_primary(context, profile, PrimaryConfiguration {
        settings: CaptureSettings::new(settings)?,
        outputs: if outputs.get() == 0 { None } else { outputs.checked_add(11)?; Some(outputs) },
        conversion: ConversionCode(conversion), burst: ProfileOption(burst), option: ProfileOption(option), state: ProfileOption(state),
        config65c: ProfileOption(config65c), config678: ProfileOption(config678), config_extra: ProfileOption(config_extra),
    }, scratch)
}
