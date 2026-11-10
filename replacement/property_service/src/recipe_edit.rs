#[derive(Clone, Copy)]
#[repr(u32)]
pub(crate) enum RecipeEditProperty {
    DynamicRange = 0x267,
    HighlightTone = 0x268,
    ShadowTone = 0x269,
    Color = 0x26a,
    Sharpness = 0x26b,
    NoiseReduction = 0x26c,
    WhiteBalance = 0x26d,
    Iso = 0x26e,
    FilmSimulation = 0x26f,
}

#[repr(transparent)]
struct HostMinusOne(u32);

#[repr(transparent)]
pub(crate) struct StoredCode(u32);

extern "C" {
    fn xm1_recipe_edit_get(host: HostMinusOne) -> StoredCode;
}

impl RecipeEditProperty {
    pub(crate) fn parse(raw: u32) -> Option<Self> {
        match raw {
            0x267 => Some(Self::DynamicRange),
            0x268 => Some(Self::HighlightTone),
            0x269 => Some(Self::ShadowTone),
            0x26a => Some(Self::Color),
            0x26b => Some(Self::Sharpness),
            0x26c => Some(Self::NoiseReduction),
            0x26d => Some(Self::WhiteBalance),
            0x26e => Some(Self::Iso),
            0x26f => Some(Self::FilmSimulation),
            _ => None,
        }
    }

    pub(crate) unsafe fn stored_code(self) -> StoredCode {
        xm1_recipe_edit_get(HostMinusOne(self as u32 - 1))
    }
}

impl StoredCode {
    pub(crate) fn word(self) -> u32 {
        self.0
    }
}
