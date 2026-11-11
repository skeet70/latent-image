use crate::frame::{QueuedCommand, Sequence};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueError {
    Full,
    MissingHistory,
}

pub(crate) struct CommandQueue {
    records: [Option<QueuedCommand>; 20],
    write: usize,
    read: usize,
    pending: usize,
}

impl CommandQueue {
    pub const fn new() -> Self {
        Self { records: [None; 20], write: 0, read: 0, pending: 0 }
    }

    pub fn push(&mut self, command: QueuedCommand) -> Result<(), QueueError> {
        if self.pending >= 18 {
            return Err(QueueError::Full);
        }
        self.records[self.write] = Some(command);
        self.write = (self.write + 1) % self.records.len();
        self.pending += 1;
        Ok(())
    }

    pub fn pop(&mut self) -> Option<QueuedCommand> {
        if self.pending == 0 {
            return None;
        }
        let record = self.records[self.read];
        self.read = (self.read + 1) % self.records.len();
        self.pending -= 1;
        record
    }

    pub fn rewind(&mut self, sequence: Sequence) -> Result<QueuedCommand, QueueError> {
        let mut index = self.read;
        for skipped in 0..3 {
            index = (index + self.records.len() - 1) % self.records.len();
            if let Some(record) = self.records[index] {
                if (record.acknowledgement.raw() >> 24) & 7 == u32::from(sequence.get()) {
                    self.read = (index + 1) % self.records.len();
                    self.pending += skipped;
                    return Ok(record);
                }
            }
        }
        Err(QueueError::MissingHistory)
    }
}
