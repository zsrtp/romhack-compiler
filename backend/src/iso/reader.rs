use super::virtual_file_system::{Directory, File, Node};
use super::{consts::*, FstEntry, FstNodeType};
use byteorder::{ByteOrder, BE};
use failure::{Error, ResultExt};
use std::fs;
use std::io::{Read, Result as IOResult};
use std::path::Path;
use std::str;
use wii_crypto::wii_disc::{Partition, Partitions};

pub fn load_iso_buf<P: AsRef<Path>>(path: P) -> IOResult<Vec<u8>> {
    let mut file = fs::File::open(path)?;
    let len = file.metadata()?.len();
    let mut buf = Vec::with_capacity(len as usize + 1);
    file.read_to_end(&mut buf)?;
    Ok(buf)
}

pub fn load_iso<'a>(buf: &'a [u8], parts_opt: &Option<Partitions>) -> Result<Directory<'a>, Error> {
    let part_opt = parts_opt.clone().and_then(|p| Some(p.partitions[p.data_idx]));
    let partition_offset = if let Some(part) = part_opt {part.part_offset + part.header.data_offset} else {0 as usize};
    let is_wii: bool = is_wii(buf);
    let fst_offset = (BE::read_u32(&buf[partition_offset + OFFSET_FST_OFFSET..]) as usize) << (if is_wii {2} else {0});
    let num_entries = BE::read_u32(&buf[partition_offset + fst_offset + 8..]) as usize;
    let string_table_offset = num_entries * 0xC;

    let mut fst_entries = Vec::with_capacity(num_entries);
    for i in 0..num_entries {
        let kind = if buf[partition_offset + fst_offset + i*12] == 0 {
            FstNodeType::File
        } else {
            FstNodeType::Directory
        };

        let string_offset = (BE::read_u32(&buf[partition_offset + fst_offset + i*12..]) & 0x00ffffff) as usize;

        let pos = string_offset + string_table_offset + fst_offset;
        let mut end = pos;
        while buf[partition_offset + end] != 0 {
            end += 1;
        }
        let relative_file_name =
            str::from_utf8(&buf[partition_offset + pos..partition_offset + end]).context("Couldn't parse the relative file name")?;

        let file_offset_parent_dir = (BE::read_u32(&buf[partition_offset + fst_offset + i*12 + 4..]) as usize) << (if is_wii {2} else {0});
        let file_size_next_dir_index = BE::read_u32(&buf[partition_offset + fst_offset + i*12 + 8..]) as usize;

        fst_entries.push(FstEntry {
            kind,
            relative_file_name,
            file_offset_parent_dir,
            file_size_next_dir_index,
            file_name_offset: 0,
        });
    }

    let mut root_dir = Directory::new("root");
    let mut sys_data = Directory::new("&&systemdata");

    if let Some(parts) = parts_opt {
        sys_data.children.push(Node::File(File::new("w_iso.hdr", &buf[..0x50000])));
        for (i, part) in parts.partitions.iter().enumerate() {
            sys_data.children.push(Node::File(File::new(format!("w_part{}.prt", i).as_str(), &buf[part.part_offset ..][.. part.header.data_offset + part.header.data_size])));
        }
    }

    sys_data
        .children
        .push(Node::File(File::new("iso.hdr", &buf[partition_offset..partition_offset + HEADER_LENGTH])));

    let dol_offset = (BE::read_u32(&buf[partition_offset + OFFSET_DOL_OFFSET..]) as usize) << (if is_wii {2} else {0});
    println!("dol offset: {}", dol_offset);

    sys_data.children.push(Node::File(File::new(
        "AppLoader.ldr",
        &buf[partition_offset + HEADER_LENGTH..partition_offset + dol_offset],
    )));

    sys_data.children.push(Node::File(File::new(
        "Start.dol",
        &buf[partition_offset + dol_offset..partition_offset + fst_offset],
    )));

    let fst_size = BE::read_u32(&buf[partition_offset + OFFSET_FST_SIZE..]) as usize;
    sys_data.children.push(Node::File(File::new(
        "Game.toc",
        &buf[partition_offset + fst_offset..][..fst_size],
    )));

    root_dir.children.push(Node::Directory(Box::new(sys_data)));

    let mut count = 1;

    while count < num_entries {
        let entry = &fst_entries[count];
        if fst_entries[count].kind == FstNodeType::Directory {
            let mut dir = Directory::new(entry.relative_file_name);

            while count < entry.file_size_next_dir_index - 1 {
                count = get_dir_structure_recursive(count + 1, &fst_entries, &mut dir, buf, &part_opt);
            }

            root_dir.children.push(Node::Directory(Box::new(dir)));
        } else {
            let file = get_file_data(&fst_entries[count], buf, &part_opt);
            root_dir.children.push(Node::File(file));
        }
        count += 1;
    }

    Ok(root_dir)
}

fn get_dir_structure_recursive<'a>(
    mut cur_index: usize,
    fst: &[FstEntry<'a>],
    parent_dir: &mut Directory<'a>,
    buf: &'a [u8],
    part_opt: &Option<Partition>,
) -> usize {
    let entry = &fst[cur_index];

    if entry.kind == FstNodeType::Directory {
        let mut dir = Directory::new(entry.relative_file_name);

        while cur_index < entry.file_size_next_dir_index - 1 {
            cur_index = get_dir_structure_recursive(cur_index + 1, fst, &mut dir, buf, &part_opt);
        }

        parent_dir.children.push(Node::Directory(Box::new(dir)));
    } else {
        let file = get_file_data(entry, buf, &part_opt);
        parent_dir.children.push(Node::File(file));
    }

    cur_index
}

fn get_file_data<'a>(fst_data: &FstEntry<'a>, buf: &'a [u8], part_opt: &Option<Partition>) -> File<'a> {
    let part_data_offset = if let Some(part) = part_opt {part.part_offset + part.header.data_offset} else {0};
    let data = &buf[part_data_offset + fst_data.file_offset_parent_dir..][..fst_data.file_size_next_dir_index];
    File::new(fst_data.relative_file_name, data)
}

fn is_wii(buf: &[u8]) -> bool {
    BE::read_u32(&buf[OFFSET_WII_MAGIC..]) == 0x5D1C9EA3
}

// fn read_partition(in_buf: &[u8], out_buf: &mut [u8], offset: usize, size: usize, part_opt: &Option<Partition>) {
//     let mut block = [0u8; 0x7c00];
//     if let Some(part) = part_opt {
//         let mut offset = offset;
//         let mut len = size;
//         let mut out_offset = 0usize;
//         while len > 0 {
//             let offset_in_block = offset % 0x7c00;
//             let mut len_in_block = 0x7c00 - offset_in_block;
//             if len_in_block > len {
//                 len_in_block = len;
//             }
//             let block_no = offset / 0x7c00;
//             block.copy_from_slice(&in_buf[part.part_offset + 0x8000 * block_no + 0x400..][..0x7c00]);
//             out_buf[out_offset..][..len_in_block].copy_from_slice(&block[offset_in_block..][..len_in_block]);
//             out_offset += len_in_block;
//             offset += len_in_block;
//             len -= len_in_block;
//         }
//     } else {
//         out_buf.copy_from_slice(&in_buf[offset..][..size]);
//     }
// }

// fn read(buf: &[u8], offset: usize, size: usize, part_opt: &Option<Partition>) -> Vec<u8> {
//     let mut out: Vec<u8> = Vec::new();
//     out.resize(size, 0);
//     read_partition(buf, &mut out[..], offset, size, part_opt);
//     out
// }

// fn read_u16(buf: &[u8], offset: usize, part_opt: &Option<Partition>) -> u16 {
//     let mut block = [0u8; 0x2];
//     read_partition(buf, &mut block, offset, 4, part_opt);
//     BE::read_u16(&block[..])
// }

// fn read_u32(buf: &[u8], offset: usize, part_opt: &Option<Partition>) -> u32 {
//     let mut block = [0u8; 0x4];
//     read_partition(buf, &mut block, offset, 4, part_opt);
//     BE::read_u32(&block[..])
// }
