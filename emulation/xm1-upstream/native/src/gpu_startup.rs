//! State-only paired-list initialization.
//! The two-tick completion delay is unmeasured.

const PATH_WORDS: usize = 32;
const INITIALIZED_PATH_WORDS: usize = 16;
const FILL_WORDS: usize = 592;
const MAX_FRAME_BYTES: u32 = 64 * 1024 * 1024;
const COMPLETION_STATUS: u32 = 0x8000_0000;
const COMPLETION_IRQ: u8 = 0x3c;

const PATH: [u32; PATH_WORDS] = [
    0x40080081, 0x01000000, 0x400f007b, 0,
    0x40080040, 0, 0x40070040, 0x003f0000,
    0x40070041, 0, 0x40070042, 0,
    0x40070043, 0, 0x40070044, 0x003f0000,
    0x40070045, 0, 0x40070046, 0,
    0x40070047, 0, 0x40070048, 0x003f0000,
    0x40020080, 0, 0x40010060, 2,
    0x40010081, 1, 0x40010060, 2,
];

const INITIALIZED_PATH: [u32; INITIALIZED_PATH_WORDS] = [
    0x40080081, 0x01000000, 0x400f007b, 0,
    0x40010060, 2, 0x40010081, 1,
    0, 0, 0, 0, 0, 0, 0, 0,
];

const FILL_PREFIX: [u32; 36] = [
    0x40080140, 0, 0x40010190, 0, 0x400101c0, 0, 0x4001014d, 1,
    0x40010140, 3, 0x4001014d, 0, 0x400f014d, 0, 0x400f014d, 0,
    0x40040140, 0x50000, 0x40010140, 0, 0x400f0143, 0, 0x400f0153, 0,
    0x400f0144, 0, 0x400f0154, 0, 0x400101a2, 0, 0x400f01a3, 0,
    0x40040184, 0, 0x40010187, 0,
];

const FILL_SUFFIX: [u32; 20] = [
    0x40020140, 0, 0x400f014d, 0, 0x400f014d, 0, 0x40010178, 1,
    0x400f016c, 0x40001, 0x400101a7, 0, 0x400201a7, 0x100, 0x40010147, 1,
    0x40010157, 1, 0x400801a0, 0,
];

const ZERO_CLIP: [u32; 4] = [0x40010100, 0, 0x40010115, 0];
const FOOTER: [u32; 6] = [
    0x400f0028, 0x12345678, 0x40010031, 0xf0, 0x400f0018, 0x12345678,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CoreId(pub u8);

impl CoreId {
    fn validate(self) -> Result<(), Error> {
        if self.0 <= 1 { Ok(()) } else { Err(Error::InvalidCore(self)) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RamBytes(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VirtualTick(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AckRegister {
    Path,
    Fill,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Error {
    InvalidCore(CoreId),
    Busy,
    PathNotStarted,
    SubmitterMismatch { expected: CoreId, actual: CoreId },
    PathChanged,
    PathLength { actual: usize },
    UnsupportedPath { word: usize },
    InvalidDimensions,
    FrameTooLarge,
    ArithmeticOverflow,
    FillLength { actual: usize },
    UnsupportedFill { word: usize },
    InvalidGamma { entry: usize },
    InvalidOrder,
    InvalidTarget,
    TargetOutOfRange,
    NonzeroAcknowledgment,
    NoCompletion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Completion {
    pub(crate) core: CoreId,
    pub(crate) irq: u8,
    pub(crate) status: u32,
}

struct Dimensions {
    width: u32,
    height: u32,
    pixels: u32,
    frame_bytes: u32,
}

impl Dimensions {
    fn parse(word: u32, ram: RamBytes) -> Result<Self, Error> {
        let width = word >> 16;
        let height = word & 0xffff;
        if width == 0 || height == 0 || width & 7 != 0 || height & 7 != 0 {
            return Err(Error::InvalidDimensions);
        }
        let pixels = width.checked_mul(height).ok_or(Error::ArithmeticOverflow)?;
        let frame_bytes = pixels.checked_mul(4).ok_or(Error::ArithmeticOverflow)?;
        let dimensions = Self { width, height, pixels, frame_bytes };
        dimensions.validate_ram(ram)?;
        Ok(dimensions)
    }

    fn validate_ram(&self, ram: RamBytes) -> Result<(), Error> {
        if self.frame_bytes > MAX_FRAME_BYTES.min(ram.0) {
            return Err(Error::FrameTooLarge);
        }
        Ok(())
    }

    fn reversed_word(&self) -> u32 {
        self.height << 16 | self.width
    }
}

pub(crate) enum PathWords {
    FirstGlobal([u32; PATH_WORDS]),
    Initialized([u32; INITIALIZED_PATH_WORDS]),
}

impl PathWords {
    pub(crate) fn as_slice(&self) -> &[u32] {
        match self {
            Self::FirstGlobal(words) => words,
            Self::Initialized(words) => words,
        }
    }
}

struct PendingPath {
    core: CoreId,
    words: PathWords,
    dimensions: Dimensions,
}

#[derive(Clone, Copy)]
enum OutstandingAcks {
    Both,
    Path,
    Fill,
}

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Path(PendingPath),
    Running { deadline: VirtualTick, irq_core: CoreId },
    Completed(OutstandingAcks),
}

#[derive(Default)]
pub(crate) struct Device {
    phase: Phase,
}

impl Device {
    pub(crate) const fn new() -> Self {
        Self { phase: Phase::Idle }
    }

    pub(crate) fn start_path(
        &mut self,
        core: CoreId,
        words: &[u32],
        ram: RamBytes,
    ) -> Result<(), Error> {
        core.validate()?;
        if !matches!(self.phase, Phase::Idle) {
            return Err(Error::Busy);
        }
        let captured = match words.len() {
            PATH_WORDS => {
                let mut captured = PATH;
                captured[3] = words[3];
                PathWords::FirstGlobal(captured)
            }
            INITIALIZED_PATH_WORDS => {
                let mut captured = INITIALIZED_PATH;
                captured[3] = words[3];
                PathWords::Initialized(captured)
            }
            _ => return Err(Error::PathLength { actual: words.len() }),
        };
        for (word, (&actual, &expected)) in words.iter().zip(captured.as_slice()).enumerate() {
            if actual != expected {
                return Err(Error::UnsupportedPath { word });
            }
        }
        let dimensions = Dimensions::parse(words[3], ram)?;
        self.phase = Phase::Path(PendingPath { core, words: captured, dimensions });
        Ok(())
    }

    pub(crate) fn start_fill(
        &mut self,
        core: CoreId,
        path: &[u32],
        fill: &[u32],
        irq_core: CoreId,
        now: VirtualTick,
        ram: RamBytes,
    ) -> Result<VirtualTick, Error> {
        core.validate()?;
        irq_core.validate()?;
        let pending = match &self.phase {
            Phase::Path(pending) => pending,
            Phase::Idle => return Err(Error::PathNotStarted),
            _ => return Err(Error::Busy),
        };
        if core != pending.core {
            return Err(Error::SubmitterMismatch { expected: pending.core, actual: core });
        }
        if path != pending.words.as_slice() {
            return Err(Error::PathChanged);
        }
        pending.dimensions.validate_ram(ram)?;
        validate_fill(fill, &pending.dimensions, ram)?;
        let deadline = VirtualTick(now.0.checked_add(2).ok_or(Error::ArithmeticOverflow)?);
        self.phase = Phase::Running { deadline, irq_core };
        Ok(deadline)
    }

    pub(crate) fn complete(&mut self, now: VirtualTick) -> Result<Option<Completion>, Error> {
        let core = match &self.phase {
            Phase::Running { deadline, irq_core } if now.0 >= deadline.0 => *irq_core,
            _ => return Ok(None),
        };
        self.phase = Phase::Completed(OutstandingAcks::Both);
        Ok(Some(Completion { core, irq: COMPLETION_IRQ, status: COMPLETION_STATUS }))
    }

    pub(crate) fn acknowledge(&mut self, register: AckRegister, value: u32) -> Result<(), Error> {
        if value != 0 {
            return Err(Error::NonzeroAcknowledgment);
        }
        let Phase::Completed(outstanding) = &self.phase else {
            return Err(Error::NoCompletion);
        };
        self.phase = match (*outstanding, register) {
            (OutstandingAcks::Both, AckRegister::Path) => Phase::Completed(OutstandingAcks::Fill),
            (OutstandingAcks::Both, AckRegister::Fill) => Phase::Completed(OutstandingAcks::Path),
            (OutstandingAcks::Path, AckRegister::Path)
            | (OutstandingAcks::Fill, AckRegister::Fill) => Phase::Idle,
            (outstanding, _) => Phase::Completed(outstanding),
        };
        Ok(())
    }

    pub(crate) fn status(&self) -> u32 {
        if matches!(self.phase, Phase::Completed(_)) { COMPLETION_STATUS } else { 0 }
    }
}

fn validate_fill(words: &[u32], dimensions: &Dimensions, ram: RamBytes) -> Result<(), Error> {
    if words.len() != FILL_WORDS {
        return Err(Error::FillLength { actual: words.len() });
    }
    for (word, &expected) in FILL_PREFIX.iter().enumerate() {
        let expected = if word == 31 { dimensions.reversed_word() } else { expected };
        expect_fill_word(words, word, expected)?;
    }
    for (entry, update) in words[36..548].chunks_exact(2).enumerate() {
        expect_fill_word(words, 36 + entry * 2, 0x40070188)?;
        let triplet = update[1];
        if triplet != (triplet & 0xff) * 0x010101 {
            return Err(Error::InvalidGamma { entry });
        }
    }
    if words[559] > 1 {
        return Err(Error::InvalidOrder);
    }
    for (offset, &expected) in FILL_SUFFIX.iter().enumerate() {
        if offset != 11 {
            expect_fill_word(words, 548 + offset, expected)?;
        }
    }
    let target = words[571];
    if !matches!(target & 0xc0000000, 0 | 0x40000000) || target & 3 != 0 {
        return Err(Error::InvalidTarget);
    }
    let physical = target & 0x3fffffff;
    let end = physical.checked_add(dimensions.frame_bytes).ok_or(Error::ArithmeticOverflow)?;
    if end > ram.0.min(0x40000000) {
        return Err(Error::TargetOutOfRange);
    }
    let target_end = target.checked_add(dimensions.pixels)
        .and_then(|end| end.checked_sub(8))
        .ok_or(Error::ArithmeticOverflow)?;
    let tail = [
        0x400101b0, 0xb, 0x400f01a4, target,
        0x400f01a8, target_end, 0x400101a1, 0,
    ];
    for (offset, &expected) in tail.iter().enumerate() {
        expect_fill_word(words, 568 + offset, expected)?;
    }
    let footer_start = if words[576] == ZERO_CLIP[0] {
        for (offset, &expected) in ZERO_CLIP.iter().enumerate() {
            expect_fill_word(words, 576 + offset, expected)?;
        }
        580
    } else {
        576
    };
    for (offset, &expected) in FOOTER.iter().enumerate() {
        expect_fill_word(words, footer_start + offset, expected)?;
    }
    for word in footer_start + FOOTER.len()..FILL_WORDS {
        expect_fill_word(words, word, 0)?;
    }
    Ok(())
}

fn expect_fill_word(words: &[u32], word: usize, expected: u32) -> Result<(), Error> {
    if words[word] != expected { Err(Error::UnsupportedFill { word }) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::{AckRegister, Completion, CoreId, Device, Error, RamBytes, VirtualTick};

    const RAM: RamBytes = RamBytes(128 * 1024 * 1024);
    const WIDTH: u32 = 720;
    const HEIGHT: u32 = 480;
    const TARGET: u32 = 0x40001000;

    fn path(width: u32, height: u32) -> [u32; 32] {
        [
            0x40080081, 0x01000000, 0x400f007b, width << 16 | height,
            0x40080040, 0, 0x40070040, 0x003f0000,
            0x40070041, 0, 0x40070042, 0,
            0x40070043, 0, 0x40070044, 0x003f0000,
            0x40070045, 0, 0x40070046, 0,
            0x40070047, 0, 0x40070048, 0x003f0000,
            0x40020080, 0, 0x40010060, 2,
            0x40010081, 1, 0x40010060, 2,
        ]
    }

    fn initialized_path(width: u32, height: u32) -> [u32; 16] {
        [
            0x40080081, 0x01000000, 0x400f007b, width << 16 | height,
            0x40010060, 2, 0x40010081, 1,
            0, 0, 0, 0, 0, 0, 0, 0,
        ]
    }

    fn fill(width: u32, height: u32, target: u32, zero_clip: bool) -> [u32; 592] {
        let mut words = [0; 592];
        words[..36].copy_from_slice(&[
            0x40080140, 0, 0x40010190, 0, 0x400101c0, 0, 0x4001014d, 1,
            0x40010140, 3, 0x4001014d, 0, 0x400f014d, 0, 0x400f014d, 0,
            0x40040140, 0x50000, 0x40010140, 0, 0x400f0143, 0, 0x400f0153, 0,
            0x400f0144, 0, 0x400f0154, 0, 0x400101a2, 0, 0x400f01a3, height << 16 | width,
            0x40040184, 0, 0x40010187, 0,
        ]);
        for update in words[36..548].chunks_exact_mut(2) {
            update.copy_from_slice(&[0x40070188, 0x5a5a5a]);
        }
        words[548..576].copy_from_slice(&[
            0x40020140, 0, 0x400f014d, 0, 0x400f014d, 0, 0x40010178, 1,
            0x400f016c, 0x40001, 0x400101a7, 0, 0x400201a7, 0x100, 0x40010147, 1,
            0x40010157, 1, 0x400801a0, 0, 0x400101b0, 0xb, 0x400f01a4, target,
            0x400f01a8, target + width * height - 8, 0x400101a1, 0,
        ]);
        let footer = if zero_clip {
            words[576..580].copy_from_slice(&[0x40010100, 0, 0x40010115, 0]);
            580
        } else {
            576
        };
        words[footer..footer + 6].copy_from_slice(&[
            0x400f0028, 0x12345678, 0x40010031, 0xf0, 0x400f0018, 0x12345678,
        ]);
        words
    }

    fn event(core: CoreId) -> Completion {
        Completion { core, irq: 0x3c, status: 0x80000000 }
    }

    #[test]
    fn separate_starts_complete_once_at_the_deadline() {
        let path = path(WIDTH, HEIGHT);
        let fill = fill(WIDTH, HEIGHT, TARGET, false);
        let mut device = Device::new();
        assert_eq!(device.status(), 0);
        assert_eq!(device.start_fill(CoreId(0), &path, &fill, CoreId(1), VirtualTick(10), RAM), Err(Error::PathNotStarted));
        assert_eq!(device.complete(VirtualTick(100)), Ok(None));
        assert_eq!(device.start_path(CoreId(0), &path, RAM), Ok(()));
        assert_eq!(device.complete(VirtualTick(100)), Ok(None));
        assert_eq!(device.start_fill(CoreId(0), &path, &fill, CoreId(1), VirtualTick(10), RAM), Ok(VirtualTick(12)));
        assert_eq!(device.status(), 0);
        assert_eq!(device.complete(VirtualTick(10)), Ok(None));
        assert_eq!(device.complete(VirtualTick(11)), Ok(None));
        assert_eq!(device.status(), 0);
        assert_eq!(device.complete(VirtualTick(12)), Ok(Some(event(CoreId(1)))));
        assert_eq!(device.status(), 0x80000000);
        assert_eq!(device.complete(VirtualTick(12)), Ok(None));
        assert_eq!(device.complete(VirtualTick(u64::MAX)), Ok(None));
        assert_eq!(device.status(), 0x80000000);
    }

    #[test]
    fn initialized_path_with_gamma_fill_completes_without_identity_refresh() {
        let path = initialized_path(WIDTH, HEIGHT);
        let fill = fill(WIDTH, HEIGHT, TARGET, true);
        let mut device = Device::new();
        device.start_path(CoreId(0), &path, RAM).unwrap();
        assert_eq!(device.complete(VirtualTick(u64::MAX)), Ok(None));
        assert_eq!(device.start_fill(CoreId(0), &path, &fill, CoreId(1), VirtualTick(10), RAM), Ok(VirtualTick(12)));
        assert_eq!(device.complete(VirtualTick(11)), Ok(None));
        assert_eq!(device.complete(VirtualTick(12)), Ok(Some(event(CoreId(1)))));
        assert_eq!(device.complete(VirtualTick(u64::MAX)), Ok(None));
        device.acknowledge(AckRegister::Fill, 0).unwrap();
        assert_eq!(device.status(), 0x80000000);
        device.acknowledge(AckRegister::Path, 0).unwrap();
        assert_eq!(device.status(), 0);
    }

    #[test]
    fn initialized_path_unknown_commands_padding_and_mutation_never_complete() {
        let path = initialized_path(WIDTH, HEIGHT);
        for word in 0..path.len() {
            if word == 3 { continue; }
            let mut unknown = path;
            unknown[word] ^= 1;
            let mut device = Device::new();
            assert_eq!(device.start_path(CoreId(0), &unknown, RAM), Err(Error::UnsupportedPath { word }));
            assert_eq!(device.complete(VirtualTick(u64::MAX)), Ok(None));
        }
        let mut device = Device::new();
        assert_eq!(device.start_path(CoreId(0), &path[..15], RAM), Err(Error::PathLength { actual: 15 }));
        let mut extended = [0; 17];
        extended[..16].copy_from_slice(&path);
        assert_eq!(device.start_path(CoreId(0), &extended, RAM), Err(Error::PathLength { actual: 17 }));
        device.start_path(CoreId(0), &path, RAM).unwrap();
        let mut changed = path;
        changed[3] = 640 << 16 | HEIGHT;
        let fill = fill(WIDTH, HEIGHT, TARGET, true);
        assert_eq!(device.start_fill(CoreId(0), &changed, &fill, CoreId(0), VirtualTick(10), RAM), Err(Error::PathChanged));
        assert_eq!(device.start_fill(CoreId(0), &self::path(WIDTH, HEIGHT), &fill, CoreId(0), VirtualTick(10), RAM), Err(Error::PathChanged));
        assert_eq!(device.complete(VirtualTick(u64::MAX)), Ok(None));
    }

    #[test]
    fn both_zero_acknowledgments_are_required_in_either_order() {
        let path = path(WIDTH, HEIGHT);
        let fill = fill(WIDTH, HEIGHT, TARGET, false);
        for (first, second) in [(AckRegister::Path, AckRegister::Fill), (AckRegister::Fill, AckRegister::Path)] {
            let mut device = Device::default();
            assert_eq!(device.acknowledge(first, 0), Err(Error::NoCompletion));
            device.start_path(CoreId(1), &path, RAM).unwrap();
            device.start_fill(CoreId(1), &path, &fill, CoreId(0), VirtualTick(20), RAM).unwrap();
            assert_eq!(device.acknowledge(first, 0), Err(Error::NoCompletion));
            assert_eq!(device.complete(VirtualTick(1000)), Ok(Some(event(CoreId(0)))));
            assert_eq!(device.start_path(CoreId(1), &path, RAM), Err(Error::Busy));
            assert_eq!(device.acknowledge(first, 1), Err(Error::NonzeroAcknowledgment));
            assert_eq!(device.status(), 0x80000000);
            assert_eq!(device.acknowledge(first, 0), Ok(()));
            assert_eq!(device.acknowledge(first, 0), Ok(()));
            assert_eq!(device.status(), 0x80000000);
            assert_eq!(device.start_path(CoreId(1), &path, RAM), Err(Error::Busy));
            assert_eq!(device.start_fill(CoreId(1), &path, &fill, CoreId(0), VirtualTick(30), RAM), Err(Error::Busy));
            assert_eq!(device.acknowledge(second, u32::MAX), Err(Error::NonzeroAcknowledgment));
            assert_eq!(device.complete(VirtualTick(1001)), Ok(None));
            assert_eq!(device.status(), 0x80000000);
            assert_eq!(device.acknowledge(second, 0), Ok(()));
            assert_eq!(device.status(), 0);
            assert_eq!(device.acknowledge(second, 0), Err(Error::NoCompletion));
            assert_eq!(device.complete(VirtualTick(1002)), Ok(None));
            device.start_path(CoreId(0), &path, RAM).unwrap();
            assert_eq!(device.start_fill(CoreId(0), &path, &fill, CoreId(1), VirtualTick(1002), RAM), Ok(VirtualTick(1004)));
            assert_eq!(device.complete(VirtualTick(1004)), Ok(Some(event(CoreId(1)))));
        }
    }

    #[test]
    fn invalid_cores_cross_core_fill_and_mutation_preserve_the_pending_path() {
        let path = path(WIDTH, HEIGHT);
        let fill = fill(WIDTH, HEIGHT, TARGET, false);
        let mut device = Device::new();
        assert_eq!(device.start_path(CoreId(2), &path, RAM), Err(Error::InvalidCore(CoreId(2))));
        device.start_path(CoreId(0), &path, RAM).unwrap();
        assert_eq!(device.start_path(CoreId(0), &path, RAM), Err(Error::Busy));
        assert_eq!(device.start_fill(CoreId(2), &path, &fill, CoreId(0), VirtualTick(0), RAM), Err(Error::InvalidCore(CoreId(2))));
        assert_eq!(device.start_fill(CoreId(0), &path, &fill, CoreId(255), VirtualTick(0), RAM), Err(Error::InvalidCore(CoreId(255))));
        assert_eq!(device.start_fill(CoreId(1), &path, &fill, CoreId(0), VirtualTick(0), RAM), Err(Error::SubmitterMismatch { expected: CoreId(0), actual: CoreId(1) }));
        let mut changed = path;
        changed[3] = 640 << 16 | HEIGHT;
        assert_eq!(device.start_fill(CoreId(0), &changed, &fill, CoreId(0), VirtualTick(0), RAM), Err(Error::PathChanged));
        assert_eq!(device.start_fill(CoreId(0), &path[..31], &fill, CoreId(0), VirtualTick(0), RAM), Err(Error::PathChanged));
        assert_eq!(device.complete(VirtualTick(u64::MAX)), Ok(None));
        assert_eq!(device.status(), 0);
        assert_eq!(device.start_fill(CoreId(0), &path, &fill, CoreId(1), VirtualTick(0), RAM), Ok(VirtualTick(2)));
        assert_eq!(device.complete(VirtualTick(2)), Ok(Some(event(CoreId(1)))));
    }

    #[test]
    fn rejected_operations_do_not_replace_a_running_completion() {
        let path = path(WIDTH, HEIGHT);
        let fill = fill(WIDTH, HEIGHT, TARGET, false);
        let mut device = Device::new();
        device.start_path(CoreId(0), &path, RAM).unwrap();
        device.start_fill(CoreId(0), &path, &fill, CoreId(1), VirtualTick(10), RAM).unwrap();
        assert_eq!(device.start_path(CoreId(1), &path, RAM), Err(Error::Busy));
        assert_eq!(device.start_fill(CoreId(1), &path, &fill, CoreId(0), VirtualTick(0), RAM), Err(Error::Busy));
        assert_eq!(device.acknowledge(AckRegister::Fill, 1), Err(Error::NonzeroAcknowledgment));
        assert_eq!(device.status(), 0);
        assert_eq!(device.complete(VirtualTick(11)), Ok(None));
        assert_eq!(device.complete(VirtualTick(12)), Ok(Some(event(CoreId(1)))));
    }

    #[test]
    fn every_unknown_path_word_and_nonexact_length_is_refused() {
        let path = path(WIDTH, HEIGHT);
        for word in 0..path.len() {
            if word == 3 { continue; }
            let mut unknown = path;
            unknown[word] ^= 1;
            let mut device = Device::new();
            assert_eq!(device.start_path(CoreId(0), &unknown, RAM), Err(Error::UnsupportedPath { word }));
            assert_eq!(device.complete(VirtualTick(u64::MAX)), Ok(None));
            assert_eq!(device.status(), 0);
            assert_eq!(device.start_path(CoreId(0), &path, RAM), Ok(()));
        }
        let mut device = Device::new();
        assert_eq!(device.start_path(CoreId(0), &path[..31], RAM), Err(Error::PathLength { actual: 31 }));
        let mut extended = [0; 33];
        extended[..32].copy_from_slice(&path);
        assert_eq!(device.start_path(CoreId(0), &extended, RAM), Err(Error::PathLength { actual: 33 }));
        assert_eq!(device.start_path(CoreId(0), &path, RAM), Ok(()));
    }

    #[test]
    fn unknown_fill_words_gamma_shapes_footer_and_padding_never_complete() {
        let path = path(WIDTH, HEIGHT);
        for zero_clip in [false, true] {
            let fill = fill(WIDTH, HEIGHT, TARGET, zero_clip);
            for word in 0..fill.len() {
                if word == 559 { continue; }
                let mut unknown = fill;
                unknown[word] ^= 1;
                let mut device = Device::new();
                device.start_path(CoreId(0), &path, RAM).unwrap();
                assert!(device.start_fill(CoreId(0), &path, &unknown, CoreId(1), VirtualTick(0), RAM).is_err(), "word {word}, zero_clip {zero_clip}");
                assert_eq!(device.complete(VirtualTick(u64::MAX)), Ok(None));
                assert_eq!(device.status(), 0);
                assert_eq!(device.start_fill(CoreId(0), &path, &fill, CoreId(1), VirtualTick(0), RAM), Ok(VirtualTick(2)));
                assert_eq!(device.complete(VirtualTick(2)), Ok(Some(event(CoreId(1)))));
            }
        }
    }

    #[test]
    fn gamma_values_are_state_not_a_fixed_curve() {
        let path = path(WIDTH, HEIGHT);
        for zero_clip in [false, true] {
            for order in [0, 1] {
                let mut fill = fill(WIDTH, HEIGHT, TARGET, zero_clip);
                fill[559] = order;
                for (grey, update) in fill[36..548].chunks_exact_mut(2).enumerate() {
                    update[1] = grey as u32 * 0x010101;
                }
                let mut device = Device::new();
                device.start_path(CoreId(0), &path, RAM).unwrap();
                assert_eq!(device.start_fill(CoreId(0), &path, &fill, CoreId(0), VirtualTick(0), RAM), Ok(VirtualTick(2)));
                assert_eq!(device.complete(VirtualTick(2)), Ok(Some(event(CoreId(0)))));
            }
        }
    }

    #[test]
    fn draw_headers_lane_masks_and_invalid_orders_are_refused() {
        let path = path(WIDTH, HEIGHT);
        let fill = fill(WIDTH, HEIGHT, TARGET, false);
        for (word, value) in [
            (36, 0x400f0188), (37, 0x015a5a5a), (37, 0x005a595a),
            (559, 2), (568, 0x400700ad), (569, 0xf), (573, u32::MAX),
            (576, 0x28000000), (582, 0x400101a9),
        ] {
            let mut unknown = fill;
            unknown[word] = value;
            let mut device = Device::new();
            device.start_path(CoreId(0), &path, RAM).unwrap();
            assert!(device.start_fill(CoreId(0), &path, &unknown, CoreId(0), VirtualTick(0), RAM).is_err());
            assert_eq!(device.complete(VirtualTick(u64::MAX)), Ok(None));
            assert_eq!(device.status(), 0);
        }
    }

    #[test]
    fn fill_requires_exact_word_count() {
        let path = path(WIDTH, HEIGHT);
        let fill = fill(WIDTH, HEIGHT, TARGET, false);
        let mut device = Device::new();
        device.start_path(CoreId(0), &path, RAM).unwrap();
        for length in [576, 591] {
            assert_eq!(device.start_fill(CoreId(0), &path, &fill[..length], CoreId(0), VirtualTick(0), RAM), Err(Error::FillLength { actual: length }));
        }
        let mut extended = [0; 656];
        extended[..592].copy_from_slice(&fill);
        for length in [593, 656] {
            assert_eq!(device.start_fill(CoreId(0), &path, &extended[..length], CoreId(0), VirtualTick(0), RAM), Err(Error::FillLength { actual: length }));
        }
        assert_eq!(device.complete(VirtualTick(u64::MAX)), Ok(None));
        assert_eq!(device.start_fill(CoreId(0), &path, &fill, CoreId(0), VirtualTick(0), RAM), Ok(VirtualTick(2)));
    }

    #[test]
    fn dimensions_and_frame_arithmetic_are_bounded() {
        for (width, height, ram, error) in [
            (0, 480, RAM, Error::InvalidDimensions),
            (720, 0, RAM, Error::InvalidDimensions),
            (721, 480, RAM, Error::InvalidDimensions),
            (720, 481, RAM, Error::InvalidDimensions),
            (65528, 65528, RAM, Error::ArithmeticOverflow),
            (4104, 4096, RAM, Error::FrameTooLarge),
            (720, 480, RamBytes(0), Error::FrameTooLarge),
            (720, 480, RamBytes(720 * 480 * 4 - 1), Error::FrameTooLarge),
        ] {
            let mut device = Device::new();
            assert_eq!(device.start_path(CoreId(0), &path(width, height), ram), Err(error));
            assert_eq!(device.status(), 0);
            assert_eq!(device.complete(VirtualTick(u64::MAX)), Ok(None));
            assert_eq!(device.start_path(CoreId(0), &path(WIDTH, HEIGHT), RAM), Ok(()));
        }
        let mut device = Device::new();
        let path = path(4096, 4096);
        let fill = fill(4096, 4096, 0, false);
        let ram = RamBytes(64 * 1024 * 1024);
        device.start_path(CoreId(0), &path, ram).unwrap();
        assert_eq!(device.start_fill(CoreId(0), &path, &fill, CoreId(0), VirtualTick(0), ram), Ok(VirtualTick(2)));
        assert_eq!(device.complete(VirtualTick(2)), Ok(Some(event(CoreId(0)))));
    }

    #[test]
    fn target_alias_alignment_and_full_frame_range_are_validated() {
        let path = path(WIDTH, HEIGHT);
        let good_fill = fill(WIDTH, HEIGHT, TARGET, false);
        let frame_bytes = WIDTH * HEIGHT * 4;
        for (target, error) in [
            (0x80001000, Error::InvalidTarget),
            (0xc0001000, Error::InvalidTarget),
            (0x40001001, Error::InvalidTarget),
            (RAM.0 - frame_bytes + 4, Error::TargetOutOfRange),
            (0x40000000 + RAM.0 - frame_bytes + 4, Error::TargetOutOfRange),
            (0x7ffffffc, Error::TargetOutOfRange),
        ] {
            let mut bad_fill = good_fill;
            bad_fill[571] = target;
            let mut device = Device::new();
            device.start_path(CoreId(0), &path, RAM).unwrap();
            assert_eq!(device.start_fill(CoreId(0), &path, &bad_fill, CoreId(0), VirtualTick(0), RAM), Err(error));
            assert_eq!(device.complete(VirtualTick(u64::MAX)), Ok(None));
            assert_eq!(device.status(), 0);
            assert_eq!(device.start_fill(CoreId(0), &path, &good_fill, CoreId(0), VirtualTick(0), RAM), Ok(VirtualTick(2)));
        }
        for target in [0, 0x40000000, RAM.0 - frame_bytes, 0x40000000 + RAM.0 - frame_bytes] {
            let fill = fill(WIDTH, HEIGHT, target, false);
            let mut device = Device::new();
            device.start_path(CoreId(0), &path, RAM).unwrap();
            assert_eq!(device.start_fill(CoreId(0), &path, &fill, CoreId(0), VirtualTick(0), RAM), Ok(VirtualTick(2)));
            assert_eq!(device.complete(VirtualTick(2)), Ok(Some(event(CoreId(0)))));
        }
    }

    #[test]
    fn fill_rechecks_the_ram_boundary_without_losing_the_path() {
        let path = path(WIDTH, HEIGHT);
        let fill = fill(WIDTH, HEIGHT, TARGET, false);
        let mut device = Device::new();
        device.start_path(CoreId(0), &path, RAM).unwrap();
        assert_eq!(device.start_fill(CoreId(0), &path, &fill, CoreId(0), VirtualTick(0), RamBytes(WIDTH * HEIGHT * 4 - 1)), Err(Error::FrameTooLarge));
        assert_eq!(device.complete(VirtualTick(u64::MAX)), Ok(None));
        assert_eq!(device.start_fill(CoreId(0), &path, &fill, CoreId(0), VirtualTick(0), RAM), Ok(VirtualTick(2)));
    }

    #[test]
    fn deadline_overflow_refuses_without_scheduling() {
        let path = path(WIDTH, HEIGHT);
        let fill = fill(WIDTH, HEIGHT, TARGET, false);
        let mut device = Device::new();
        device.start_path(CoreId(0), &path, RAM).unwrap();
        for now in [u64::MAX - 1, u64::MAX] {
            assert_eq!(device.start_fill(CoreId(0), &path, &fill, CoreId(1), VirtualTick(now), RAM), Err(Error::ArithmeticOverflow));
            assert_eq!(device.complete(VirtualTick(u64::MAX)), Ok(None));
            assert_eq!(device.status(), 0);
        }
        assert_eq!(device.start_fill(CoreId(0), &path, &fill, CoreId(1), VirtualTick(u64::MAX - 2), RAM), Ok(VirtualTick(u64::MAX)));
        assert_eq!(device.complete(VirtualTick(u64::MAX - 1)), Ok(None));
        assert_eq!(device.complete(VirtualTick(u64::MAX)), Ok(Some(event(CoreId(1)))));
        assert_eq!(device.complete(VirtualTick(u64::MAX)), Ok(None));
    }
}
