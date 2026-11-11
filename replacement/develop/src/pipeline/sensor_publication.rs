use super::shared::{Address32, ByteOffset};
use super::sensor_requests::{word, DescriptorHardwareFlags, PipelineDescriptor};
use super::sensor_registers::{xm1_pipeline_sensor_acquire_hardware, xm1_pipeline_sensor_release_hardware, xm1_pipeline_sensor_apply_register_transfers, xm1_pipeline_sensor_capture_register_transfers, RegisterShadow, RegisterTransferMap, RegisterTransferCount};
use super::sensor_register_fields::*;
use super::sensor_snapshot_fields::xm1_pipeline_sensor_pack_snapshot;

#[derive(Clone, Copy)]
enum RegisterBank { Input, FirstFilter, SecondFilter, Window, Origin, Region, Coefficients, Dimensions, Control, PrimaryTable, SecondaryTable, Snapshot }
impl RegisterBank {
    fn transfer(self)->(RegisterTransferMap,RegisterTransferCount) {
        let (address,count)=match self {
            Self::Input=>(0x821934,12),Self::FirstFilter=>(0x82182c,13),Self::SecondFilter=>(0x8218c8,3),Self::Window=>(0x8210ac,6),Self::Origin=>(0x8219c4,6),Self::Region=>(0x8210f4,31),Self::Coefficients=>(0x821268,11),Self::Dimensions=>(0x820f98,22),Self::Control=>(0x8218ec,6),Self::PrimaryTable=>(0x821a18,28),Self::SecondaryTable=>(0x821b68,5),Self::Snapshot=>(0x8212ec,56),
        };(RegisterTransferMap(Address32(address)),RegisterTransferCount(count))
    }
    fn shadow(self)->RegisterShadow { RegisterShadow(Address32(match self { Self::Input=>0x14c6ce0,Self::FirstFilter|Self::SecondFilter=>0x14c7ac4,Self::Window=>0x14c7bd0,Self::Origin=>0x14c7a04,Self::Region=>0x14c7a1c,Self::Coefficients=>0x14c7a98,Self::Dimensions=>0x14c6acc,Self::Control=>0x14c6a58,Self::PrimaryTable=>0x14c6d10,Self::SecondaryTable=>0x14c6ca8,Self::Snapshot=>0x14c6b24 })) }
    unsafe fn apply(self,input:FieldConfiguration,output:PackedRegisters) {
        unsafe { match self {
            Self::Input=>xm1_pipeline_sensor_pack_input(output,input),Self::FirstFilter=>xm1_pipeline_sensor_pack_first_filter(output,input),Self::SecondFilter=>xm1_pipeline_sensor_pack_filter_small(output,input),Self::Window=>xm1_pipeline_sensor_pack_window(output,input),Self::Origin=>xm1_pipeline_sensor_pack_origin(output,input),Self::Region=>xm1_pipeline_sensor_pack_region(output,input),Self::Coefficients=>xm1_pipeline_sensor_pack_coefficients(output,input),Self::Dimensions=>xm1_pipeline_sensor_pack_dimensions(output,input),Self::Control=>xm1_pipeline_sensor_pack_control(output,input),Self::PrimaryTable=>xm1_pipeline_sensor_apply_primary_table(input,output),Self::SecondaryTable=>xm1_pipeline_sensor_pack_secondary_table(output,input),Self::Snapshot=>xm1_pipeline_sensor_pack_snapshot(output,input),
        } }
    }
    unsafe fn capture(self,output:Address32) { let (map,count)=self.transfer();unsafe { xm1_pipeline_sensor_capture_register_transfers(RegisterShadow(output),map,count); } }
    unsafe fn commit(self,input:Address32) { let (map,count)=self.transfer();unsafe { xm1_pipeline_sensor_apply_register_transfers(RegisterShadow(input),map,count); } }
}

macro_rules! register_routes {
    ($($apply:ident,$commit:ident,$capture:ident,$bank:ident;)+) => { $(
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $apply(input: FieldConfiguration,output: PackedRegisters) { unsafe { RegisterBank::$bank.apply(input,output); } }
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $commit(input: Address32) { unsafe { RegisterBank::$bank.commit(input); } }
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $capture(output: Address32) { unsafe { RegisterBank::$bank.capture(output); } }
    )+ };
}
register_routes! {
    xm1_pipeline_sensor_apply_input_registers,xm1_pipeline_sensor_commit_input_registers,xm1_pipeline_sensor_capture_input_registers,Input;
    xm1_pipeline_sensor_apply_first_filter_registers,xm1_pipeline_sensor_commit_first_filter_registers,xm1_pipeline_sensor_capture_first_filter_registers,FirstFilter;
    xm1_pipeline_sensor_apply_second_filter_registers,xm1_pipeline_sensor_commit_second_filter_registers,xm1_pipeline_sensor_capture_second_filter_registers,SecondFilter;
    xm1_pipeline_sensor_apply_window_registers,xm1_pipeline_sensor_commit_window_registers,xm1_pipeline_sensor_capture_window_registers,Window;
    xm1_pipeline_sensor_apply_origin_registers,xm1_pipeline_sensor_commit_origin_registers,xm1_pipeline_sensor_capture_origin_registers,Origin;
    xm1_pipeline_sensor_apply_region_registers,xm1_pipeline_sensor_commit_region_registers,xm1_pipeline_sensor_capture_region_registers,Region;
    xm1_pipeline_sensor_apply_coefficient_registers,xm1_pipeline_sensor_commit_coefficient_registers,xm1_pipeline_sensor_capture_coefficient_registers,Coefficients;
    xm1_pipeline_sensor_apply_dimension_registers,xm1_pipeline_sensor_commit_dimension_registers,xm1_pipeline_sensor_capture_dimension_registers,Dimensions;
    xm1_pipeline_sensor_apply_control_registers,xm1_pipeline_sensor_commit_control_registers,xm1_pipeline_sensor_capture_control_registers,Control;
    xm1_pipeline_sensor_apply_secondary_table_registers,xm1_pipeline_sensor_commit_secondary_table_registers,xm1_pipeline_sensor_capture_secondary_table_registers,SecondaryTable;
    xm1_pipeline_sensor_apply_snapshot_registers,xm1_pipeline_sensor_commit_snapshot_registers,xm1_pipeline_sensor_capture_snapshot_registers,Snapshot;
}

unsafe extern "C" { fn xm1_pipeline_sensor_clear_primary_table_gate(); }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_commit_primary_table_registers(input: Address32) { unsafe { xm1_pipeline_sensor_clear_primary_table_gate();RegisterBank::PrimaryTable.commit(input); } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_capture_primary_table_registers(output: Address32) { unsafe { RegisterBank::PrimaryTable.capture(output); } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_capture_primary_control_registers(output: Address32) { unsafe { xm1_pipeline_sensor_capture_register_transfers(RegisterShadow(output),RegisterTransferMap(Address32(0x821a0c)),RegisterTransferCount(1)); } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_commit_primary_control_registers(input: Address32) { unsafe { xm1_pipeline_sensor_apply_register_transfers(RegisterShadow(input),RegisterTransferMap(Address32(0x821a0c)),RegisterTransferCount(1)); } }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_publish_configurations(flags: DescriptorHardwareFlags,input: FieldConfiguration,first_filter: FieldConfiguration,second_filter: FieldConfiguration,window: FieldConfiguration,origin: FieldConfiguration,region: FieldConfiguration,coefficients: FieldConfiguration,dimensions: FieldConfiguration,control: FieldConfiguration,primary_table: FieldConfiguration,secondary_table: FieldConfiguration,snapshot: FieldConfiguration) {
    unsafe {
        if flags.0&1!=0 { RegisterBank::Input.apply(input,PackedRegisters(RegisterBank::Input.shadow().0)); }
        if flags.0&2!=0 { RegisterBank::FirstFilter.apply(first_filter,PackedRegisters(RegisterBank::FirstFilter.shadow().0)); }
        if flags.0&4!=0 { RegisterBank::SecondFilter.apply(second_filter,PackedRegisters(RegisterBank::SecondFilter.shadow().0)); }
        if flags.0&8!=0 { RegisterBank::Window.apply(window,PackedRegisters(RegisterBank::Window.shadow().0)); }
        if flags.0&16!=0 { RegisterBank::Origin.apply(origin,PackedRegisters(RegisterBank::Origin.shadow().0)); }
        if flags.0&32!=0 { RegisterBank::Region.apply(region,PackedRegisters(RegisterBank::Region.shadow().0)); }
        if flags.0&64!=0 { RegisterBank::Coefficients.apply(coefficients,PackedRegisters(RegisterBank::Coefficients.shadow().0)); }
        if flags.0&128!=0 { RegisterBank::Dimensions.apply(dimensions,PackedRegisters(RegisterBank::Dimensions.shadow().0)); }
        if flags.0&256!=0 { RegisterBank::Control.apply(control,PackedRegisters(RegisterBank::Control.shadow().0)); }
        if flags.0&512!=0 { RegisterBank::PrimaryTable.apply(primary_table,PackedRegisters(RegisterBank::PrimaryTable.shadow().0)); }
        if flags.0&1024!=0 { RegisterBank::SecondaryTable.apply(secondary_table,PackedRegisters(RegisterBank::SecondaryTable.shadow().0)); }
        if flags.0&4096!=0 { RegisterBank::Snapshot.apply(snapshot,PackedRegisters(RegisterBank::Snapshot.shadow().0)); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_reset_configurations(flags: DescriptorHardwareFlags) {
    unsafe {
        if flags.0&4096!=0 { let users=xm1_pipeline_sensor_acquire_hardware(DescriptorHardwareFlags(4096));RegisterBank::Snapshot.commit(RegisterBank::Snapshot.shadow().0);xm1_pipeline_sensor_release_hardware(DescriptorHardwareFlags(4096),users); }
        if flags.0&0x7ff==0 { return; }
        let users=xm1_pipeline_sensor_acquire_hardware(DescriptorHardwareFlags(2048));
        for (mask,bank) in [(1,RegisterBank::Input),(2,RegisterBank::FirstFilter),(4,RegisterBank::SecondFilter),(8,RegisterBank::Window),(16,RegisterBank::Origin),(32,RegisterBank::Region),(64,RegisterBank::Coefficients),(128,RegisterBank::Dimensions),(256,RegisterBank::Control),(512,RegisterBank::PrimaryTable),(1024,RegisterBank::SecondaryTable)] {
            if flags.0&mask!=0 { if mask==512 { xm1_pipeline_sensor_commit_primary_table_registers(bank.shadow().0); } else { bank.commit(bank.shadow().0); } }
        }
        xm1_pipeline_sensor_release_hardware(DescriptorHardwareFlags(2048),users);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_publish_descriptor(record: PipelineDescriptor) {
    let payload=record.0.offset(ByteOffset(4));let field=|offset|FieldConfiguration(payload.offset(ByteOffset(offset)));
    unsafe { xm1_pipeline_sensor_publish_configurations(DescriptorHardwareFlags(word(record.0,0)),field(0),field(0xa2),field(0x198),field(0x1a2),field(0xfc2),field(0xf20),field(0xf98),field(0xfd4),field(0x2c),field(0x230),field(0x222),field(0x1018)); }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_reset_descriptor(record: PipelineDescriptor) { unsafe { xm1_pipeline_sensor_reset_configurations(DescriptorHardwareFlags(word(record.0,0))); } }
