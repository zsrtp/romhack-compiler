use crate::COMMON_KEY;
use crate::consts;
use aes::Aes128;
use block_modes::{BlockMode, Cbc};
use block_modes::block_padding::NoPadding;
use std::mem;
use std::convert::TryFrom;
use byteorder::{BE, ByteOrder};
use rayon::prelude::*;
use failure::Fail;
// use std::fs::File;
// use std::io::prelude::*;

// create an alias for convenience
type Aes128Cbc = Cbc<Aes128, NoPadding>;

trait Unpackable {
    const BLOCK_SIZE: usize;
}

#[derive(Debug, Fail)]
pub enum WiiCryptoError {
    #[fail(display = "Invalid Wii disc: magic is {:#010X}", magic)]
    NotWiiDisc {
        magic: u32,
    },
    #[fail(display = "Could not decrypt block")]
    AesDecryptError,
    #[fail(display = "Could not encrypt block")]
    AesEncryptError,
    #[fail(display = "The provided slice is too small to be converted into a {}.", name)]
    ConvertError {
        name: String,
    },
}

#[macro_export]
macro_rules! declare_tryfrom {
    ( $T:ty ) => {
        impl TryFrom<&[u8]> for $T {
            type Error = WiiCryptoError;

            fn try_from(slice: &[u8]) -> Result<$T, Self::Error> {
                if slice.len() < <$T>::BLOCK_SIZE {
                    Err(WiiCryptoError::ConvertError{name: stringify![$T].to_string()})
                } else {
                    let mut buf = [0 as u8; <$T>::BLOCK_SIZE];
                    buf.clone_from_slice(&slice[..<$T>::BLOCK_SIZE]);
                    Ok(<$T>::from(&buf))
                }
            }
        }
    };
}

#[derive(Copy, Clone, Debug)]
pub struct DiscHeader {
    pub disc_id: char,
    pub game_code: [char; 2],
    pub region_code: char,
    pub maker_code: [char; 2],
    pub disc_number: u8,
    pub disc_version: u8,
    pub audio_streaming: bool,
    pub streaming_buffer_size: u8,
    pub unk1: [u8; 14],
    pub wii_magic: u32,
    pub gc_magic: u32,
    pub game_title: [char; 64],
    pub disable_hash_verif: u8,
    pub disable_disc_encrypt: u8,
}

impl Unpackable for DiscHeader {
    const BLOCK_SIZE: usize = 0x62;
}

pub fn disc_get_header(raw: &[u8]) -> DiscHeader {
    let mut unk1 = [0 as u8; 14];
    let mut game_title = [0 as char; 64];
    unsafe {
        unk1.copy_from_slice(&raw[0xA..0x18]);
        game_title.copy_from_slice(mem::transmute(&raw[0x20..0x60]));
    };
    DiscHeader {
        disc_id: raw[0] as char,
        game_code: [raw[1] as char, raw[2] as char],
        region_code: raw[3] as char,
        maker_code: [raw[4] as char, raw[5] as char],
        disc_number: raw[6],
        disc_version: raw[7],
        audio_streaming: raw[8] != 0,
        streaming_buffer_size: raw[9],
        unk1: unk1,
        wii_magic: BE::read_u32(&raw[0x18..0x1C]),
        gc_magic: BE::read_u32(&raw[0x1C..0x20]),
        game_title: game_title,
        disable_hash_verif: raw[0x60],
        disable_disc_encrypt: raw[0x61],
    }
}

pub fn disc_set_header(buffer: &mut [u8], dh: &DiscHeader) {
    unsafe {
        buffer[0x00] = dh.disc_id as u8;
        buffer[0x01] = dh.game_code[0] as u8;
        buffer[0x02] = dh.game_code[1] as u8;
        buffer[0x03] = dh.region_code as u8;
        buffer[0x04] = dh.maker_code[0] as u8;
        buffer[0x05] = dh.maker_code[1] as u8;
        buffer[0x06] = dh.disc_number;
        buffer[0x07] = dh.disc_version;
        buffer[0x08] = dh.audio_streaming as u8;
        buffer[0x09] = dh.streaming_buffer_size;
        (&mut buffer[0x0A..0x18]).copy_from_slice(&dh.unk1[..]);
        BE::write_u32(&mut buffer[0x18..], dh.wii_magic);
        BE::write_u32(&mut buffer[0x1C..], dh.gc_magic);
        (&mut buffer[0x20..0x60]).copy_from_slice(mem::transmute(&dh.game_title[..]));
        buffer[0x60] = dh.disable_hash_verif;
        buffer[0x61] = dh.disable_disc_encrypt;
    };
}

#[derive(Copy, Clone, Debug)]
pub struct PartInfoEntry {
    pub part_type: u32,
    pub offset: usize,
}

#[derive(Clone, Debug)]
pub struct PartInfo {
    pub offset: usize,
    pub entries: Vec<PartInfoEntry>,
}

pub fn disc_get_part_info(buf: &[u8]) -> PartInfo {
    let mut entries: Vec<PartInfoEntry> = Vec::new();
    let n_part = BE::read_u32(&buf[0x40000..]) as usize;
    let part_info_offset = (BE::read_u32(&buf[0x40004..]) as usize) << 2;
    for i in 0..n_part {
        entries.push(PartInfoEntry {
            offset: (BE::read_u32(&buf[part_info_offset + (8 * i)..]) as usize) << 2,
            part_type: BE::read_u32(&buf[part_info_offset + (8 * i) + 4..]),
        });
    }
    PartInfo {
        offset: part_info_offset,
        entries: entries,
    }
}

pub fn disc_set_part_info(buffer: &mut [u8], pi: &PartInfo) {
    BE::write_u32(&mut buffer[0x40000..], pi.entries.len() as u32);
    BE::write_u32(&mut buffer[0x40004..], (pi.offset >> 2) as u32);
    for (i, entry) in pi.entries.iter().enumerate() {
        BE::write_u32(&mut buffer[pi.offset + (8 * i)..], (entry.offset >> 2) as u32);
        BE::write_u32(&mut buffer[pi.offset + (8 * i) + 4..], entry.part_type as u32);
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Ticket {
    pub sig_type: u32,
    pub sig: [u8; 0x100],
    pub sig_padding: [u8; 0x3C],
    pub sig_issuer: [char; 0x40],
    pub unk1: [u8; 0x3F],
    pub title_key: [u8; 0x10],
    pub unk2: u8,
    pub ticket_id: [u8; 8],
    pub console_id: u32,
    pub title_id: [u8; 8],
    pub unk3: u16,
    pub n_dlc: u16,
    pub unk4: [u8; 0x09],
    pub common_key_index: u8,
    pub unk5: [u8; 0x70],
    pub padding2: u16,
    pub enable_time_limit: u32,
    pub time_limit: u32,
    pub time_limits: [u8; 0x38],
}
declare_tryfrom!(Ticket);

impl Unpackable for Ticket {
    const BLOCK_SIZE: usize = 0x2A4;
}

impl From<&[u8; Ticket::BLOCK_SIZE]> for Ticket {
    fn from(buf: &[u8; Ticket::BLOCK_SIZE]) -> Self {
        let mut sig = [0 as u8; 0x100];
        let mut sig_padding = [0 as u8; 0x3C];
        let mut sig_issuer = [0 as char; 0x40];
        let mut unk1 = [0 as u8; 0x3F];
        let mut title_key = [0 as u8; 0x10];
        let mut ticket_id = [0 as u8; 0x08];
        let mut title_id = [0 as u8; 0x08];
        let mut unk4 = [0 as u8; 0x09];
        let mut unk5 = [0 as u8; 0x70];
        let mut time_limits = [0 as u8; 0x38];
        unsafe {
            sig.copy_from_slice(&buf[0x4..0x104]);
            sig_padding.copy_from_slice(&buf[0x104..0x140]);
            sig_issuer.copy_from_slice(mem::transmute(&buf[0x140..0x180]));
            unk1.copy_from_slice(&buf[0x180..0x1BF]);
            title_key.copy_from_slice(&buf[0x1BF..0x1CF]);
            ticket_id.copy_from_slice(&buf[0x1D0..0x1D8]);
            title_id.copy_from_slice(&buf[0x1DC..0x1E4]);
            unk4.copy_from_slice(&buf[0x1E8..0x1F1]);
            unk5.copy_from_slice(&buf[0x1F2..0x262]);
            time_limits.copy_from_slice(&buf[0x26C..0x2A4]);
        };
        Ticket {
            sig_type: BE::read_u32(&buf[0x00..]),
            sig: sig,
            sig_padding: sig_padding,
            sig_issuer: sig_issuer,
            unk1: unk1,
            title_key: title_key,
            unk2: buf[0x1CF],
            ticket_id: ticket_id,
            console_id: BE::read_u32(&buf[0x1D8..]),
            title_id: title_id,
            unk3: BE::read_u16(&buf[0x1E4..]),
            n_dlc: BE::read_u16(&buf[0x1E6..]),
            unk4: unk4,
            common_key_index: buf[0x1F1],
            unk5: unk5,
            padding2: BE::read_u16(&buf[0x262..]),
            enable_time_limit: BE::read_u32(&buf[0x264..]),
            time_limit: BE::read_u32(&buf[0x268..]),
            time_limits: time_limits,
        }
    }
}

impl From<&Ticket> for [u8; Ticket::BLOCK_SIZE] {
    fn from(t: &Ticket) -> Self {
        let mut buf = [0 as u8; Ticket::BLOCK_SIZE];
        unsafe {
            BE::write_u32(&mut buf[0x00..], t.sig_type);
            (&mut buf[0x04..0x104]).copy_from_slice(&t.sig[..]);
            (&mut buf[0x104..0x140]).copy_from_slice(&t.sig_padding[..]);
            (&mut buf[0x140..0x180]).copy_from_slice(mem::transmute(&t.sig_issuer[..]));
            (&mut buf[0x180..0x1BF]).copy_from_slice(&t.unk1[..]);
            (&mut buf[0x1BF..0x1CF]).copy_from_slice(&t.title_key[..]);
            buf[0x1CF] = t.unk2;
            (&mut buf[0x1D0..0x1D8]).copy_from_slice(&t.ticket_id[..]);
            BE::write_u32(&mut buf[0x1D8..], t.console_id);
            (&mut buf[0x1DC..0x1E4]).copy_from_slice(&t.title_id[..]);
            BE::write_u16(&mut buf[0x1E4..], t.unk3);
            BE::write_u16(&mut buf[0x1E6..], t.n_dlc);
            (&mut buf[0x1E8..0x1F1]).copy_from_slice(&t.unk4[..]);
            buf[0x1F1] = t.common_key_index;
            (&mut buf[0x1F2..0x262]).copy_from_slice(&t.unk5[..]);
            BE::write_u16(&mut buf[0x262..], t.padding2);
            BE::write_u32(&mut buf[0x264..], t.enable_time_limit);
            BE::write_u32(&mut buf[0x268..], t.time_limit);
            (&mut buf[0x26C..0x2A4]).copy_from_slice(&t.time_limits[..]);
        };
        buf
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PartHeader {
    pub ticket: Ticket,
    pub tmd_size: usize,
    pub tmd_offset: usize,
    pub cert_size: usize,
    pub cert_offset: usize,
    pub h3_offset: usize,
    pub data_offset: usize,
    pub data_size: usize,
}
declare_tryfrom!(PartHeader);

impl Unpackable for PartHeader {
    const BLOCK_SIZE: usize = 0x2C0;
}

impl From<&[u8; PartHeader::BLOCK_SIZE]> for PartHeader {
    fn from(buf: &[u8; PartHeader::BLOCK_SIZE]) -> Self {
        PartHeader {
            ticket: Ticket::try_from(&buf[..0x2A4]).unwrap(),
            tmd_size: ((BE::read_u32(&buf[0x2A4..]) as usize)),
            tmd_offset: ((BE::read_u32(&buf[0x2A8..]) as usize) << 2),
            cert_size: ((BE::read_u32(&buf[0x2AC..]) as usize)),
            cert_offset: ((BE::read_u32(&buf[0x2B0..]) as usize) << 2),
            h3_offset: ((BE::read_u32(&buf[0x2B4..]) as usize) << 2),
            data_offset: ((BE::read_u32(&buf[0x2B8..]) as usize) << 2),
            data_size: ((BE::read_u32(&buf[0x2BC..]) as usize) << 2),
        }
    }
}

impl From<&PartHeader> for [u8; PartHeader::BLOCK_SIZE] {
    fn from(ph: &PartHeader) -> Self {
        let mut buf = [0 as u8; PartHeader::BLOCK_SIZE];
        (&mut buf[..0x2A4]).copy_from_slice(&<[u8; 0x2A4]>::from(&ph.ticket));
        BE::write_u32(&mut buf[0x2A4..], ph.tmd_size as u32);
        BE::write_u32(&mut buf[0x2A8..], (ph.tmd_offset >> 2) as u32);
        BE::write_u32(&mut buf[0x2AC..], ph.cert_size as u32);
        BE::write_u32(&mut buf[0x2B0..], (ph.cert_offset >> 2) as u32);
        BE::write_u32(&mut buf[0x2B4..], (ph.h3_offset >> 2) as u32);
        BE::write_u32(&mut buf[0x2B8..], (ph.data_offset >> 2) as u32);
        BE::write_u32(&mut buf[0x2BC..], (ph.data_size >> 2) as u32);
        buf
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Partition {
    pub part_type: u32,
    pub part_offset: usize,
    pub header: PartHeader,
}

#[derive(Debug, Clone)]
pub struct Partitions {
    pub data_idx: usize,
    pub part_info: PartInfo,
    pub partitions: Vec<Partition>,
}

pub fn aes_decrypt_inplace<'a>(data: &'a mut[u8], iv: &[u8], key: &[u8]) -> Result<&'a [u8], WiiCryptoError> {
    let cipher = Aes128Cbc::new_var(key, iv).unwrap();
    cipher.decrypt(&mut *data).or(Err(WiiCryptoError::AesDecryptError))
}

pub fn aes_encrypt_inplace<'a>(data: &'a mut [u8], iv: &[u8], key: &[u8], size: usize) -> Result<&'a[u8], WiiCryptoError> {
    let cipher = Aes128Cbc::new_var(key, iv).unwrap();
    cipher.encrypt(&mut *data, size).or(Err(WiiCryptoError::AesEncryptError))
}

pub fn decrypt_title_key(tik: &Ticket) -> [u8; 0x10] {
    let mut buf = [0 as u8; 0x10];
    let key = &COMMON_KEY[tik.common_key_index as usize];
    let mut iv = [0 as u8; consts::WII_KEY_SIZE];
    iv[0..tik.title_id.len()].copy_from_slice(&tik.title_id);
    let cipher = Aes128Cbc::new_var(&key[..], &iv[..]).unwrap();
    let mut block = [0 as u8; 256];
    block[0..tik.title_key.len()].copy_from_slice(&tik.title_key);

    buf.copy_from_slice(&cipher.decrypt(&mut block).unwrap()[..tik.title_key.len()]);
    buf
}

fn decrypt_partition_inplace<'a>(buf: &'a mut [u8], part: Partition) -> Result<(), WiiCryptoError> {
    let part_key = decrypt_title_key(&part.header.ticket);
    let sector_count = part.header.data_size / 0x8000;
    let mut data_pool: Vec<&mut[u8]> = Vec::with_capacity(sector_count);
    let (_, mut data_slice) = buf.split_at_mut(part.part_offset + part.header.data_offset);
    for _ in 0 .. sector_count {
        let (section, new_data_slice) = data_slice.split_at_mut(consts::WII_SECTOR_SIZE);
        data_slice = new_data_slice;
        data_pool.push(section);
    }
    data_pool.par_iter_mut().for_each(|data| {
        let mut iv = [0 as u8; consts::WII_KEY_SIZE];
        iv[..consts::WII_KEY_SIZE].copy_from_slice(&data[consts::WII_SECTOR_IV_OFF..][..consts::WII_KEY_SIZE]);
        aes_decrypt_inplace(&mut data[..consts::WII_SECTOR_HASH_SIZE], &[0 as u8; consts::WII_KEY_SIZE], &part_key).unwrap();
        aes_decrypt_inplace(&mut data[consts::WII_SECTOR_HASH_SIZE..][..consts::WII_SECTOR_DATA_SIZE], &iv, &part_key).unwrap();
    });
    for i in 0..sector_count {
        let mut data = [0u8; consts::WII_SECTOR_DATA_SIZE];
        data.copy_from_slice(&buf[part.part_offset + part.header.data_offset + i * consts::WII_SECTOR_SIZE + consts::WII_SECTOR_HASH_SIZE..part.part_offset + part.header.data_offset + (i + 1) * consts::WII_SECTOR_SIZE]);
        buf[part.part_offset + part.header.data_offset + i * consts::WII_SECTOR_DATA_SIZE..][..consts::WII_SECTOR_DATA_SIZE].copy_from_slice(&data);
    }
    println!("");
    println!("title code: {}", String::from_utf8_lossy(&buf[part.part_offset + part.header.data_offset..][..0x06]));
    println!("title code of the partition: {}", String::from_utf8_lossy(&buf[part.part_offset + part.header.data_offset..][0x20..0x46]));
    return Ok(());
}

pub fn parse_disc(buf: &mut [u8]) -> Result<Option<Partitions>, WiiCryptoError> {
    let header = disc_get_header(buf);
    if header.wii_magic != 0x5D1C9EA3 {
        return Err(WiiCryptoError::NotWiiDisc{magic: header.wii_magic})
    }
    // if header.disable_disc_encrypt == 1 || header.disable_hash_verif == 1 {
    //     return Ok(None);
    // } else {
    let part_info = disc_get_part_info(buf);
    let mut ret_vec: Vec<Partition> = Vec::new();
    let mut ret: Option<Partitions> = None;
    let mut data_idx: Option<usize> = None;
    for (i, entry) in part_info.entries.iter().enumerate() {
        let part = Partition {
            part_offset: entry.offset,
            part_type: entry.part_type,
            header: PartHeader::try_from(&buf[entry.offset..][..0x2C0])?,
        };
        println!("part{}: part type={}, part offset={:08X}; tmd size={}, tmd offset={:08X}; cert size={}, cert offset={:08X}; h3 offset={:08X}; data size={}, data offset={:08X}", i, part.part_type, part.part_offset, part.header.tmd_size, part.header.tmd_offset, part.header.cert_size, part.header.cert_offset, part.header.h3_offset, part.header.data_size, part.header.data_offset);
        if part.part_type == 0 && data_idx.is_none() {
            data_idx = Some(i);
            decrypt_partition_inplace(buf, part)?;
        }
        ret_vec.push(part);
    }
    if ret_vec.len() > 0 {
        if let Some(data_idx) = data_idx {
            ret = Some(Partitions{
                data_idx,
                part_info,
                partitions: ret_vec,
            });
        }
    }
    // if let Some(mut file) = File::create("test.bin").ok() {
    //     file.write_all(buf).unwrap();
    // }
    return Ok(ret);
    // }
}