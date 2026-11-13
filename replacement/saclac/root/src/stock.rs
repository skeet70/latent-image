#![forbid(unsafe_code)]

//! The root's stock boundary: X-M1 1.50 entries and memory the bound modules reach that no merged
//! module owns. Entries are entered and memory read through the shared layer's `Machine`.

use xm1_saclac_interface::lens::{LensPayload, SelectionCode, Setting};
use xm1_saclac_interface::stock::{lens_callback_word, record_handle_word};
use xm1_saclac_shared::stock::Machine;

/// Lens control's resource index (0x76d9c8).
pub const RESOURCE_INDEX: u32 = 0x76_d9c8;
/// The configuration lookup selection the bring-up applies the resource index to (0x740b18).
pub const SELECT_LOOKUP: u32 = 0x74_0b18;
/// The adjustment configuration's first parameter (0xfa8018).
pub const ADJUSTMENT_FIRST_PARAMETER: u32 = 0xfa_8018;
/// saclac's fault reaction (0xf46c20).
pub const FAULT: u32 = 0xf4_6c20;
/// The word the live drive's callback records from Lens34, read by the reply at 0xf14f14.
pub const LIVE_DRIVE_RESULT: u32 = 0x95_4dbc;
/// The word the S1 drive's callback 0xf28e94 stores (0xf28eb8), read by the reply at 0xf28f94.
pub const S1_DRIVE_REPORT: u32 = 0x16b_750c;
/// The profile-record handle the focus controller holds (0x16be004 + 0x18), under which
/// 0xf32a48 clears the focus history.
pub const FOCUS_CONTROLLER_RECORD: u32 = 0x16b_e01c;

/// Lens control's queue-50 senders; each returns 0 when it posted.
const SEND_4202: u32 = 0x76_dbe4;
const SEND_4203: u32 = 0x76_dc90;
const SEND_4204: u32 = 0x76_dd24;
const SEND_4205: u32 = 0x76_ddb0;
const SEND_4206: u32 = 0x76_de3c;
const SEND_4207: u32 = 0x76_dec8;
const SEND_4208: u32 = 0x76_df7c;
const SEND_4209: u32 = 0x76_e008;
const SEND_420B: u32 = 0x76_e1ac;
const SEND_4211: u32 = 0x76_ffec;
const SEND_4212: u32 = 0x77_0080;

/// Posts `payload` through its sender with the arguments the lens children pass (0xf14dc8,
/// 0xf14e38, 0xf14e6c, 0xf15148, 0xf151dc, 0xf28ed8, 0xf14fd0) and returns the sender's word.
pub fn send_lens(machine: &impl Machine, payload: &LensPayload) -> u32 {
    let callback = lens_callback_word(payload) as usize;
    let word = |value: u32| value as usize;
    match *payload {
        LensPayload::BringUp { parameters: p, record } => machine.enter(
            SEND_4202,
            &[callback, word(p[0].0), word(p[1].0), word(p[2].0), word(record_handle_word(record)), word(p[3].0), word(p[4].0)],
        ),
        LensPayload::Stop { parameters: p, record } => {
            machine.enter(SEND_4203, &[callback, word(p[0].0), word(p[1].0), word(record_handle_word(record))])
        }
        LensPayload::Setting { setting, slot, mode } => {
            let sender = match setting {
                Setting::Request04 => SEND_4204,
                Setting::Request05 => SEND_4205,
                Setting::Request06 => SEND_4206,
            };
            machine.enter(sender, &[callback, word(slot.0), word(mode.0)])
        }
        LensPayload::Drive { parameters: p, slot, .. } => {
            machine.enter(SEND_4207, &[callback, word(p[0].0), word(p[1].0), word(p[2].0), word(p[3].0), word(slot.0)])
        }
        LensPayload::Finish { value, record } => machine.enter(SEND_4208, &[callback, word(value.0), word(record_handle_word(record))]),
        LensPayload::Halt => machine.enter(SEND_4209, &[]),
        LensPayload::Step(step) => machine.enter(SEND_420B, &[callback, step.0 as isize as usize, 0, 1, 0, 0, 4]),
        LensPayload::Select { value, code, record } => {
            let code = match code {
                SelectionCode::Unconfigured => 11,
                SelectionCode::Configured => 12,
            };
            machine.enter(SEND_4211, &[callback, word(value.0), code, word(record_handle_word(record))])
        }
        LensPayload::Release => machine.enter(SEND_4212, &[]),
    }
}
