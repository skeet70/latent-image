#![forbid(unsafe_code)]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeometryPair(pub [u16; 2]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeometryCalibration(pub [u16; 4]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetainedGeometryValue(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MeasurementGeometry {
    pub resource: GeometryPair,
    pub calibration: GeometryCalibration,
    pub converted_profile: RetainedGeometryValue,
    pub first_parameter: RetainedGeometryValue,
    pub second_parameter: RetainedGeometryValue,
}
