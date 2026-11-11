use crate::{
    backend::{BackendError, ConfigImages},
    configuration_actions_low::{ActionCompletion, ConfigurationKey},
};

#[path = "configuration_audio_dma.rs"]
pub mod dma;
#[path = "configuration_audio_packets.rs"]
pub mod packets;
#[path = "configuration_audio_pipeline.rs"]
pub mod pipeline;
#[path = "configuration_audio_sound.rs"]
pub mod sound;
#[path = "configuration_audio_storage.rs"]
pub mod storage;
#[path = "configuration_audio_stream.rs"]
pub mod stream;

pub const PCM_BYTES_PER_SECOND: usize = 192_000;
pub const PCM_FRAMES_PER_SECOND: usize = 48_000;
pub const MAX_RECORDING_SECONDS: usize = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordingSeconds(u8);
impl RecordingSeconds {
    pub const fn zero() -> Self { Self(0) }
    pub fn parse(value: u8) -> Result<Self, BackendError> {
        if value <= MAX_RECORDING_SECONDS as u8 { Ok(Self(value)) } else { Err(BackendError::Range) }
    }
    pub const fn get(self) -> u8 { self.0 }
    pub const fn byte_length(self) -> usize { self.0 as usize * PCM_BYTES_PER_SECOND }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackChannels { Stereo, Left, Right }
impl PlaybackChannels {
    pub fn parse(value: u8) -> Result<Self, BackendError> {
        match value { 0 => Ok(Self::Stereo), 1 => Ok(Self::Left), 2 => Ok(Self::Right), _ => Err(BackendError::Range) }
    }
}

pub struct RecordedPcm<'a> {
    seconds: RecordingSeconds,
    bytes: &'a [u8],
}
impl<'a> RecordedPcm<'a> {
    pub fn adopt(seconds: RecordingSeconds, bytes: &'a [u8]) -> Result<Self, BackendError> {
        let bytes = bytes.get(..seconds.byte_length()).ok_or(BackendError::Capacity)?;
        Ok(Self { seconds, bytes })
    }
    pub const fn seconds(&self) -> RecordingSeconds { self.seconds }

    // 0xdbc41c copies the recording before validating the channel selector.
    pub fn prepare_playback<'b>(&self, selector: u8, destination: &'b mut [u8]) -> Result<PlaybackPcm<'b>, BackendError> {
        let destination = destination.get_mut(..self.bytes.len()).ok_or(BackendError::Capacity)?;
        destination.copy_from_slice(self.bytes);
        let channels = PlaybackChannels::parse(selector)?;
        match channels {
            PlaybackChannels::Stereo => {},
            PlaybackChannels::Left => {
                for frame in destination.chunks_exact_mut(4) { frame[2] = 0; frame[3] = 0; }
            },
            PlaybackChannels::Right => {
                for frame in destination.chunks_exact_mut(4) { frame[0] = 0; frame[1] = 0; }
            },
        }
        Ok(PlaybackPcm { seconds: self.seconds, channels, bytes: destination })
    }
}

pub struct PlaybackPcm<'a> {
    seconds: RecordingSeconds,
    channels: PlaybackChannels,
    bytes: &'a [u8],
}
impl PlaybackPcm<'_> {
    pub const fn seconds(&self) -> RecordingSeconds { self.seconds }
    pub const fn channels(&self) -> PlaybackChannels { self.channels }
    pub const fn bytes(&self) -> &[u8] { self.bytes }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PcmSecond(u8);
impl PcmSecond {
    pub const fn get(self) -> u8 { self.0 }
    pub const fn byte_offset(self) -> usize { self.0 as usize * PCM_BYTES_PER_SECOND }
}

// The service producer permits two outstanding packets.
pub struct ServicePcmWindow {
    seconds: RecordingSeconds,
    submitted: u8,
    completed: u32,
}
impl ServicePcmWindow {
    pub const fn new(seconds: RecordingSeconds) -> Self { Self { seconds, submitted: 0, completed: 0 } }
    pub fn next_second(&mut self) -> Option<PcmSecond> {
        if self.submitted == self.seconds.0 || u32::from(self.submitted) >= self.completed.wrapping_add(2) { return None; }
        let second = PcmSecond(self.submitted);
        self.submitted += 1;
        Some(second)
    }
    pub fn submit_next(&mut self, base: packets::PcmAddress, queue: &mut packets::ServicePacketQueue) -> bool {
        let Some(second) = self.next_second() else { return false; };
        queue.submit_second(packets::PcmAddress::from_abi(base.get().wrapping_add(second.byte_offset() as u32)));
        true
    }
    // 0xdbb5f4 ignores its callback argument and increments a wrapping word.
    pub fn stream_completion(&mut self) { self.completed = self.completed.wrapping_add(1); }
    pub const fn all_submitted(&self) -> bool { self.submitted == self.seconds.0 }
    pub fn all_completed(&self) -> bool { self.all_submitted() && self.completed == u32::from(self.seconds.0) }
}

pub fn action26_hdmi(keys: &[ConfigurationKey; 19], configuration: &ConfigImages<'_>) -> Result<ActionCompletion, BackendError> {
    let mode = keys[0].read(configuration)?;
    let valid = match mode {
        0 => match keys[1].read(configuration)? {
            0 => keys[2].read(configuration)? == 0,
            1 => {
                if keys[2].read(configuration)? != 0 { false } else {
                    for key in keys[3..19].iter().rev() { key.read(configuration)?; }
                    true
                }
            },
            _ => false,
        },
        1 => {
            let first = u16::from_be_bytes([keys[3].read(configuration)?, keys[4].read(configuration)?]);
            let second = u16::from_be_bytes([keys[5].read(configuration)?, keys[6].read(configuration)?]);
            let third = u16::from_be_bytes([keys[7].read(configuration)?, keys[8].read(configuration)?]);
            first < 0x1000 && second < 0x1000 && third < 0x1000
        },
        _ => false,
    };
    // 0x6ce7e4, 0x6ce7fc and 0x6ce814 return zero without consuming arguments.
    Ok(if valid { ActionCompletion::service_success() } else { ActionCompletion::service_rejected() })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StorageSnapshotFlags(u32);
impl StorageSnapshotFlags {
    pub const fn from_snapshot_word(word: u32) -> Self { Self(word) }
    pub fn service_byte(self) -> Result<u8, BackendError> {
        if self.0 == 0 { Ok(0) }
        else if self.0 & 1 != 0 { Ok(8) }
        else if self.0 & 6 != 0 { Ok(32) }
        else if self.0 & 8 != 0 { Ok(16) }
        else if self.0 & 16 != 0 { Ok(4) }
        else { Err(BackendError::Range) }
    }
}

pub fn consume_storage_snapshot(key: ConfigurationKey, flags: StorageSnapshotFlags, configuration: &mut ConfigImages<'_>) -> Result<ActionCompletion, BackendError> {
    match flags.service_byte() {
        Ok(value) => { key.write(configuration, value)?; Ok(ActionCompletion::service_success()) },
        Err(BackendError::Range) => { key.write(configuration, 0)?; Ok(ActionCompletion::service_rejected()) },
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::{PlaybackChannels, RecordedPcm, RecordingSeconds, ServicePcmWindow, StorageSnapshotFlags, PCM_BYTES_PER_SECOND};
    use crate::backend::BackendError;

    #[test]
    fn left_playback_preserves_left_and_silences_right() {
        let mut recording = [0u8; PCM_BYTES_PER_SECOND];
        recording[..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let source = RecordedPcm::adopt(RecordingSeconds::parse(1).unwrap(), &recording).unwrap();
        let mut output = [0xff; PCM_BYTES_PER_SECOND];
        let playback = source.prepare_playback(1, &mut output).unwrap();
        assert_eq!(playback.channels(), PlaybackChannels::Left);
        assert_eq!(&playback.bytes()[..8], &[1, 2, 0, 0, 5, 6, 0, 0]);
    }

    #[test]
    fn right_playback_preserves_right_and_silences_left() {
        let mut recording = [0u8; PCM_BYTES_PER_SECOND];
        recording[..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let source = RecordedPcm::adopt(RecordingSeconds::parse(1).unwrap(), &recording).unwrap();
        let mut output = [0xff; PCM_BYTES_PER_SECOND];
        assert_eq!(&source.prepare_playback(2, &mut output).unwrap().bytes()[..8], &[0, 0, 3, 4, 0, 0, 7, 8]);
    }

    #[test]
    fn invalid_channel_still_copies_recording() {
        let recording = [7u8; PCM_BYTES_PER_SECOND];
        let source = RecordedPcm::adopt(RecordingSeconds::parse(1).unwrap(), &recording).unwrap();
        let mut output = [0u8; PCM_BYTES_PER_SECOND];
        assert!(matches!(source.prepare_playback(3, &mut output), Err(BackendError::Range)));
        assert_eq!(output, recording);
    }

    #[test]
    fn zero_length_recording_completes_without_submitting_a_packet() {
        let seconds = RecordingSeconds::parse(0).unwrap();
        let mut window = ServicePcmWindow::new(seconds);
        assert_eq!(window.next_second(), None);
        assert!(window.all_completed());
    }

    #[test]
    fn third_packet_waits_for_a_completion() {
        let mut window = ServicePcmWindow::new(RecordingSeconds::parse(3).unwrap());
        assert_eq!(window.next_second().unwrap().byte_offset(), 0);
        assert_eq!(window.next_second().unwrap().byte_offset(), PCM_BYTES_PER_SECOND);
        assert_eq!(window.next_second(), None);
        window.stream_completion();
        assert_eq!(window.next_second().unwrap().byte_offset(), 2 * PCM_BYTES_PER_SECOND);
        assert!(!window.all_completed());
        window.stream_completion();
        window.stream_completion();
        assert!(window.all_completed());
        window.stream_completion();
        assert!(!window.all_completed());
    }

    #[test]
    fn snapshot_priority_uses_the_whole_word_for_the_empty_case() {
        for (word, byte) in [(0, 0), (31, 8), (30, 32), (24, 16), (16, 4)] {
            assert_eq!(StorageSnapshotFlags::from_snapshot_word(word).service_byte(), Ok(byte));
        }
        assert_eq!(StorageSnapshotFlags::from_snapshot_word(0x100).service_byte(), Err(BackendError::Range));
        assert!(matches!(RecordingSeconds::parse(11), Err(BackendError::Range)));
    }

    #[test]
    fn hdmi_accepts_twelve_bit_values_and_rejects_the_next_value() {
        use super::action26_hdmi;
        use crate::backend::ConfigImages;
        use crate::configuration_actions_low::{ActionCompletion, ConfigurationKey};
        let keys = core::array::from_fn(|index| ConfigurationKey::from_abi(index as u16));
        let mut bytes = [0u8; 19];
        bytes[0] = 1;
        bytes[3..9].copy_from_slice(&[15, 255, 15, 255, 15, 255]);
        {
            let configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
            assert_eq!(action26_hdmi(&keys, &configuration), Ok(ActionCompletion::service_success()));
        }
        bytes[3] = 16;
        let configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        assert_eq!(action26_hdmi(&keys, &configuration), Ok(ActionCompletion::service_rejected()));
    }

    #[test]
    fn hdmi_packet_branch_requires_zero_reserved_selector() {
        use super::action26_hdmi;
        use crate::backend::ConfigImages;
        use crate::configuration_actions_low::{ActionCompletion, ConfigurationKey};
        let keys = core::array::from_fn(|index| ConfigurationKey::from_abi(index as u16));
        let mut bytes = [0u8; 19];
        bytes[1] = 1;
        bytes[3..].fill(255);
        {
            let configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
            assert_eq!(action26_hdmi(&keys, &configuration), Ok(ActionCompletion::service_success()));
        }
        bytes[2] = 1;
        let configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        assert_eq!(action26_hdmi(&keys, &configuration), Ok(ActionCompletion::service_rejected()));
    }
}
