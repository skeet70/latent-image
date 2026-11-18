use xm1_focus_measurement_round::{image::{ChannelCount, ImageLog, ImageLogging, LensReport, Ports, ReplyPorts}, stock as round};
use xm1_saclac_calc::adjust::focus_profile;
use xm1_saclac_config::{adjustment, channel};
use xm1_saclac_controllers::layers::{LensControl, LensProbe};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{round_image::{ImageArea, ImageChannel, ImageLaneRequest}, stock::{self as wire, round_image as codec, StockCallbackWord}, Calc, Calc1, ImageProcessor, ImgDc, ImgDcRequest};
use xm1_saclac_profile_geometry::controller::Tables;
use xm1_saclac_profile_records::{Memory, StockEntries, stock::shared::group_20};
use xm1_saclac_shared::stock::{Machine, Stock};
use crate::{focus_area::{Areas, Resources}, round_ports::RoundPorts, services::NO_HANDLE};

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor + Calc + Calc1 + ImgDc + xm1_saclac_interface::Statistics, I: Isp, T: Tables>
    ReplyPorts for RoundPorts<'_, M, R, E, P, I, T> {
    fn channel_count(&mut self) -> ChannelCount { ChannelCount(channel::count(&self.services.stock) as u8) }
    fn lens_report(&mut self) -> LensReport {
        LensReport {
            first: self.services.overridden(LensProbe::First),
            second: self.services.overridden(LensProbe::Second),
            third: self.services.overridden(LensProbe::Third),
        }
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor + Calc + Calc1 + ImgDc + xm1_saclac_interface::Statistics, I: Isp, T: Tables>
    Ports for RoundPorts<'_, M, R, E, P, I, T> {
    fn image_client_callback(&mut self) -> Option<StockCallbackWord> {
        self.services.records.read_shared(NO_HANDLE, &group_20::IMAGE_CALLBACK).ok().filter(|callback| callback.0 != 0)
    }
    fn notify_image_client(&mut self, callback: StockCallbackWord) {
        self.services.machine.enter(callback.0, &codec::IMAGE_CLIENT_START.map(|word| word as usize));
    }
    fn prepare_image_area(&mut self, channel: ImageChannel, area: ImageArea) -> ImageArea {
        let machine = self.services.machine.clone();
        let stock = Stock::new(machine.clone());
        let mut lens = Resources(&machine, &stock);
        let mut configuration = Areas(&machine);
        let record = self.search_record;
        ImageArea(focus_profile::program_round_area(self, &mut lens, &mut configuration, record, channel.0, area.0))
    }
    fn image_logging(&mut self) -> ImageLogging {
        round::image_logging(adjustment::half(&self.services.stock, round::IMAGE_LOGGING.word()))
    }
    fn log_image(&mut self, entry: ImageLog) {
        self.services.machine.enter(0x64_744c, &[round::IMAGE_LOG_SOURCE as usize, round::image_log_text(entry) as usize]);
    }
    fn request_image(&mut self, request: ImageLaneRequest) {
        let callbacks = self.services.round_callbacks.expect("round callbacks bound before image requests");
        let callback = match request {
            ImageLaneRequest::Image { .. } => callbacks.image,
            ImageLaneRequest::Snapshot { .. } => callbacks.snapshot,
            ImageLaneRequest::Cancel(_) | ImageLaneRequest::Stop => StockCallbackWord(0),
        };
        let (operation, bytes) = codec::image_lane_request(&request, callback);
        if ImgDc::request(&mut self.services.peers, ImgDcRequest { operation, parameters: wire::parameters(bytes.bytes()), completions: &[] }).is_err() {
            crate::stock::acquisition::fault(&self.services.machine);
        }
    }
}
