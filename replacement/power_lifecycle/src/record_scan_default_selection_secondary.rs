use super::*;

#[derive(Clone,Copy)]
enum Evaluation { Reset, Trigger }
#[derive(Clone,Copy)]
enum VoteSource { First, Second, Third, Fourth, Fifth, Sixth }
impl VoteSource {
    fn parse(value:u32)->Option<Self> {
        match value.min(6) { 0=>None,1=>Some(Self::First),2=>Some(Self::Second),3=>Some(Self::Third),4=>Some(Self::Fourth),5=>Some(Self::Fifth),_=>Some(Self::Sixth) }
    }
    fn index(self)->u32 { match self { Self::First=>1,Self::Second=>2,Self::Third=>3,Self::Fourth=>4,Self::Fifth=>5,Self::Sixth=>6 } }
    unsafe fn query(self,evaluation:Evaluation,profile:Profile,kind:Kind)->u32 {
        unsafe {
            match (evaluation,self) {
                (Evaluation::Reset,Self::First)=>xm1_power_record_helper_f4ad7c(profile.0,kind.0),
                (Evaluation::Reset,Self::Second)=>xm1_power_record_helper_f4be18(profile.0,kind.0),
                (Evaluation::Reset,Self::Third)=>xm1_power_record_helper_f51a28(profile.0,kind.0),
                (Evaluation::Reset,Self::Fourth)=>xm1_power_record_helper_f4bdb8(profile.0,kind.0),
                (Evaluation::Reset,Self::Fifth)=>xm1_power_record_helper_f4b0a8(profile.0,kind.0),
                (Evaluation::Reset,Self::Sixth)=>xm1_power_record_helper_f4c324(profile.0,kind.0),
                (Evaluation::Trigger,Self::First)=>xm1_power_record_helper_f4afb0(profile.0,kind.0),
                (Evaluation::Trigger,Self::Second)=>xm1_power_record_helper_f4befc(profile.0,kind.0),
                (Evaluation::Trigger,Self::Third)=>xm1_power_record_helper_f51abc(profile.0,kind.0),
                (Evaluation::Trigger,Self::Fourth)=>xm1_power_record_helper_f4be9c(profile.0,kind.0),
                (Evaluation::Trigger,Self::Fifth)=>xm1_power_record_helper_f4b11c(profile.0,kind.0),
                (Evaluation::Trigger,Self::Sixth)=>xm1_power_record_helper_f4dcf8(profile.0,kind.0),
            }
        }
    }
}
unsafe extern "C" {
    fn xm1_power_record_helper_f4ad7c(profile:u32,kind:u32)->u32;
    fn xm1_power_record_helper_f4be18(profile:u32,kind:u32)->u32;
    fn xm1_power_record_helper_f51a28(profile:u32,kind:u32)->u32;
    fn xm1_power_record_helper_f4bdb8(profile:u32,kind:u32)->u32;
    fn xm1_power_record_helper_f4b0a8(profile:u32,kind:u32)->u32;
    fn xm1_power_record_helper_f4c324(profile:u32,kind:u32)->u32;
    fn xm1_power_record_helper_f4afb0(profile:u32,kind:u32)->u32;
    fn xm1_power_record_helper_f4befc(profile:u32,kind:u32)->u32;
    fn xm1_power_record_helper_f51abc(profile:u32,kind:u32)->u32;
    fn xm1_power_record_helper_f4be9c(profile:u32,kind:u32)->u32;
    fn xm1_power_record_helper_f4b11c(profile:u32,kind:u32)->u32;
    fn xm1_power_record_helper_f4dcf8(profile:u32,kind:u32)->u32;
    fn xm1_power_record_helper_724b68(id:u32,timeout:u32)->i32;
    fn xm1_power_record_helper_724c00(id:u32)->i32;
    fn xm1_power_record_helper_fb2438(record:u32);
    fn xm1_power_record_helper_fb259c(record:u32);
    fn xm1_power_record_helper_fb26a4(record:u32);
    fn xm1_power_record_helper_fb27dc(record:u32);
    fn xm1_power_record_helper_fb35d8(record:u32);
    fn xm1_power_record_selection_callback(callback:u32,selection:u32,flags:u32,first:u32,second:u32,enabled:u32);
}

unsafe fn evaluate(evaluation:Evaluation,profile:Profile,kind:Kind)->Option<Selection> {
    unsafe {
        let (first,threshold)=match evaluation { Evaluation::Reset=>(0x75b6,0x75c2),Evaluation::Trigger=>(0x7619,0x7625) };
        for offset in 0..44 { put_byte(STATE+0xbc+offset,0); }
        let mut weight=0u8;
        for index in 0..6 {
            let key=first+index;
            if let Some(source)=VoteSource::parse(configuration(ConfigurationKey(key))) {
                let value=source.query(evaluation,profile,kind);
                put_byte(STATE+0xbc+index,source.index() as u8);
                put_byte(STATE+0xc2+index,configuration(ConfigurationKey(key+6)) as u8);
                put(STATE+0xc8+index*4,value);
                if value!=0 {
                    weight=weight.wrapping_add(configuration(ConfigurationKey(key+6)) as u8);
                    put_byte(STATE+0xe0,weight);
                    if (weight as u32)>=configuration(ConfigurationKey(threshold)) { return Some(Selection(value)); }
                }
            }
        }
        None
    }
}

unsafe fn clear_after_reset() {
    unsafe {
        xm1_power_record_helper_f4dc18();
        for index in 0..6 { put_byte(STATE+0xbc+index,0); put_byte(STATE+0xc2+index,0); put(STATE+0xc8+index*4,0); }
        put_byte(STATE+0xe0,0); put(STATE+0xe4,0); put(STATE+0xa4,0); put(STATE+0xb8,0); put(STATE+0xb94,1); put(STATE+0x98c,58);
    }
}
unsafe fn reset(profile:Profile,kind:Kind) {
    unsafe {
        if configuration(ConfigurationKey(0x75b2))!=0 {
            let next=byte(STATE+0xb0).wrapping_add(1);
            put_byte(STATE+0xb0,next);
            if (next as u32)>=configuration(ConfigurationKey(0x75b5)) {
                put(STATE+0xa4,0); put_byte(STATE+0xb0,0); put(STATE+0xb8,0); put(STATE+0xb94,1); put(STATE+0x98c,58);
            }
            return;
        }
        put(STATE+0xb4,0);
        let selected=evaluate(Evaluation::Reset,profile,kind).unwrap_or(Selection(0));
        put(STATE+0xe4,selected.0);
        xm1_power_record_helper_f4dea4(STATE+0xe8);
        let history=0x16c5e0c;
        let value=selected.0 as u8;
        if value!=100 {
            let mut carried=byte(history);
            put_byte(history,value);
            for index in 1..20 {
                if carried==100 { break; }
                let previous=byte(history+index);
                put_byte(history+index,carried);
                carried=previous;
            }
        }
        if selected.0==1 {
            put(STATE+0xb4,1);
            let count=configuration(ConfigurationKey(0x75b3)).clamp(1,20);
            let mut accepted=0u8;
            if byte(history+count-1)!=100 {
                for index in (0..count).rev() { if byte(history+index)==1 { accepted=accepted.wrapping_add(1); } }
            }
            if configuration(ConfigurationKey(0x75b4))<=accepted as u32 { clear_after_reset(); }
        }
    }
}

unsafe fn initial_records(initial:Selection,promoted:u32,baseline:Selection,filtered:Selection,choice:&Choice) {
    unsafe {
        put(STATE+0x19a8,initial.0); put(STATE+0x19ac,promoted); put(STATE+0x19b0,baseline.0);
        put(STATE+0x19b4,word(STATE+0x988)); put(STATE+0x19cc,filtered.0); put(STATE+0x19d8,choice.selection.0);
    }
}
unsafe fn select_direct(profile:Profile,kind:Kind,initial:Selection,promoted:u32,baseline:Selection,choice:&mut Choice) {
    unsafe {
        put(STATE+0x19d0,word(STATE+0xb98)); put(STATE+0x19d4,word(STATE+0xb9c));
        put(STATE+0xb98,baseline.0); put(STATE+0xb9c,baseline.0);
        choice.selection=baseline;
        adjust(profile,choice);
        put(STATE+0x19fc,choice.selection.0); put_half(STATE+0x1a00,choice.flags.0);
        bound_flags(profile,kind,choice);
        put(STATE+0x14,choice.selection.0); put_half(STATE+0x18,choice.flags.0);
        initial_records(initial,promoted,baseline,baseline,choice);
        put_half(STATE+0x19dc,choice.flags.0);
    }
}
unsafe fn select_filtered(profile:Profile,kind:Kind,initial:Selection,promoted:u32,baseline:Selection,choice:&mut Choice) {
    unsafe {
        let mut filtered=Selection(xm1_power_record_helper_f4a848(word(STATE+0x98c),baseline.0,kind.0));
        if configuration(ConfigurationKey(0x7690))!=0 && word(STATE+0xb8)==0 {
            let key=match filtered.0 { 0x12..=0x14=>Some(0x7691),0x15..=0x17=>Some(0x7693),0x19..=0x1b=>Some(0x7692),0x22..=0x24=>Some(0x7694),0x25..=0x27=>Some(0x7695),0x28..=0x2a=>Some(0x7696),_=>None };
            if let Some(key)=key { if configuration(ConfigurationKey(key))==0 { filtered=baseline; } }
        }
        put(STATE+0x19d0,word(STATE+0xb98)); put(STATE+0x19d4,word(STATE+0xb9c));
        put(STATE+0xb98,baseline.0); put(STATE+0xb9c,filtered.0);
        choice.selection=Selection(xm1_power_record_helper_f4a154(baseline.0,filtered.0));
        put(STATE+0x14,choice.selection.0); put_half(STATE+0x18,choice.flags.0);
        initial_records(initial,promoted,baseline,filtered,choice);
        adjust(profile,choice);
        put(STATE+0x19fc,choice.selection.0); put_half(STATE+0x1a00,choice.flags.0);
        bound_flags(profile,kind,choice);
        put_half(STATE+0x19dc,choice.flags.0);
    }
}

unsafe fn aggregate_history(choice:&mut Choice,scratch:&mut Scratch) {
    unsafe {
        *choice=Choice::empty();
        let count=configuration(ConfigurationKey(0x75b1)).clamp(1,20);
        let history=0x16c2c40;
        if byte(history+(count-1)*0x214)==100 { xm1_power_record_helper_f4a014(0,choice.value_pointer(),choice.flags_pointer()); }
        else {
            for index in 0..87 { scratch.pointer().add(index).write(0); }
            let mut selected=0u8;
            let mut selected_index=0;
            for index in (0..count).rev() {
                let candidate=byte(history+index*0x214);
                let counter=scratch.pointer().add(candidate as usize);
                let votes=counter.read().wrapping_add(1);
                counter.write(votes);
                if scratch.pointer().add(selected as usize).read()<=votes { selected=candidate; selected_index=index; }
            }
            put_byte(STATE+0xe8,selected);
            let record=history+selected_index*0x214;
            for offset in (2..=0x200).step_by(2) { put_half(STATE+0xe8+offset,half(record+offset)); }
            put_half(STATE+0x2ea,half(record+0x202));
            for offset in [0x204,0x208,0x20c,0x210] { put(STATE+0xe8+offset,word(record+offset)); }
            xm1_power_record_helper_f4a014(selected as u32,choice.value_pointer(),choice.flags_pointer());
        }
        put(STATE+0x9c,choice.selection.0); put_half(STATE+0xa0,choice.flags.0); put(STATE+0xa8,0);
        put(STATE+8,choice.selection.0); put_half(STATE+12,choice.flags.0);
        if choice.selection.0==0 && choice.flags.0==0 { put(STATE+0xa4,0); }
        else {
            put(STATE+0xa4,1);
            let _=xm1_power_record_helper_724b68(0xa8,u32::MAX);
            let base=0x16c9164;
            xm1_power_record_helper_fb2438(base); xm1_power_record_helper_fb259c(base+300);
            xm1_power_record_helper_fb26a4(base+0x192c); xm1_power_record_helper_fb27dc(base+0x1c2c); xm1_power_record_helper_fb35d8(base+0x1d58);
            let _=xm1_power_record_helper_724c00(0xa8);
        }
    }
}

unsafe fn advance(profile:Profile,kind:Kind,choice:&mut Choice,scratch:&mut Scratch) {
    unsafe {
        if word(STATE+0xa8)!=1 { put_byte(STATE+0xac,0); xm1_power_record_helper_f4dbac(); xm1_power_record_helper_f4d884(); }
        let mut initial=Choice::empty();
        if xm1_power_record_helper_f4acd4(profile.0)!=0 {
            if word(STATE+0x90)==1 { put(STATE+0x8c,xm1_power_record_helper_f4c210(profile.0,0)); }
            else { put(STATE+0x8c,xm1_power_record_helper_f4c210(profile.0,1)); xm1_power_record_helper_f4dc50(STATE+0x514); }
            put(STATE+0x90,0);
        }
        query(profile,kind,&mut initial);
        choice.flags=initial.flags;
        let promoted=xm1_power_record_helper_f4a520(profile.0,kind.0);
        let baseline=if promoted!=0 { Selection(xm1_power_record_helper_f4bc58(profile.0,kind.0,initial.selection.0)) } else { initial.selection };
        let mut filter=xm1_power_record_helper_f4a554(profile.0)!=0
            && xm1_power_record_helper_74f09c(profile.0).wrapping_sub(4)>1 && xm1_power_record_helper_f4a57c(baseline.0)==1;
        if filter {
            if word(STATE+0x988)==0 { put(STATE+0x988,1); }
            if configuration(ConfigurationKey(0x768f))!=0 && word(STATE+0xb8)!=1 { filter=false; }
        }
        if filter { select_filtered(profile,kind,initial.selection,promoted,baseline,choice); }
        else { select_direct(profile,kind,initial.selection,promoted,baseline,choice); }
        if xm1_power_record_helper_74e5c0(profile.0)==1 || ((word(STATE+0x988)!=1 || word(STATE+0x98c)!=58) && word(STATE+0xb94)!=1) {
            xm1_power_record_helper_f4d968(choice.selection.0,choice.flags.0 as u32);
            put_byte(STATE+0xac,byte(STATE+0xac).wrapping_add(1));
        } else if byte(STATE+0xac)!=0 { put_byte(STATE+0xac,0); xm1_power_record_helper_f4dbac(); }
        let count=byte(STATE+0xac);
        if (count as u32)<configuration(ConfigurationKey(0x75b1)) { put(STATE+0xa8,1); }
        else { aggregate_history(choice,scratch); }
    }
}

pub(super) unsafe fn select(profile:Profile,kind:Kind,choice:&mut Choice,scratch:&mut Scratch) {
    unsafe {
        if configuration(ConfigurationKey(0x75b0))==0 {
            query(profile,kind,choice);
            xm1_power_record_helper_f4d8bc(choice.selection.0,choice.flags.0 as u32);
            history(kind,choice);
            put(STATE+8,choice.selection.0); put_half(STATE+12,choice.flags.0);
            put(STATE+0x94,choice.selection.0); put_half(STATE+0x98,choice.flags.0);
        } else {
            choice.flags=Flags(0);
            if word(STATE+0xa4)==1 { reset(profile,kind); }
            else {
                if configuration(ConfigurationKey(0x7610))!=0 {
                    if let Some(value)=evaluate(Evaluation::Trigger,profile,kind) { put(STATE+0xe4,value.0); }
                    else { put(STATE+0xe4,0); put(STATE+0xb8,1); }
                }
                if configuration(ConfigurationKey(0x7611))==0 || word(STATE+0xb8)==1 { advance(profile,kind,choice,scratch); }
            }
            if configuration(ConfigurationKey(0x7610))!=0 {
                xm1_power_record_helper_7588e8(scratch.pointer(),profile.0);
                let source=word(STATE+0x2fc);
                for offset in (0..512).step_by(2) { put_half(STATE+0x538+offset,half(source+offset)); }
                put_half(STATE+0x738,half(STATE+0x504)); put(STATE+0x73c,word(STATE+0x50c));
                put(STATE+0x740,scratch.word(56)); put(STATE+0x744,word(STATE+0x510)); put(STATE+0x748,word(STATE+0x514));
            }
            choice.selection=Selection(word(STATE+8)); choice.flags=Flags(half(STATE+12));
        }
        override_selection(profile,ConfigurationKey(0x4a12),3,choice);
        copy(STATE+0xfa8,STATE+0x14,128); copy(STATE+0x1028,STATE+0x94,8); copy(STATE+0x1030,STATE+0x9c,0x8ec);
        if word(STATE)!=choice.selection.0 || half(STATE+4)!=choice.flags.0 {
            let callback=xm1_power_record_helper_756d04(profile.0);
            if callback!=0 { xm1_power_record_selection_callback(callback,choice.selection.0,choice.flags.0 as u32,0,0,1); }
            put(STATE,choice.selection.0); put_half(STATE+4,choice.flags.0);
        }
    }
}
