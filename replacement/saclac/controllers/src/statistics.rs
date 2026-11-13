#![forbid(unsafe_code)]

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Changes {
    pub processing: bool,
    pub geometry: bool,
    pub measurement: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Group {
    Processing,
    Geometry,
    Measurement,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Configure(Group),
    Complete,
    UnexpectedCompletion,
    Busy,
}

#[derive(Default)]
pub struct Configuration {
    changes: Changes,
    active: Option<Group>,
}

impl Configuration {
    pub const fn new() -> Self {
        Self {
            changes: Changes { processing: false, geometry: false, measurement: false },
            active: None,
        }
    }

    pub fn start(&mut self, changes: Changes) -> Outcome {
        if self.active.is_some() {
            return Outcome::Busy;
        }
        self.changes = changes;
        self.active = if changes.measurement {
            Some(Group::Measurement)
        } else if changes.geometry {
            Some(Group::Geometry)
        } else if changes.processing {
            Some(Group::Processing)
        } else {
            None
        };
        self.outcome()
    }

    pub fn complete(&mut self, group: Group) -> Outcome {
        if self.active != Some(group) {
            return Outcome::UnexpectedCompletion;
        }
        match group {
            Group::Processing => self.changes.processing = false,
            Group::Geometry => self.changes.geometry = false,
            Group::Measurement => self.changes.measurement = false,
        }
        self.active = if self.changes.processing {
            Some(Group::Processing)
        } else if self.changes.geometry {
            Some(Group::Geometry)
        } else if self.changes.measurement {
            Some(Group::Measurement)
        } else {
            None
        };
        self.outcome()
    }

    fn outcome(&self) -> Outcome {
        match self.active {
            Some(group) => Outcome::Configure(group),
            None => Outcome::Complete,
        }
    }
}
