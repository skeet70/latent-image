use core::ptr::read_volatile;

const CONFIGURATION_POINTER: usize = 0x0096_ca50;
const RUNTIME_CONFIGURATION: usize = 0x0096_b400;
pub const CONFIGURATION_BYTES: usize = 0x1641e;
pub const RUNTIME_CONFIGURATION_BYTES: usize = 0x16b;

#[derive(Clone, Copy)]
struct ConfigurationOffset(usize);

struct Configuration<'a>(&'a [u8; CONFIGURATION_BYTES]);
impl Configuration<'_> {
    fn byte(&self, offset: ConfigurationOffset) -> u32 { u32::from(self.0[offset.0]) }
}

#[repr(transparent)]
pub struct RuntimeConfiguration(pub [u8; RUNTIME_CONFIGURATION_BYTES]);
impl RuntimeConfiguration {
    fn byte(&mut self, offset: usize, value: u32) { self.0[offset] = value as u8; }
    fn half(&mut self, offset: usize, value: u32) {
        self.0[offset..offset + 2].copy_from_slice(&(value as u16).to_le_bytes());
    }
}

fn serial_number(configuration: &Configuration<'_>) -> [u8; 16] {
    let mut packed = [b'0'; 8];
    for index in 3..8 {
        let source = 0x11d + (index - 3) * 2;
        let high = configuration.byte(ConfigurationOffset(source)) as u8;
        if !high.is_ascii_digit() { packed[index] = 0; break; }
        packed[index] = high << 4;
        let low = configuration.byte(ConfigurationOffset(source + 1)) as u8;
        if !low.is_ascii_digit() { packed[index] = 0; break; }
        packed[index] |= low & 15;
    }
    if let Some(first) = packed.iter().position(|value| !value.is_ascii_digit()) {
        for _ in first + 1..8 {
            packed.copy_within(0..7, 1);
            packed[0] = b'0';
        }
    }
    let mut output = [0; 16];
    for (index, value) in packed.into_iter().enumerate() {
        output[index * 2] = (value >> 4) + b'0';
        output[index * 2 + 1] = (value & 15) + b'0';
    }
    output
}

pub fn project_runtime_configuration(configuration: &[u8; CONFIGURATION_BYTES], output: &mut RuntimeConfiguration) {
    let input = Configuration(configuration);
    output.byte(0, input.byte(ConfigurationOffset(0x204)));
    output.byte(1, input.byte(ConfigurationOffset(0x205)));
    output.0[2..18].copy_from_slice(&serial_number(&input));
    output.half(0x12, 0u32);
    let configuration_21a_1 = input.byte(ConfigurationOffset(0x21a));
    output.half(0x14, (((configuration_21a_1 + (configuration_21a_1 << 2)) + ((configuration_21a_1 + (configuration_21a_1 << 2)) << 2)) << 2));
    let configuration_21b_1 = input.byte(ConfigurationOffset(0x21b));
    output.half(0x16, (((configuration_21b_1 + (configuration_21b_1 << 2)) + ((configuration_21b_1 + (configuration_21b_1 << 2)) << 2)) << 2));
    let configuration_103 = input.byte(ConfigurationOffset(0x103));
    let configuration_102 = input.byte(ConfigurationOffset(0x102));
    output.half(0x18, (configuration_102 | (configuration_103 << 8)));
    output.half(0x1a, 0u32);
    output.byte(0x1c, input.byte(ConfigurationOffset(0xd4)));
    output.byte(0x1d, input.byte(ConfigurationOffset(0xd5)));
    output.byte(0x1e, 0u32);
    output.byte(0x1f, input.byte(ConfigurationOffset(0x387)));
    output.byte(0x20, input.byte(ConfigurationOffset(0x3ab)));
    output.byte(0x21, input.byte(ConfigurationOffset(0x3a4)));
    output.byte(0x22, input.byte(ConfigurationOffset(0x26a)));
    output.byte(0x23, input.byte(ConfigurationOffset(0x26b)));
    output.byte(0x24, input.byte(ConfigurationOffset(0x26c)));
    output.byte(0x25, input.byte(ConfigurationOffset(0x26d)));
    output.byte(0x26, 0u32);
    output.byte(0x27, input.byte(ConfigurationOffset(0x217)));
    output.byte(0x28, input.byte(ConfigurationOffset(0x218)));
    output.byte(0x29, input.byte(ConfigurationOffset(0x22e)));
    output.byte(0x2a, input.byte(ConfigurationOffset(0x22f)));
    output.byte(0x2b, 0u32);
    output.byte(0x2c, 0u32);
    output.byte(0x2d, input.byte(ConfigurationOffset(0x254)));
    output.byte(0x2e, input.byte(ConfigurationOffset(0x20c)));
    output.byte(0x2f, input.byte(ConfigurationOffset(0x20d)));
    output.byte(0x30, input.byte(ConfigurationOffset(0x20e)));
    output.byte(0x31, input.byte(ConfigurationOffset(0x20f)));
    output.byte(0x32, input.byte(ConfigurationOffset(0x210)));
    let configuration_214_1 = input.byte(ConfigurationOffset(0x214));
    output.byte(0x33, configuration_214_1);
    if configuration_214_1 > 50 { output.byte(0x33, 10); }
    output.byte(0x34, input.byte(ConfigurationOffset(0x22b)));
    output.byte(0x35, input.byte(ConfigurationOffset(0x20b)));
    output.byte(0x36, input.byte(ConfigurationOffset(0xcf)));
    output.byte(0x37, input.byte(ConfigurationOffset(0x220)));
    output.byte(0x38, input.byte(ConfigurationOffset(0x221)));
    output.byte(0x39, input.byte(ConfigurationOffset(0x288)));
    output.byte(0x3a, input.byte(ConfigurationOffset(0x289)));
    output.byte(0x3b, input.byte(ConfigurationOffset(0xc5)));
    output.byte(0x3c, 0u32);
    output.byte(0x3d, 0u32);
    output.byte(0x3e, 0u32);
    output.byte(0x3f, 0u32);
    output.byte(0x40, 0u32);
    output.byte(0x41, 0u32);
    output.byte(0x42, 0u32);
    output.byte(0x43, 0u32);
    output.byte(0x44, 0u32);
    output.byte(0x45, 0u32);
    output.byte(0x46, 0u32);
    output.byte(0x47, input.byte(ConfigurationOffset(0x285)));
    output.byte(0x48, input.byte(ConfigurationOffset(0x286)));
    output.byte(0x49, input.byte(ConfigurationOffset(0x920c)));
    output.byte(0x4a, input.byte(ConfigurationOffset(0x920d)));
    output.byte(0x4b, input.byte(ConfigurationOffset(0xa001)));
    output.byte(0x4c, input.byte(ConfigurationOffset(0xa002)));
    output.byte(0x4d, input.byte(ConfigurationOffset(0xa003)));
    output.byte(0x4e, input.byte(ConfigurationOffset(0xa004)));
    output.byte(0x4f, input.byte(ConfigurationOffset(0xa005)));
    output.byte(0x50, input.byte(ConfigurationOffset(0xa006)));
    output.byte(0x51, input.byte(ConfigurationOffset(0xa007)));
    output.byte(0x52, input.byte(ConfigurationOffset(0xa008)));
    output.byte(0x53, input.byte(ConfigurationOffset(0xa009)));
    output.byte(0x54, input.byte(ConfigurationOffset(0xa00a)));
    output.byte(0x55, input.byte(ConfigurationOffset(0xa00b)));
    output.byte(0x56, input.byte(ConfigurationOffset(0xa00c)));
    output.byte(0x57, input.byte(ConfigurationOffset(0xa00d)));
    output.byte(0x58, input.byte(ConfigurationOffset(0xa00e)));
    output.byte(0x59, input.byte(ConfigurationOffset(0xa00f)));
    output.byte(0x5a, input.byte(ConfigurationOffset(0xa010)));
    output.byte(0x5b, input.byte(ConfigurationOffset(0xa011)));
    output.byte(0x5c, input.byte(ConfigurationOffset(0xa012)));
    output.byte(0x5d, input.byte(ConfigurationOffset(0xa018)));
    output.byte(0x5e, input.byte(ConfigurationOffset(0xa014)));
    output.byte(0x5f, input.byte(ConfigurationOffset(0xa017)));
    output.byte(0x60, input.byte(ConfigurationOffset(0xa000)));
    output.byte(0x61, input.byte(ConfigurationOffset(0x388)));
    output.byte(0x62, input.byte(ConfigurationOffset(0x389)));
    output.byte(0x63, input.byte(ConfigurationOffset(0xa444)));
    output.byte(0x64, input.byte(ConfigurationOffset(0xa440)));
    output.byte(0x65, input.byte(ConfigurationOffset(0x264)));
    output.byte(0x66, 0u32);
    output.byte(0x67, input.byte(ConfigurationOffset(0xa451)));
    output.byte(0x68, input.byte(ConfigurationOffset(0xa452)));
    output.byte(0x69, input.byte(ConfigurationOffset(0x28a)));
    output.byte(0x6a, input.byte(ConfigurationOffset(0x28b)));
    output.byte(0x6b, input.byte(ConfigurationOffset(0x28c)));
    output.byte(0x6c, input.byte(ConfigurationOffset(0x222)));
    output.byte(0x6d, input.byte(ConfigurationOffset(0x223)));
    output.byte(0x6e, input.byte(ConfigurationOffset(0x224)));
    output.byte(0x6f, input.byte(ConfigurationOffset(0x225)));
    output.byte(0x70, input.byte(ConfigurationOffset(0x226)));
    output.byte(0x71, input.byte(ConfigurationOffset(0x227)));
    output.byte(0x72, input.byte(ConfigurationOffset(0x228)));
    output.byte(0x73, input.byte(ConfigurationOffset(0x229)));
    output.byte(0x74, input.byte(ConfigurationOffset(0xa44f)));
    output.byte(0x75, input.byte(ConfigurationOffset(0xa450)));
    output.byte(0x76, input.byte(ConfigurationOffset(0xdb)));
    output.byte(0x77, input.byte(ConfigurationOffset(0x265)));
    output.byte(0x78, 0u32);
    output.byte(0x79, input.byte(ConfigurationOffset(0x281)));
    output.byte(0x7a, input.byte(ConfigurationOffset(0x282)));
    output.byte(0x7b, input.byte(ConfigurationOffset(0x283)));
    output.byte(0x7c, input.byte(ConfigurationOffset(0x284)));
    output.byte(0x7d, input.byte(ConfigurationOffset(0x1211)));
    output.byte(0x7e, input.byte(ConfigurationOffset(0x1212)));
    output.byte(0x7f, input.byte(ConfigurationOffset(0x1213)));
    output.byte(0x80, input.byte(ConfigurationOffset(0xa902)));
    output.byte(0x81, input.byte(ConfigurationOffset(0x208)));
    output.byte(0x82, input.byte(ConfigurationOffset(0x209)));
    output.byte(0x83, input.byte(ConfigurationOffset(0x20a)));
    output.byte(0x84, input.byte(ConfigurationOffset(0x219)));
    let configuration_9f68_1 = input.byte(ConfigurationOffset(0x9f68));
    let configuration_9f67_1 = input.byte(ConfigurationOffset(0x9f67));
    output.half(0xe0, (configuration_9f67_1 | (configuration_9f68_1 << 8)));
    let configuration_9f6a_1 = input.byte(ConfigurationOffset(0x9f6a));
    let configuration_9f69_1 = input.byte(ConfigurationOffset(0x9f69));
    output.half(0xe2, (configuration_9f69_1 | (configuration_9f6a_1 << 8)));
    output.byte(0xe4, input.byte(ConfigurationOffset(0xa115)));
    output.byte(0xe5, input.byte(ConfigurationOffset(0x9f71)));
    output.byte(0xe6, input.byte(ConfigurationOffset(0x9f82)));
    output.byte(0xe7, input.byte(ConfigurationOffset(0x9f83)));
    output.byte(0xe8, input.byte(ConfigurationOffset(0x2d7)));
    output.byte(0xe9, input.byte(ConfigurationOffset(0x2b83)));
    output.byte(0xea, 0u32);
    output.byte(0xeb, input.byte(ConfigurationOffset(0x215)));
    output.byte(0xec, input.byte(ConfigurationOffset(0x9f72)));
    output.byte(0xed, input.byte(ConfigurationOffset(0x9f73)));
    output.byte(0xee, input.byte(ConfigurationOffset(0x9fe6)));
    output.byte(0xef, input.byte(ConfigurationOffset(0x9fe7)));
    output.byte(0xf0, input.byte(ConfigurationOffset(0x2e8)));
    output.byte(0xf6, input.byte(ConfigurationOffset(0x2e9)));
    output.half(0xf2, input.byte(ConfigurationOffset(0x3b1)) * 56 / 10);
    output.half(0xf4, input.byte(ConfigurationOffset(0x3b0)) * 56 / 10);
    output.byte(0x85, input.byte(ConfigurationOffset(0x9937)));
    let configuration_993a_1 = input.byte(ConfigurationOffset(0x993a));
    output.half(0x86, ((configuration_993a_1 << 8) & 0xffff));
    let configuration_9939_1 = input.byte(ConfigurationOffset(0x9939));
    output.half(0x86, (((configuration_993a_1 << 8) & 0xffff) | configuration_9939_1));
    let configuration_9944_1 = input.byte(ConfigurationOffset(0x9944));
    output.half(0x88, ((configuration_9944_1 << 8) & 0xffff));
    let configuration_9943_1 = input.byte(ConfigurationOffset(0x9943));
    output.half(0x88, (((configuration_9944_1 << 8) & 0xffff) | configuration_9943_1));
    let configuration_9948_1 = input.byte(ConfigurationOffset(0x9948));
    output.half(0x8a, ((configuration_9948_1 << 8) & 0xffff));
    let configuration_9947_1 = input.byte(ConfigurationOffset(0x9947));
    output.half(0x8a, (((configuration_9948_1 << 8) & 0xffff) | configuration_9947_1));
    let configuration_994c_1 = input.byte(ConfigurationOffset(0x994c));
    output.half(0x8c, ((configuration_994c_1 << 8) & 0xffff));
    let configuration_994b_1 = input.byte(ConfigurationOffset(0x994b));
    output.half(0x8c, (((configuration_994c_1 << 8) & 0xffff) | configuration_994b_1));
    let configuration_9950_1 = input.byte(ConfigurationOffset(0x9950));
    output.half(0x8e, ((configuration_9950_1 << 8) & 0xffff));
    let configuration_994f_1 = input.byte(ConfigurationOffset(0x994f));
    output.half(0x8e, (((configuration_9950_1 << 8) & 0xffff) | configuration_994f_1));
    let configuration_9954_1 = input.byte(ConfigurationOffset(0x9954));
    output.half(0x90, ((configuration_9954_1 << 8) & 0xffff));
    let configuration_9953_1 = input.byte(ConfigurationOffset(0x9953));
    output.half(0x90, (((configuration_9954_1 << 8) & 0xffff) | configuration_9953_1));
    let configuration_9946_1 = input.byte(ConfigurationOffset(0x9946));
    output.half(0x92, ((configuration_9946_1 << 8) & 0xffff));
    let configuration_9945_1 = input.byte(ConfigurationOffset(0x9945));
    output.half(0x92, (((configuration_9946_1 << 8) & 0xffff) | configuration_9945_1));
    let configuration_994a_1 = input.byte(ConfigurationOffset(0x994a));
    output.half(0x94, ((configuration_994a_1 << 8) & 0xffff));
    let configuration_9949_1 = input.byte(ConfigurationOffset(0x9949));
    output.half(0x94, (((configuration_994a_1 << 8) & 0xffff) | configuration_9949_1));
    let configuration_994e_1 = input.byte(ConfigurationOffset(0x994e));
    output.half(0x96, ((configuration_994e_1 << 8) & 0xffff));
    let configuration_994d_1 = input.byte(ConfigurationOffset(0x994d));
    output.half(0x96, (((configuration_994e_1 << 8) & 0xffff) | configuration_994d_1));
    let configuration_9952_1 = input.byte(ConfigurationOffset(0x9952));
    output.half(0x98, ((configuration_9952_1 << 8) & 0xffff));
    let configuration_9951_1 = input.byte(ConfigurationOffset(0x9951));
    output.half(0x98, (((configuration_9952_1 << 8) & 0xffff) | configuration_9951_1));
    let configuration_9956_1 = input.byte(ConfigurationOffset(0x9956));
    output.half(0x9a, ((configuration_9956_1 << 8) & 0xffff));
    let configuration_9955_1 = input.byte(ConfigurationOffset(0x9955));
    output.half(0x9a, (((configuration_9956_1 << 8) & 0xffff) | configuration_9955_1));
    let configuration_996c_1 = input.byte(ConfigurationOffset(0x996c));
    output.half(0x9c, ((configuration_996c_1 << 8) & 0xffff));
    let configuration_996b_1 = input.byte(ConfigurationOffset(0x996b));
    output.half(0x9c, (((configuration_996c_1 << 8) & 0xffff) | configuration_996b_1));
    let configuration_9970_1 = input.byte(ConfigurationOffset(0x9970));
    output.half(0x9c, ((configuration_9970_1 << 8) & 0xffff));
    let configuration_996f_1 = input.byte(ConfigurationOffset(0x996f));
    output.half(0x9c, (((configuration_9970_1 << 8) & 0xffff) | configuration_996f_1));
    let configuration_9974_1 = input.byte(ConfigurationOffset(0x9974));
    output.half(0x9c, ((configuration_9974_1 << 8) & 0xffff));
    let configuration_9973_1 = input.byte(ConfigurationOffset(0x9973));
    output.half(0x9c, (((configuration_9974_1 << 8) & 0xffff) | configuration_9973_1));
    let configuration_9978_1 = input.byte(ConfigurationOffset(0x9978));
    output.half(0x9c, ((configuration_9978_1 << 8) & 0xffff));
    let configuration_9977_1 = input.byte(ConfigurationOffset(0x9977));
    output.half(0x9c, (((configuration_9978_1 << 8) & 0xffff) | configuration_9977_1));
    let configuration_997c_1 = input.byte(ConfigurationOffset(0x997c));
    output.half(0x9c, ((configuration_997c_1 << 8) & 0xffff));
    let configuration_997b_1 = input.byte(ConfigurationOffset(0x997b));
    output.half(0x9c, (((configuration_997c_1 << 8) & 0xffff) | configuration_997b_1));
    let configuration_996e_1 = input.byte(ConfigurationOffset(0x996e));
    output.half(0xa6, ((configuration_996e_1 << 8) & 0xffff));
    let configuration_996d_1 = input.byte(ConfigurationOffset(0x996d));
    output.half(0xa6, (((configuration_996e_1 << 8) & 0xffff) | configuration_996d_1));
    let configuration_9972_1 = input.byte(ConfigurationOffset(0x9972));
    output.half(0xa6, ((configuration_9972_1 << 8) & 0xffff));
    let configuration_9971_1 = input.byte(ConfigurationOffset(0x9971));
    output.half(0xa6, (((configuration_9972_1 << 8) & 0xffff) | configuration_9971_1));
    let configuration_9976_1 = input.byte(ConfigurationOffset(0x9976));
    output.half(0xa6, ((configuration_9976_1 << 8) & 0xffff));
    let configuration_9975_1 = input.byte(ConfigurationOffset(0x9975));
    output.half(0xa6, (((configuration_9976_1 << 8) & 0xffff) | configuration_9975_1));
    let configuration_997a_1 = input.byte(ConfigurationOffset(0x997a));
    output.half(0xa6, ((configuration_997a_1 << 8) & 0xffff));
    let configuration_9979_1 = input.byte(ConfigurationOffset(0x9979));
    output.half(0xa6, (((configuration_997a_1 << 8) & 0xffff) | configuration_9979_1));
    let configuration_997e_1 = input.byte(ConfigurationOffset(0x997e));
    output.half(0xa6, ((configuration_997e_1 << 8) & 0xffff));
    let configuration_997d_1 = input.byte(ConfigurationOffset(0x997d));
    output.half(0xa6, (((configuration_997e_1 << 8) & 0xffff) | configuration_997d_1));
    let configuration_9995_1 = input.byte(ConfigurationOffset(0x9995));
    output.half(0xb0, ((configuration_9995_1 << 8) & 0xffff));
    let configuration_9994_1 = input.byte(ConfigurationOffset(0x9994));
    output.half(0xb0, (((configuration_9995_1 << 8) & 0xffff) | configuration_9994_1));
    let configuration_999b_1 = input.byte(ConfigurationOffset(0x999b));
    output.half(0xb2, ((configuration_999b_1 << 8) & 0xffff));
    let configuration_999a_1 = input.byte(ConfigurationOffset(0x999a));
    output.half(0xb2, (((configuration_999b_1 << 8) & 0xffff) | configuration_999a_1));
    let configuration_9997_1 = input.byte(ConfigurationOffset(0x9997));
    output.half(0xb4, ((configuration_9997_1 << 8) & 0xffff));
    let configuration_9996_1 = input.byte(ConfigurationOffset(0x9996));
    output.half(0xb4, (((configuration_9997_1 << 8) & 0xffff) | configuration_9996_1));
    let configuration_999d_1 = input.byte(ConfigurationOffset(0x999d));
    output.half(0xb6, ((configuration_999d_1 << 8) & 0xffff));
    let configuration_999c_1 = input.byte(ConfigurationOffset(0x999c));
    output.half(0xb6, (((configuration_999d_1 << 8) & 0xffff) | configuration_999c_1));
    let configuration_9999_1 = input.byte(ConfigurationOffset(0x9999));
    output.half(0xb8, ((configuration_9999_1 << 8) & 0xffff));
    let configuration_9998_1 = input.byte(ConfigurationOffset(0x9998));
    output.half(0xb8, (((configuration_9999_1 << 8) & 0xffff) | configuration_9998_1));
    let configuration_999f_1 = input.byte(ConfigurationOffset(0x999f));
    output.half(0xba, ((configuration_999f_1 << 8) & 0xffff));
    let configuration_999e_1 = input.byte(ConfigurationOffset(0x999e));
    output.half(0xba, (((configuration_999f_1 << 8) & 0xffff) | configuration_999e_1));
    let configuration_993e_1 = input.byte(ConfigurationOffset(0x993e));
    output.half(0xbc, ((configuration_993e_1 << 8) & 0xffff));
    let configuration_993d_1 = input.byte(ConfigurationOffset(0x993d));
    output.half(0xbc, (((configuration_993e_1 << 8) & 0xffff) | configuration_993d_1));
    output.byte(0xbe, 0u32);
    output.byte(0xbf, 0u32);
    output.byte(0xc0, 0u32);
    output.byte(0xc1, 0u32);
    output.byte(0xc2, 0u32);
    output.byte(0xc3, 0u32);
    output.byte(0xc4, 0u32);
    output.byte(0xc5, 0u32);
    output.byte(0xc6, 0u32);
    output.byte(0xc7, 0u32);
    output.byte(0xc8, 0u32);
    output.byte(0xc9, 0u32);
    output.byte(0xca, 0u32);
    output.byte(0xcb, 0u32);
    output.byte(0xcc, 0u32);
    output.byte(0xcd, 0u32);
    output.byte(0xce, 0u32);
    output.byte(0xcf, 0u32);
    output.byte(0xd0, 0u32);
    output.byte(0xd1, 0u32);
    output.byte(0xd2, 0u32);
    output.byte(0xd3, 0u32);
    output.byte(0xd4, 0u32);
    output.byte(0xd5, 0u32);
    output.byte(0xd6, 0u32);
    output.byte(0xd7, 0u32);
    output.byte(0xd8, 0u32);
    output.byte(0xd9, 0u32);
    output.byte(0xda, 0u32);
    output.byte(0xdb, 0u32);
    output.byte(0xdc, 0u32);
    output.byte(0xdd, 0u32);
    output.byte(0xde, 0u32);
    output.byte(0xdf, 0u32);
    let configuration_9f68_2 = input.byte(ConfigurationOffset(0x9f68));
    let configuration_9f67_2 = input.byte(ConfigurationOffset(0x9f67));
    output.half(0xe0, (configuration_9f67_2 | (configuration_9f68_2 << 8)));
    let configuration_9f6a_2 = input.byte(ConfigurationOffset(0x9f6a));
    let configuration_9f69_2 = input.byte(ConfigurationOffset(0x9f69));
    output.half(0xe2, (configuration_9f69_2 | (configuration_9f6a_2 << 8)));
    output.byte(0xe4, input.byte(ConfigurationOffset(0xa115)));
    output.byte(0xe5, input.byte(ConfigurationOffset(0x9f71)));
    output.byte(0xe6, input.byte(ConfigurationOffset(0x9f82)));
    output.byte(0xe7, input.byte(ConfigurationOffset(0x9f83)));
    output.byte(0xe9, input.byte(ConfigurationOffset(0x2b83)));
    output.byte(0x16a, input.byte(ConfigurationOffset(0x4065)));
    output.byte(0xf7, input.byte(ConfigurationOffset(0x3d2)));
    output.byte(0xf8, input.byte(ConfigurationOffset(0x3d3)));
    output.byte(0xf9, input.byte(ConfigurationOffset(0x3d4)));
    output.byte(0xfa, input.byte(ConfigurationOffset(0x3d5)));
    let configuration_255_1 = input.byte(ConfigurationOffset(0x255));
    output.half(0xfc, (configuration_255_1 + (configuration_255_1 << 2)));
    let configuration_256_1 = input.byte(ConfigurationOffset(0x256));
    output.half(0xfe, (configuration_256_1 + (configuration_256_1 << 2)));
    let configuration_257_1 = input.byte(ConfigurationOffset(0x257));
    output.half(0x100, (configuration_257_1 + (configuration_257_1 << 2)));
    let configuration_1640f_1 = input.byte(ConfigurationOffset(0x1640f));
    let configuration_1640e_1 = input.byte(ConfigurationOffset(0x1640e));
    output.half(0x102, (configuration_1640e_1 | (configuration_1640f_1 << 8)));
    let configuration_1640f_2 = input.byte(ConfigurationOffset(0x1640f));
    let configuration_1640e_2 = input.byte(ConfigurationOffset(0x1640e));
    output.half(0x104, (configuration_1640e_2 | (configuration_1640f_2 << 8)));
    let configuration_16411_1 = input.byte(ConfigurationOffset(0x16411));
    let configuration_16410_1 = input.byte(ConfigurationOffset(0x16410));
    output.half(0x106, (configuration_16410_1 | (configuration_16411_1 << 8)));
    let configuration_1641d_1 = input.byte(ConfigurationOffset(0x1641d));
    let configuration_1641c_1 = input.byte(ConfigurationOffset(0x1641c));
    output.half(0x112, (configuration_1641c_1 | (configuration_1641d_1 << 8)));
    let configuration_16413_1 = input.byte(ConfigurationOffset(0x16413));
    let configuration_16412_1 = input.byte(ConfigurationOffset(0x16412));
    output.half(0x108, (configuration_16412_1 | (configuration_16413_1 << 8)));
    let configuration_16415_1 = input.byte(ConfigurationOffset(0x16415));
    let configuration_16414_1 = input.byte(ConfigurationOffset(0x16414));
    output.half(0x10a, (configuration_16414_1 | (configuration_16415_1 << 8)));
    let configuration_16417_1 = input.byte(ConfigurationOffset(0x16417));
    let configuration_16416_1 = input.byte(ConfigurationOffset(0x16416));
    output.half(0x10c, (configuration_16416_1 | (configuration_16417_1 << 8)));
    let configuration_16419_1 = input.byte(ConfigurationOffset(0x16419));
    let configuration_16418_1 = input.byte(ConfigurationOffset(0x16418));
    output.half(0x10e, (configuration_16418_1 | (configuration_16419_1 << 8)));
    let configuration_1641b_1 = input.byte(ConfigurationOffset(0x1641b));
    let configuration_1641a_1 = input.byte(ConfigurationOffset(0x1641a));
    output.half(0x110, (configuration_1641a_1 | (configuration_1641b_1 << 8)));
    output.byte(0x114, input.byte(ConfigurationOffset(0x9ac3)));
    output.byte(0x115, input.byte(ConfigurationOffset(0x9ac4)));
    output.byte(0x116, input.byte(ConfigurationOffset(0x9ac5)));
    for index in 0..32 {
        output.byte(0x117 + index, input.byte(ConfigurationOffset(0x9742 + index)));
    }
    output.byte(0x137, input.byte(ConfigurationOffset(0x9762)));
    output.byte(0x138, input.byte(ConfigurationOffset(0x9763)));
    output.byte(0x139, input.byte(ConfigurationOffset(0x976a)));
    output.byte(0x13a, input.byte(ConfigurationOffset(0x976b)));
    output.byte(0x13b, input.byte(ConfigurationOffset(0x976c)));
    for index in 0..32 {
        output.byte(0x13c + index, input.byte(ConfigurationOffset(0x9773 + index)));
    }
    output.byte(0x15c, input.byte(ConfigurationOffset(0x9793)));
    output.byte(0x15d, input.byte(ConfigurationOffset(0x9794)));
    output.byte(0x15e, input.byte(ConfigurationOffset(0x9796)));
    output.byte(0x15f, input.byte(ConfigurationOffset(0x9797)));
    output.byte(0x160, input.byte(ConfigurationOffset(0x97b9)));
    output.byte(0x161, input.byte(ConfigurationOffset(0x97ba)));
    output.byte(0x162, input.byte(ConfigurationOffset(0x97bb)));
    output.byte(0x163, input.byte(ConfigurationOffset(0x97bc)));
    output.byte(0x164, input.byte(ConfigurationOffset(0x97bd)));
    output.byte(0x165, input.byte(ConfigurationOffset(0x97be)));
    output.byte(0x166, 0u32);
    let configuration_213_1 = input.byte(ConfigurationOffset(0x213));
    let configuration_212_1 = input.byte(ConfigurationOffset(0x212));
    output.half(0x168, (configuration_212_1 | (configuration_213_1 << 8)));
}

/// # Safety
/// The configuration pointer cell, its CONFIGURATION_BYTES-byte backing, and the runtime output must be mapped and exclusively accessible.
pub unsafe fn initialize_runtime_configuration() -> i32 {
    let configuration = unsafe { read_volatile(CONFIGURATION_POINTER as *const u32) } as *const [u8; CONFIGURATION_BYTES];
    let output = RUNTIME_CONFIGURATION as *mut RuntimeConfiguration;
    unsafe { project_runtime_configuration(&*configuration, &mut *output); }
    0
}

#[cfg(test)]
#[path = "mode_initialization_projector_tests.rs"]
mod tests;
