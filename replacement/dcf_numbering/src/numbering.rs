//! DCF folder and file numbering over the stock media context, through a [`Guest`] that
//! supplies memory and the retained services.

/// Guest memory and the retained services the numbering calls. Addresses are guest
/// addresses.
pub trait Guest {
    fn read_u8(&self, address: u32) -> u8;
    fn read_u16(&self, address: u32) -> u16;
    fn read_u32(&self, address: u32) -> u32;
    fn write_u8(&mut self, address: u32, value: u8);
    fn write_u16(&mut self, address: u32, value: u16);
    fn write_u32(&mut self, address: u32, value: u32);

    /// The media context, the word at `0x910204`.
    fn context(&self) -> u32;
    /// The six bytes `\DCIM\`.
    fn dcim_text(&self) -> u32;

    /// `0x71ddf0(0)`: zero when the card is usable.
    fn media_status(&mut self) -> u32;
    /// `0x71f708()`; zero when no drive is mounted.
    fn current_drive(&mut self) -> u32;
    /// `0x715f0c(mask)`: the lowest selected naming group, `0xff` for none.
    fn group_index(&mut self, mask: u32) -> u32;
    /// `0x726c74(2, 0x51)`: the folder-number-exhausted event.
    fn report_folder_limit(&mut self);
    /// `0x712d08(destination)`: drive letter, `:` and NUL.
    fn drive_prefix(&mut self, destination: u32);
    /// `0x70e6c4(destination, source, length)`.
    fn copy_bytes(&mut self, destination: u32, source: u32, length: u32);
    /// `0x70eaa0(destination, value, digits, 1)`: zero-padded decimal digits.
    fn format_decimal(&mut self, destination: u32, value: u32, digits: u32);
    /// `0x69f8fc(key)`: one configuration byte.
    fn configuration_byte(&mut self, key: u32) -> u32;
    /// `0x6a8040(drive, group)`: the persisted directory number.
    fn profile_directory(&mut self, drive: u32, group: u32) -> u32;
    /// `0x6a801c(drive, group)`: the persisted file number.
    fn profile_file(&mut self, drive: u32, group: u32) -> u32;
    /// `0x6a802c(drive, group, directory, file)`.
    fn store_profile_directory(&mut self, drive: u32, group: u32, directory: u32, file: u32);
    /// `0x6a8008(drive, group, file)`.
    fn store_profile_file(&mut self, drive: u32, group: u32, file: u32);
    /// `0x6a8050()`: persisted pair back to 100 and 0.
    fn reset_profile(&mut self);
    /// `0x712b4c()`: the numbering snapshot readers use.
    fn refresh_snapshot(&mut self);
    /// `0x43550(destination, 0, 30)`.
    fn clear_path_tail(&mut self, destination: u32);
    /// `0x722218(destination, number, 3, 1)`.
    fn format_folder_number(&mut self, destination: u32, number: u32);
    /// `0x722054(destination, source, 5)`.
    fn copy_folder_suffix(&mut self, destination: u32, source: u32);
    /// `0x701e18(path, entry, 0)`: opens or creates the directory, filling its entry.
    fn open_directory(&mut self, path: u32, entry: u32) -> u32;
    /// `0x713460(entry)`: the first cluster of a directory entry.
    fn entry_cluster(&mut self, entry: u32) -> u32;
    /// `0x7160b8(mask, path)`: the naming-group mask of a path.
    fn path_groups(&mut self, mask: u32, path: u32);
    /// `0x71d450(0, status)`.
    fn translate_status(&mut self, status: u32) -> u32;
}

// Media context fields.
const PREFIX: u32 = 0x0;
const WRITTEN_FILES: u32 = 0x1fc;
const MAXIMUM_DIRECTORY: u32 = 0x2168;
const MAXIMUM_FILE: u32 = 0x216c;
const LARGEST_FIRST_GROUP_DIRECTORY: u32 = 0x21b8;
const COUNTERS: u32 = 0x21d0;
const SELECTED_GROUPS: u32 = 0x2210;
const DIRECTORY_GROUPS: u32 = 0x2218;
const COUNTER_COUNT: u32 = 0x2600;
const DIRECTORY_CLUSTERS: u32 = 0x2604;
const SEQUENCE: u32 = 0x127d4;
const SEQUENCE_LATCH: u32 = 0x16bd0;
const PREFIX_MODE: u32 = 0x16bd4;
const LAST_SEQUENCE: u32 = 0x16c64;
const PROFILE_ENABLED: u32 = 0x16c6c;

// Counter record: count, directory, file, then the five-byte folder suffix.
const COUNTER_DIRECTORY: u32 = 4;
const COUNTER_FILE: u32 = 6;
const COUNTER_SUFFIX: u32 = 0xa;

// Path layout `L:\DCIM\NNN_SSSSS\PPPPFFFF`.
const PATH_DCIM: u32 = 2;
const PATH_FOLDER: u32 = 8;
const PATH_SUFFIX: u32 = 0xb;
const PATH_SEPARATOR: u32 = 0x10;
const PATH_PREFIX: u32 = 0x11;
const PATH_SEQUENCE: u32 = 0x13;
const PATH_FILE: u32 = 0x15;
const PATH_END: u32 = 0x19;

const NO_GROUP: u32 = 0xff;
const FIRST_FOLDER: u32 = 100;
const FOLDER_LIMIT: u32 = 1000;
const LAST_FILE: u32 = 9999;
const SEQUENCE_LIMIT: u32 = 99;

/// The result word of a numbering entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Done,
    /// No folder number below 1000 remains.
    FolderLimit,
    /// No drive, no naming group, an invalid name or an occupied folder.
    Refused,
    /// The nonzero media status, passed through.
    Media(u32),
}

impl Status {
    pub fn word(self) -> u32 {
        match self {
            Status::Done => 0,
            Status::FolderLimit => 6,
            Status::Refused => 8,
            Status::Media(word) => word,
        }
    }
}

/// A DCF folder and file number as the context stores them, in halfwords.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pair {
    pub directory: u16,
    pub file: u16,
}

/// The file-name prefix source selected by the save path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrefixMode {
    /// The four context bytes.
    Context,
    /// Configuration bytes `0x142`, `0x143` and the two-digit sequence.
    SequencedA,
    /// Configuration bytes `0x144`, `0x145` and the two-digit sequence.
    SequencedB,
}

impl PrefixMode {
    fn from_word(word: u32) -> Option<PrefixMode> {
        match word {
            0 => Some(PrefixMode::Context),
            1 => Some(PrefixMode::SequencedA),
            2 => Some(PrefixMode::SequencedB),
            _ => None,
        }
    }

    fn word(self) -> u32 {
        match self {
            PrefixMode::Context => 0,
            PrefixMode::SequencedA => 1,
            PrefixMode::SequencedB => 2,
        }
    }
}

/// The sequence latch: armed by a sequenced mode, consumed by the next name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Latch {
    Idle = 0,
    Consumed = 1,
    Armed = 2,
}

fn counter(context: u32, group: u32) -> u32 {
    context.wrapping_add(COUNTERS).wrapping_add(group.wrapping_mul(16))
}

fn directory_cluster(context: u32, number: u32) -> u32 {
    context.wrapping_add(DIRECTORY_CLUSTERS).wrapping_add(number.wrapping_mul(4))
}

fn directory_group(context: u32, number: u32) -> u32 {
    context.wrapping_add(DIRECTORY_GROUPS).wrapping_add(number)
}

/// Reciprocal multiplication: the image does not link `__aeabi_uidiv`.
fn tenths(value: u32) -> u32 {
    ((u64::from(value) * 0xcccc_cccd) >> 35) as u32
}

fn last_digit(value: u32) -> u32 {
    value - 10 * tenths(value)
}

fn thousands(value: u32) -> u32 {
    ((u64::from(value) * 0x1062_4dd3) >> 38) as u32
}

fn below_thousand(value: u32) -> u32 {
    value - 1000 * thousands(value)
}

/// The last file number of a folder's thousand block.
fn block_end(directory: u32) -> u32 {
    (last_digit(directory) * 1000 + 999) & 0xffff
}

/// The first file number of a folder's thousand block.
fn block_start(directory: u32) -> u32 {
    (last_digit(directory) * 1000 + 1) & 0xffff
}

/// Entered at `0x70f790`.
pub fn set_prefix_mode<G: Guest>(guest: &mut G, word: u32) {
    let Some(mode) = PrefixMode::from_word(word) else { return };
    let latch = if mode == PrefixMode::Context { Latch::Idle } else { Latch::Armed };
    let context = guest.context();
    guest.write_u8(context + SEQUENCE_LATCH, latch as u8);
    guest.write_u32(context + PREFIX_MODE, mode.word());
}

/// Entered at `0x70f840`: whether `length` bytes at `name` are all `[A-Za-z0-9_]`.
pub fn check_name<G: Guest>(guest: &G, name: u32, length: u32) -> Status {
    for index in 0..length {
        let byte = guest.read_u8(name.wrapping_add(index));
        if !(byte.is_ascii_alphanumeric() || byte == b'_' || (cfg!(dcf_control_name_accepts_dash) && byte == b'-')) {
            return Status::Refused;
        }
    }
    Status::Done
}

/// Entered at `0x7159f8`: the persisted pair of a naming group.
pub fn read_profile<G: Guest>(guest: &mut G, drive: u32, group: u32) -> Pair {
    let directory = guest.profile_directory(drive, group) as u16;
    let file = guest.profile_file(drive, group) as u16;
    Pair { directory, file }
}

/// Entered at `0x715a2c`: persists a naming group's pair.
pub fn write_profile<G: Guest>(guest: &mut G, drive: u32, group: u32, directory: u32, file: u32) {
    guest.store_profile_directory(drive, group, directory, file);
    guest.store_profile_file(drive, group, file);
}

/// Entered at `0x70faa0`: FRAME NO. CONT (1) persists every group's pair, any other value
/// resets the persisted pair; the value then selects whether naming consults it.
pub fn apply_continuous_numbering<G: Guest>(guest: &mut G, enabled: u32) {
    let status = guest.media_status();
    let drive = guest.current_drive();
    if status == 0 && drive != 0 {
        if enabled == 1 {
            let mut group: u8 = 0;
            while guest.read_u8(guest.context() + COUNTER_COUNT) > group {
                let record = counter(guest.context(), u32::from(group));
                let directory = u32::from(guest.read_u16(record + COUNTER_DIRECTORY));
                let file = u32::from(guest.read_u16(record + COUNTER_FILE));
                write_profile(guest, drive, u32::from(group), directory, file);
                group = group.wrapping_add(1);
            }
        } else {
            guest.reset_profile();
        }
    }
    let context = guest.context();
    guest.write_u32(context + PROFILE_ENABLED, enabled);
}

/// Entered at `0x716324`: the last-used pair of a naming group, from the card scan and,
/// when continuous numbering is on, the persisted pair.
pub fn last_used<G: Guest>(guest: &mut G, drive: u32, group: u32) -> Pair {
    let context = guest.context();
    let record = counter(context, group);
    let mut directory = u32::from(guest.read_u16(record + COUNTER_DIRECTORY));
    let mut file = u32::from(guest.read_u16(record + COUNTER_FILE));
    if directory < FIRST_FOLDER {
        let maximum = guest.read_u32(context + MAXIMUM_DIRECTORY);
        if maximum <= FIRST_FOLDER && guest.read_u32(directory_cluster(context, FIRST_FOLDER)) == 0 {
            directory = FIRST_FOLDER;
            file = 0;
        } else {
            directory = maximum & 0xffff;
            file = block_end(directory);
        }
    }
    let persisted = if guest.read_u32(context + PROFILE_ENABLED) == 0 || cfg!(dcf_control_profile_ignored) {
        Pair { directory: FIRST_FOLDER as u16, file: 0 }
    } else {
        let mut persisted = read_profile(guest, drive, group);
        let ahead = directory < u32::from(persisted.directory)
            || (directory == u32::from(persisted.directory) && u32::from(persisted.file) > file);
        if ahead {
            let context = guest.context();
            let owner = u32::from(guest.read_u8(directory_group(context, u32::from(persisted.directory))));
            if owner != group && owner != NO_GROUP {
                let maximum = guest.read_u16(context + MAXIMUM_DIRECTORY);
                persisted = Pair { directory: maximum, file: block_end(u32::from(maximum)) as u16 };
            }
        }
        persisted
    };
    let persisted_directory = u32::from(persisted.directory);
    if directory < persisted_directory {
        directory = persisted_directory;
        file = u32::from(persisted.file);
    } else if directory == persisted_directory && u32::from(persisted.file) > file {
        file = u32::from(persisted.file);
    }
    if group == 0 {
        let context = guest.context();
        let maximum = guest.read_u32(context + MAXIMUM_DIRECTORY);
        if directory < maximum {
            directory = maximum & 0xffff;
            file = block_end(directory);
        } else if directory == maximum
            && directory != guest.read_u32(context + LARGEST_FIRST_GROUP_DIRECTORY)
            && (directory > FIRST_FOLDER || guest.read_u32(directory_cluster(context, FIRST_FOLDER)) != 0)
        {
            file = block_end(directory);
        }
    }
    Pair { directory: directory as u16, file: file as u16 }
}

/// Entered at `0x70fb68`: the last-used pair of the selected naming group.
pub fn read_numbers<G: Guest>(guest: &mut G) -> Result<Pair, Status> {
    let mask = guest.read_u32(guest.context() + SELECTED_GROUPS);
    let status = guest.media_status();
    if status != 0 {
        return Err(Status::Media(status));
    }
    let drive = guest.current_drive();
    if drive == 0 {
        return Err(Status::Refused);
    }
    let group = guest.group_index(mask);
    if group == NO_GROUP {
        return Err(Status::Refused);
    }
    Ok(last_used(guest, drive, group))
}

/// The folder after `previous`, or past the highest folder on the card.
fn folder_after(previous: u32, maximum: u32) -> u32 {
    (if previous < maximum { maximum.wrapping_add(1) } else { previous + 1 }) & 0xffff
}

fn next_folder<G: Guest>(guest: &mut G, previous: u32) -> Result<u32, Status> {
    let next = folder_after(previous, guest.read_u32(guest.context() + MAXIMUM_DIRECTORY));
    if next >= FOLDER_LIMIT {
        guest.report_folder_limit();
        return Err(Status::FolderLimit);
    }
    Ok(next)
}

fn restart_sequence<G: Guest>(guest: &mut G) -> u32 {
    let context = guest.context();
    guest.write_u16(context + SEQUENCE, 1);
    1
}

fn sequence_exhausted<G: Guest>(guest: &G, sequence: u32) -> bool {
    guest.read_u8(guest.context() + SEQUENCE_LATCH) != Latch::Idle as u8 && sequence > SEQUENCE_LIMIT
}

struct Next {
    directory: u32,
    file: u32,
    sequence: u32,
}

#[inline(never)]
fn allocate<G: Guest>(guest: &mut G, last: Pair, mut sequence: u32) -> Result<Next, Status> {
    let previous = u32::from(last.directory);
    let mut directory = previous;
    let mut file = u32::from(last.file);
    if last_digit(previous) == thousands(file) {
        file = (file + 1) & 0xffff;
        if below_thousand(file) == 0 || sequence_exhausted(guest, sequence) {
            directory = next_folder(guest, previous)?;
            file = block_start(directory);
            if !cfg!(dcf_control_block_sequence_kept) {
                sequence = restart_sequence(guest);
            }
        }
        return Ok(Next { directory, file, sequence });
    }
    let stepped = (file + 1) & 0xffff;
    let wrapped = stepped > LAST_FILE;
    if wrapped {
        file = 1;
        directory = next_folder(guest, previous)?;
        sequence = restart_sequence(guest);
    } else if below_thousand(stepped) == 0 {
        file = (file + 2) & 0xffff;
    } else {
        file = stepped;
    }
    let block_of_file = thousands(file);
    if block_of_file == last_digit(directory) {
        return Ok(Next { directory, file, sequence });
    }
    if wrapped || block_of_file > last_digit(directory) {
        let maximum = guest.read_u32(guest.context() + MAXIMUM_DIRECTORY);
        directory = folder_after(previous, maximum);
        let block = last_digit(thousands(file));
        while last_digit(directory) != block {
            directory = (directory + 1) & 0xffff;
        }
        if directory >= FOLDER_LIMIT {
            guest.report_folder_limit();
            return Err(Status::FolderLimit);
        }
        sequence = restart_sequence(guest);
    } else if sequence_exhausted(guest, sequence) {
        directory = next_folder(guest, previous)?;
        file = block_start(directory);
        sequence = restart_sequence(guest);
    } else {
        file = block_start(previous);
    }
    Ok(Next { directory, file, sequence })
}

/// Entered at `0x70fbfc`: writes the next `L:\DCIM\NNN_SSSSS\PPPPFFFF` base name into `path`.
/// A nonzero `suffix` or `prefix` names caller text that replaces the context's when valid.
pub fn prepare_name<G: Guest>(guest: &mut G, path: u32, suffix: u32, prefix: u32) -> Status {
    let status = guest.media_status();
    if status != 0 {
        return Status::Media(status);
    }
    let group = guest.group_index(guest.read_u32(guest.context() + SELECTED_GROUPS));
    if group == NO_GROUP {
        return Status::Refused;
    }
    let context = guest.context();
    if guest.read_u8(context + SEQUENCE_LATCH) == Latch::Armed as u8 && !cfg!(dcf_control_latch_kept) {
        let sequence = guest.read_u16(context + SEQUENCE).wrapping_add(1);
        guest.write_u16(context + SEQUENCE, sequence);
        guest.write_u8(context + SEQUENCE_LATCH, Latch::Consumed as u8);
    }
    let sequence = u32::from(guest.read_u16(context + SEQUENCE));
    let last = match read_numbers(guest) {
        Ok(last) => last,
        Err(status) => return status,
    };
    let next = match allocate(guest, last, sequence) {
        Ok(next) => next,
        Err(status) => return status,
    };

    guest.drive_prefix(path);
    let dcim = guest.dcim_text();
    guest.copy_bytes(path + PATH_DCIM, dcim, 6);
    guest.format_decimal(path + PATH_FOLDER, next.directory, 3);
    let own_suffix = counter(guest.context(), group) + COUNTER_SUFFIX;
    let suffix = if suffix != 0 && check_name(guest, suffix, 5) == Status::Done { suffix } else { own_suffix };
    guest.copy_bytes(path + PATH_SUFFIX, suffix, 5);
    guest.write_u8(path + PATH_SEPARATOR, b'\\');
    if prefix != 0 {
        let prefix = if check_name(guest, prefix, 4) == Status::Done { prefix } else { guest.context() + PREFIX };
        guest.copy_bytes(path + PATH_PREFIX, prefix, 4);
    } else {
        let keys = match PrefixMode::from_word(guest.read_u32(guest.context() + PREFIX_MODE)) {
            Some(PrefixMode::Context) => {
                let own = guest.context() + PREFIX;
                guest.copy_bytes(path + PATH_PREFIX, own, 4);
                None
            }
            Some(PrefixMode::SequencedA) => Some((0x142, 0x143)),
            Some(PrefixMode::SequencedB) => Some((0x144, 0x145)),
            None => None,
        };
        if let Some((first, second)) = keys {
            let byte = guest.configuration_byte(first) as u8;
            guest.write_u8(path + PATH_PREFIX, byte);
            let byte = guest.configuration_byte(second) as u8;
            guest.write_u8(path + PATH_PREFIX + 1, byte);
            guest.format_decimal(path + PATH_SEQUENCE, next.sequence, 2);
        }
    }
    guest.format_decimal(path + PATH_FILE, next.file, 4);
    guest.write_u8(path + PATH_END, 0);
    Status::Done
}

/// Entered at `0x71141c` after a written file in context slot `slot` closes: advances its
/// group's last-used pair, persists it and prepares the next folder.
pub fn note_written<G: Guest>(guest: &mut G, slot: u32, scratch: FolderScratch) {
    let context = guest.context();
    let entry = context.wrapping_add(slot.wrapping_mul(16));
    let directory = u32::from(guest.read_u16(entry + 4));
    let file = u32::from(guest.read_u16(entry + 6));
    let group = u32::from(guest.read_u8(directory_group(context, directory)));
    let record = counter(context, group);
    let known = u32::from(guest.read_u16(record + COUNTER_DIRECTORY));
    let newer = directory > known || (directory == known && u32::from(guest.read_u16(record + COUNTER_FILE)) < file);
    if !newer {
        return;
    }
    guest.write_u16(record + COUNTER_DIRECTORY, directory as u16);
    guest.write_u16(record + COUNTER_FILE, file as u16);
    let count = guest.read_u32(record);
    guest.write_u32(record, count.wrapping_add(1));
    let drive = guest.current_drive();
    if !cfg!(dcf_control_written_unpersisted) {
        write_profile(guest, drive, group, directory, file);
    }

    let context = guest.context();
    let sequence = guest.read_u16(context + SEQUENCE);
    guest.write_u16(context + LAST_SEQUENCE, sequence);
    let written = guest.read_u32(context + WRITTEN_FILES);
    guest.write_u32(context + WRITTEN_FILES, written.wrapping_add(1));
    let maximum = guest.read_u32(context + MAXIMUM_DIRECTORY);
    if maximum < directory || (maximum == directory && guest.read_u32(context + MAXIMUM_FILE) < file) {
        guest.write_u32(context + MAXIMUM_DIRECTORY, directory);
        guest.write_u32(context + MAXIMUM_FILE, file);
    }

    let context = guest.context();
    let maximum = guest.read_u32(context + MAXIMUM_DIRECTORY);
    if guest.read_u8(directory_group(context, maximum)) != 0 {
        let mut next = maximum.wrapping_add(1) & 0xffff;
        let drive = guest.current_drive();
        let persisted = u32::from(read_profile(guest, drive, 0).directory);
        let context = guest.context();
        if guest.read_u32(context + PROFILE_ENABLED) == 1 && next < persisted {
            next = persisted;
        }
        create_folder(guest, next, counter(context, 0) + COUNTER_SUFFIX, scratch);
    }
    guest.refresh_snapshot();
}

/// Guest scratch for [`create_folder`]: the path request, the directory entry and the
/// group mask.
#[derive(Clone, Copy, Debug)]
pub struct FolderScratch {
    pub request: u32,
    pub entry: u32,
    pub mask: u32,
}

/// Bytes of the path request, the leading 18 of them the `A:\DCIM\100_FUJI\` template.
pub const FOLDER_REQUEST_BYTES: u32 = 48;
/// Bytes the directory service may fill.
pub const FOLDER_ENTRY_BYTES: u32 = 0x44;
/// Offset of the short directory entry the cluster service reads.
const FOLDER_ENTRY_RECORD: u32 = 0x24;
const FOLDER_TEMPLATE_BYTES: u32 = 18;

/// `A:\DCIM\100_FUJI\` and NUL, as the request's first four words and a halfword.
const FOLDER_TEMPLATE_WORDS: [u32; 4] = [0x445c_3a41, 0x5c4d_4943, 0x5f30_3031, 0x494a_5546];
const FOLDER_TEMPLATE_TAIL: u16 = 0x005c;

/// Entered at `0x716184`: creates folder `number` with the five-byte `suffix` when it is in
/// 100..999 and not on the card, and records it in the catalog.
pub fn create_folder<G: Guest>(guest: &mut G, number: u32, suffix: u32, scratch: FolderScratch) -> u32 {
    let request = scratch.request;
    for (index, word) in FOLDER_TEMPLATE_WORDS.iter().enumerate() {
        guest.write_u32(request + 4 * index as u32, *word);
    }
    guest.write_u16(request + 16, FOLDER_TEMPLATE_TAIL);
    guest.clear_path_tail(request + FOLDER_TEMPLATE_BYTES);
    if number.wrapping_sub(FIRST_FOLDER) & 0xffff >= FOLDER_LIMIT - FIRST_FOLDER {
        return Status::Refused.word();
    }
    let drive = guest.current_drive();
    if drive == 0 {
        return Status::Refused.word();
    }
    guest.write_u8(request, drive as u8);
    guest.format_folder_number(request + PATH_FOLDER, number);
    guest.copy_folder_suffix(request + PATH_SUFFIX, suffix);
    if guest.read_u32(directory_cluster(guest.context(), number)) != 0 {
        return Status::Refused.word();
    }
    let status = guest.open_directory(request, scratch.entry);
    if status == 0 {
        let cluster = guest.entry_cluster(scratch.entry + FOLDER_ENTRY_RECORD);
        let context = guest.context();
        if !cfg!(dcf_control_folder_cluster_unrecorded) {
            guest.write_u32(directory_cluster(context, number), cluster);
        }
        guest.write_u32(scratch.mask, 0);
        guest.path_groups(scratch.mask, request);
        let group = guest.group_index(guest.read_u32(scratch.mask));
        if group != NO_GROUP {
            let context = guest.context();
            guest.write_u8(directory_group(context, number), group as u8);
            let record = counter(context, group);
            if u32::from(guest.read_u16(record + COUNTER_DIRECTORY)) < number {
                guest.write_u16(record + COUNTER_DIRECTORY, number as u16);
                guest.write_u16(record + COUNTER_FILE, 0);
                if group == 0 {
                    guest.write_u32(context + LARGEST_FIRST_GROUP_DIRECTORY, number);
                }
            }
        }
        let context = guest.context();
        if number > guest.read_u32(context + MAXIMUM_DIRECTORY) {
            guest.write_u32(context + MAXIMUM_DIRECTORY, number);
            guest.write_u32(context + MAXIMUM_FILE, 0);
        }
    }
    guest.translate_status(status)
}
