use crate::backend_adapter::{ConfigurationRule, RulePermission};

const SELECTORS: [u16; 144] = [
    0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,16,17,18,19,20,21,22,23,24,
    32,33,34,35,36,40,48,49,50,51,52,53,54,64,66,67,68,69,70,71,72,73,
    80,81,82,83,84,85,86,96,97,98,99,100,101,102,103,103,104,106,107,108,109,110,
    112,113,114,115,116,122,123,128,129,130,131,132,133,134,135,136,137,138,139,140,141,
    144,145,146,147,160,161,162,163,164,165,166,167,168,169,170,172,176,177,178,
    192,193,194,195,196,197,198,200,201,202,203,204,208,209,210,211,212,213,214,215,
    216,218,219,220,221,222,224,225,226,227,228,232,233,234,235,240,
];
const fn rules() -> [ConfigurationRule; 144] {
    let mut rules = [ConfigurationRule { selector: 0, permission: RulePermission::Reject }; 144];
    let mut index = 0;
    while index < rules.len() {
        let selector = SELECTORS[index];
        let permission = match selector { 19 | 51 | 64 | 100 | 101 | 102 | 103 | 209 | 216 | 218 | 219 | 220 | 221 => RulePermission::Reject, _ => RulePermission::Dispatch };
        rules[index] = ConfigurationRule { selector, permission };
        index += 1;
    }
    rules
}
pub const STOCK_RULES: [ConfigurationRule;144] = rules();
