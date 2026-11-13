#![forbid(unsafe_code)]

//! What the capture dump reads outside the capture steps: the calc library's workspaces, the
//! configuration's adjustment bytes and settings, the shared capture window and the shot
//! context's lens marks. Layouts and stock entries are in `capture_stock`.

use crate::boundary::Frame;

/// A workspace of the calc library the capture dump copies; names follow the library's
/// accessors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Workspace {
    PrimaryGrid,
    SecondaryGrid,
    GridParameters,
    GridLimits,
    PrimaryKeyDefaults,
    SecondaryKeyDefaults,
    TertiaryKeyDefaults,
    KeyReferences,
    KeyIndexes,
    ScaledInputs,
}

/// The two rate grids, whose heads share a layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RateGrid { Primary, Secondary }

impl RateGrid {
    pub const fn workspace(self) -> Workspace {
        match self {
            Self::Primary => Workspace::PrimaryGrid,
            Self::Secondary => Workspace::SecondaryGrid,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldWidth { Byte, Half, SignedHalf, Word }

/// A field of a workspace; `capture_stock` names every field the dump reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceField {
    pub offset: u32,
    pub width: FieldWidth,
}

impl WorkspaceField {
    /// The same field `bytes` further on.
    pub const fn advanced(self, bytes: u32) -> Self { Self { offset: self.offset + bytes, width: self.width } }
}

/// The calc library's workspaces.
pub trait DumpWorkspaces {
    /// `field` of `workspace`, sign-extended for `SignedHalf` and zero-extended otherwise.
    fn workspace_field(&mut self, workspace: Workspace, field: WorkspaceField) -> u32;
}

/// A byte of the adjustment area.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdjustmentKey(pub u32);

/// The configuration the capture dump reads.
pub trait DumpSettings {
    fn adjustment_byte(&mut self, key: AdjustmentKey) -> u8;
    /// 0 when the settings' feature is off, else 1, 2 or 3 by its mode.
    fn settings_class(&mut self) -> u32;
    /// Writes the configuration's part of the dump for the slot `distance` into `frame`.
    fn export_configuration(&mut self, frame: Frame, distance: u32);
}

/// Lens references the shot context keeps for the capture and whether the lens moved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShotWord {
    RequestedReference,
    StartReference,
    SecondReference,
    ThirdReference,
    Moved,
    MovedLate,
}

/// The shared helpers and the shot context the capture dump reads.
pub trait DumpShared {
    /// The capture window's two words for `mode` at `layout`, when the layout has one for the
    /// mode.
    fn capture_window(&mut self, mode: u32, layout: [u16; 2]) -> Option<[u32; 2]>;
    fn shot_word(&mut self, word: ShotWord) -> u32;
}
