//! Shared startup capture ownership: the armed site set is the union of every component's requests.
use crate::{machine::Machine,provenance::Guest};
use std::{cell::RefCell,collections::{BTreeMap,BTreeSet},rc::Rc};

#[derive(Clone,Copy,PartialEq,Eq)]
pub enum Component {Route,Dispatcher,Consumer,Bootstrap}
impl Component {
    fn index(self)->usize {match self {Self::Route=>0,Self::Dispatcher=>1,Self::Consumer=>2,Self::Bootstrap=>3}}
    fn label(self,address:Guest)->String {
        match self {Self::Route=>"temporary-boundary".into(),Self::Dispatcher=>"shared-dispatch-return".into(),
            Self::Consumer=>format!("consumer-{:x}",address.0),Self::Bootstrap=>"first-carrier-fetch".into()}
    }
}
#[derive(Default)]
struct Requests {permanent:BTreeSet<Guest>,dynamic:BTreeSet<Guest>}
impl Requests {fn holds(&self,address:Guest)->bool {self.permanent.contains(&address)||self.dynamic.contains(&address)}}
#[derive(Clone)]
pub struct StartupSites(Rc<RefCell<[Requests;4]>>);
impl StartupSites {
    /// Takes each component's permanent sites in `Component` order and returns the initial armed map, labelled by the first requesting component.
    pub fn new(permanent:[BTreeMap<u32,String>;4])->(Self,BTreeMap<u32,String>) {
        let mut initial=BTreeMap::new();
        let requests=permanent.map(|sites|{
            for (pc,label) in &sites {initial.entry(*pc).or_insert_with(||label.clone());}
            Requests {permanent:sites.into_keys().map(Guest).collect(),dynamic:BTreeSet::new()}
        });
        (Self(Rc::new(RefCell::new(requests))),initial)
    }
    pub fn acquire(&self,machine:&mut Machine,component:Component,address:Guest)->Result<(),String> {
        if address.0&3!=0 {return Err("shared startup site request is not A32 aligned".into());}
        self.0.borrow_mut()[component.index()].dynamic.insert(address);
        if !machine.traces.sites.contains_key(&address.0) {
            machine.traces.add_sites(&machine.session,BTreeMap::from([(address.0,component.label(address))]))?;
        }
        Ok(())
    }
    /// Drops dynamic requests; the component's permanent requests remain.
    pub fn release(&self,machine:&mut Machine,component:Component,addresses:&[Guest])->Result<(),String> {
        self.withdraw(machine,component,addresses,false)
    }
    /// Drops dynamic and permanent requests.
    pub fn revoke(&self,machine:&mut Machine,component:Component,addresses:&[Guest])->Result<(),String> {
        self.withdraw(machine,component,addresses,true)
    }
    pub fn close(&self,machine:&mut Machine,component:Component)->Result<(),String> {
        let addresses:Vec<Guest>={let requests=self.0.borrow();let own=&requests[component.index()];own.permanent.union(&own.dynamic).copied().collect()};
        self.withdraw(machine,component,&addresses,true)
    }
    fn withdraw(&self,machine:&mut Machine,component:Component,addresses:&[Guest],permanent:bool)->Result<(),String> {
        let mut remove=Vec::new();
        {
            let mut requests=self.0.borrow_mut();
            for &address in addresses {
                let own=&mut requests[component.index()];
                own.dynamic.remove(&address);
                if permanent {own.permanent.remove(&address);}
                if !requests.iter().any(|other|other.holds(address))
                    && machine.traces.sites.contains_key(&address.0) && !remove.contains(&address) {remove.push(address);}
            }
        }
        if !remove.is_empty() {machine.traces.remove_sites(&machine.session,&remove)?;}
        Ok(())
    }
}
