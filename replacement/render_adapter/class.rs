#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DrawFunction { Tree, Picker }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotSource { Live, SavedSlot1, SavedSlot2, Skipped }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListKind { Flat, Tree }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeOrigin { Stock, Package, Unknown }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReturnAddress(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallSite {
    MenuList, SecondaryTree, ValuePicker, Picker9304, Picker9a14, Pickera1b8,
    Pickera8b0, Pickerb52c, Pickerb568, FnPicker, Unowned,
}
impl ReturnAddress {
    pub fn call_site(self) -> CallSite {
        match self.0 {
            0x01167a60 => CallSite::MenuList,
            0x0116c058 => CallSite::SecondaryTree,
            0x01168ce4 => CallSite::ValuePicker,
            0x01169308 => CallSite::Picker9304,
            0x01169a18 => CallSite::Picker9a14,
            0x0116a1bc => CallSite::Pickera1b8,
            0x0116a8b4 => CallSite::Pickera8b0,
            0x0116b530 => CallSite::Pickerb52c,
            0x0116b56c => CallSite::Pickerb568,
            0x0116c588 => CallSite::FnPicker,
            _ => CallSite::Unowned,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntryClass {
    pub function: DrawFunction,
    pub caller: ReturnAddress,
    pub mode: u32,
    pub node: u32,
    pub live_kind: u32,
    pub live_parent: u32,
    pub live_selected: u32,
    pub origin: NodeOrigin,
}
impl EntryClass {
    pub fn snapshot(self) -> SnapshotSource {
        match self.function {
            DrawFunction::Picker if self.mode == 2 || self.mode == 3 => SnapshotSource::Skipped,
            DrawFunction::Picker if self.mode <= 13 && (1 << self.mode) & 0x2280 != 0 => SnapshotSource::SavedSlot2,
            DrawFunction::Tree if self.mode <= 13 && (1 << self.mode) & 0x2280 != 0 => SnapshotSource::SavedSlot2,
            DrawFunction::Tree if self.mode <= 13 && (1 << self.mode) & 0x1860 != 0 => SnapshotSource::SavedSlot1,
            _ => SnapshotSource::Live,
        }
    }
    pub fn list_kind(self) -> ListKind {
        if (self.node == 0x25 && self.live_parent == 0x27 && self.live_selected == 0)
            || (self.node == 0x38 && self.live_parent == 0x228 && self.live_selected == 11)
            || self.live_kind == 1 { ListKind::Flat } else { ListKind::Tree }
    }
    pub fn owned(self) -> bool {
        self.origin == NodeOrigin::Stock && self.snapshot() == SnapshotSource::Live && match self.function {
            DrawFunction::Picker => matches!(self.caller.call_site(), CallSite::ValuePicker | CallSite::FnPicker),
            DrawFunction::Tree => self.caller.call_site() == CallSite::MenuList && self.list_kind() == ListKind::Flat,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneNode(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneMode(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SnapshotDepth(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SnapshotKind(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneEntryClass {
    pub caller: ReturnAddress,
    pub node: PaneNode,
    pub mode: PaneMode,
    pub depth: SnapshotDepth,
    pub kind: SnapshotKind,
    pub origin: NodeOrigin,
}
impl PaneEntryClass {
    pub fn owned(self) -> bool {
        self.origin == NodeOrigin::Stock && self.node.0 == 1 && self.caller == ReturnAddress(0x01158184)
            && matches!(self.mode.0, 0 | 1 | 3 | 4 | 6 | 8 | 9 | 10 | 12 | 14 | 255)
            && matches!(self.depth.0, 0..=2) && matches!(self.kind.0, 0 | 1 | 2 | 3 | 9)
    }
}
