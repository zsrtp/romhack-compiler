use crate::COMMON_KEY;
use crate::consts;
use libaes::Cipher;
use std::mem;
use std::convert::TryFrom;
use byteorder::{BE, ByteOrder};
use rayon::prelude::*;
// use std::fs::File;
// use std::io::prelude::*;

trait Unpackable {
    const BLOCK_SIZE: usize;
}

pub type Error = String;

#[macro_export]
macro_rules! declare_tryfrom {
    ( $T:ty ) => {
        impl TryFrom<&[u8]> for $T {
            type Error = Error;

            fn try_from(slice: &[u8]) -> Result<$T, Self::Error> {
                if slice.len() < <$T>::BLOCK_SIZE {
                    Err(format!("The provided slice is too small to be converted into a {}.", stringify![$T]))
                } else {
                    let mut buf = [0 as u8; <$T>::BLOCK_SIZE];
                    buf.clone_from_slice(&slice[..<$T>::BLOCK_SIZE]);
                    Ok(<$T>::from(&buf))
                }
            }
        }
    };
}

#[derive(Copy, Clone)]
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
declare_tryfrom!(DiscHeader);

impl Unpackable for DiscHeader {
    const BLOCK_SIZE: usize = 0x62;
}

impl From<&[u8; DiscHeader::BLOCK_SIZE]> for DiscHeader {
    fn from(buf: &[u8; DiscHeader::BLOCK_SIZE]) -> Self {
        let mut unk1 = [0 as u8; 14];
        let mut game_title = [0 as char; 64];
        unsafe {
            unk1.copy_from_slice(&buf[0xA..0x18]);
            game_title.copy_from_slice(mem::transmute(&buf[0x20..0x60]));
        };
        DiscHeader {
            disc_id: buf[0] as char,
            game_code: [buf[1] as char, buf[2] as char],
            region_code: buf[3] as char,
            maker_code: [buf[4] as char, buf[5] as char],
            disc_number: buf[6],
            disc_version: buf[7],
            audio_streaming: buf[8] != 0,
            streaming_buffer_size: buf[9],
            unk1: unk1,
            wii_magic: BE::read_u32(&buf[0x18..0x1C]),
            gc_magic: BE::read_u32(&buf[0x1C..0x20]),
            game_title: game_title,
            disable_hash_verif: buf[0x60],
            disable_disc_encrypt: buf[0x61],
        }
    }
}

impl From<&DiscHeader> for [u8; DiscHeader::BLOCK_SIZE] {
    fn from(dh: &DiscHeader) -> Self {
        let mut buffer = [0 as u8; DiscHeader::BLOCK_SIZE];
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
        buffer
    }
}

#[derive(Copy, Clone)]
pub struct PartInfoEntry {
    pub count: usize,
    pub offset: usize,
}

#[derive(Copy, Clone)]
pub struct PartInfo {
    pub entries: [PartInfoEntry; 4],
}
declare_tryfrom!(PartInfo);

impl Unpackable for PartInfo {
    const BLOCK_SIZE: usize = 0x20;
}

impl From<&[u8; PartInfo::BLOCK_SIZE]> for PartInfo {
    fn from(buf: &[u8; PartInfo::BLOCK_SIZE]) -> Self {
        PartInfo {
            entries: [ PartInfoEntry {
                count: BE::read_u32(&buf[0x00..]) as usize,
                offset: (BE::read_u32(&buf[0x04..]) << 2) as usize,
            }, PartInfoEntry {
                count: BE::read_u32(&buf[0x08..]) as usize,
                offset: (BE::read_u32(&buf[0x0C..]) << 2) as usize,
            }, PartInfoEntry {
                count: BE::read_u32(&buf[0x10..]) as usize,
                offset: (BE::read_u32(&buf[0x14..]) << 2) as usize,
            }, PartInfoEntry {
                count: BE::read_u32(&buf[0x18..]) as usize,
                offset: (BE::read_u32(&buf[0x1C..]) << 2) as usize,
            },]
        }
    }
}

impl From<&PartInfo> for [u8; PartInfo::BLOCK_SIZE] {
    fn from(pi: &PartInfo) -> Self {
        let mut buffer = [0 as u8; PartInfo::BLOCK_SIZE];
        BE::write_u32(&mut buffer[0x00..], pi.entries[0].count as u32);
        BE::write_u32(&mut buffer[0x04..], pi.entries[0].offset as u32 >> 2);
        BE::write_u32(&mut buffer[0x08..], pi.entries[1].count as u32);
        BE::write_u32(&mut buffer[0x0C..], pi.entries[1].offset as u32 >> 2);
        BE::write_u32(&mut buffer[0x10..], pi.entries[2].count as u32);
        BE::write_u32(&mut buffer[0x14..], pi.entries[2].offset as u32 >> 2);
        BE::write_u32(&mut buffer[0x18..], pi.entries[3].count as u32);
        BE::write_u32(&mut buffer[0x1C..], pi.entries[3].offset as u32 >> 2);
        buffer
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
            tmd_size: (BE::read_u32(&buf[0x2A4..])) as usize,
            tmd_offset: (BE::read_u32(&buf[0x2A8..]) << 2) as usize,
            cert_size: (BE::read_u32(&buf[0x2AC..])) as usize,
            cert_offset: (BE::read_u32(&buf[0x2B0..]) << 2) as usize,
            h3_offset: (BE::read_u32(&buf[0x2B4..]) << 2) as usize,
            data_offset: (BE::read_u32(&buf[0x2B8..]) << 2) as usize,
            data_size: (BE::read_u32(&buf[0x2BC..]) << 2) as usize,
        }
    }
}

impl From<&PartHeader> for [u8; PartHeader::BLOCK_SIZE] {
    fn from(ph: &PartHeader) -> Self {
        let mut buf = [0 as u8; PartHeader::BLOCK_SIZE];
        (&mut buf[..0x2A4]).copy_from_slice(&<[u8; 0x2A4]>::from(&ph.ticket));
        BE::write_u32(&mut buf[0x2A4..], ph.tmd_size as u32);
        BE::write_u32(&mut buf[0x2A8..], ph.tmd_offset as u32 >> 2);
        BE::write_u32(&mut buf[0x2AC..], ph.cert_size as u32);
        BE::write_u32(&mut buf[0x2B0..], ph.cert_offset as u32 >> 2);
        BE::write_u32(&mut buf[0x2B4..], ph.h3_offset as u32 >> 2);
        BE::write_u32(&mut buf[0x2B8..], ph.data_offset as u32 >> 2);
        BE::write_u32(&mut buf[0x2BC..], ph.data_size as u32 >> 2);
        buf
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Partition {
    pub part_type: u32,
    pub part_offset: usize,
    pub header: PartHeader,
}

fn decrypt_title_key(tik: &Ticket) -> [u8; 0x10] {
    let mut buf = [0 as u8; 0x10];
    let key = &COMMON_KEY[tik.common_key_index as usize];
    let mut iv = [0 as u8; consts::WII_KEY_SIZE];
    iv[0..tik.title_id.len()].copy_from_slice(&tik.title_id);
    let cipher = Cipher::new_128(&key);
    let mut block = [0 as u8; 256];
    block[0..tik.title_key.len()].copy_from_slice(&tik.title_key);

    buf.copy_from_slice(&cipher.cbc_decrypt(&iv, &block)[..tik.title_key.len()]);
    buf
}

fn extract_partition(buf: &[u8], part: Partition) -> Result<(Vec<u8>, Option<Partition>), Error> {
    let part_key = decrypt_title_key(&part.header.ticket);
    let sector_count = part.header.data_size / 0x8000;
    println!("partition size: {} ({} sector(s))", sector_count * 0x7C00, sector_count);
    println!("decrypting...");
    let mut data_pool: Vec<(usize, &[u8])> = Vec::with_capacity(sector_count);
    for i in 0..sector_count {
        data_pool.push((i, &buf[part.part_offset + part.header.data_offset + i * consts::WII_SECTOR_SIZE..][..consts::WII_SECTOR_SIZE]));
    }
    let mut output = vec![0 as u8; sector_count * 0x7C00];
    let data: Vec<(usize, Vec<u8>)> = data_pool.par_iter().map(|entry| {
        let i = entry.0;
        let buf = entry.1;
        let mut data = vec![0 as u8; consts::WII_SECTOR_DATA_SIZE];
        let mut iv = [0 as u8; consts::WII_KEY_SIZE];
        let mut hash = [0 as u8; consts::WII_SECTOR_HASH_SIZE];
        let cipher = Cipher::new_128(&part_key);
        hash.copy_from_slice(&cipher.cbc_decrypt(&[0 as u8; consts::WII_KEY_SIZE], &buf[..consts::WII_SECTOR_HASH_SIZE])[..]);
        iv[..consts::WII_KEY_SIZE].copy_from_slice(&buf[consts::WII_SECTOR_IV_OFF..][..consts::WII_KEY_SIZE]);
        let out = cipher.cbc_decrypt(&iv, &buf[consts::WII_SECTOR_HASH_SIZE..][..consts::WII_SECTOR_DATA_SIZE]);
        data[..out.len()].copy_from_slice(&out);
        (i, data)
    }).collect();
    for j in 0..sector_count {
        let i = data[j].0;
        output[i * consts::WII_SECTOR_DATA_SIZE..(i + 1) * consts::WII_SECTOR_DATA_SIZE].copy_from_slice(&data[j].1);
    }
    println!("");
    println!("title code of the partition: {}", std::str::from_utf8(&output[0x20..0x46]).unwrap());
    // let mut file = File::create(format!("dbg_{:#X}.bin", part.part_offset)).unwrap();
    // file.write_all(&output[..]).unwrap();
    Ok((output, Some(part)))
}

pub fn parse_disc(buf: &[u8]) -> Result<(Vec<u8>, Option<Partition>), Error> {
    let header = DiscHeader::try_from(&buf[..0x62])?;
    if header.wii_magic != 0x5D1C9EA3 {
        return Err(String::from(format!("Not a wii disc: magic code is {:#X}", header.wii_magic)))
    }
    if header.disable_disc_encrypt == 1 || header.disable_hash_verif == 1 {
        let mut v = vec![0 as u8; buf.len()];
        v.copy_from_slice(&buf[..]);
        return Ok((v, None));
    } else {
        let part_info = PartInfo::try_from(&buf[0x40000..0x40020])?;
        for entry in part_info.entries.iter() {
            for i in 0 as usize..entry.count as usize {
                let part_offset = (BE::read_u32(&buf[entry.offset + i * 8..]) << 2) as usize;
                let part = Partition {
                    part_offset: part_offset,
                    part_type: BE::read_u32(&buf[entry.offset + 4 + i * 8..]),
                    header: PartHeader::try_from(&buf[part_offset..][..0x2C0])?,
                };
                println!("part{}: part type={}, part offset={:08X}; tmd size={}, tmd offset={:08X}; cert size={}, cert offset={:08X}; h3 offset={:08X}; data size={}, data offset={:08X}", i, part.part_type, part.part_offset, part.header.tmd_size, part.header.tmd_offset, part.header.cert_size, part.header.cert_offset, part.header.h3_offset, part.header.data_size, part.header.data_offset);
                if part.part_type == 0 {
                    return extract_partition(buf, part);
                }
            }
        }
        return Err(String::from("Encrypted"))
    }
}