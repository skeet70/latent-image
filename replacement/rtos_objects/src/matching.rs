use crate::status::ObjectStatus;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Pattern(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct KeepMask(pub u32);

/// A nonzero wait mask. `admit` already refuses zero with -17, so this only carries that rule to
/// the bodies that cannot re-check it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WantedMask(Pattern);

impl WantedMask {
    pub const fn new(want: Pattern) -> Result<Self, ObjectStatus> {
        if want.0 == 0 {
            Err(ObjectStatus::Param)
        } else {
            Ok(Self(want))
        }
    }

    pub const fn pattern(self) -> Pattern {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatchMode {
    All,
    Any,
}

impl MatchMode {
    pub const fn from_word(word: u32) -> Result<Self, ObjectStatus> {
        if word & !1 != 0 {
            return Err(ObjectStatus::Param);
        }
        if word & 1 != 0 {
            Ok(Self::Any)
        } else {
            Ok(Self::All)
        }
    }
}

/// `event_flag_check`. The reported value is the whole current pattern, taken before the clear.
pub fn check(pattern: &mut Pattern, want: Pattern, mode: MatchMode, clear_on_match: bool) -> Option<Pattern> {
    let satisfied = match mode {
        MatchMode::Any => want.0 & pattern.0 != 0,
        MatchMode::All => want.0 & pattern.0 == want.0,
    };
    if !satisfied {
        return None;
    }
    let reported = *pattern;
    if clear_on_match {
        pattern.0 = 0;
    }
    Some(reported)
}
