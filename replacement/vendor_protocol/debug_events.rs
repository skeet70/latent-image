use crate::backend::{BackendError, DebugRequest};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebugKey(u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyState { On, Off }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationEvent { PowerOffRequested, Key { key: DebugKey, state: KeyState } }
impl ApplicationEvent {
    pub const fn message(self) -> (u16, u32) {
        match self {
            Self::PowerOffRequested => (0x86, 0),
            Self::Key { key, state } => (key.0 | match state { KeyState::On => 0, KeyState::Off => 0x80 }, 0),
        }
    }
    pub const fn key_value(self) -> (u16, u8) {
        match self { Self::PowerOffRequested => (6, 0x80), Self::Key { key, state } => (key.0, match state { KeyState::On => 0, KeyState::Off => 0x80 }) }
    }
}
fn c_string(bytes: &[u8]) -> &[u8] { &bytes[..bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len())] }
fn key(name: &[u8]) -> Option<DebugKey> {
    NAMES.iter().find(|(entry, _)| name.eq_ignore_ascii_case(entry)).map(|(_, code)| DebugKey(*code))
}
pub fn interpret(request: DebugRequest<'_>) -> Result<ApplicationEvent, BackendError> {
    let command = c_string(request.command);
    let arguments = c_string(request.arguments);
    let (key, state) = if command.starts_with(b"MODE") {
        (key(arguments), Some(KeyState::On))
    } else {
        let state = if arguments.eq_ignore_ascii_case(b"ON") { Some(KeyState::On) } else if arguments.eq_ignore_ascii_case(b"OFF") { Some(KeyState::Off) } else { None };
        (key(command), state)
    };
    let key = key.ok_or(BackendError::InvalidCommand)?;
    let state = state.ok_or(BackendError::InvalidCommand)?;
    if key.0 == 6 && state == KeyState::Off { Ok(ApplicationEvent::PowerOffRequested) } else { Ok(ApplicationEvent::Key { key, state }) }
}

const NAMES: &[(&[u8], u16)] = &[
    (b"EXE_M",1),(b"UP",2),(b"DOWN",3),(b"LEFT",4),(b"RIGHT",5),(b"POWER",6),(b"S1",7),(b"S2",8),
    (b"BARR_OP",0xd),(b"BARR_CL",0xe),(b"BARRIER",0xc),(b"CRADLE",0x214),(b"ADAPTER",0xf),(b"USB",0x10),(b"DOOR",0x210),(b"CARD",0x11),
    (b"FMODE",0xa),(b"EXP",0x12),(b"METER",0x21d),(b"TELE2",0x15),(b"WIDE2",0x16),(b"TELE",0x13),(b"WIDE",0x14),(b"AF",0x17),
    (b"VIDEO",0x19),(b"HDTV",0x1a),(b"CONT",0x1b),(b"AS_MODE",0x311),(b"DISP_B",9),(b"K_HOME",0x21b),(b"K_PLAY",0xb),(b"FACE",0x211),
    (b"CRD_P",0x215),(b"CRD_U",0x216),(b"MET_SP",0x20d),(b"MET_AV",0x20f),(b"MET_MU",0x20e),(b"HVS_0",0x200),(b"HVS_90",0x201),(b"HVS_180",0x202),(b"HVS_270",0x203),
    (b"POP",0x1c),(b"C_DIAL+",0x1d),(b"C_DIAL-",0x1e),(b"W_DIAL+",0x207),(b"W_DIAL-",0x208),(b"MF_AF_A",0x1f),(b"MF_AF_C",0x20),(b"MF_AF_M",0x21),(b"1P_AF+",0x22),
    (b"F_RING+",0x209),(b"F_RING-",0x20a),(b"Z_RING+",0x23),(b"Z_RING-",0x24),(b"AE-LOCK",0x25),(b"EVF_LCD",0x26),(b"INFO",0x21c),(b"AEAF_LOCK",0x31),
    (b"WB",0x2e),(b"K_PS",0x213),(b"AE",0x2f),(b"VIEW",0x217),(b"K_Q",0x32),(b"EVF_OVF",0x29),(b"MACRO",0x212),(b"CMD_PUSH",0x2b),(b"CMD_LEFT",0x2c),(b"CMD_RIGHT",0x2d),
    (b"MOVIE_REC",0x34),(b"EYE_SEN",0x33),(b"RING_OFF",0x204),(b"RING_STB",0x205),(b"RING_ON",0x206),(b"FUNCTION2",0x219),(b"FUNCTION",0x28),(b"WLAN",0x37),
    (b"PLAY",0x300),(b"REC",0x301),(b"AUTO",0x302),(b"QUICK",0x303),(b"SP",0x304),(b"SP2",0x305),(b"SR",0x306),(b"PORTRAIT",0x307),(b"LANDSCAPE",0x308),(b"SPORTS",0x309),
    (b"NIGHT",0x30a),(b"BLURRE",0x30c),(b"NATURAL",0x30b),(b"PROGRAM",0x30e),(b"SHUTTER",0x30f),(b"APERTURE",0x310),(b"MANUAL",0x312),(b"N_FLASH",0x30d),
    (b"C1",0x317),(b"C2",0x318),(b"C3",0x319),(b"FSB",0x31a),(b"ISO",0x27),(b"ADVANCE",0x31c),(b"S-A",0x31e),(b"S-1/4000",0x31f),(b"S-1/2000",0x320),
    (b"S-1/1000",0x321),(b"S-1/500",0x322),(b"S-1/250",0x323),(b"S-1/125",0x324),(b"S-1/60",0x325),(b"S-1/30",0x326),(b"S-1/15",0x327),(b"S-1/8",0x328),(b"S-1/4",0x329),(b"S-T",0x32a),(b"S-B",0x32b),
    (b"E-D+2.0",0x32c),(b"E-D+1.7",0x32d),(b"E-D+1.3",0x32d),(b"E-D+1.0",0x32f),(b"E-D+0.7",0x330),(b"E-D+0.3",0x331),(b"E-D=0",0x332),
    (b"E-D-0.3",0x333),(b"E-D-0.7",0x334),(b"E-D-1.0",0x335),(b"E-D-1.3",0x337),(b"E-D-1.7",0x337),(b"E-D-2.0",0x338),(b"I-M_A",0x400),
    (b"I-M_F16",0x500),(b"I-M_F11",0x501),(b"I-M_F8.0",0x502),(b"I-M_F5.6",0x503),(b"I-M_F4.0",0x504),(b"I-M_F2.8",0x505),(b"I-M_F2.0",0x506),
    (b"I-M_POS30",0x41f),(b"I-M_POS29",0x41e),(b"I-M_POS28",0x41d),(b"I-M_POS27",0x41c),(b"I-M_POS26",0x41b),(b"I-M_POS25",0x41a),(b"I-M_POS24",0x419),
    (b"I-M_POS23",0x418),(b"I-M_POS22",0x417),(b"I-M_POS21",0x416),(b"I-M_POS20",0x415),(b"I-M_POS19",0x414),(b"I-M_POS18",0x413),(b"I-M_POS17",0x412),
    (b"I-M_POS16",0x411),(b"I-M_POS15",0x410),(b"I-M_POS14",0x40f),(b"I-M_POS13",0x40e),(b"I-M_POS12",0x40d),(b"I-M_POS11",0x40c),(b"I-M_POS10",0x40b),
    (b"I-M_POS9",0x40a),(b"I-M_POS8",0x409),(b"I-M_POS7",0x408),(b"I-M_POS6",0x407),(b"I-M_POS5",0x406),(b"I-M_POS4",0x405),(b"I-M_POS3",0x404),(b"I-M_POS2",0x403),(b"I-M_POS1",0x402),(b"I-M_POS0",0x401),
    (b"F_R_+4",0x101),(b"F_R_+3",0x102),(b"F_R_+2",0x103),(b"F_R_+1",0x104),(b"F_R_STOP",0x105),(b"F_R_-1",0x106),(b"F_R_-2",0x107),(b"F_R_-3",0x108),(b"F_R_-4",0x109),
    (b"SETUP",0x315),(b"M_SEL",0x312),(b"VOICE",0x313),(b"MOVIE",0x314),(b"JIG",0x339),(b"EXR",0x31b),(b"VREC",0x313),(b"MJPEG",0x314),(b"SCEN_P",0x304),(b"SCEN_S",0x304),
    (b"MMOD_P",0x30e),(b"MMOD_S",0x30f),(b"MMOD_A",0x310),(b"MMOD_M",0x312),(b"APERT",0x12),
];
