#![forbid(unsafe_code)]

//! The record fields task 43's own accessors reach, by access group, with the accessors each one
//! replaces. `AT_` names are offsets whose meaning no caller has shown yet.

use crate::field::{ChangeBank, ChangeBit, RecordArray, RecordField, RecordRead, RecordWrite, Unread, Unwritten};
use crate::handle::AccessGroup;

/// Access group 0. The change-tracked header the staged record is applied to field by field, and the record
/// words around it.
pub mod group_00 {
    use super::*;

    /// Read by 0x74ec30; written by 0x74eca0.
    pub const AT_0000: RecordField<u32> = RecordField::grouped(0x0, 0, Some(ChangeBit::new(ChangeBank::First, 7)));

    /// Read by 0x74fab4; written by 0x74fb10.
    pub const AT_0004: RecordField<[u8; 8]> = RecordField::grouped(0x4, 0, Some(ChangeBit::new(ChangeBank::First, 26)));

    /// Read by 0x750b48; written by 0x750bbc.
    pub const AT_000C: RecordField<u16> = RecordField::grouped(0xc, 0, Some(ChangeBit::new(ChangeBank::First, 62)));

    /// Read by 0x74ff0c; written by 0x74ff68.
    pub const AT_000E: RecordField<[u8; 6]> = RecordField::grouped(0xe, 0, Some(ChangeBit::new(ChangeBank::First, 32)));

    /// Read by 0x74f674; written by 0x74f6b8.
    pub const AT_0014: RecordField<u32> = RecordField::grouped(0x14, 0, Some(ChangeBit::new(ChangeBank::First, 19)));

    /// Written by 0x74e9cc.
    pub const AT_0018: RecordField<u16, Unread, RecordWrite> = RecordField::new(0x18, Unread, RecordWrite::Granted(AccessGroup::at(0)), Some(ChangeBit::new(ChangeBank::First, 49)));

    /// Read by 0x74f454; written by 0x74f498.
    pub const AT_001A: RecordField<i16> = RecordField::grouped(0x1a, 0, Some(ChangeBit::new(ChangeBank::First, 15)));

    /// Read by 0x74f5ec; written by 0x74f630.
    pub const AT_001C: RecordField<i16> = RecordField::grouped(0x1c, 0, Some(ChangeBit::new(ChangeBank::First, 18)));

    /// Read by 0x74f6fc; written by 0x74f740.
    pub const AT_001E: RecordField<i16> = RecordField::grouped(0x1e, 0, Some(ChangeBit::new(ChangeBank::First, 20)));

    /// Read by 0x74f80c; written by 0x74f850.
    pub const AT_0020: RecordField<i16> = RecordField::grouped(0x20, 0, Some(ChangeBit::new(ChangeBank::First, 55)));

    /// Read by 0x74f894; written by 0x74f8d8.
    pub const AT_0022: RecordField<i16> = RecordField::grouped(0x22, 0, Some(ChangeBit::new(ChangeBank::First, 22)));

    /// Read by 0x74f91c; written by 0x74f960.
    pub const AT_0024: RecordField<i16> = RecordField::grouped(0x24, 0, Some(ChangeBit::new(ChangeBank::First, 23)));

    /// Read by 0x74e900; written by 0x74e944.
    pub const AT_0026: RecordField<u16> = RecordField::grouped(0x26, 0, Some(ChangeBit::new(ChangeBank::First, 48)));

    /// Read by 0x74fd74; written by 0x74fd30.
    pub const AT_0028: RecordField<u16> = RecordField::grouped(0x28, 0, Some(ChangeBit::new(ChangeBank::First, 45)));

    /// Read by 0x74ffac; written by 0x74fff0.
    pub const AT_002A: RecordField<u16> = RecordField::grouped(0x2a, 0, Some(ChangeBit::new(ChangeBank::First, 44)));

    /// Written by 0x750d10.
    pub const AT_002C: RecordField<u16, Unread, RecordWrite> = RecordField::new(0x2c, Unread, RecordWrite::Granted(AccessGroup::at(0)), Some(ChangeBit::new(ChangeBank::Second, 5)));

    /// Read by 0x74e718; written by 0x74e75c.
    pub const AT_002E: RecordField<u16> = RecordField::grouped(0x2e, 0, Some(ChangeBit::new(ChangeBank::Second, 15)));

    /// 0 none, 1 the record (FocusRound `ProfileSelection`).
    /// Written by 0x74e604.
    pub const MEASUREMENT_SELECTION: RecordField<u8, Unread, RecordWrite> = RecordField::new(0x30, Unread, RecordWrite::Granted(AccessGroup::at(0)), Some(ChangeBit::new(ChangeBank::First, 0)));

    /// Written by 0x74e6d4.
    pub const AT_0031: RecordField<u8, Unread, RecordWrite> = RecordField::new(0x31, Unread, RecordWrite::Granted(AccessGroup::at(0)), Some(ChangeBit::new(ChangeBank::First, 1)));

    /// Read by 0x74e7f0; written by 0x74e834.
    pub const AT_0032: RecordField<u8> = RecordField::grouped(0x32, 0, Some(ChangeBit::new(ChangeBank::First, 2)));

    /// Read by 0x74e878; written by 0x74e8bc.
    pub const AT_0033: RecordField<u8> = RecordField::grouped(0x33, 0, Some(ChangeBit::new(ChangeBank::First, 47)));

    /// Read by 0x74eba8; written by 0x74ebec.
    pub const AT_0034: RecordField<u8> = RecordField::grouped(0x34, 0, Some(ChangeBit::new(ChangeBank::First, 6)));

    /// Read by 0x74ece4; written by 0x74ed28.
    pub const AT_0035: RecordField<u8> = RecordField::grouped(0x35, 0, Some(ChangeBit::new(ChangeBank::First, 58)));

    /// Read by 0x74ed6c; written by 0x74edb0.
    pub const AT_0036: RecordField<u8> = RecordField::grouped(0x36, 0, Some(ChangeBit::new(ChangeBank::First, 8)));

    /// Read by 0x74edf4; written by 0x74ee38.
    pub const AT_0037: RecordField<u8> = RecordField::grouped(0x37, 0, Some(ChangeBit::new(ChangeBank::First, 9)));

    /// Read by 0x74ee7c; written by 0x74eec0.
    pub const AT_0038: RecordField<u8> = RecordField::grouped(0x38, 0, Some(ChangeBit::new(ChangeBank::First, 10)));

    /// Read by 0x74ef04; written by 0x74ef48.
    pub const AT_0039: RecordField<u8> = RecordField::grouped(0x39, 0, Some(ChangeBit::new(ChangeBank::First, 11)));

    /// Read by 0x74ef8c; written by 0x74efd0.
    pub const AT_003A: RecordField<u8> = RecordField::grouped(0x3a, 0, Some(ChangeBit::new(ChangeBank::First, 50)));

    /// Read by 0x74f014; written by 0x74f058.
    pub const AT_003B: RecordField<u8> = RecordField::grouped(0x3b, 0, Some(ChangeBit::new(ChangeBank::First, 51)));

    /// Read by 0x74f09c; written by 0x74f0e0.
    pub const AT_003C: RecordField<u8> = RecordField::grouped(0x3c, 0, Some(ChangeBit::new(ChangeBank::First, 12)));

    /// Read by 0x74f124; written by 0x74f168.
    pub const AT_003D: RecordField<u8> = RecordField::grouped(0x3d, 0, Some(ChangeBit::new(ChangeBank::First, 13)));

    /// Read by 0x74f1f0; written by 0x74f234.
    pub const AT_003E: RecordField<u8> = RecordField::grouped(0x3e, 0, Some(ChangeBit::new(ChangeBank::Second, 9)));

    /// Read by 0x74f1ac; written by 0x74f278.
    pub const AT_003F: RecordField<u8> = RecordField::grouped(0x3f, 0, Some(ChangeBit::new(ChangeBank::First, 14)));

    /// Read by 0x74f2bc; written by 0x74f300.
    pub const AT_0040: RecordField<u8> = RecordField::grouped(0x40, 0, Some(ChangeBit::new(ChangeBank::First, 53)));

    /// Read by 0x74f344; written by 0x74f388.
    pub const AT_0041: RecordField<u8> = RecordField::grouped(0x41, 0, Some(ChangeBit::new(ChangeBank::First, 54)));

    /// Read by 0x74f3cc; written by 0x74f410.
    pub const AT_0042: RecordField<u8> = RecordField::grouped(0x42, 0, Some(ChangeBit::new(ChangeBank::Second, 6)));

    /// Read by 0x74f564; written by 0x74f5a8.
    pub const AT_0043: RecordField<u8> = RecordField::grouped(0x43, 0, Some(ChangeBit::new(ChangeBank::First, 17)));

    /// Read by 0x74f784; written by 0x74f7c8.
    pub const AT_0044: RecordField<u8> = RecordField::grouped(0x44, 0, Some(ChangeBit::new(ChangeBank::First, 21)));

    /// Read by 0x750034; written by 0x750078.
    pub const AT_0045: RecordField<u8> = RecordField::grouped(0x45, 0, Some(ChangeBit::new(ChangeBank::First, 33)));

    /// Written by 0x74f9e8.
    pub const AT_0046: RecordField<u8, Unread, RecordWrite> = RecordField::new(0x46, Unread, RecordWrite::Granted(AccessGroup::at(0)), Some(ChangeBit::new(ChangeBank::First, 24)));

    /// Written by 0x74fa70.
    pub const AT_0047: RecordField<u8, Unread, RecordWrite> = RecordField::new(0x47, Unread, RecordWrite::Granted(AccessGroup::at(0)), Some(ChangeBit::new(ChangeBank::First, 25)));

    /// Read by 0x74fcec; written by 0x74fdb8.
    pub const AT_0048: RecordField<u8> = RecordField::grouped(0x48, 0, Some(ChangeBit::new(ChangeBank::First, 29)));

    /// Read by 0x74fdfc; written by 0x74fe40.
    pub const AT_0049: RecordField<u8> = RecordField::grouped(0x49, 0, Some(ChangeBit::new(ChangeBank::First, 30)));

    /// Read by 0x74fe84; written by 0x74fec8.
    pub const AT_004A: RecordField<u8> = RecordField::grouped(0x4a, 0, Some(ChangeBit::new(ChangeBank::First, 31)));

    /// Read by 0x7500bc; written by 0x750100.
    pub const AT_004B: RecordField<u8> = RecordField::grouped(0x4b, 0, Some(ChangeBit::new(ChangeBank::First, 34)));

    /// Read by 0x750144; written by 0x750188.
    pub const AT_004C: RecordField<u8> = RecordField::grouped(0x4c, 0, Some(ChangeBit::new(ChangeBank::First, 37)));

    /// Read by 0x7501cc; written by 0x750210.
    pub const AT_004D: RecordField<u8> = RecordField::grouped(0x4d, 0, Some(ChangeBit::new(ChangeBank::First, 38)));

    /// Read by 0x750254; written by 0x750298.
    pub const AT_004E: RecordField<u8> = RecordField::grouped(0x4e, 0, Some(ChangeBit::new(ChangeBank::First, 35)));

    /// Read by 0x7502dc; written by 0x750320.
    pub const AT_004F: RecordField<u8> = RecordField::grouped(0x4f, 0, Some(ChangeBit::new(ChangeBank::First, 36)));

    /// Read by 0x750364; written by 0x7503a8.
    pub const AT_0050: RecordField<u8> = RecordField::grouped(0x50, 0, Some(ChangeBit::new(ChangeBank::First, 43)));

    /// Read by 0x75060c; written by 0x750650.
    pub const AT_0051: RecordField<u8> = RecordField::grouped(0x51, 0, Some(ChangeBit::new(ChangeBank::First, 40)));

    /// Read by 0x750694; written by 0x7506d8.
    pub const AT_0052: RecordField<u8> = RecordField::grouped(0x52, 0, Some(ChangeBit::new(ChangeBank::First, 41)));

    /// Read by 0x7507a4; written by 0x7507e8.
    pub const AT_0053: RecordField<u8> = RecordField::grouped(0x53, 0, Some(ChangeBit::new(ChangeBank::First, 56)));

    /// Read by 0x75071c; written by 0x750760.
    pub const AT_0054: RecordField<u8> = RecordField::grouped(0x54, 0, Some(ChangeBit::new(ChangeBank::First, 52)));

    /// Written by 0x7508f8.
    pub const AT_0055: RecordField<u8, Unread, RecordWrite> = RecordField::new(0x55, Unread, RecordWrite::Granted(AccessGroup::at(0)), Some(ChangeBit::new(ChangeBank::Second, 1)));

    /// Read by 0x758cf0; written by 0x758d34.
    pub const AT_0056: RecordField<u8> = RecordField::grouped(0x56, 0, Some(ChangeBit::new(ChangeBank::Second, 10)));

    /// Read by 0x758d78; written by 0x758dbc.
    pub const AT_0057: RecordField<u8> = RecordField::grouped(0x57, 0, Some(ChangeBit::new(ChangeBank::Second, 11)));

    /// Read by 0x7590b8; written by 0x7590fc.
    pub const AT_0058: RecordField<u8> = RecordField::grouped(0x58, 0, Some(ChangeBit::new(ChangeBank::Second, 18)));

    /// Read by 0x759140; written by 0x759184.
    pub const AT_0059: RecordField<u8> = RecordField::grouped(0x59, 0, Some(ChangeBit::new(ChangeBank::Second, 19)));

    /// Read by 0x7591c8; written by 0x75920c.
    pub const AT_005A: RecordField<u8> = RecordField::grouped(0x5a, 0, Some(ChangeBit::new(ChangeBank::Second, 20)));

    /// Read by 0x74ea10; written by 0x74ea54.
    pub const AT_005B: RecordField<u8> = RecordField::grouped(0x5b, 0, Some(ChangeBit::new(ChangeBank::First, 3)));

    /// Written by 0x74eadc.
    pub const AT_005C: RecordField<u8, Unread, RecordWrite> = RecordField::new(0x5c, Unread, RecordWrite::Granted(AccessGroup::at(0)), Some(ChangeBit::new(ChangeBank::First, 4)));

    /// Read by 0x74eb20; written by 0x74eb64.
    pub const AT_005D: RecordField<u8> = RecordField::grouped(0x5d, 0, Some(ChangeBit::new(ChangeBank::First, 5)));

    /// Written by 0x74f520.
    pub const AT_005E: RecordField<u8, Unread, RecordWrite> = RecordField::new(0x5e, Unread, RecordWrite::Granted(AccessGroup::at(0)), Some(ChangeBit::new(ChangeBank::First, 16)));

    /// Read by 0x74fb54; written by 0x74fb98.
    pub const AT_005F: RecordField<u8> = RecordField::grouped(0x5f, 0, Some(ChangeBit::new(ChangeBank::First, 27)));

    /// Written by 0x74fc20.
    pub const AT_0060: RecordField<u8, Unread, RecordWrite> = RecordField::new(0x60, Unread, RecordWrite::Granted(AccessGroup::at(0)), Some(ChangeBit::new(ChangeBank::First, 28)));

    /// Written by 0x74fca8.
    pub const AT_0061: RecordField<u8, Unread, RecordWrite> = RecordField::new(0x61, Unread, RecordWrite::Granted(AccessGroup::at(0)), Some(ChangeBit::new(ChangeBank::First, 42)));

    /// What the profile does with the frame-interval history (FocusRound `IntervalTracking`).
    /// Read by 0x750474; written by 0x7504b8.
    pub const INTERVAL_TRACKING: RecordField<u8> = RecordField::grouped(0x62, 0, Some(ChangeBit::new(ChangeBank::Second, 0)));

    /// Written by 0x750430.
    pub const AT_0063: RecordField<u8, Unread, RecordWrite> = RecordField::new(0x63, Unread, RecordWrite::Granted(AccessGroup::at(0)), Some(ChangeBit::new(ChangeBank::First, 39)));

    /// Read by 0x75093c; written by 0x750980.
    pub const AT_0064: RecordField<u8> = RecordField::grouped(0x64, 0, Some(ChangeBit::new(ChangeBank::First, 46)));

    /// Read by 0x750584; written by 0x7505c8.
    pub const AT_0065: RecordField<u8> = RecordField::grouped(0x65, 0, Some(ChangeBit::new(ChangeBank::First, 57)));

    /// Read by 0x7509c4; written by 0x750a08.
    pub const AT_0066: RecordField<u8> = RecordField::grouped(0x66, 0, Some(ChangeBit::new(ChangeBank::First, 59)));

    /// Read by 0x750ac0; written by 0x750b04.
    pub const AT_0067: RecordField<u8> = RecordField::grouped(0x67, 0, Some(ChangeBit::new(ChangeBank::First, 60)));

    /// Read by 0x750c00; written by 0x750c44.
    pub const AT_0068: RecordField<u8> = RecordField::grouped(0x68, 0, Some(ChangeBit::new(ChangeBank::First, 61)));

    /// Read by 0x750c88; written by 0x750ccc.
    pub const AT_0069: RecordField<u8> = RecordField::grouped(0x69, 0, Some(ChangeBit::new(ChangeBank::Second, 4)));

    /// Written by 0x750540.
    pub const AT_006A: RecordField<u8, Unread, RecordWrite> = RecordField::new(0x6a, Unread, RecordWrite::Granted(AccessGroup::at(0)), Some(ChangeBit::new(ChangeBank::Second, 3)));

    /// Read by 0x75082c; written by 0x750870.
    pub const AT_006B: RecordField<u8> = RecordField::grouped(0x6b, 0, Some(ChangeBit::new(ChangeBank::Second, 2)));

    /// Read by 0x750d54; written by 0x750e20.
    pub const AT_006C: RecordField<u8> = RecordField::grouped(0x6c, 0, Some(ChangeBit::new(ChangeBank::Second, 7)));

    /// Read by 0x750d98; written by 0x750ddc.
    pub const AT_006D: RecordField<u8> = RecordField::grouped(0x6d, 0, Some(ChangeBit::new(ChangeBank::Second, 8)));

    /// Read by 0x759030; written by 0x759074.
    pub const AT_006E: RecordField<u8> = RecordField::grouped(0x6e, 0, Some(ChangeBit::new(ChangeBank::Second, 17)));

    /// Read by 0x75a178.
    pub const AT_006F: RecordField<u8, RecordRead, Unwritten> = RecordField::new(0x6f, RecordRead(AccessGroup::at(0)), Unwritten, None);

    /// Read by 0x758fa8; written by 0x758fec.
    pub const AT_0070: RecordField<u8> = RecordField::grouped(0x70, 0, Some(ChangeBit::new(ChangeBank::Second, 16)));

    /// Read by 0x758f20; written by 0x758f64.
    pub const AT_0071: RecordField<i8> = RecordField::grouped(0x71, 0, Some(ChangeBit::new(ChangeBank::Second, 14)));

    /// Read by 0x758e00; written by 0x758e4c.
    pub const AT_0072: RecordField<[u8; 4]> = RecordField::grouped(0x72, 0, Some(ChangeBit::new(ChangeBank::Second, 12)));

    /// Read by 0x758e90; written by 0x758edc.
    pub const AT_0076: RecordField<[u8; 4]> = RecordField::grouped(0x76, 0, Some(ChangeBit::new(ChangeBank::Second, 13)));

    /// Written by 0x74e108.
    pub const AT_0160_12: RecordField<[u8; 12], Unread, RecordWrite> = RecordField::new(0x160, Unread, RecordWrite::Granted(AccessGroup::at(0)), None);

    /// Written by 0x74e0b8.
    pub const AT_20B0_104: RecordField<[u8; 104], Unread, RecordWrite> = RecordField::new(0x20b0, Unread, RecordWrite::Granted(AccessGroup::at(0)), None);

    /// Written by 0x74e154.
    pub const AT_2118: RecordField<[u8; 40], Unread, RecordWrite> = RecordField::new(0x2118, Unread, RecordWrite::Granted(AccessGroup::at(0)), None);

    /// Read by 0x74e520.
    pub const AT_2128: RecordField<u32, RecordRead, Unwritten> = RecordField::new(0x2128, RecordRead(AccessGroup::at(0)), Unwritten, None);
}

/// Access group 1.
pub mod group_01 {
    use super::*;

    /// Read by 0x750e64; written by 0x750eb0.
    pub const AT_0090_2: RecordField<i16> = RecordField::grouped(0x90, 1, None);

    /// Read by 0x750a4c; written by 0x74e1a4.
    pub const AT_0090_56: RecordField<[u8; 56]> = RecordField::grouped(0x90, 1, None);

    /// Read by 0x750efc; written by 0x750f48.
    pub const AT_0092: RecordField<i16> = RecordField::grouped(0x92, 1, None);

    /// Read by 0x750f94; written by 0x750fe0.
    pub const AT_0094: RecordField<i16> = RecordField::grouped(0x94, 1, None);

    /// Read by 0x75102c; written by 0x751078.
    pub const AT_0096: RecordField<i16> = RecordField::grouped(0x96, 1, None);

    /// Written by 0x751110.
    pub const AT_0098: RecordField<u16, Unread, RecordWrite> = RecordField::new(0x98, Unread, RecordWrite::Granted(AccessGroup::at(1)), None);

    /// Written by 0x7511a8.
    pub const AT_009A: RecordField<u16, Unread, RecordWrite> = RecordField::new(0x9a, Unread, RecordWrite::Granted(AccessGroup::at(1)), None);

    /// Read by 0x7511f4; written by 0x751240.
    pub const AT_009C: RecordField<i16> = RecordField::grouped(0x9c, 1, None);

    /// Written by 0x7512d8.
    pub const AT_009E: RecordField<u16, Unread, RecordWrite> = RecordField::new(0x9e, Unread, RecordWrite::Granted(AccessGroup::at(1)), None);

    /// Read by 0x751324; written by 0x751370.
    pub const AT_00A0: RecordField<i16> = RecordField::grouped(0xa0, 1, None);

    /// Written by 0x751408.
    pub const AT_00A2: RecordField<u16, Unread, RecordWrite> = RecordField::new(0xa2, Unread, RecordWrite::Granted(AccessGroup::at(1)), None);

    /// Read by 0x751454; written by 0x7514a0.
    pub const AT_00A4: RecordField<i16> = RecordField::grouped(0xa4, 1, None);

    /// Read by 0x7514ec; written by 0x751538.
    pub const AT_00A6: RecordField<i16> = RecordField::grouped(0xa6, 1, None);

    /// Read by 0x751584; written by 0x7515d0.
    pub const AT_00A8: RecordField<i16> = RecordField::grouped(0xa8, 1, None);

    /// Read by 0x75161c; written by 0x751668.
    pub const AT_00AA: RecordField<i16> = RecordField::grouped(0xaa, 1, None);

    /// Read by 0x7516b4; written by 0x751700.
    pub const AT_00AC: RecordField<i16> = RecordField::grouped(0xac, 1, None);

    /// Read by 0x75174c; written by 0x751798.
    pub const AT_00AE: RecordField<i16> = RecordField::grouped(0xae, 1, None);

    /// Read by 0x7517e4; written by 0x751830.
    pub const AT_00B0: RecordField<i16> = RecordField::grouped(0xb0, 1, None);

    /// Read by 0x75187c; written by 0x7518c8.
    pub const AT_00B2: RecordField<i16> = RecordField::grouped(0xb2, 1, None);

    /// Read by 0x751914; written by 0x751960.
    pub const AT_00B4: RecordField<i16> = RecordField::grouped(0xb4, 1, None);

    /// Read by 0x7519ac; written by 0x7519f8.
    pub const AT_00B6: RecordField<i16> = RecordField::grouped(0xb6, 1, None);

    /// Read by 0x751a44; written by 0x751a90.
    pub const AT_00B8: RecordField<i16> = RecordField::grouped(0xb8, 1, None);

    /// Read by 0x751adc; written by 0x751b28.
    pub const AT_00BA: RecordField<i16> = RecordField::grouped(0xba, 1, None);

    /// Read by 0x751b74; written by 0x751bc0.
    pub const AT_00BC: RecordField<u32> = RecordField::grouped(0xbc, 1, None);

    /// Written by 0x751c58.
    pub const AT_00C0: RecordField<u32, Unread, RecordWrite> = RecordField::new(0xc0, Unread, RecordWrite::Granted(AccessGroup::at(1)), None);

    /// Read by 0x751ca4; written by 0x751cf0.
    pub const AT_00C4: RecordField<u32> = RecordField::grouped(0xc4, 1, None);
}

/// Access group 2.
pub mod group_02 {
    use super::*;

    /// Read by 0x751d98; written by 0x751de4.
    pub const AT_00C8_4: RecordField<u32> = RecordField::grouped(0xc8, 2, None);

    /// Read by 0x751d3c; written by 0x74e1f0.
    pub const AT_00C8_124: RecordField<[u8; 124]> = RecordField::grouped(0xc8, 2, None);

    /// Written by 0x7520c4.
    pub const AT_00CC: RecordField<u32, Unread, RecordWrite> = RecordField::new(0xcc, Unread, RecordWrite::Granted(AccessGroup::at(2)), None);

    /// Read by 0x751e30; written by 0x751e7c.
    pub const AT_00D0: RecordField<u32> = RecordField::grouped(0xd0, 2, None);

    /// Read by 0x752110; written by 0x752178.
    pub const AT_00D4: RecordField<[u8; 16]> = RecordField::grouped(0xd4, 2, None);

    /// Written by 0x75222c.
    pub const AT_00E4: RecordField<[u8; 16], Unread, RecordWrite> = RecordField::new(0xe4, Unread, RecordWrite::Granted(AccessGroup::at(2)), None);

    /// Read by 0x752278; written by 0x7522d4.
    pub const AT_00F4: RecordField<[u8; 8]> = RecordField::grouped(0xf4, 2, None);

    /// Written by 0x75236c.
    pub const AT_00FC: RecordField<u16, Unread, RecordWrite> = RecordField::new(0xfc, Unread, RecordWrite::Granted(AccessGroup::at(2)), None);

    /// Written by 0x752404.
    pub const AT_0100: RecordField<u32, Unread, RecordWrite> = RecordField::new(0x100, Unread, RecordWrite::Granted(AccessGroup::at(2)), None);
}

/// Access group 3.
pub mod group_03 {
    use super::*;

    /// Written by 0x7524fc.
    pub const AT_0144_4: RecordField<u32, Unread, RecordWrite> = RecordField::new(0x144, Unread, RecordWrite::Granted(AccessGroup::at(3)), None);

    /// Read by 0x752450.
    pub const AT_0144_28: RecordField<[u8; 28], RecordRead, Unwritten> = RecordField::new(0x144, RecordRead(AccessGroup::at(3)), Unwritten, None);

    /// Written by 0x75259c.
    pub const AT_0148: RecordField<[u8; 6], Unread, RecordWrite> = RecordField::new(0x148, Unread, RecordWrite::Granted(AccessGroup::at(3)), None);

    /// Written by 0x75263c.
    pub const AT_014E: RecordField<[u8; 6], Unread, RecordWrite> = RecordField::new(0x14e, Unread, RecordWrite::Granted(AccessGroup::at(3)), None);

    /// Written by 0x7526cc.
    pub const AT_0154: RecordField<u32, Unread, RecordWrite> = RecordField::new(0x154, Unread, RecordWrite::Granted(AccessGroup::at(3)), None);

    /// Written by 0x754a9c.
    pub const AT_0158: RecordField<[u8; 8], Unread, RecordWrite> = RecordField::new(0x158, Unread, RecordWrite::Granted(AccessGroup::at(3)), None);
}

/// Access group 4.
pub mod group_04 {
    use super::*;

    /// Written by 0x7527ac.
    pub const AT_0160_4: RecordField<u32, Unread, RecordWrite> = RecordField::new(0x160, Unread, RecordWrite::Granted(AccessGroup::at(4)), None);

    /// Read by 0x7527f8; written by 0x752844.
    pub const AT_0164: RecordField<u32> = RecordField::grouped(0x164, 4, None);

    /// Read by 0x752890; written by 0x7528dc.
    pub const AT_0168: RecordField<u32> = RecordField::grouped(0x168, 4, None);
}

/// Access group 5. Fields of the three 0xa8-byte elements from 0x16c; groups 5 to 10 share the element.
pub mod group_05 {
    use super::*;

    /// Read by 0x752990; written by 0x7529e8.
    pub const AT_016C_2: RecordArray<i16, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x16c, RecordRead(AccessGroup::at(5)), RecordWrite::Granted(AccessGroup::at(5)), None), 0xa8);

    /// Read by 0x752928; written by 0x74e284.
    pub const AT_016C_168: RecordArray<[u8; 168], 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x16c, RecordRead(AccessGroup::at(5)), RecordWrite::GrantedAll(&[AccessGroup::at(5), AccessGroup::at(6), AccessGroup::at(7), AccessGroup::at(8), AccessGroup::at(9), AccessGroup::at(10)]), None), 0xa8);

    /// Read by 0x752a40; written by 0x752a98.
    pub const AT_0170: RecordArray<u32, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x170, RecordRead(AccessGroup::at(5)), RecordWrite::Granted(AccessGroup::at(5)), None), 0xa8);

    /// Read by 0x752af0; written by 0x752b48.
    pub const AT_0174: RecordArray<i16, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x174, RecordRead(AccessGroup::at(5)), RecordWrite::Granted(AccessGroup::at(5)), None), 0xa8);

    /// Read by 0x752ba0; written by 0x752bf8.
    pub const AT_0176: RecordArray<i16, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x176, RecordRead(AccessGroup::at(5)), RecordWrite::Granted(AccessGroup::at(5)), None), 0xa8);

    /// Read by 0x752c50; written by 0x752ca8.
    pub const AT_0178: RecordArray<i16, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x178, RecordRead(AccessGroup::at(5)), RecordWrite::Granted(AccessGroup::at(5)), None), 0xa8);

    /// Read by 0x752d00; written by 0x752d58.
    pub const AT_017A: RecordArray<i16, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x17a, RecordRead(AccessGroup::at(5)), RecordWrite::Granted(AccessGroup::at(5)), None), 0xa8);

    /// Read by 0x752f10; written by 0x752f68.
    pub const AT_017C: RecordArray<i16, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x17c, RecordRead(AccessGroup::at(5)), RecordWrite::Granted(AccessGroup::at(5)), None), 0xa8);

    /// Read by 0x752fc0; written by 0x753018.
    pub const AT_017E: RecordArray<i16, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x17e, RecordRead(AccessGroup::at(5)), RecordWrite::Granted(AccessGroup::at(5)), None), 0xa8);

    /// Read by 0x753070; written by 0x7530c8.
    pub const AT_0180: RecordArray<i16, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x180, RecordRead(AccessGroup::at(5)), RecordWrite::Granted(AccessGroup::at(5)), None), 0xa8);

    /// Read by 0x753540; written by 0x753598.
    pub const AT_0182: RecordArray<i16, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x182, RecordRead(AccessGroup::at(5)), RecordWrite::Granted(AccessGroup::at(5)), None), 0xa8);

    /// Read by 0x7531d0; written by 0x753228.
    pub const AT_0184: RecordArray<i16, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x184, RecordRead(AccessGroup::at(5)), RecordWrite::Granted(AccessGroup::at(5)), None), 0xa8);

    /// Read by 0x753120; written by 0x753178.
    pub const AT_0186: RecordArray<i16, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x186, RecordRead(AccessGroup::at(5)), RecordWrite::Granted(AccessGroup::at(5)), None), 0xa8);

    /// Read by 0x753330; written by 0x753388.
    pub const AT_0188: RecordArray<u32, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x188, RecordRead(AccessGroup::at(5)), RecordWrite::Granted(AccessGroup::at(5)), None), 0xa8);

    /// Written by 0x753438.
    pub const AT_018C: RecordArray<u32, 3, Unread, RecordWrite> = RecordArray::new(RecordField::new(0x18c, Unread, RecordWrite::Granted(AccessGroup::at(5)), None), 0xa8);

    /// Written by 0x7534e8.
    pub const AT_0190: RecordArray<u32, 3, Unread, RecordWrite> = RecordArray::new(RecordField::new(0x190, Unread, RecordWrite::Granted(AccessGroup::at(5)), None), 0xa8);

    /// Read by 0x7535f0; written by 0x753648.
    pub const AT_0194: RecordArray<u32, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x194, RecordRead(AccessGroup::at(5)), RecordWrite::Granted(AccessGroup::at(5)), None), 0xa8);

    /// Read by 0x753280; written by 0x7532d8.
    pub const AT_0198: RecordArray<u8, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x198, RecordRead(AccessGroup::at(5)), RecordWrite::Granted(AccessGroup::at(5)), None), 0xa8);

    /// Read by 0x752db0; written by 0x752e08.
    pub const AT_019A: RecordArray<i16, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x19a, RecordRead(AccessGroup::at(5)), RecordWrite::Granted(AccessGroup::at(5)), None), 0xa8);

    /// Read by 0x752e60; written by 0x752eb8.
    pub const AT_019C: RecordArray<i16, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x19c, RecordRead(AccessGroup::at(5)), RecordWrite::Granted(AccessGroup::at(5)), None), 0xa8);
}

/// Access group 6.
pub mod group_06 {
    use super::*;

    /// Read by 0x7536a0; written by 0x7536f8.
    pub const AT_01A0: RecordArray<u32, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x1a0, RecordRead(AccessGroup::at(6)), RecordWrite::Granted(AccessGroup::at(6)), None), 0xa8);

    /// Read by 0x753750; written by 0x7537a8.
    pub const AT_01A4: RecordArray<i16, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x1a4, RecordRead(AccessGroup::at(6)), RecordWrite::Granted(AccessGroup::at(6)), None), 0xa8);

    /// Read by 0x753800; written by 0x753858.
    pub const AT_01A8: RecordArray<u32, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x1a8, RecordRead(AccessGroup::at(6)), RecordWrite::Granted(AccessGroup::at(6)), None), 0xa8);

    /// Read by 0x7538b0; written by 0x753908.
    pub const AT_01AC: RecordArray<u32, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x1ac, RecordRead(AccessGroup::at(6)), RecordWrite::Granted(AccessGroup::at(6)), None), 0xa8);
}

/// Access group 7.
pub mod group_07 {
    use super::*;

    /// Read by 0x753960; written by 0x7539b8.
    pub const AT_01B0: RecordArray<u32, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x1b0, RecordRead(AccessGroup::at(7)), RecordWrite::Granted(AccessGroup::at(7)), None), 0xa8);

    /// Read by 0x753a10; written by 0x753a68.
    pub const AT_01B4: RecordArray<u32, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x1b4, RecordRead(AccessGroup::at(7)), RecordWrite::Granted(AccessGroup::at(7)), None), 0xa8);

    /// Written by 0x753b18.
    pub const AT_01B8: RecordArray<u32, 3, Unread, RecordWrite> = RecordArray::new(RecordField::new(0x1b8, Unread, RecordWrite::Granted(AccessGroup::at(7)), None), 0xa8);

    /// Written by 0x753bc8.
    pub const AT_01BC: RecordArray<u32, 3, Unread, RecordWrite> = RecordArray::new(RecordField::new(0x1bc, Unread, RecordWrite::Granted(AccessGroup::at(7)), None), 0xa8);

    /// Read by 0x753c20; written by 0x753c78.
    pub const AT_01C0: RecordArray<u32, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x1c0, RecordRead(AccessGroup::at(7)), RecordWrite::Granted(AccessGroup::at(7)), None), 0xa8);

    /// Written by 0x758460.
    pub const AT_01C4: RecordArray<[u8; 16], 3, Unread, RecordWrite> = RecordArray::new(RecordField::new(0x1c4, Unread, RecordWrite::Granted(AccessGroup::at(7)), None), 0xa8);

    /// Written by 0x758520.
    pub const AT_01D4: RecordArray<[u8; 16], 3, Unread, RecordWrite> = RecordArray::new(RecordField::new(0x1d4, Unread, RecordWrite::Granted(AccessGroup::at(7)), None), 0xa8);

    /// Written by 0x753d28.
    pub const AT_01E4: RecordArray<u16, 3, Unread, RecordWrite> = RecordArray::new(RecordField::new(0x1e4, Unread, RecordWrite::Granted(AccessGroup::at(7)), None), 0xa8);

    /// Written by 0x753dd8.
    pub const AT_01E8: RecordArray<u32, 3, Unread, RecordWrite> = RecordArray::new(RecordField::new(0x1e8, Unread, RecordWrite::Granted(AccessGroup::at(7)), None), 0xa8);

    /// Read by 0x753e30; written by 0x753e88.
    pub const AT_01EC: RecordArray<u32, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x1ec, RecordRead(AccessGroup::at(7)), RecordWrite::Granted(AccessGroup::at(7)), None), 0xa8);
}

/// Access group 8.
pub mod group_08 {
    use super::*;

    /// Read by 0x753ee0; written by 0x753f38.
    pub const AT_01F0: RecordArray<i16, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x1f0, RecordRead(AccessGroup::at(8)), RecordWrite::Granted(AccessGroup::at(8)), None), 0xa8);

    /// Read by 0x753f90; written by 0x753fe8.
    pub const AT_01F2: RecordArray<i16, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x1f2, RecordRead(AccessGroup::at(8)), RecordWrite::Granted(AccessGroup::at(8)), None), 0xa8);

    /// Read by 0x754040; written by 0x754098.
    pub const AT_01F4: RecordArray<u32, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x1f4, RecordRead(AccessGroup::at(8)), RecordWrite::Granted(AccessGroup::at(8)), None), 0xa8);

    /// Written by 0x754148.
    pub const AT_01F8: RecordArray<u32, 3, Unread, RecordWrite> = RecordArray::new(RecordField::new(0x1f8, Unread, RecordWrite::Granted(AccessGroup::at(8)), None), 0xa8);

    /// Read by 0x7541a0; written by 0x7541f8.
    pub const AT_01FC: RecordArray<u32, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x1fc, RecordRead(AccessGroup::at(8)), RecordWrite::Granted(AccessGroup::at(8)), None), 0xa8);

    /// Read by 0x754300; written by 0x754358.
    pub const AT_0200: RecordArray<u32, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x200, RecordRead(AccessGroup::at(8)), RecordWrite::Granted(AccessGroup::at(8)), None), 0xa8);

    /// Read by 0x7543b0; written by 0x754408.
    pub const AT_0204: RecordArray<u32, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x204, RecordRead(AccessGroup::at(8)), RecordWrite::Granted(AccessGroup::at(8)), None), 0xa8);
}

/// Access group 10.
pub mod group_10 {
    use super::*;

    /// Read by 0x754460; written by 0x7544b8.
    pub const AT_0208: RecordArray<u32, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x208, RecordRead(AccessGroup::at(10)), RecordWrite::Granted(AccessGroup::at(10)), None), 0xa8);

    /// Read by 0x754510; written by 0x754568.
    pub const AT_020C: RecordArray<u32, 3, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x20c, RecordRead(AccessGroup::at(10)), RecordWrite::Granted(AccessGroup::at(10)), None), 0xa8);

    /// Written by 0x7542a8.
    pub const AT_0210: RecordArray<u32, 3, Unread, RecordWrite> = RecordArray::new(RecordField::new(0x210, Unread, RecordWrite::Granted(AccessGroup::at(10)), None), 0xa8);
}

/// Access group 11.
pub mod group_11 {
    use super::*;

    /// Written by 0x75466c.
    pub const AT_0364_4: RecordField<u32, Unread, RecordWrite> = RecordField::new(0x364, Unread, RecordWrite::Granted(AccessGroup::at(11)), None);

    /// Read by 0x7545c0.
    pub const AT_0364_28: RecordField<[u8; 28], RecordRead, Unwritten> = RecordField::new(0x364, RecordRead(AccessGroup::at(11)), Unwritten, None);

    /// Written by 0x75470c.
    pub const AT_0368: RecordField<[u8; 6], Unread, RecordWrite> = RecordField::new(0x368, Unread, RecordWrite::Granted(AccessGroup::at(11)), None);

    /// Written by 0x7547ac.
    pub const AT_036E: RecordField<[u8; 6], Unread, RecordWrite> = RecordField::new(0x36e, Unread, RecordWrite::Granted(AccessGroup::at(11)), None);

    /// Written by 0x75484c.
    pub const AT_0374: RecordField<[u8; 6], Unread, RecordWrite> = RecordField::new(0x374, Unread, RecordWrite::Granted(AccessGroup::at(11)), None);

    /// Written by 0x7548dc.
    pub const AT_037A: RecordField<u16, Unread, RecordWrite> = RecordField::new(0x37a, Unread, RecordWrite::Granted(AccessGroup::at(11)), None);

    /// Written by 0x75496c.
    pub const AT_037C: RecordField<u16, Unread, RecordWrite> = RecordField::new(0x37c, Unread, RecordWrite::Granted(AccessGroup::at(11)), None);

    /// Written by 0x7549fc.
    pub const AT_037E: RecordField<u16, Unread, RecordWrite> = RecordField::new(0x37e, Unread, RecordWrite::Granted(AccessGroup::at(11)), None);
}

/// Access group 12.
pub mod group_12 {
    use super::*;

    /// Written by 0x754b88.
    pub const AT_0380_4: RecordField<u32, Unread, RecordWrite> = RecordField::new(0x380, Unread, RecordWrite::Granted(AccessGroup::at(12)), None);

    /// Read by 0x754ae4.
    pub const AT_0380_12: RecordField<[u8; 12], RecordRead, Unwritten> = RecordField::new(0x380, RecordRead(AccessGroup::at(12)), Unwritten, None);

    /// Written by 0x754c20.
    pub const AT_0384: RecordField<u8, Unread, RecordWrite> = RecordField::new(0x384, Unread, RecordWrite::Granted(AccessGroup::at(12)), None);

    /// Written by 0x754cb8.
    pub const AT_0386: RecordField<u16, Unread, RecordWrite> = RecordField::new(0x386, Unread, RecordWrite::Granted(AccessGroup::at(12)), None);

    /// Written by 0x754d50.
    pub const AT_0388: RecordField<u16, Unread, RecordWrite> = RecordField::new(0x388, Unread, RecordWrite::Granted(AccessGroup::at(12)), None);
}

/// Access group 13.
pub mod group_13 {
    use super::*;

    /// Read by 0x754d9c; written by 0x754dec.
    pub const AT_20B0_4: RecordField<u32> = RecordField::grouped(0x20b0, 13, None);

    /// Read by 0x754e3c; written by 0x754e8c.
    pub const AT_20B4: RecordField<u32> = RecordField::grouped(0x20b4, 13, None);

    /// Read by 0x754edc; written by 0x754f3c.
    pub const AT_20B8: RecordField<[u8; 16]> = RecordField::grouped(0x20b8, 13, None);

    /// Read by 0x754f8c; written by 0x754fdc.
    pub const AT_20C8: RecordField<u32> = RecordField::grouped(0x20c8, 13, None);

    /// Read by 0x75502c; written by 0x75507c.
    pub const AT_20CC: RecordField<u32> = RecordField::grouped(0x20cc, 13, None);

    /// Read by 0x7550cc; written by 0x75511c.
    pub const AT_20D0: RecordField<u32> = RecordField::grouped(0x20d0, 13, None);

    /// Read by 0x75516c; written by 0x7551bc.
    pub const AT_20D4: RecordField<u32> = RecordField::grouped(0x20d4, 13, None);

    /// Read by 0x75520c; written by 0x75526c.
    pub const AT_20D8: RecordField<[u8; 16]> = RecordField::grouped(0x20d8, 13, None);

    /// Read by 0x7552bc; written by 0x75531c.
    pub const AT_20E8: RecordField<[u8; 8]> = RecordField::grouped(0x20e8, 13, None);

    /// Read by 0x75536c; written by 0x7553bc.
    pub const AT_20F0: RecordField<u32> = RecordField::grouped(0x20f0, 13, None);

    /// Read by 0x75540c; written by 0x75545c.
    pub const AT_20F4: RecordField<u32> = RecordField::grouped(0x20f4, 13, None);

    /// Read by 0x7554ac; written by 0x7554fc.
    pub const AT_20F8: RecordField<u32> = RecordField::grouped(0x20f8, 13, None);

    /// Read by 0x75554c; written by 0x75559c.
    pub const AT_20FC: RecordField<u32> = RecordField::grouped(0x20fc, 13, None);

    /// Read by 0x7555ec; written by 0x75564c.
    pub const AT_2100: RecordField<[u8; 16]> = RecordField::grouped(0x2100, 13, None);
}

/// Access group 30. The seven entry lists (a count word and ten 104-byte entries each) and the record between
/// the fifth and sixth list.
pub mod group_30 {
    use super::*;

    /// Read by 0x75587c; written by 0x7558c8.
    pub const AT_038C: RecordField<u32> = RecordField::grouped(0x38c, 30, None);

    /// Read by 0x755914; written by 0x755968.
    pub const AT_0390: RecordArray<[u8; 104], 10, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x390, RecordRead(AccessGroup::at(30)), RecordWrite::Granted(AccessGroup::at(30)), None), 0x68);

    /// Read by 0x75624c; written by 0x756298.
    pub const AT_07A0: RecordField<u32> = RecordField::grouped(0x7a0, 30, None);

    /// Read by 0x7562e4; written by 0x756338.
    pub const AT_07A4: RecordArray<[u8; 104], 10, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x7a4, RecordRead(AccessGroup::at(30)), RecordWrite::Granted(AccessGroup::at(30)), None), 0x68);

    /// Read by 0x7559bc; written by 0x755a08.
    pub const AT_0BB4: RecordField<u32, RecordRead, RecordWrite> = RecordField::new(0xbb4, RecordRead(AccessGroup::at(30)), RecordWrite::Selected(AccessGroup::at(30)), None);

    /// Read by 0x755a5c; written by 0x755ab0.
    pub const AT_0BB8: RecordArray<[u8; 104], 10, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0xbb8, RecordRead(AccessGroup::at(30)), RecordWrite::Selected(AccessGroup::at(30)), None), 0x68);

    /// Read by 0x755b0c; written by 0x755b58.
    pub const AT_0FC8: RecordField<u32, RecordRead, RecordWrite> = RecordField::new(0xfc8, RecordRead(AccessGroup::at(30)), RecordWrite::Selected(AccessGroup::at(30)), None);

    /// Read by 0x755bac; written by 0x755c00.
    pub const AT_0FCC: RecordArray<[u8; 104], 10, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0xfcc, RecordRead(AccessGroup::at(30)), RecordWrite::Selected(AccessGroup::at(30)), None), 0x68);

    /// Read by 0x755c5c; written by 0x755ca8.
    pub const AT_13DC: RecordField<u32, RecordRead, RecordWrite> = RecordField::new(0x13dc, RecordRead(AccessGroup::at(30)), RecordWrite::Selected(AccessGroup::at(30)), None);

    /// Read by 0x755cfc; written by 0x755d50.
    pub const AT_13E0: RecordArray<[u8; 104], 10, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x13e0, RecordRead(AccessGroup::at(30)), RecordWrite::Selected(AccessGroup::at(30)), None), 0x68);

    /// Read by 0x756550; written by 0x756598.
    pub const AT_17F0: RecordField<i16> = RecordField::grouped(0x17f0, 30, None);

    /// Read by 0x7565e0; written by 0x756628.
    pub const AT_17F2: RecordField<i16> = RecordField::grouped(0x17f2, 30, None);

    /// Read by 0x756670; written by 0x7566b8.
    pub const AT_17F4: RecordField<i16> = RecordField::grouped(0x17f4, 30, None);

    /// Read by 0x7563d4; written by 0x75638c.
    pub const AT_17F8: RecordField<u32> = RecordField::grouped(0x17f8, 30, None);

    /// Read by 0x75641c; written by 0x756478.
    pub const AT_17FC: RecordField<[u8; 104]> = RecordField::grouped(0x17fc, 30, None);

    /// Read by 0x7557dc; written by 0x75582c.
    pub const AT_1864: RecordArray<i16, 10, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x1864, RecordRead(AccessGroup::at(30)), RecordWrite::Granted(AccessGroup::at(30)), None), 0x2);

    /// Read by 0x7564c0; written by 0x756508.
    pub const AT_1878: RecordField<u32> = RecordField::grouped(0x1878, 30, None);

    /// Read by 0x755dac; written by 0x755dfc.
    pub const AT_187C: RecordField<u32, RecordRead, RecordWrite> = RecordField::new(0x187c, RecordRead(AccessGroup::at(30)), RecordWrite::Selected(AccessGroup::at(30)), None);

    /// Read by 0x755e54; written by 0x755eac.
    pub const AT_1880: RecordArray<[u8; 104], 10, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x1880, RecordRead(AccessGroup::at(30)), RecordWrite::Selected(AccessGroup::at(30)), None), 0x68);

    /// Read by 0x755f0c; written by 0x755f5c.
    pub const AT_1C90: RecordField<u32, RecordRead, RecordWrite> = RecordField::new(0x1c90, RecordRead(AccessGroup::at(30)), RecordWrite::Selected(AccessGroup::at(30)), None);

    /// Read by 0x755fb4; written by 0x75600c.
    pub const AT_1C94: RecordArray<[u8; 104], 10, RecordRead, RecordWrite> = RecordArray::new(RecordField::new(0x1c94, RecordRead(AccessGroup::at(30)), RecordWrite::Selected(AccessGroup::at(30)), None), 0x68);

    /// Read by 0x75606c; written by 0x7560b8.
    pub const AT_20A4: RecordField<u32, RecordRead, RecordWrite> = RecordField::new(0x20a4, RecordRead(AccessGroup::at(30)), RecordWrite::Selected(AccessGroup::at(30)), None);

    /// Read by 0x75610c; written by 0x756158.
    pub const AT_20A8: RecordField<u32, RecordRead, RecordWrite> = RecordField::new(0x20a8, RecordRead(AccessGroup::at(30)), RecordWrite::Selected(AccessGroup::at(30)), None);

    /// Read by 0x7561ac; written by 0x7561f8.
    pub const AT_20AC: RecordField<u32, RecordRead, RecordWrite> = RecordField::new(0x20ac, RecordRead(AccessGroup::at(30)), RecordWrite::Selected(AccessGroup::at(30)), None);
}

/// Access group 31.
pub mod group_31 {
    use super::*;

    /// Read by 0x75569c; written by 0x7556ec.
    pub const AT_2110: RecordField<u32> = RecordField::grouped(0x2110, 31, None);

    /// Read by 0x75573c; written by 0x75578c.
    pub const AT_2114: RecordField<u32> = RecordField::grouped(0x2114, 31, None);
}

/// Access group 32.
pub mod group_32 {
    use super::*;

    /// Read by 0x758578; written by 0x7585dc.
    pub const AT_2140: RecordField<[u8; 20]> = RecordField::grouped(0x2140, 32, None);
}

/// A change-tracked header field, as the staged-record apply (0x759250) writes it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tracked {
    U8(RecordField<u8, Unread, RecordWrite>),
    I8(RecordField<i8, Unread, RecordWrite>),
    U16(RecordField<u16, Unread, RecordWrite>),
    I16(RecordField<i16, Unread, RecordWrite>),
    U32(RecordField<u32, Unread, RecordWrite>),
    Bytes4(RecordField<[u8; 4], Unread, RecordWrite>),
    Bytes6(RecordField<[u8; 6], Unread, RecordWrite>),
    Bytes8(RecordField<[u8; 8], Unread, RecordWrite>),
}

/// Every change-tracked field, in the order 0x759250 tests its bit and writes it.
pub const CHANGE_TRACKED: [Tracked; 84] = [
    Tracked::U8(group_00::MEASUREMENT_SELECTION.writer()),
    Tracked::U8(group_00::AT_0031.writer()),
    Tracked::U16(group_00::AT_002E.writer()),
    Tracked::U8(group_00::AT_0032.writer()),
    Tracked::U8(group_00::AT_0033.writer()),
    Tracked::U16(group_00::AT_0026.writer()),
    Tracked::U16(group_00::AT_0018.writer()),
    Tracked::U8(group_00::AT_005B.writer()),
    Tracked::U8(group_00::AT_005C.writer()),
    Tracked::U8(group_00::AT_005D.writer()),
    Tracked::U8(group_00::AT_0034.writer()),
    Tracked::U32(group_00::AT_0000.writer()),
    Tracked::U8(group_00::AT_0035.writer()),
    Tracked::U8(group_00::AT_0036.writer()),
    Tracked::U8(group_00::AT_0037.writer()),
    Tracked::U8(group_00::AT_0038.writer()),
    Tracked::U8(group_00::AT_0039.writer()),
    Tracked::U8(group_00::AT_003A.writer()),
    Tracked::U8(group_00::AT_003B.writer()),
    Tracked::U8(group_00::AT_003C.writer()),
    Tracked::U8(group_00::AT_003D.writer()),
    Tracked::U8(group_00::AT_003E.writer()),
    Tracked::U8(group_00::AT_003F.writer()),
    Tracked::I16(group_00::AT_001A.writer()),
    Tracked::U8(group_00::AT_005E.writer()),
    Tracked::U8(group_00::AT_0040.writer()),
    Tracked::U8(group_00::AT_0041.writer()),
    Tracked::U8(group_00::AT_0042.writer()),
    Tracked::U8(group_00::AT_0043.writer()),
    Tracked::I16(group_00::AT_001C.writer()),
    Tracked::U32(group_00::AT_0014.writer()),
    Tracked::I16(group_00::AT_001E.writer()),
    Tracked::U8(group_00::AT_0044.writer()),
    Tracked::I16(group_00::AT_0020.writer()),
    Tracked::I16(group_00::AT_0022.writer()),
    Tracked::I16(group_00::AT_0024.writer()),
    Tracked::U8(group_00::AT_0046.writer()),
    Tracked::U8(group_00::AT_0047.writer()),
    Tracked::Bytes8(group_00::AT_0004.writer()),
    Tracked::U8(group_00::AT_005F.writer()),
    Tracked::U8(group_00::AT_0060.writer()),
    Tracked::U8(group_00::AT_0061.writer()),
    Tracked::U16(group_00::AT_0028.writer()),
    Tracked::U8(group_00::AT_0048.writer()),
    Tracked::U8(group_00::AT_0049.writer()),
    Tracked::U8(group_00::AT_004A.writer()),
    Tracked::Bytes6(group_00::AT_000E.writer()),
    Tracked::U16(group_00::AT_002A.writer()),
    Tracked::U8(group_00::AT_0045.writer()),
    Tracked::U8(group_00::AT_004B.writer()),
    Tracked::U8(group_00::AT_004C.writer()),
    Tracked::U8(group_00::AT_004D.writer()),
    Tracked::U8(group_00::AT_004E.writer()),
    Tracked::U8(group_00::AT_004F.writer()),
    Tracked::U8(group_00::AT_0050.writer()),
    Tracked::U8(group_00::AT_0063.writer()),
    Tracked::U8(group_00::AT_0065.writer()),
    Tracked::U8(group_00::AT_0051.writer()),
    Tracked::U8(group_00::AT_0052.writer()),
    Tracked::U8(group_00::AT_0064.writer()),
    Tracked::U8(group_00::AT_0066.writer()),
    Tracked::U8(group_00::AT_0054.writer()),
    Tracked::U8(group_00::AT_0053.writer()),
    Tracked::U8(group_00::AT_0055.writer()),
    Tracked::U8(group_00::AT_0067.writer()),
    Tracked::U16(group_00::AT_000C.writer()),
    Tracked::U8(group_00::AT_0069.writer()),
    Tracked::U16(group_00::AT_002C.writer()),
    Tracked::U8(group_00::AT_0068.writer()),
    Tracked::U8(group_00::AT_006B.writer()),
    Tracked::U8(group_00::INTERVAL_TRACKING.writer()),
    Tracked::U8(group_00::AT_006A.writer()),
    Tracked::U8(group_00::AT_006C.writer()),
    Tracked::U8(group_00::AT_006D.writer()),
    Tracked::U8(group_00::AT_0070.writer()),
    Tracked::U8(group_00::AT_006E.writer()),
    Tracked::U8(group_00::AT_0058.writer()),
    Tracked::U8(group_00::AT_0059.writer()),
    Tracked::U8(group_00::AT_0056.writer()),
    Tracked::U8(group_00::AT_0057.writer()),
    Tracked::Bytes4(group_00::AT_0072.writer()),
    Tracked::Bytes4(group_00::AT_0076.writer()),
    Tracked::I8(group_00::AT_0071.writer()),
    Tracked::U8(group_00::AT_005A.writer()),
];

/// The fields 0x752714 chooses between: the halfword when the flag byte is 1, otherwise the word.
pub(crate) const CHOICE_FLAG: RecordField<u8> = group_00::AT_0069;
pub(crate) const CHOICE_HALFWORD: RecordField<u16, RecordRead, Unwritten> = RecordField::new(0x2c, RecordRead(AccessGroup::at(0)), Unwritten, None);
pub(crate) const CHOICE_WORD: RecordField<u32, RecordRead, Unwritten> = RecordField::new(0x160, RecordRead(AccessGroup::at(4)), Unwritten, None);
