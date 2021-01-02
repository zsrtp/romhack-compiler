use super::virtual_file_system::{Directory, File, Node};
use super::{consts::*, FstEntry, FstNodeType};
use byteorder::{ByteOrder, BE};
use failure::{Error, ResultExt};
use std::fs;
use std::io::{Read, Result as IOResult};
use std::path::Path;
use std::str;
use wii_crypto::wii_disc::{Partition};

pub fn load_iso_buf<P: AsRef<Path>>(path: P) -> IOResult<Vec<u8>> {
    let mut file = fs::File::open(path)?;
    let len = file.metadata()?.len();
    let mut buf = Vec::with_capacity(len as usize + 1);
    file.read_to_end(&mut buf)?;
    Ok(buf)
}

pub fn load_iso<'a>(buf: &'a [u8], part_opt: &Option<Partition>) -> Result<Directory<'a>, Error> {
    let partition_offset = if let Some(part) = part_opt {part.part_offset} else {0 as usize};
    // println!("buffer len: {:#010x}", buf.len());
    let is_gc: bool = is_gc(buf);
    let fst_offset = (BE::read_u32(&buf[partition_offset + OFFSET_FST_OFFSET..]) << (if is_gc {0} else {2})) as usize;
    let mut pos = fst_offset;
    let num_entries = BE::read_u32(&buf[partition_offset + fst_offset + 8..]) as usize;
    let string_table_offset = num_entries * 0xC;

    let mut fst_entries = Vec::with_capacity(num_entries);
    for _ in 0..num_entries {
        let kind = if buf[partition_offset + pos] == 0 {
            FstNodeType::File
        } else {
            FstNodeType::Directory
        };
        pos += 2;

        let cur_pos = pos;
        let string_offset = BE::read_u16(&buf[partition_offset + pos..]) as usize;

        pos = string_offset + string_table_offset + fst_offset;
        let mut end = pos;
        while buf[partition_offset + end] != 0 {
            end += 1;
        }
        let relative_file_name =
            str::from_utf8(&buf[partition_offset + pos..partition_offset + end]).context("Couldn't parse the relative file name")?;

        pos = cur_pos + 2;
        let file_offset_parent_dir = BE::read_u32(&buf[partition_offset + pos..]) as usize;
        let file_size_next_dir_index = BE::read_u32(&buf[partition_offset + pos + 4..]) as usize;
        if kind == FstNodeType::File {
            println!("({}) addr: {:#010x} [size: {:#010x}]", relative_file_name, (file_offset_parent_dir << 2), file_size_next_dir_index); // + 0x400 * ((file_offset_parent_dir << 2) / 0x7C00 + 1)
        }
        pos += 8;

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

    sys_data
        .children
        .push(Node::File(File::new("iso.hdr", &buf[partition_offset..partition_offset + HEADER_LENGTH])));

    let dol_offset = (BE::read_u32(&buf[partition_offset + OFFSET_DOL_OFFSET..]) << (if is_gc {0} else {2})) as usize;

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
    let file_offset = fst_data.file_offset_parent_dir << (if is_gc(buf) {0} else {2});
    let data = &buf[part_data_offset + file_offset..][..fst_data.file_size_next_dir_index]; //  + 0x400 * ((file_offset) / 0x7C00 + 1)
    if fst_data.relative_file_name == "RframeworkF.map" {
        println!("data len: {:#x}; {:#010x}; first bytes: \"{}\"", data.len(), (part_data_offset + file_offset) as usize, String::from_utf8_lossy(&data[..32]));
    }
    File::new(fst_data.relative_file_name, data)
}

fn is_gc(buf: &[u8]) -> bool {
    BE::read_u32(&buf[OFFSET_GC_MAGIC..]) == 0xC2339F3D
}
