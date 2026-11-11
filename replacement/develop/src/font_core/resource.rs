use super::{bitmap::{BdfResource, IconResource}, outline::FdlResource, FontError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetOffset(u32);
impl AssetOffset {
    pub const fn new(value: u32) -> Self { Self(value) }
    pub const fn get(self) -> u32 { self.0 }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontAssetKind { Fdl, Bdf, DcwIcon }
pub(super) enum FontBackend<'asset> {
    Fdl(FdlResource<'asset>), Bdf(BdfResource<'asset>), Icon(IconResource<'asset>),
}
pub struct FontAsset<'asset> { pub(super) backend: FontBackend<'asset> }
impl<'asset> FontAsset<'asset> {
    pub fn parse(bytes: &'asset [u8]) -> Result<Self, FontError> {
        let backend = if bytes.starts_with(b"FDL Font") {
            FontBackend::Fdl(FdlResource::parse(bytes)?)
        } else if bytes.starts_with(b"DCW ICON") {
            FontBackend::Icon(IconResource::parse(bytes)?)
        } else if bytes.starts_with(b"STARTFONT ") {
            FontBackend::Bdf(BdfResource::parse(bytes)?)
        } else {
            return Err(if bytes.len() < 8 { FontError::Truncated } else { FontError::InvalidSignature });
        };
        Ok(Self { backend })
    }
    pub const fn kind(&self) -> FontAssetKind {
        match &self.backend { FontBackend::Fdl(_) => FontAssetKind::Fdl, FontBackend::Bdf(_) => FontAssetKind::Bdf, FontBackend::Icon(_) => FontAssetKind::DcwIcon }
    }
}
