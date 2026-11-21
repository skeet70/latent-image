//! The media task's volume workers: attach the volume a letter names (0x2c00), detach the
//! attached volumes (0x2c01), and format (0x2c11) or reformat (0x2c12) a volume and reset the
//! media-state context. The attach and detach workers hand their callback the r0 the last
//! retained call left.

use crate::requests::Callback;
use crate::workers::Offset;

/// A volume slot index as 0x70ec84 and 0x71f4f8 return it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Volume(pub u32);

impl Volume {
    pub const NONE: Volume = Volume(u32::MAX);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Letter(pub u8);

/// The media-state context address held at 0x910204.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Context(pub u32);

impl Context {
    pub const NONE: Context = Context(0);
}

/// A byte offset into the media-state context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field(pub u32);

/// The r0 a retained call returns with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Residue(pub u32);

const CALLBACK: Offset = Offset(0);

mod attach {
    use super::Offset;
    pub const DESCRIPTOR: Offset = Offset(4);
    pub const LETTER: Offset = Offset(0x38);
    pub const FLAGGED: Offset = Offset(0x3c);
}

mod detach {
    use super::Offset;
    pub const REPEAT: Offset = Offset(4);
}

mod format {
    use super::Offset;
    pub const LETTER: Offset = Offset(4);
}

mod field {
    use super::Field;
    pub const VOLUME: Field = Field(0x114);
    pub const DEFAULTS: Field = Field(0x118);
    pub const DEFAULTS_KIND: Field = Field(0x1f0);
    pub const CLEARED: [Field; 11] = [
        Field(0x1fc),
        Field(0x2168),
        Field(0x216c),
        Field(0x2148),
        Field(0x214c),
        Field(0x2150),
        Field(0x2154),
        Field(0x2158),
        Field(0x215c),
        Field(0x2164),
        Field(0x2160),
    ];
    pub const NUMBERS: Field = Field(0x35a4);
    pub const DIRECTORIES: Field = Field(0x2604);
    pub const ZEROED: [Field; 2] = [Field(0x21bc), Field(0x21c0)];
    pub const HUNDREDS: [Field; 3] = [Field(0x2180), Field(0x2184), Field(0x2168)];
    pub const THOUSANDS: [Field; 2] = [Field(0x2178), Field(0x217c)];
    pub const NEXT_THOUSANDS: [Field; 2] = [Field(0x218c), Field(0x2190)];
    pub const SLOT_CURSOR: Field = Field(0x2214);
    pub const SLOT_COUNT: Field = Field(0x2600);
    /// Slot records of 16 bytes: a word at +0 and halfwords at +4, +6 and +8.
    pub const SLOTS: Field = Field(0x21d0);
    pub const MEDIA_STATE: Field = Field(0x21cc);
    pub const ATTACHED: Field = Field(0x2210);
    pub const MEDIA_FLAGS: Field = Field(0x16bd8);
    pub const PENDING: [Field; 2] = [Field(0x16c6a), Field(0x16c6b)];
}

const NUMBERS_BYTES: u32 = 2000;
const DIRECTORIES_BYTES: u32 = 4000;
const SLOT_BYTES: u32 = 16;
const DEFAULTS_KIND: u32 = 9;
const MEDIA_STATE_RESET: u32 = 2;
const FLAGGED: u32 = 1;
const ATTACHED: u32 = 1;
/// The only media flag bit that still selects the 0x719de0 rebuild.
const REBUILD_FLAG: u32 = 0x10;
/// The status 0x2c11 and 0x2c12 deliver without a context or a volume.
const NO_VOLUME: u32 = 5;
/// The 0x910208 word with no volume attached.
const DETACHED: u32 = u32::MAX;
const LEVEL_SLOT: u32 = 0;
const LEVEL: u32 = 10;
const VOLUME_SLOTS: u32 = 2;

pub trait Port {
    fn word(&self, at: Offset) -> u32;
    fn set_word(&mut self, at: Offset, value: u32);
    fn byte(&self, at: Offset) -> u8;

    fn context(&self) -> Context;
    fn set_context(&mut self, context: Context);
    /// The word at 0x910208.
    fn attach_state(&self) -> u32;
    fn field(&self, context: Context, at: Field) -> u32;
    fn field_byte(&self, context: Context, at: Field) -> u8;
    fn set_field(&mut self, context: Context, at: Field, value: u32);
    fn set_field_half(&mut self, context: Context, at: Field, value: u16);
    fn set_field_byte(&mut self, context: Context, at: Field, value: u8);

    fn complete(&mut self, callback: Callback, value: u32);

    fn device_for_letter(&mut self, letter: Letter) -> Volume;
    /// The volume slot's record pointer at 0x917f64.
    fn volume_record(&mut self, volume: Volume) -> u32;
    fn release_volume(&mut self, volume: Volume) -> Residue;
    fn switch_volume(&mut self, from: Volume, to: Volume) -> Residue;
    /// Another occupied volume slot than `volume`.
    fn other_volume(&mut self, volume: Volume) -> Volume;
    /// 0x71f7dc with the request.
    fn attach(&mut self) -> Residue;
    /// 0x71fcbc with the request.
    fn reattach(&mut self) -> Residue;
    /// 0x71fa9c with the request.
    fn detach(&mut self) -> Residue;
    /// 0x71e100 with the request.
    fn format(&mut self, volume: Volume) -> u32;
    /// 0x71e1bc with the request.
    fn reformat(&mut self, volume: Volume) -> u32;
    /// Copies the 216 default bytes at 0x814558 to the field.
    fn copy_defaults(&mut self, context: Context, at: Field) -> Residue;
    fn fill_zero(&mut self, context: Context, at: Field, bytes: u32);
    fn snapshot_media_state(&mut self);
    fn set_level(&mut self, slot: u32, level: u32);
    fn flagged_prepare(&mut self);
    fn catalog_rebuild(&mut self, first: u32, second: u32) -> Residue;
    fn flagged_select(&mut self, first: u32, second: u32) -> Residue;
    /// 0x6ed770.
    fn no_volume_status(&mut self) -> Residue;
}

/// The attach worker at 0x7120d8 for code 0x2c00; it returns 0.
pub fn attach<P: Port>(port: &mut P) -> u32 {
    let callback = Callback(port.word(CALLBACK));
    let volume = port.device_for_letter(Letter(port.byte(attach::LETTER)));
    if volume == Volume::NONE {
        port.complete(callback, volume.0);
        port.set_level(LEVEL_SLOT, LEVEL);
        return 0;
    }
    if port.context() != Context::NONE {
        release_others(port, volume);
        select(port, volume);
    }
    let context = port.context();
    let mut residue = if context != Context::NONE && port.attach_state() != DETACHED && port.field(context, field::ATTACHED) == ATTACHED {
        port.reattach()
    } else {
        attach_fresh(port)
    };
    if port.word(attach::FLAGGED) == FLAGGED {
        let context = port.context();
        residue = if port.field(context, field::MEDIA_FLAGS) & !REBUILD_FLAG == 0 {
            port.catalog_rebuild(0, 0)
        } else {
            port.flagged_select(0, 1)
        };
    }
    port.complete(callback, residue.0);
    port.set_level(LEVEL_SLOT, LEVEL);
    0
}

/// Releases each occupied slot holding the descriptor under another volume, and the
/// volume's own slot holding another descriptor.
fn release_others<P: Port>(port: &mut P, volume: Volume) {
    for slot in (0..VOLUME_SLOTS).map(Volume) {
        let record = port.volume_record(slot);
        if record == 0 {
            continue;
        }
        let release = if port.word(attach::DESCRIPTOR) == record { slot != volume } else { slot == volume };
        if release {
            port.release_volume(slot);
        }
    }
}

/// Makes the volume current: a context of another volume either switches to it or, when it
/// is the descriptor's, is dropped with the volume's slot.
fn select<P: Port>(port: &mut P, volume: Volume) {
    let descriptor = port.word(attach::DESCRIPTOR);
    let context = port.context();
    if descriptor == context.0 {
        if port.field(Context(descriptor), field::VOLUME) == volume.0 {
            return;
        }
    } else {
        let current = Volume(port.field(context, field::VOLUME));
        if current != volume {
            port.switch_volume(current, volume);
            return;
        }
    }
    port.set_context(Context::NONE);
    port.release_volume(volume);
}

fn attach_fresh<P: Port>(port: &mut P) -> Residue {
    if port.word(attach::FLAGGED) == FLAGGED {
        port.flagged_prepare();
    }
    port.set_word(CALLBACK, 0);
    port.attach();
    let context = port.context();
    let residue = port.copy_defaults(context, field::DEFAULTS);
    let context = port.context();
    if !cfg!(file_service_control_volume_attach_kind_unset) {
        port.set_field(context, field::DEFAULTS_KIND, DEFAULTS_KIND);
    }
    residue
}

/// The detach worker at 0x7122dc for code 0x2c01: detaches the current volume and switches
/// to another occupied slot, once or, with a nonzero repeat word, until none is left. It
/// returns 0.
pub fn detach_all<P: Port>(port: &mut P) -> u32 {
    let callback = Callback(port.word(CALLBACK));
    port.set_word(CALLBACK, 0);
    let repeat = port.word(detach::REPEAT);
    let mut residue = port.no_volume_status();
    if port.context() != Context::NONE {
        loop {
            let context = port.context();
            let volume = Volume(port.field(context, field::VOLUME));
            if context != Context::NONE {
                for pending in field::PENDING {
                    port.set_field_byte(context, pending, 0);
                }
            }
            port.detach();
            port.release_volume(volume);
            let other = port.other_volume(volume);
            if other == Volume::NONE {
                residue = Residue(other.0);
                break;
            }
            residue = port.switch_volume(volume, other);
            let context = port.context();
            port.set_field(context, field::VOLUME, other.0);
            if repeat == 0 {
                break;
            }
        }
    }
    if !cfg!(file_service_control_volume_detach_callback_unrestored) {
        port.set_word(CALLBACK, callback.0);
    }
    port.complete(callback, residue.0);
    0
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    /// 0x7123b4 for code 0x2c11: through 0x71e100, also resetting the slot records.
    Armed,
    /// 0x712580 for code 0x2c12: through 0x71e1bc.
    Plain,
}

/// The format worker at 0x7123b4 for code 0x2c11; it returns the status.
pub fn format_armed<P: Port>(port: &mut P) -> u32 {
    format_volume(port, Format::Armed)
}

/// The format worker at 0x712580 for code 0x2c12; it returns the status.
pub fn format<P: Port>(port: &mut P) -> u32 {
    format_volume(port, Format::Plain)
}

fn format_volume<P: Port>(port: &mut P, kind: Format) -> u32 {
    let callback = Callback(port.word(CALLBACK));
    port.set_word(CALLBACK, 0);
    let volume = port.device_for_letter(Letter(port.byte(format::LETTER)));
    let context = port.context();
    let status = if context == Context::NONE || volume == Volume::NONE {
        NO_VOLUME
    } else {
        for pending in field::PENDING {
            port.set_field_byte(context, pending, 0);
        }
        let status = match kind {
            Format::Armed => port.format(volume),
            Format::Plain => port.reformat(volume),
        };
        if status == 0 {
            reset(port, kind);
        }
        status
    };
    port.complete(callback, status);
    status
}

fn reset<P: Port>(port: &mut P, kind: Format) {
    let context = port.context();
    for cleared in field::CLEARED {
        port.set_field(context, cleared, 0);
    }
    port.fill_zero(context, field::NUMBERS, NUMBERS_BYTES);
    let context = port.context();
    port.fill_zero(context, field::DIRECTORIES, DIRECTORIES_BYTES);
    let context = port.context();
    for zeroed in field::ZEROED {
        port.set_field(context, zeroed, 0);
    }
    for at in field::HUNDREDS {
        port.set_field(context, at, 100);
    }
    for at in field::THOUSANDS {
        port.set_field(context, at, 1000);
    }
    for at in field::NEXT_THOUSANDS {
        port.set_field(context, at, 1001);
    }
    if kind == Format::Armed {
        port.set_field(context, field::SLOT_CURSOR, 0);
        if !cfg!(file_service_control_volume_format_slots_kept) {
            let count = u32::from(port.field_byte(context, field::SLOT_COUNT));
            for slot in 0..count {
                let record = field::SLOTS.0 + SLOT_BYTES * slot;
                port.set_field(context, Field(record), 0);
                for half in [4, 6, 8] {
                    port.set_field_half(context, Field(record + half), 0);
                }
            }
        }
    }
    port.snapshot_media_state();
    let context = port.context();
    if !cfg!(file_service_control_volume_reformat_state_unset) {
        port.set_field(context, field::MEDIA_STATE, MEDIA_STATE_RESET);
    }
}
