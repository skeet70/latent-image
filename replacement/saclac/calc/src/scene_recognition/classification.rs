use xm1_saclac_interface::calc_sub::{AspectRatio, ClassificationRequest, Frame, Orientation};
use xm1_saclac_interface::stock::{self, StockCallbackWord};
use xm1_saclac_interface::{CalcSub, CalcSubOperation, CalcSubRequest};

use super::ports::ClassificationControl;
use super::SceneRecognition;

impl SceneRecognition {
    /// 0xf4eae0: `aspect` is [width, height], the high and low halves of the caller's word.
    /// The change mark clears after the post, including a refused post (0xf4ebdc..0xf4ebe4).
    pub fn request_classification<P: ClassificationControl + CalcSub>(
        &mut self,
        ports: &mut P,
        frame: Frame,
        aspect: [u16; 2],
        orientation: u32,
        completion: StockCallbackWord,
    ) -> Result<(), P::Error> {
        ports.enable_classification();
        let aspect = match aspect {
            [3, 2] => AspectRatio::ThreeTwo,
            [16, 9] => AspectRatio::SixteenNine,
            [1, 1] => AspectRatio::Square,
            _ => AspectRatio::Other,
        };
        let orientation = match orientation {
            2 => Orientation::Reading2,
            3 => Orientation::Reading3,
            4 => Orientation::Reading4,
            _ => Orientation::Reading1,
        };
        let payload = stock::calc_sub::classification(&ClassificationRequest {
            frame,
            aspect,
            orientation,
            restart: self.adjustments_changed,
            completion,
        });
        let result = ports.request(CalcSubRequest {
            operation: CalcSubOperation::Request00,
            parameters: stock::parameters(&payload),
        });
        self.adjustments_changed = false;
        result
    }
}
