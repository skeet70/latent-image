use crate::records::{Error, Ports, RecordPair, RecordValue, Records, Reply, Source};
use std::vec::Vec;
use xm1_saclac_interface::{stock, ClientCallback, CompletionStatus};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Buffer(u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Sample(u8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Traffic {
    Read { sample: Sample, destination: Buffer },
    FinishPrimary(Buffer),
    FinishCapture,
    Release(Source),
    Reply(Reply<Buffer>),
}

#[derive(Default)]
struct Services(Vec<Traffic>);
impl Ports<Buffer> for Services {
    type Sample = Sample;
    fn read_sample(&mut self, sample: Sample, destination: Buffer) -> [RecordPair; 2] {
        self.0.push(Traffic::Read { sample, destination });
        match sample {
            Sample(1) => [pair(11, 12), pair(13, 14)],
            Sample(2) => [pair(21, 22), pair(23, 24)],
            _ => [pair(31, 32), pair(33, 34)],
        }
    }
    fn finish_primary(&mut self, buffer: Buffer) { self.0.push(Traffic::FinishPrimary(buffer)); }
    fn finish_capture(&mut self) { self.0.push(Traffic::FinishCapture); }
    fn release(&mut self, source: Source) { self.0.push(Traffic::Release(source)); }
    fn reply(&mut self, reply: Reply<Buffer>) { self.0.push(Traffic::Reply(reply)); }
}

fn pair(first: u32, second: u32) -> RecordPair { RecordPair([RecordValue(first), RecordValue(second)]) }
fn callback(word: u32) -> ClientCallback { stock::optional_callback(word).unwrap() }

#[test]
fn primary_record_finishes_its_buffer_before_replying_with_all_values() {
    let mut records = Records::new();
    let mut services = Services::default();
    records.start_primary(callback(1), Buffer(7));
    let values = [pair(1,2), pair(3,4), pair(5,6)];
    assert_eq!(records.primary_completed(CompletionStatus(-5), values, &mut services), Ok(()));
    assert_eq!(services.0, [
        Traffic::FinishPrimary(Buffer(7)),
        Traffic::Reply(Reply::Primary { callback: callback(1), status: CompletionStatus(-5), buffer: Buffer(7), values }),
    ]);
}

#[test]
fn capture_routes_second_sample_to_second_buffer_and_replies_with_first_sample_values() {
    let mut records = Records::new();
    let mut services = Services::default();
    records.start_capture(callback(2), [Buffer(8), Buffer(9)]);
    assert_eq!(records.capture_sample(Sample(1), &mut services), Ok(()));
    assert_eq!(records.capture_sample(Sample(2), &mut services), Ok(()));
    records.capture_auxiliary(pair(90,91));
    assert_eq!(records.capture_completed(CompletionStatus(20), &mut services), Ok(()));
    assert_eq!(services.0, [
        Traffic::Read { sample: Sample(1), destination: Buffer(8) },
        Traffic::Read { sample: Sample(2), destination: Buffer(9) },
        Traffic::FinishCapture,
        Traffic::Release(Source::Primary), Traffic::Release(Source::Secondary), Traffic::Release(Source::Tertiary),
        Traffic::Reply(Reply::Capture { callback: callback(2), status: CompletionStatus(20), buffers: [Buffer(8), Buffer(9)],
            values: [pair(11,12), pair(13,14), pair(90,91)] }),
    ]);
}

#[test]
fn later_capture_samples_update_first_buffer_and_restarting_resets_sample_selection() {
    let mut records = Records::new();
    let mut services = Services::default();
    records.start_capture(callback(2), [Buffer(8), Buffer(9)]);
    for sample in [Sample(1), Sample(2), Sample(3)] {
        assert_eq!(records.capture_sample(sample, &mut services), Ok(()));
    }
    records.start_capture(callback(3), [Buffer(10), Buffer(11)]);
    assert_eq!(records.capture_sample(Sample(1), &mut services), Ok(()));
    assert_eq!(services.0, [
        Traffic::Read { sample: Sample(1), destination: Buffer(8) },
        Traffic::Read { sample: Sample(2), destination: Buffer(9) },
        Traffic::Read { sample: Sample(3), destination: Buffer(8) },
        Traffic::Read { sample: Sample(1), destination: Buffer(10) },
    ]);
}

#[test]
fn record_updates_reply_to_the_latest_update_client_without_completing_capture() {
    let mut records = Records::new();
    let mut services = Services::default();
    records.start_capture(callback(1), [Buffer(2), Buffer(3)]);
    records.start_update(callback(4));
    records.start_update(callback(5));
    assert_eq!(records.update_completed(CompletionStatus(7), &mut services), Ok(()));
    assert_eq!(records.capture_completed(CompletionStatus(0), &mut services), Ok(()));
    assert_eq!(services.0, [
        Traffic::Reply(Reply::Updated { callback: callback(5), status: CompletionStatus(7) }),
        Traffic::FinishCapture,
        Traffic::Release(Source::Primary), Traffic::Release(Source::Secondary), Traffic::Release(Source::Tertiary),
        Traffic::Reply(Reply::Capture { callback: callback(1), status: CompletionStatus(0), buffers: [Buffer(2), Buffer(3)],
            values: [pair(0,0); 3] }),
    ]);
}

#[test]
fn starting_a_primary_record_discards_the_previous_capture_and_update_clients() {
    let mut records = Records::new();
    let mut services = Services::default();
    records.start_capture(callback(1), [Buffer(2), Buffer(3)]);
    records.start_update(callback(4));
    records.start_primary(callback(5), Buffer(6));
    assert_eq!(records.capture_completed(CompletionStatus(0), &mut services), Err(Error::NoCaptureClient));
    assert_eq!(records.update_completed(CompletionStatus(0), &mut services), Err(Error::NoUpdateClient));
    assert!(services.0.is_empty());
}
