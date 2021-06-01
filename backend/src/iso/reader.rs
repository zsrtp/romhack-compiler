use crate::KeyValPrint;
use wii_crypto::transform::WiiDisc;
use super::virtual_file_system::{Directory, File, Node};
use super::{consts::*, FstEntry, FstNodeType};
use byteorder::{ByteOrder, BE};
use failure::{Error, ResultExt};
use core::str;
#[cfg(not(target_arch = "wasm32"))]
use async_std::path::Path;
use futures::io::{Result as IOResult, AsyncRead, AsyncSeek};
#[cfg(not(target_arch = "wasm32"))]
use wii_crypto::transform::Disc;
use wii_crypto::transform::WiiPartData;
use async_recursion::async_recursion;

pub trait AsyncSeekReadMarker: AsyncRead + AsyncSeek + Send {}

impl<T> AsyncSeekReadMarker for T
where T: async_std::io::Read + async_std::io::Seek + Send,
{}

#[cfg(not(target_arch = "wasm32"))]
pub async fn load_iso_buf<P: AsRef<Path>>(path: P) -> Result<Disc<'static, Box<(dyn AsyncSeekReadMarker + Unpin + 'static)>>, Error> {
    let file = async_std::fs::File::open(path).await?;
    let b: Box<(dyn AsyncSeekReadMarker + Unpin + 'static)> = Box::new(file);
    Disc::extract(b).await
}

pub fn load_iso<'a>(buf: &'a [u8]) -> Result<Directory<'a>, Error> {
    let is_wii: bool = is_wii(buf);
    let fst_offset = (BE::read_u32(&buf[OFFSET_FST_OFFSET..]) as usize) << (if is_wii {2} else {0});
    let num_entries = BE::read_u32(&buf[fst_offset + 8..]) as usize;
    let string_table_offset = num_entries * 0xC;

    let mut fst_entries = Vec::with_capacity(num_entries);
    for i in 0..num_entries {
        let kind = if buf[fst_offset + i*12] == 0 {
            FstNodeType::File
        } else {
            FstNodeType::Directory
        };

        let string_offset = (BE::read_u32(&buf[fst_offset + i*12..]) & 0x00ffffff) as usize;

        let pos = string_offset + string_table_offset + fst_offset;
        let mut end = pos;
        while buf[end] != 0 {
            end += 1;
        }
        let relative_file_name =
            String::from(str::from_utf8(&buf[pos.. end]).context("Couldn't parse the relative file name")?);

        let file_offset_parent_dir = (BE::read_u32(&buf[fst_offset + i*12 + 4..]) as usize) << (if is_wii {2} else {0});
        let file_size_next_dir_index = BE::read_u32(&buf[fst_offset + i*12 + 8..]) as usize;

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
        .push(Node::File(File::new("iso.hdr", &buf[.. HEADER_LENGTH])));

    let dol_offset = (BE::read_u32(&buf[OFFSET_DOL_OFFSET..]) as usize) << (if is_wii {2} else {0});

    sys_data.children.push(Node::File(File::new(
        "AppLoader.ldr",
        &buf[HEADER_LENGTH..dol_offset],
    )));

    sys_data.children.push(Node::File(File::new(
        "Start.dol",
        &buf[dol_offset..fst_offset],
    )));

    let fst_size = BE::read_u32(&buf[OFFSET_FST_SIZE..]) as usize;
    sys_data.children.push(Node::File(File::new(
        "Game.toc",
        &buf[fst_offset..][..fst_size],
    )));

    root_dir.children.push(Node::Directory(Box::new(sys_data)));

    let mut count = 1;

    while count < num_entries {
        let entry = &fst_entries[count];
        if fst_entries[count].kind == FstNodeType::Directory {
            let mut dir = Directory::new(entry.relative_file_name.as_str());

            while count < entry.file_size_next_dir_index - 1 {
                count = get_dir_structure_recursive(count + 1, &fst_entries, &mut dir, buf);
            }

            root_dir.children.push(Node::Directory(Box::new(dir)));
        } else {
            let file = get_file_data(&fst_entries[count], buf);
            root_dir.children.push(Node::File(file));
        }
        count += 1;
    }

    Ok(root_dir)
}

fn get_dir_structure_recursive<'a>(
    mut cur_index: usize,
    fst: &[FstEntry],
    parent_dir: &mut Directory<'a>,
    buf: &'a [u8],
) -> usize {
    let entry = &fst[cur_index];

    if entry.kind == FstNodeType::Directory {
        let mut dir = Directory::new(entry.relative_file_name.as_str());

        while cur_index < entry.file_size_next_dir_index - 1 {
            cur_index = get_dir_structure_recursive(cur_index + 1, fst, &mut dir, buf);
        }

        parent_dir.children.push(Node::Directory(Box::new(dir)));
    } else {
        let file = get_file_data(entry, buf);
        parent_dir.children.push(Node::File(file));
    }

    cur_index
}

fn get_file_data<'a>(fst_data: &FstEntry, buf: &'a [u8]) -> File<'a> {
    let data = &buf[fst_data.file_offset_parent_dir..][..fst_data.file_size_next_dir_index];
    File::new(fst_data.relative_file_name.as_str(), data)
}

pub async fn load_iso_wii<'a, R: AsyncRead + AsyncSeek + Unpin + Send + 'a, P: KeyValPrint>(disc: &mut WiiDisc<'a, R>, printer: &P) -> Result<Directory<'a>, Error> {
    let is_wii: bool = true;
    let data = &mut disc.partition.data;
    let fst_offset = (BE::read_u32(&data.get(OFFSET_FST_OFFSET as u64, 4).await?) as u64) << (if is_wii {2} else {0});
    let num_entries = BE::read_u32(&data.get(fst_offset + 8, 4).await?) as u64;
    let string_table_offset = num_entries * 0xC;

    printer.print(None, "Loading", "File System Table...");

    let mut fst_entries = Vec::with_capacity(num_entries as usize);
    for i in 0..num_entries {
        let kind = if data.get(fst_offset + i*12, 1).await?[0] == 0 {
            FstNodeType::File
        } else {
            FstNodeType::Directory
        };

        let string_offset = (BE::read_u32(&data.get(fst_offset + i*12, 4).await?) & 0x00ffffff) as u64;

        let pos = string_offset + string_table_offset + fst_offset;
        let mut end = pos;
        while data.get(end, 1).await?[0] != 0 {
            end += 1;
        }
        let relative_file_name =
            String::from_utf8(data.get(pos, (end - pos) as usize).await?).context("Couldn't parse the relative file name")?;

        let file_offset_parent_dir = (BE::read_u32(&data.get(fst_offset + i*12 + 4, 4).await?) as usize) << (if is_wii {2} else {0});
        let file_size_next_dir_index = BE::read_u32(&data.get(fst_offset + i*12 + 8, 4).await?) as usize;

        fst_entries.push(FstEntry {
            kind,
            relative_file_name,
            file_offset_parent_dir,
            file_size_next_dir_index,
            file_name_offset: 0,
        });
    }

    printer.print(None, "Loading", "System Data...");

    let mut root_dir = Directory::new("root");
    let mut sys_data = Directory::new("&&systemdata");

    sys_data
        .children
        .push(Node::File(File::new("iso.hdr", data.get(0, HEADER_LENGTH).await?)));

    let dol_offset = (BE::read_u32(&data.get(OFFSET_DOL_OFFSET as u64, 4).await?) as usize) << (if is_wii {2} else {0});

    sys_data.children.push(Node::File(File::new(
        "AppLoader.ldr",
        data.get(HEADER_LENGTH as u64, dol_offset - HEADER_LENGTH).await?,
    )));

    sys_data.children.push(Node::File(File::new(
        "Start.dol",
        data.get(dol_offset as u64, fst_offset as usize - dol_offset).await?,
    )));

    let fst_size = BE::read_u32(&data.get(OFFSET_FST_SIZE as u64, 4).await?) as usize;
    sys_data.children.push(Node::File(File::new(
        "Game.toc",
        data.get(fst_offset, fst_size).await?,
    )));

    root_dir.children.push(Node::Directory(Box::new(sys_data)));

    let mut count: usize = 1;

    printer.print(None, "Loading", "Files...");

    while count < num_entries as usize {
        let entry = &fst_entries[count];
        if fst_entries[count].kind == FstNodeType::Directory {
            let mut dir = Directory::new(entry.relative_file_name.as_str());
            // printer.print(None, "Loading", &format!("Dir {} ...", entry.relative_file_name));

            while count < entry.file_size_next_dir_index - 1 {
                count = get_dir_structure_recursive_wii(count + 1, &fst_entries, &mut dir, data, printer).await?;
            }

            root_dir.children.push(Node::Directory(Box::new(dir)));
        } else {
            let file = get_file_data_wii(&fst_entries[count], data, printer).await?;
            root_dir.children.push(Node::File(file));
        }
        count += 1;
    }

    Ok(root_dir)
}

#[async_recursion(?Send)]
async fn get_dir_structure_recursive_wii<'a, R: AsyncRead + AsyncSeek + Unpin + Send + 'a, P: KeyValPrint>(
    mut cur_index: usize,
    fst: &[FstEntry],
    parent_dir: &mut Directory<'a>,
    data: &mut WiiPartData<'a, R>,
    printer: &P,
) -> IOResult<usize> {
    let entry = &fst[cur_index];

    if entry.kind == FstNodeType::Directory {
        let mut dir = Directory::new(entry.relative_file_name.as_str());
        // printer.print(None, "Loading", &format!("Dir {} ...", entry.relative_file_name));

        while cur_index < entry.file_size_next_dir_index - 1 {
            cur_index = get_dir_structure_recursive_wii(cur_index + 1, fst, &mut dir, data, printer).await?;
        }

        parent_dir.children.push(Node::Directory(Box::new(dir)));
    } else {
        let file = get_file_data_wii(entry, data, printer).await?;
        parent_dir.children.push(Node::File(file));
    }

    Ok(cur_index)
}

async fn get_file_data_wii<'a, R: AsyncRead + AsyncSeek + Unpin + Send + 'a, P: KeyValPrint>(fst_data: &FstEntry, data: &mut WiiPartData<'a, R>, _printer: &P) -> IOResult<File<'a>> {
    // printer.print(None, "Loading", &format!("File {} ...", fst_data.relative_file_name));
    let data = data.get(fst_data.file_offset_parent_dir as u64, fst_data.file_size_next_dir_index).await?;
    Ok(File::new(fst_data.relative_file_name.as_str(), data))
}

fn is_wii(buf: &[u8]) -> bool {
    BE::read_u32(&buf[OFFSET_WII_MAGIC..]) == 0x5D1C9EA3
}