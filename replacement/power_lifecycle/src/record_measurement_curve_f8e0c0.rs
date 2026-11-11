//! Reading notes for the image-5 body at 0x00f8_e0c0, which is not written.
//!
//! The body is 2340 bytes, 585 instructions, and over the 1500-byte limit.
//! It first requires the words at record offsets 0x04, 0x08, 0x0c, 0x10 and
//! 0x14 to be non-negative and otherwise answers through 0x00f8_deb0. It then
//! stores the 0x00ee_97f0 hundredths logarithm of those five words in the
//! locals at 0x28-0x38, reads the ratio of 0x00f8_cbd0 and the byte at offset
//! 0x00, and selects: the first path when that byte is three or the ratio is
//! nonzero, the second path when the byte is four and the ratio is zero, and
//! zero otherwise. Both paths scale the five record words at 0x18-0x28 through
//! 0x00f8_a390, then accumulate two 64-bit quadratic forms over the local
//! logarithms, one pair of terms per sample for as many samples as the byte at
//! offset 0x00, using umull, mla, strd and ldrd. The first form divides the
//! second through the signed 64-bit division helper, and the quotient is
//! rounded to a multiple of ten and answered in tenths, as 0x00f8_dd3c does.
