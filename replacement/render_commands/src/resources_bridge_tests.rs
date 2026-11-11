use super::{BitmapCatalog, BitmapError, DataAddress, DataRegion, IndexedItemPalette, ResourceError, ResourceMemory};
use super::super::commands::{Alpha, Colour, DrawCommand, Rgba, ShapePaint, ShapePalette, ShapeSecondarySource};
use super::super::producer::{classify_stock_resource, CommandBuffer, ProducerError, ResourceContent, SemanticPrerequisite};
use super::super::shape_assets::ShapeError;
use super::super::{FontId, ImageId, PaletteIndex, PanelPoint, ResourceId};

fn descriptors() -> [u8; 36] {
    let mut bytes = [0; 36];
    for (index, record) in bytes.chunks_exact_mut(12).enumerate() {
        record[..2].copy_from_slice(&8u16.to_le_bytes());
        record[2..4].copy_from_slice(&6u16.to_le_bytes());
        record[4..8].copy_from_slice(&(index as u32).to_le_bytes());
        record[8..12].copy_from_slice(&0x00300000u32.to_le_bytes());
    }
    bytes
}

#[test]
fn actual_bitmap_tags_choose_pixel_or_shape_commands() {
    let descriptors = descriptors();
    let rgb = [10, 20, 30, 40, 50, 60];
    let regions = [
        DataRegion { base: DataAddress(0x0022a8b4), bytes: &descriptors },
        DataRegion { base: DataAddress(0x007a1f24), bytes: &rgb },
    ];
    let catalog = BitmapCatalog { memory: ResourceMemory::new(&regions).unwrap() };
    let palette = IndexedItemPalette {
        primary: Colour { palette: PaletteIndex(0), alpha: Alpha(128) },
        secondary: Colour { palette: PaletteIndex(1), alpha: Alpha(64) },
        secondary_source: ShapeSecondarySource::White,
    };
    let paint = ShapePaint::Recolour(ShapePalette {
        primary: Rgba { red: 10, green: 20, blue: 30, alpha: 128 },
        secondary: Rgba { red: 40, green: 50, blue: 60, alpha: 64 },
        secondary_source: ShapeSecondarySource::White,
    });
    assert_eq!(catalog.resolve(ResourceId(0xff000000), None), Ok(ResourceContent::Image { image: ImageId::new(0).unwrap(), alpha: Alpha::OPAQUE }));
    assert_eq!(catalog.resolve(ResourceId(0xff000001), Some(palette)), Ok(ResourceContent::Shape { image: ImageId::new(1).unwrap(), alpha: Alpha::OPAQUE, paint }));
    assert_eq!(catalog.resolve(ResourceId(0xff000001), None), Err(ProducerError::MissingSemantic(SemanticPrerequisite::PaletteOptions)));
    assert_eq!(catalog.resolve(ResourceId(0xff000002), None), Ok(ResourceContent::Shape { image: ImageId::new(2).unwrap(), alpha: Alpha::OPAQUE, paint: ShapePaint::Original }));
    let mut storage = [DrawCommand::ResetClip; 2];
    let mut commands = CommandBuffer::new(&mut storage);
    let origin = PanelPoint::new(20, 24).unwrap();
    commands.resource(origin, catalog.resolve(ResourceId(0xff000001), Some(palette)).unwrap(), FontId(13), Colour { palette: PaletteIndex(0), alpha: Alpha(128) }).unwrap();
    assert_eq!(commands.draw_list().commands(), &[DrawCommand::Shape { origin, image: ImageId::new(1).unwrap(), alpha: Alpha::OPAQUE, paint }]);
}

#[test]
fn missing_bitmap_metadata_is_not_an_image_or_shape_fallback() {
    let catalog = BitmapCatalog { memory: ResourceMemory::new(&[]).unwrap() };
    assert_eq!(catalog.resolve(ResourceId(0xff000007), None), Err(ProducerError::Bitmap(BitmapError::Resource(ResourceError::MissingData(DataAddress(0x0022a908))))));
    assert_eq!(classify_stock_resource(ResourceId(0xff000007)), Err(ProducerError::MissingSemantic(SemanticPrerequisite::BitmapKind(ResourceId(0xff000007)))));
}

#[test]
fn catalog_shape_binding_validates_the_borrowed_payload() {
    let descriptors = descriptors();
    let invalid_gvff = [0u8; 64];
    let regions = [
        DataRegion { base: DataAddress(0x0022a8b4), bytes: &descriptors },
        DataRegion { base: DataAddress(0x00300000), bytes: &invalid_gvff },
    ];
    let catalog = BitmapCatalog { memory: ResourceMemory::new(&regions).unwrap() };
    assert!(matches!(catalog.shape_binding(ResourceId(0xff000001)), Err(BitmapError::Shape(ShapeError::Magic))));
    assert!(matches!(catalog.shape_binding(ResourceId(0xff000000)), Err(BitmapError::Shape(ShapeError::NativeBitmap))));
}
