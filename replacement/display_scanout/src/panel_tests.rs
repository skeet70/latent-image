use crate::panel::*;
use std::vec::Vec;
#[derive(Debug,PartialEq,Eq)]
enum Effect {Identity,Queued(PanelRequest)}
struct Model {effects:Vec<Effect>,status:QueueStatus}
impl Transport for Model {
    fn current_task(&mut self)->TaskId{self.effects.push(Effect::Identity);TaskId(0x71)}
    fn post(&mut self,request:PanelRequest)->QueueStatus{self.effects.push(Effect::Queued(request));self.status}
}
#[test]
fn valid_panel_commands_queue_the_exact_caller_completion() {
    for raw in 0..=6 {
        let command=PanelCommand::parse(raw).unwrap();
        let completion=Completion::parse(0x231400);
        let mut model=Model{effects:Vec::new(),status:QueueStatus(0)};
        assert_eq!(send(command,completion,&mut model),Ok(()));
        assert_eq!(model.effects,[Effect::Identity,Effect::Queued(PanelRequest{sender:TaskId(0x71),command,completion})]);
    }
    assert_eq!(PanelCommand::parse(7),None);
    assert_eq!(PanelCommand::parse(u32::MAX),None);
}
#[test]
fn null_completion_and_every_send_failure_are_preserved_without_retry() {
    for status in [-255,-1,1,17] {
        let mut model=Model{effects:Vec::new(),status:QueueStatus(status)};
        assert_eq!(send(PanelCommand::Command6,Completion::parse(0),&mut model),Err(SendError::Submission));
        assert_eq!(SendError::Submission.stock(),-255);
        assert_eq!(model.effects,[Effect::Identity,Effect::Queued(PanelRequest{sender:TaskId(0x71),command:PanelCommand::Command6,completion:None})]);
    }
}
