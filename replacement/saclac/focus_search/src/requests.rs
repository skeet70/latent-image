#![forbid(unsafe_code)]

use xm1_saclac_controllers::search::Travel;
use xm1_saclac_interface::{lens_effect, CalcOperation, ImgDcOperation, LensEffect, PeerOperation};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgeMotion {
    Travel(Travel),
    /// Hand the lens back without moving it (0xf3e9cc mode 1).
    Release,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Request<Record, Drive> {
    /// imgdc 0x3f2e; imgdc's bridge answers with completion 0x46.
    Bridge { motion: BridgeMotion, drive: Drive },
    /// imgdc 0x3f2f.
    BridgeStop,
    /// calc 0x3c09 over the frame just taken in; calc answers with completion 0x89.
    Evaluate(Record),
}

impl<Record, Drive> Request<Record, Drive> {
    pub const fn operation(&self) -> PeerOperation {
        match self {
            Self::Bridge { .. } => PeerOperation::ImgDc(ImgDcOperation::Request2e),
            Self::BridgeStop => PeerOperation::ImgDc(ImgDcOperation::Request2f),
            Self::Evaluate(_) => PeerOperation::Calc(CalcOperation::Request09),
        }
    }

    pub const fn lens_effect(&self) -> LensEffect {
        lens_effect(self.operation())
    }
}
