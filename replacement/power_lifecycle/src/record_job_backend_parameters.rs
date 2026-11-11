//! X-M1 1.50 f6a0cc parameter packing and f6b890 reuse eligibility.
use crate::record_job_backend_packet::Address;
unsafe extern "C" {
    fn xm1_power_record_helper_f5f3a8(value:i32)->u32;
    fn xm1_power_record_helper_f5eb34(a:u32,b:u32,c:u32,d:u32,e:u32,f:u32);
    fn xm1_power_record_helper_69f8fc(key:u32)->u32;
    fn xm1_power_record_helper_f615bc(item:u32,reference:u32)->u32;
    fn xm1_power_record_helper_74e5c0(item:u32)->u32;
    fn xm1_power_record_helper_f60b78(a:u32,b:u32,c:u32,d:u32)->u32;
    fn xm1_power_record_helper_74eb20(item:u32)->u32;
    fn xm1_power_record_helper_7503ec(item:u32)->u32;
    fn xm1_power_record_helper_f612dc(item:u32,reference:u32)->u32;
    fn xm1_power_record_helper_f61584(item:u32,reference:u32)->u32;
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f6a0cc(row:u32,source:u32,item:u32,mode:u32,out:u32,tail:u32) {
    unsafe {
        let source=Address(source);let out=Address(out);let p=source.at(row*0x48);let s=source.at(row*0x5c);
        let ordinal=source.byte(row*8+10) as u32;let kind=source.word(row*8+12);
        out.put(0,row);
        for (target,offset) in [(4,0x5c),(6,0x58),(12,0x6c),(14,0x68),(24,0x64),(26,0x60),(68,0x7c),(70,0x80),(72,0x84),(74,0x88)] {out.put_half(target,p.word(offset) as i32);}
        out.put_half(8,16);out.put_half(10,16);out.put(16,p.word(0x50));out.put(76,s.word(0x294));out.put(84,s.word(0x298));out.put(80,s.at(0x29c).0);out.put(20,0);
        out.put_half(48,source.half(row*8+8) as i32);out.put(64,(xm1_power_record_helper_f5f3a8(Address(item).signed(0))!=0) as u32);
        xm1_power_record_helper_f5eb34(out.at(28).0,out.at(32).0,out.at(36).0,out.at(40).0,1,1);
        out.put_half(50,0x8000);
        for index in 0..source.byte(0) as u32 {if source.byte(10+index*8) as u32==ordinal.wrapping_sub(1) {out.put_half(50,source.half(8+index*8) as i32);}}
        out.put(44,(source.word(76)==0 && ordinal==source.byte(72) as u32+1) as u32);
        if matches!(kind,3|4) {out.put(60,kind);out.put_half(58,if kind==3 {-600} else {-900});}
        else {
            out.put(60,0);
            let count=source.byte(0) as u32;
            if count!=0 {
                let mut index=0;
                while source.word(0x90+index*0x48)!=2 && index+1<count {index+=1;}
                if source.word(0x90+index*0x48)==2 {out.put(60,if ordinal<source.byte(10+index*8) as u32 {2} else {1});}
            }
        }
        out.put(96,Address(mode).word(4));Address(tail).put(16,p.word(0x94));Address(tail).put(40,s.word(0x2ec));
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f6b890(context:u32)->u32 {
    unsafe {
        let c=Address(context);let diagnostic=Address(c.word(12));
        if diagnostic.0!=0 {
            for (dst,src) in [(0xfd2c,0x4f4),(0xfd30,0x4f8),(0xfd34,0x4fc),(0xfd3c,0x504)] {diagnostic.put(dst,c.word(src));}
            diagnostic.put(0xfd38,c.half(0x500) as u32);diagnostic.put(0xfd68,0);diagnostic.put(0xfd6c,(c.signed(0x508)+500) as u32);diagnostic.put(0xfd70,(c.signed(0x4b2)+500) as u32);diagnostic.put(0xfd74,xm1_power_record_helper_69f8fc(0x65a7));
        }
        if c.word(0)&2==0 && c.word(4)==1 {return 1;}
        if c.word(0x4f4)==0 || c.word(0x1264)==0 || c.word(0x1128)!=0 {return 0;}
        let delta=(c.signed(0x4b2)-c.signed(0x508)).unsigned_abs()&0xffff;
        if xm1_power_record_helper_69f8fc(0x65a7)*10<=delta {if diagnostic.0!=0 {diagnostic.put(0xfd68,1);}return 0;}
        let item=c.word(32);let mode=xm1_power_record_helper_f615bc(item,0);let kind=xm1_power_record_helper_74e5c0(item);
        if xm1_power_record_helper_f60b78(kind,mode,c.word(48),c.word(24))!=0 {return 0;}
        if mode!=6 && c.word(24)!=c.word(0x4f8) {return 0;}
        if xm1_power_record_helper_74eb20(item)!=0 && c.word(64)!=c.word(0x504) {return 0;}
        if xm1_power_record_helper_7503ec(item)==0 {return 1;}
        if xm1_power_record_helper_f612dc(item,0)!=c.word(0x4fc) {return 0;}
        (xm1_power_record_helper_f61584(item,0)==c.half(0x500) as u32) as u32
    }
}
