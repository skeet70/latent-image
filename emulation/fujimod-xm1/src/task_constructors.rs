use fujimod_xm1::{bridge::{read, sha256, write}, native_case::{self, Capture, CaptureAddress, StubSnapshot}};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt::Write as _, path::{Path, PathBuf}};

const QUEUE: u32 = 0x724cdc;
const SUBMIT: u32 = 0x725dd8;
const EXIT: u32 = 0x200000;
const CALLBACK: u32 = 0x200004;
const STACK: u32 = 0x300000;
const SP: u32 = STACK + 0x1000;
const MAGIC: u32 = 0xa5f0a5f0;
const FANOUT: [u32; 11] = [0xfea408, 0xf4c368, 0x10546c0, 0x1039500, 0xeecbf4, 0x1009974, 0x1050efc, 0x1051f30, 0x1053854, 0xef7d44, CALLBACK];

type Result<T, E = String> = std::result::Result<T, E>;
fn require(ok: bool, detail: &str) -> Result<()> { if ok { Ok(()) } else { Err(detail.into()) } }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
struct Address(u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
struct TaskId(u32);
#[derive(Clone, Copy)]
enum Behavior { Forward, Mapped, Always, Usb, Record, Saclcac }
struct Constructor { entry: Address, bytes: usize, task: TaskId, message: u32, name: Address, priority: u32, stack: u32, worker: Address, behavior: Behavior }
const fn ctor(entry: u32, bytes: usize, task: u32, message: u32, name: u32, priority: u32, stack: u32, worker: u32, behavior: Behavior) -> Constructor {
    Constructor { entry: Address(entry), bytes, task: TaskId(task), message, name: Address(name), priority, stack, worker: Address(worker), behavior }
}
// x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; NONMATCHING entries checked against the stock instruction listings.
const CONSTRUCTORS: [Constructor; 13] = [
    ctor(0xd80d44,144,36,0x20,0xd80dd4,11,0xc00,0xd81218,Behavior::Usb),
    ctor(0xec5350,120,2,0x100,0xec53e8,6,0x1000,0xec2668,Behavior::Forward),
    ctor(0xdca604,172,3,0x34,0xdca790,6,0x1000,0xe00a80,Behavior::Record),
    ctor(0xdb63e0,120,4,0xcc,0xdb6458,6,0x1000,0x1215480,Behavior::Forward),
    ctor(0xed10b4,120,7,0x54,0xed1354,6,0xc00,0xecc058,Behavior::Forward),
    ctor(0xef8c90,292,43,0xb0,0xef9a8c,5,0x1000,0xf01bc8,Behavior::Saclcac),
    ctor(0xed2b00,120,35,0xc,0xed3adc,9,0xc00,0xed2ce0,Behavior::Forward),
    ctor(0x1055d10,116,64,0x40,0x1055e04,6,0xc00,0x1054cf4,Behavior::Forward),
    ctor(0x69f000,128,80,0x10,0x781994,6,0xc00,0x69f0e0,Behavior::Mapped),
    ctor(0x10726f4,136,56,0x10,0x1072960,12,0x1400,0x1071ca4,Behavior::Mapped),
    ctor(0x1066280,132,55,0x18,0x1066474,7,0xc00,0x106ceec,Behavior::Always),
    ctor(0x1062090,132,62,0xb0,0x1062414,12,0x1000,0x1064224,Behavior::Always),
    ctor(0xd0a238,120,1,0x54,0xd0a2b0,6,0xc00,0xc22f60,Behavior::Forward),
];

#[derive(Clone, Copy, Debug, Serialize)]
struct Case { mailbox: u32, submit: u32, auxiliary: u32, argument: u32, callback: bool, optional_word: u32, failure: Option<Address> }
impl Case {
    fn submitted(self, ctor: &Constructor) -> bool { self.mailbox == 0 || matches!(ctor.behavior, Behavior::Always | Behavior::Saclcac) }
    fn status(self, service: u32) -> u32 {
        match service { QUEUE => self.mailbox, SUBMIT => self.submit, CALLBACK => 0x13579bdf, 0xef7d44 => 0x2468ace0, _ => if self.failure == Some(Address(service)) { 0xffffffd7 } else { 0 } }
    }
    fn sequence(self, ctor: &Constructor) -> Vec<u32> {
        let mut sequence = Vec::new();
        if matches!(ctor.behavior, Behavior::Saclcac) { sequence.extend([0xeea2b0, 0xeeb55c, 0xee8e08]); }
        sequence.push(QUEUE);
        if self.submitted(ctor) { sequence.push(SUBMIT); }
        match ctor.behavior {
            Behavior::Usb if self.mailbox == 0 && self.submit == 0 => sequence.push(0xd81424),
            Behavior::Record if self.mailbox == 0 && self.submit == 0 => sequence.extend([0xe66750,0x72d9f0,0xdca5dc]),
            Behavior::Saclcac => {
                if self.optional_word == 0 { sequence.push(0x76fce4); }
                sequence.extend_from_slice(&FANOUT[..10]);
                if self.callback { sequence.push(CALLBACK); }
            }
            _ => {}
        }
        sequence
    }
    fn result(self, ctor: &Constructor) -> u32 {
        match ctor.behavior {
            Behavior::Forward | Behavior::Record => if self.mailbox != 0 { self.mailbox } else { self.submit },
            Behavior::Mapped | Behavior::Usb | Behavior::Always => if self.mailbox | self.submit == 0 { 0 } else { (-255i32) as u32 },
            Behavior::Saclcac => self.status(if self.callback { CALLBACK } else { 0xef7d44 }),
        }
    }
}

#[derive(Clone)]
struct Region { address: u32, permission: u8, bytes: Vec<u8> }
struct Program { regions: Vec<Region>, functions: Vec<(u32,u32)> }
fn word(bytes: &[u8], offset: usize) -> Result<u32> {
    let value: [u8;4] = bytes.get(offset..offset.checked_add(4).ok_or("word offset overflow")?).ok_or("word outside input")?.try_into().map_err(|_| "word width")?;
    Ok(u32::from_le_bytes(value))
}
fn half(bytes: &[u8], offset: usize) -> Result<usize> {
    let value: [u8;2] = bytes.get(offset..offset.checked_add(2).ok_or("halfword offset overflow")?).ok_or("halfword outside input")?.try_into().map_err(|_| "halfword width")?;
    Ok(u16::from_le_bytes(value) as usize)
}
fn slice(bytes: &[u8], offset: u32, length: u32) -> Result<&[u8]> {
    bytes.get(offset as usize..(offset as usize).checked_add(length as usize).ok_or("ELF extent overflow")?).ok_or_else(|| "ELF extent outside file".into())
}
fn elf(bytes: &[u8], ctor: &Constructor) -> Result<Program> {
    require(bytes.get(..7) == Some(b"\x7fELF\x01\x01\x01") && half(bytes,18)? == 40, "candidate is not ELF32 little-endian ARM")?;
    let headers = word(bytes,28)? as usize;
    let stride = half(bytes,42)?;
    require(stride == 32, "ELF program header size differs")?;
    let mut regions = Vec::new();
    for index in 0..half(bytes,44)? {
        let at = headers.checked_add(index.checked_mul(stride).ok_or("ELF header overflow")?).ok_or("ELF header overflow")?;
        if word(bytes,at)? != 1 { continue; }
        let address = word(bytes,at+8)?;
        let length = word(bytes,at+16)?;
        if length == 0 { continue; }
        require(word(bytes,at+20)? == length && word(bytes,at+24)? & 2 == 0, "candidate has writable or zero-fill load segment")?;
        if CONSTRUCTORS.iter().any(|other| other.entry.0 == address && other.entry != ctor.entry) { continue; }
        require(address == ctor.entry.0, "candidate has a non-constructor load segment")?;
        regions.push(Region { address, permission: 5, bytes: slice(bytes,word(bytes,at+4)?,length)?.to_vec() });
    }
    require(regions.len() == 1, "candidate constructor has no unique load segment")?;
    let table = word(bytes,32)? as usize;
    let stride = half(bytes,46)?;
    require(stride == 40, "ELF section header size differs")?;
    let mut functions = BTreeMap::new();
    for index in 0..half(bytes,48)? {
        let at = table.checked_add(index.checked_mul(stride).ok_or("ELF section overflow")?).ok_or("ELF section overflow")?;
        if word(bytes,at+4)? != 2 { continue; }
        require(word(bytes,at+36)? == 16, "ELF symbol size differs")?;
        let symbols = slice(bytes,word(bytes,at+16)?,word(bytes,at+20)?)?;
        for symbol in symbols.chunks_exact(16) {
            if symbol[12] & 15 != 2 { continue; }
            let address = word(symbol,4)?;
            let length = word(symbol,8)?;
            if length == 0 || !regions.iter().any(|region| address >= region.address && u64::from(address)+u64::from(length) <= u64::from(region.address)+region.bytes.len() as u64) { continue; }
            require(address % 4 == 0 && length % 4 == 0, "candidate function is not A32 word sized")?;
            if let Some(prior) = functions.insert(address,length) { require(prior == length, "ELF aliases disagree on function extent")?; }
        }
    }
    require(functions.contains_key(&ctor.entry.0), "candidate ELF lacks constructor function symbol")?;
    Ok(Program { regions, functions: functions.into_iter().collect() })
}
fn stock(image4: &[u8], image5: &[u8], ctor: &Constructor) -> Result<Program> {
    let (base, image) = if ctor.entry.0 < 0xbf1a00 { (0x61c400,image4) } else { (0xbf1a00,image5) };
    let bytes = slice(image,ctor.entry.0-base,ctor.bytes as u32)?.to_vec();
    Ok(Program { regions: vec![Region { address: ctor.entry.0, permission: 5, bytes }], functions: vec![(ctor.entry.0,ctor.bytes as u32)] })
}
fn hex(bytes: &[u8]) -> String { const DIGITS: &[u8;16] = b"0123456789abcdef"; let mut result = String::with_capacity(bytes.len()*2); for byte in bytes { result.push(DIGITS[(byte>>4) as usize] as char); result.push(DIGITS[(byte&15) as usize] as char); } result }
fn set(bytes: &mut [u8], at: usize, value: u32) { bytes[at..at+4].copy_from_slice(&value.to_le_bytes()); }
fn wire(program: &Program, ctor: &Constructor, case: Case) -> (Vec<u8>, Vec<Capture>, Vec<Region>) {
    let mut regions = program.regions.clone();
    regions.push(Region { address: EXIT, permission: 5, bytes: vec![0;8] });
    let mut stack = vec![0x5a;0x1000];
    set(&mut stack,0x1000-if matches!(ctor.behavior,Behavior::Saclcac) {36} else {20},case.auxiliary);
    if ctor.task == TaskId(80) { set(&mut stack,0x1000-24,case.argument); }
    regions.push(Region { address: STACK, permission: 3, bytes: stack });
    regions.push(Region { address: 0x1526a7c, permission: 3, bytes: vec![0x96;8] });
    regions.push(Region { address: 0x169d370, permission: 3, bytes: vec![0x96;8] });
    regions.push(Region { address: 0x954dcc, permission: 1, bytes: case.optional_word.to_le_bytes().to_vec() });
    let services = [QUEUE,SUBMIT,0xd81424,0xe66750,0x72d9f0,0xdca5dc,0xeea2b0,0xeeb55c,0xee8e08,0x76fce4,0xfea408,0xf4c368,0x10546c0,0x1039500,0xeecbf4,0x1009974,0x1050efc,0x1051f30,0x1053854,0xef7d44,CALLBACK];
    let mut captures: Vec<_> = services.iter().map(|&service| Capture { service,address: CaptureAddress::Fixed(0),bytes: 0 }).collect();
    captures.extend([Capture {service:QUEUE,address:CaptureAddress::Register(1),bytes:8},Capture {service:SUBMIT,address:CaptureAddress::Register(0),bytes:28}]);
    for service in [QUEUE,SUBMIT] { for (address,bytes) in [(0x1526a7c,8),(0x169d370,8)] { captures.push(Capture {service,address:CaptureAddress::Fixed(address),bytes}); } }
    let mut text = format!("FUJI_NATIVE_V1 cortex-r5f-arm-le {:x} {EXIT:x} 10000 1000 ",ctor.entry.0);
    for index in 0..15 { let value = match index { 0 => if case.callback {CALLBACK} else {0}, 13 => SP, 14 => EXIT, _ => register_seed(index) }; write!(&mut text,"{value:x} ").expect("string append"); }
    writeln!(&mut text,"10 {} {} {}",regions.len(),program.functions.len(),services.len()).expect("string append");
    for region in &regions { writeln!(&mut text,"{:x} {:x} {} {}",region.address,region.bytes.len(),region.permission,hex(&region.bytes)).expect("string append"); }
    for (address,bytes) in &program.functions { writeln!(&mut text,"{address:x} {bytes:x}").expect("string append"); }
    for service in services { writeln!(&mut text,"{service:x} 0 1 0 {:x}",case.status(service)).expect("string append"); }
    (text.into_bytes(),captures,regions)
}
fn register_seed(index: usize) -> u32 { 0x11220000+index as u32*0x101 }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from="String")]
struct NativeWord(u32);
impl TryFrom<String> for NativeWord { type Error=String; fn try_from(value:String)->Result<Self> { u32::from_str_radix(value.strip_prefix("0x").ok_or("native word lacks prefix")?,16).map(Self).map_err(|error|error.to_string()) } }
#[derive(Deserialize)]
struct Registers { r0: NativeWord,r4: NativeWord,r5: NativeWord,r6: NativeWord,r7: NativeWord,r8: NativeWord,r9: NativeWord,r10: NativeWord,r11: NativeWord,sp: NativeWord,pc: NativeWord,cpsr: NativeWord }
#[derive(Deserialize)]
struct FinalRegion { address: NativeWord, bytes: Option<String> }
#[derive(Deserialize)]
struct Observation { registers: Registers, memory: Vec<FinalRegion> }
#[derive(Debug, PartialEq, Eq, Serialize)]
struct Effect { services: Vec<Address>, descriptor: Option<[u32;7]>, result:u32, record:[u32;2], saclcac:[u32;2] }
fn captured(snapshot: &StubSnapshot, address: u32) -> Result<&[u8]> { snapshot.memory.iter().find(|memory|memory.address==address).map(|memory|memory.bytes.as_slice()).ok_or_else(||"missing service memory capture".into()) }
fn final_bytes(observation:&Observation,address:u32)->Result<Vec<u8>> {
    let text=observation.memory.iter().find(|region|region.address.0==address).and_then(|region|region.bytes.as_ref()).ok_or("missing final memory")?;
    require(text.len()%2==0,"final memory hex width")?;
    text.as_bytes().chunks_exact(2).map(|pair| { let text=std::str::from_utf8(pair).map_err(|error|error.to_string())?; u8::from_str_radix(text,16).map_err(|error|error.to_string()) }).collect()
}
fn admit(ctor:&Constructor,case:Case,outcome:&native_case::Outcome,snapshots:&[StubSnapshot],initial:&[Region])->Result<Effect> {
    require(outcome.returned,"native execution did not return")?;
    let observation:Observation=serde_json::from_slice(&outcome.json).map_err(|error|error.to_string())?;
    require(observation.registers.cpsr.0 & 0xff == 0x10,"constructor processor mode differs")?;
    require(snapshots.iter().all(|snapshot|snapshot.registers[13]%8==0),"external service stack alignment differs")?;
    let registers=&observation.registers;
    require(registers.sp.0==SP && registers.pc.0==EXIT,"constructor return frame differs")?;
    for (index,value) in [registers.r4,registers.r5,registers.r6,registers.r7,registers.r8,registers.r9,registers.r10,registers.r11].iter().enumerate() { require(value.0==register_seed(index+4),"callee-saved register differs")?; }
    require(registers.r0.0==case.result(ctor),"constructor result differs")?;
    let sequence:Vec<_>=snapshots.iter().map(|snapshot|snapshot.service).collect();
    require(sequence==case.sequence(ctor),"external service ordering differs")?;
    let queue=snapshots.iter().find(|snapshot|snapshot.service==QUEUE).ok_or("mailbox service missing")?;
    require(queue.registers[0]==ctor.task.0,"mailbox task identifier differs")?;
    let config=captured(queue,queue.registers[1])?;
    require(word(config,0)?==48 && word(config,4)?==ctor.message,"mailbox descriptor differs")?;
    let record=if matches!(ctor.behavior,Behavior::Record) {[44,0x1526a84]} else {[0x96969696;2]};
    let saclcac=if matches!(ctor.behavior,Behavior::Saclcac) {[43,0x96963ba2]} else {[0x96969696;2]};
    for (address,expected) in [(0x1526a7c,record),(0x169d370,saclcac)] {
        let before=captured(queue,address)?;
        require([word(before,0)?,word(before,4)?]==expected,"global initialization did not precede mailbox creation")?;
        let after=final_bytes(&observation,address)?;
        require([word(&after,0)?,word(&after,4)?]==expected,"constructor final globals differ")?;
    }
    let descriptor=if case.submitted(ctor) {
        let submit=snapshots.iter().find(|snapshot|snapshot.service==SUBMIT).ok_or("task submission missing")?;
        let bytes=captured(submit,submit.registers[0])?;
        let mut descriptor=[0;7]; for (index,value) in descriptor.iter_mut().enumerate() {*value=word(bytes,index*4)?;}
        let expected=[ctor.task.0,ctor.name.0,ctor.priority,ctor.stack,ctor.worker.0,if ctor.task==TaskId(80){case.argument}else{0},case.auxiliary];
        require(descriptor==expected,"task descriptor words 0..6 differ")?;
        require((descriptor[6]==MAGIC)==(case.auxiliary==MAGIC),"task auxiliary classification differs")?;
        Some(descriptor)
    } else {None};
    if matches!(ctor.behavior,Behavior::Usb) && case.mailbox|case.submit==0 {
        require(snapshots.iter().find(|snapshot|snapshot.service==0xd81424).is_some_and(|snapshot|snapshot.registers[0]==0),"USB36 post-clear argument differs")?;
    }
    if matches!(ctor.behavior,Behavior::Record) && case.mailbox|case.submit==0 {
        require(snapshots.iter().find(|snapshot|snapshot.service==0xe66750).is_some_and(|snapshot|snapshot.registers[0]==3),"record initialize task differs")?;
    }
    if matches!(ctor.behavior,Behavior::Saclcac) && case.callback {
        let failed=case.mailbox!=0 || case.failure.is_some_and(|failure| [0x1039500,0xeecbf4,0x1050efc].contains(&failure.0) || (failure.0==0x76fce4 && case.optional_word==0));
        require(snapshots.last().is_some_and(|snapshot|snapshot.service==CALLBACK && snapshot.registers[0]==if failed {(-98i32)as u32}else{0}),"saclcac callback status differs")?;
    }
    for region in initial.iter().filter(|region|region.permission!=3) { require(final_bytes(&observation,region.address)?==region.bytes,"read-only input changed")?; }
    Ok(Effect {services:sequence.into_iter().map(Address).collect(),descriptor,result:registers.r0.0,record,saclcac})
}

#[derive(Serialize)]
struct CaseReceipt { entry:Address,case:Case,stock_observation:PathBuf,candidate_observation:PathBuf,stock_sha256:String,candidate_sha256:String,effect:Effect }
#[derive(Serialize)]
struct Receipt {schema:u32,qualification:&'static str,image4_sha256:String,image5_sha256:String,candidate_elf_sha256:String,cases:Vec<CaseReceipt>,negative_control:&'static str,limits:[&'static str;3]}
fn run_case(program:&Program,ctor:&Constructor,case:Case,path:&Path)->Result<(native_case::Outcome,Vec<StubSnapshot>,Vec<Region>)> {
    let (wire,captures,initial)=wire(program,ctor,case);
    let (outcome,snapshots)=native_case::run_captured(&wire,&captures);
    write(path,&outcome.json)?;
    write(&path.with_extension("captures.json"),&serde_json::to_vec_pretty(&snapshots).map_err(|error|error.to_string())?)?;
    Ok((outcome,snapshots,initial))
}
fn execute(image4:&Path,image5:&Path,candidate:&Path,output:&Path)->Result<()> {
    let image4=read(image4)?;let image5=read(image5)?;let candidate=read(candidate)?;
    require(sha256(&image4)=="f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16" && sha256(&image5)=="3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a","stock source identity differs")?;
    std::fs::create_dir(output).map_err(|error|error.to_string())?;
    let mut cases=Vec::new();
    for ctor in &CONSTRUCTORS {
        let stock=stock(&image4,&image5,ctor)?;let owned=elf(&candidate,ctor)?;
        let mut inputs=Vec::new();
        for auxiliary in [MAGIC,0x10203040] {for (mailbox,submit) in [(0,0),(0xffffff83,0),(0,0xffffffd7),(0xffffff83,0xffffffd7)] {
            let base=Case {mailbox,submit,auxiliary,argument:0xdec0ad12,callback:false,optional_word:0,failure:None};
            if matches!(ctor.behavior,Behavior::Saclcac) {
                for callback in [false,true] {for optional_word in [0,0x100] {for failure in [None,Some(Address(0x76fce4)),Some(Address(0x1039500)),Some(Address(0xeecbf4)),Some(Address(0x1050efc))] { inputs.push(Case {callback,optional_word,failure,..base});}}}
            } else {inputs.push(base);}
        }}
        for case in inputs {
            let index=cases.len();let stock_name=PathBuf::from(format!("{index:04}-stock.json"));let candidate_name=PathBuf::from(format!("{index:04}-candidate.json"));
            let (stock_out,stock_snapshots,stock_initial)=run_case(&stock,ctor,case,&output.join(&stock_name))?;
            let stock_effect=admit(ctor,case,&stock_out,&stock_snapshots,&stock_initial).map_err(|error|format!("stock {:x} case {index}: {error}",ctor.entry.0))?;
            let (candidate_out,candidate_snapshots,candidate_initial)=run_case(&owned,ctor,case,&output.join(&candidate_name))?;
            let candidate_effect=admit(ctor,case,&candidate_out,&candidate_snapshots,&candidate_initial).map_err(|error|format!("candidate {:x} case {index}: {error}",ctor.entry.0))?;
            require(stock_effect==candidate_effect,"stock and candidate effects differ")?;
            cases.push(CaseReceipt {entry:ctor.entry,case,stock_observation:stock_name,candidate_observation:candidate_name,stock_sha256:sha256(&stock_out.json),candidate_sha256:sha256(&candidate_out.json),effect:stock_effect});
        }
    }
    let ctor=&CONSTRUCTORS[1];let mut mutant=stock(&image4,&image5,ctor)?;
    require(word(&mutant.regions[0].bytes,0x44)?==0xe3a03006,"negative control instruction differs")?;
    set(&mut mutant.regions[0].bytes,0x44,0xe3a03005);
    let case=Case {mailbox:0,submit:0,auxiliary:MAGIC,argument:0xdec0ad12,callback:false,optional_word:0,failure:None};
    let (outcome,snapshots,initial)=run_case(&mutant,ctor,case,&output.join("negative-priority.json"))?;
    require(outcome.returned,"negative control did not execute to return")?;
    require(admit(ctor,case,&outcome,&snapshots,&initial).is_err(),"wrong-priority constructor passed the independent oracle")?;
    let receipt=Receipt {schema:1,qualification:"isolated-constructor-stock-owned-admission",image4_sha256:sha256(&image4),image5_sha256:sha256(&image5),candidate_elf_sha256:sha256(&candidate),cases,negative_control:"stock syscont priority 6 changed to 5, returned but refused",limits:["External services are synchronous host hooks; their bodies and task execution are not qualified.","Synthetic stack frames exercise both D+24 classifications and flashext's unwritten argument; natural reachability is not established.","Single-core isolated execution does not qualify concurrency, physical devices, or integrated startup."]};
    write(&output.join("qualification.json"),&serde_json::to_vec_pretty(&receipt).map_err(|error|error.to_string())?)?;
    println!("qualified {} isolated constructor pairs",receipt.cases.len());Ok(())
}
fn main() {
    let result=(||->Result<()> {
        let mut arguments=std::env::args_os().skip(1);
        let (mut image4,mut image5,mut candidate,mut output)=(None,None,None,None);
        while let Some(flag)=arguments.next() {
            let flag=flag.into_string().map_err(|_|"non-UTF8 option")?;
            let slot=match flag.as_str() {
                "--image4"=>&mut image4,"--image5"=>&mut image5,"--candidate-elf"=>&mut candidate,"--output"=>&mut output,
                _=>return Err("unknown option".into()),
            };
            let value=arguments.next().ok_or("option value missing")?;
            require(slot.replace(PathBuf::from(value)).is_none(),"duplicate option")?;
        }
        execute(&image4.ok_or("missing --image4")?,&image5.ok_or("missing --image5")?,&candidate.ok_or("missing --candidate-elf")?,&output.ok_or("missing --output")?)
    })();
    if let Err(error)=result {eprintln!("task-constructors: {error}");std::process::exit(1);}
}
