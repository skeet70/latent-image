use super::*;

#[derive(Clone,Copy)]
enum VoteSource { First, Second, Third, Force, Fifth, Sixth }
impl VoteSource {
    fn parse(value:u32)->Option<Self> {
        match value.min(6) { 0=>None,1=>Some(Self::First),2=>Some(Self::Second),3=>Some(Self::Third),4=>Some(Self::Force),5=>Some(Self::Fifth),_=>Some(Self::Sixth) }
    }
}
unsafe extern "C" {
    fn xm1_power_record_helper_f4a710(profile:u32,value:*mut u32)->u32;
    fn xm1_power_record_helper_f4bd34(profile:u32,value:*mut u32)->u32;
    fn xm1_power_record_helper_f4a314(profile:u32,value:*mut u32)->u32;
    fn xm1_power_record_helper_f4bcc4(profile:u32,value:*mut u32)->u32;
    fn xm1_power_record_helper_f4a330(profile:u32,value:*mut u32)->u32;
    fn xm1_power_record_helper_f4a34c(profile:u32,value:*mut u32)->u32;
}
impl VoteSource {
    unsafe fn query(self,profile:Profile,value:&mut u32)->u32 {
        unsafe {
            match self {
                Self::First=>xm1_power_record_helper_f4a710(profile.0,value),
                Self::Second=>xm1_power_record_helper_f4bd34(profile.0,value),
                Self::Third=>xm1_power_record_helper_f4a314(profile.0,value),
                Self::Force=>xm1_power_record_helper_f4bcc4(profile.0,value),
                Self::Fifth=>xm1_power_record_helper_f4a330(profile.0,value),
                Self::Sixth=>xm1_power_record_helper_f4a34c(profile.0,value),
            }
        }
    }
}
struct Votes { first:u32,first_value:u32,second:u32,second_value:u32,force:u32 }
impl Votes {
    unsafe fn publish(&self) {
        unsafe {
            put(STATE+0x19e8,self.first); put(STATE+0x19ec,self.first_value);
            put(STATE+0x19f0,self.second); put(STATE+0x19f4,self.second_value); put(STATE+0x19f8,self.force);
            put(STATE+0x1a14,self.first); put(STATE+0x1a18,self.first_value);
            put(STATE+0x1a1c,self.second); put(STATE+0x1a20,self.second_value); put(STATE+0x1a24,self.force);
        }
    }
}
fn base_selection(selection:u32)->u32 {
    match selection { 0x13|0x14=>0x12,0x16|0x17=>0x15,0x1a|0x1b=>0x19,0x23|0x24=>0x22,0x26|0x27=>0x25,0x29|0x2a=>0x28,_=>selection }
}

pub(super) unsafe fn select(profile:Profile,kind:Kind,choice:&mut Choice) {
    unsafe {
        let mut initial=Choice::empty();
        let mut promoted=0;
        let mut baseline=Selection(0);
        let mut previous=Selection(0);
        let mut filtered=Selection(0);
        if configuration(ConfigurationKey(0x75e6))==0 { history(kind,choice); }
        else {
            query(profile,kind,&mut initial);
            choice.flags=initial.flags;
            promoted=xm1_power_record_helper_f4a520(profile.0,kind.0);
            baseline=if promoted!=0 { Selection(xm1_power_record_helper_f4bc58(profile.0,kind.0,initial.selection.0)) } else { initial.selection };
            choice.selection=baseline;
            if xm1_power_record_helper_f4a554(profile.0)!=0 && xm1_power_record_helper_f4a57c(baseline.0)==1 && xm1_power_record_helper_f49fd8(word(STATE))!=0 {
                let mut value=0;
                let mut votes=Votes{first:0,first_value:0,second:0,second_value:0,force:0};
                let mut weight=0u8;
                for key in 0x76b3..0x76b9 {
                    if let Some(source)=VoteSource::parse(configuration(ConfigurationKey(key))) {
                        if source.query(profile,&mut value)!=0 {
                            weight=weight.wrapping_add(configuration(ConfigurationKey(key+6)) as u8);
                            match source {
                                VoteSource::First=>{votes.first=1;votes.first_value=value;},
                                VoteSource::Second=>{votes.second=1;votes.second_value=value;},
                                VoteSource::Force=>votes.force=1,
                                _=>(),
                            }
                        }
                    }
                }
                let below_threshold=(weight as u32)<configuration(ConfigurationKey(0x76bf));
                let use_filter=if below_threshold { true }
                else if votes.force!=0 { false }
                else if votes.first!=0 && (votes.first_value==0 || configuration(ConfigurationKey(0x76c0))==0) { false }
                else { votes.second==0 || (votes.second_value!=0 && configuration(ConfigurationKey(0x76c1))!=0) };
                votes.publish();
                if use_filter {
                    previous=Selection(base_selection(word(STATE)));
                    put(STATE+0x98c,previous.0);
                    filtered=Selection(xm1_power_record_helper_f4a848(previous.0,baseline.0,kind.0));
                    choice.selection=Selection(xm1_power_record_helper_f4a154(baseline.0,filtered.0));
                }
            }
        }
        adjust(profile,choice);
        put(STATE+0x1a3c,choice.selection.0); put_half(STATE+0x1a40,choice.flags.0);
        bound_flags(profile,kind,choice);
        put(STATE+0x14,choice.selection.0); put_half(STATE+0x18,choice.flags.0);
        put(STATE+0x1a04,initial.selection.0); put(STATE+0x1a08,promoted); put(STATE+0x1a0c,baseline.0);
        put(STATE+0x1a10,word(STATE+0x988)); put(STATE+0x1a28,previous.0); put(STATE+0x1a2c,word(STATE+0x98c));
        put(STATE+0x1a30,filtered.0); put(STATE+0x1a34,choice.selection.0); put_half(STATE+0x1a38,choice.flags.0);
        if configuration(ConfigurationKey(0x75e0))!=0 {
            xm1_power_record_helper_f4d8bc(choice.selection.0,choice.flags.0 as u32);
            history(kind,choice);
        }
        put(STATE+8,choice.selection.0); put_half(STATE+12,choice.flags.0); put(STATE+0xa8,0);
        put(STATE+0xa4,u32::from(choice.selection.0!=0 || choice.flags.0!=0));
        let bounded=matches!(choice.selection.0,1|4|8|9|14|15|0x13|0x14|0x16|0x17|0x1a|0x1b|0x23|0x24|0x26|0x27|0x29|0x2a);
        put(STATE+0x2ec,u32::from(bounded));
        put(STATE+0xda0,choice.selection.0); put_half(STATE+0xda4,choice.flags.0);
        override_selection(profile,ConfigurationKey(0x4a13),5,choice);
        if word(STATE)!=choice.selection.0 || half(STATE+4)!=choice.flags.0 {
            put(STATE,choice.selection.0); put_half(STATE+4,choice.flags.0);
        }
        copy(STATE+0x1920,STATE+0x14,128);
        put(STATE+0x19a0,word(STATE+0xda0)); put_half(STATE+0x19a4,half(STATE+0xda4));
    }
}
