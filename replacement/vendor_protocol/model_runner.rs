use std::{env,process};
use xm1_vendor_protocol::{session::{Session,Tick},session_abi::{observe,ControlObservation},transport::PublishedRegion};

fn hex_bytes(text:&str)->Result<Vec<u8>,String> {
    if text.len()!=24 || !text.is_ascii() {return Err("control frame must contain twelve hexadecimal bytes".into());}
    (0..12).map(|index|u8::from_str_radix(&text[index*2..index*2+2],16).map_err(|error|error.to_string())).collect()
}
fn run()->Result<(),String> {
    let args:Vec<String>=env::args().skip(1).collect();
    if args.len()<4 {return Err("usage: model-runner TICK REGION-BASE REGION-LENGTH FRAME...".into());}
    let number=|index:usize|u32::from_str_radix(args[index].trim_start_matches("0x"),16).map_err(|error|error.to_string());
    let tick=Tick(number(0)?);
    let base=number(1)?;
    let length=number(2)?;
    let region=if length==0 {None}else{Some(PublishedRegion::new(base,length).map_err(|error|format!("{error:?}"))?)};
    let mut buffer=[0;256];
    let mut session=Session::new(&mut buffer);
    for text in &args[3..] {
        let frame=hex_bytes(text)?;
        let mut out=ControlObservation::new();
        match observe(&mut session,&frame,tick,region,&[],&mut out) {
            Ok(())=>{
                print!("ok {:x} {:x} {:x} {:x} {:x} {:x} {:x} {:x} {:x} {:x} ",out.delivery,out.status,out.action,out.active,out.sid,out.requested,out.armed,out.ready,out.payload_offset,out.payload_length);
                for byte in &out.reply[..out.reply_length as usize] {print!("{byte:02x}");}
                println!(".");
            }
            Err(error)=>println!("error {error:?}"),
        }
    }
    Ok(())
}
fn main(){if let Err(error)=run(){eprintln!("{error}");process::exit(2);}}
