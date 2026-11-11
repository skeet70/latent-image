use crate::{backend::{BackendError, ConfigImages}, backend_adapter::RulePermission, configuration_rules::STOCK_RULES, persistent_queue::PersistentQueueEvent};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigurationSelector(u8);
impl ConfigurationSelector {
    pub fn parse(value: u8) -> Result<Self, BackendError> {
        if STOCK_RULES.iter().any(|rule| rule.selector == u16::from(value) && rule.permission == RulePermission::Dispatch) { Ok(Self(value)) } else { Err(BackendError::InvalidCommand) }
    }
    pub const fn get(self) -> u8 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationDestination { Broadcast }
impl ConfigurationDestination { pub const fn get(self) -> u8 { match self { Self::Broadcast => 0xff } } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessingState { State0, State1, State2, State3, State4, State5, State6 }
impl ProcessingState {
    pub fn parse(value: u16) -> Result<Self, BackendError> { Ok(match value { 0=>Self::State0,1=>Self::State1,2=>Self::State2,3=>Self::State3,4=>Self::State4,5=>Self::State5,6=>Self::State6,_=>return Err(BackendError::Range) }) }
    pub const fn get(self) -> u16 { self as u16 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CpuRoot(u32);
impl CpuRoot { pub const fn address(self) -> u32 { self.0 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueDisposition { NoAction, Rejected, Persistent(PersistentQueueEvent), UnownedCpu(CpuRoot) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueueResolution { pub disposition: QueueDisposition, pub next_state: ProcessingState }

pub fn resolve(selector: ConfigurationSelector, state: ProcessingState, configuration: &ConfigImages<'_>) -> Result<QueueResolution, BackendError> {
    let raw = selector.get();
    if raw == 141 { return Ok(QueueResolution { disposition: QueueDisposition::NoAction, next_state: state }); }
    let event = if raw == 163 { if configuration.byte(0x81).ok_or(BackendError::Capacity)? == 0 { 93 } else { 94 } }
    else {
        let event = SELECTOR_EVENTS.iter().find(|(selector,_)| *selector == raw).map(|(_,event)| *event).ok_or(BackendError::InvalidCommand)?;
        event
    };
    let current = state.get();
    let accepted = match event {
        12 => matches!(current,1|2|3|5),
        20..=22 | 88 | 103 => current == 0,
        36 | 45..=72 | 91 | 92 | 95 | 99 | 102 | 118..=128 | 130 | 131 | 141 => matches!(current,2|3),
        73 => matches!(current,0|1|3),
        74 => current == 1,
        75 => matches!(current,1..=4),
        76..=82 | 87 => matches!(current,1|3),
        83..=85 | 96..=98 | 100 => current == 3,
        90 | 106 => matches!(current,0|1),
        93 => current == 2,
        94 => current == 3,
        101 => matches!(current,2|4),
        104 | 105 => current == 6,
        107..=108 | 110..=117 => matches!(current,2|3|5),
        109 => matches!(current,0|1|2|3|5),
        _ => current != 4,
    };
    if !accepted { return Ok(QueueResolution { disposition: QueueDisposition::Rejected, next_state: state }); }
    let action = match event { 0=>2, 94=>96, 95..=97=>event+2, 98|100=>98, 99=>100, 101=>if current==2 {101} else {102}, 102..=141=>event+1, _=>event+3 };
    let next_state = match (event,current) {
        (73,0)=>ProcessingState::State1,
        (90,_) | (94,_)=>ProcessingState::State2,
        (91,_) | (104,_) | (107,_) | (109,0|1)=>ProcessingState::State0,
        (93,_)=>ProcessingState::State3,
        (103,_)=>ProcessingState::State6,
        (106,_)=>ProcessingState::State5,
        _=>state,
    };
    let disposition = if action == 9 { QueueDisposition::Persistent(PersistentQueueEvent::ProfileBytes) }
    else {
        let address = if action <= 101 { 0xecc3e4 + (u32::from(action)-2)*24 } else { 0xeccd7c + (u32::from(action)-102)*24 };
        QueueDisposition::UnownedCpu(CpuRoot(address))
    };
    Ok(QueueResolution { disposition, next_state })
}

const SELECTOR_EVENTS: &[(u8,u8)] = &[
    (0,0),(1,1),(2,2),(3,3),(4,4),(5,5),(6,6),(7,7),(8,8),(9,9),(10,10),(11,11),(12,12),(13,14),(14,13),
    (16,15),(17,16),(18,17),(19,18),(20,19),(21,20),(22,21),(23,22),(24,23),(32,24),(33,25),(34,26),(35,27),(36,28),(40,29),
    (48,30),(49,31),(50,32),(52,33),(53,34),(54,35),(64,36),(66,37),(67,38),(68,39),(69,40),(70,41),(71,42),(72,43),(73,44),
    (80,45),(81,46),(82,47),(83,48),(84,49),(85,50),(86,51),(96,52),(97,53),(98,54),(99,55),(100,56),(101,57),(102,58),(103,59),
    (104,60),(106,61),(107,62),(108,63),(109,64),(110,65),(112,66),(113,67),(114,68),(115,69),(116,70),(122,71),(123,72),
    (128,73),(129,74),(130,75),(131,76),(132,77),(133,78),(134,79),(135,80),(136,81),(137,82),(138,83),(139,84),(140,85),
    (144,86),(145,87),(146,88),(147,89),(160,90),(161,91),(162,92),(164,95),(165,96),(166,97),(167,98),(168,99),(169,100),(170,101),(172,102),
    (176,103),(177,104),(178,105),(192,106),(193,107),(194,108),(195,109),(196,110),(197,111),(198,112),(200,113),(201,114),
    (202,115),(203,116),(204,117),(208,118),(209,119),(210,120),(211,121),(212,122),(213,123),(214,124),(215,125),(216,126),
    (218,127),(219,128),(220,129),(221,130),(222,131),(224,132),(225,133),(226,134),(227,135),(228,136),(232,137),(233,138),(234,139),(235,140),(240,141),
];
