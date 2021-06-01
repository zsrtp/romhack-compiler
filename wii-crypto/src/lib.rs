#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate core as std;
#[cfg(not(feature = "std"))]
extern crate alloc;
extern crate byteorder;
#[macro_use]
extern crate lazy_static;
extern crate block_modes;
extern crate aes;
extern crate sha1;
extern crate rayon;
extern crate failure;
extern crate const_format;
extern crate futures;
extern crate async_std;
extern crate async_trait;
extern crate async_recursion;

pub mod wii_disc;
pub mod array_stream;
pub mod transform;

pub mod consts {
    // DOL_ALIGNMENT and FST_ALIGNMENT are set to 1024 and 256 to match the
    // original ISO. Due to poor documentation of how, and why, these values
    // should or shouldn't be changed we opted to preserve their values since
    // there was no observed benefit of setting them higher, however lower
    // values were not tested.

    pub const WII_HASH_SIZE: usize = 20;
    pub const WII_KEY_SIZE: usize = 16;
    pub const WII_CKEY_AMNT: usize = 3;
    pub const WII_H3_SIZE: usize = 0x18000;
    pub const WII_SECTOR_HASH_SIZE: usize = 0x400;
    pub const WII_SECTOR_SIZE: usize = 0x8000;
    pub const WII_SECTOR_DATA_SIZE: usize = WII_SECTOR_SIZE - WII_SECTOR_HASH_SIZE;
    pub const WII_SECTOR_IV_OFF: usize = 0x3D0;
}

const COMMON_KEY_: [[u8; consts::WII_KEY_SIZE]; consts::WII_CKEY_AMNT] = [[2, 26, 224, 229, 43, 205, 59, 3, 6, 0, 157, 118, 65, 31, 22, 93], [0x0; consts::WII_KEY_SIZE], [0x0; consts::WII_KEY_SIZE],];
const COMMON_KEY_MASK: [[u8; consts::WII_KEY_SIZE]; consts::WII_CKEY_AMNT] = [[233, 254, 202, 199, 117, 72, 168, 231, 78, 217, 88, 51, 50, 158, 188, 170], [0x0; consts::WII_KEY_SIZE], [0x0; consts::WII_KEY_SIZE],];

lazy_static! {
    pub static ref COMMON_KEY: [[u8; consts::WII_KEY_SIZE]; consts::WII_CKEY_AMNT] = {
        let mut ck = [[0 as u8; consts::WII_KEY_SIZE]; consts::WII_CKEY_AMNT];
        for j in 0..consts::WII_CKEY_AMNT {
            for i in 0..consts::WII_KEY_SIZE {
                ck[j][i] = COMMON_KEY_[j][i] ^ COMMON_KEY_MASK[j][i];
            }
        }
        ck
    };
}
