use super::virtual_file_system::{Directory, File, Node};
use super::{consts::*, FstEntry, FstNodeType, IsoBuf};
use byteorder::{ByteOrder, BE};
use failure::{Error, ResultExt};
use std::fs;
use std::io::{Read};
use std::path::{Path, PathBuf};
use std::str;

pub fn load_iso_buf<P: AsRef<Path>>(path: &P) -> Result<IsoBuf, Error> {
    if path.as_ref().is_file() {
        Ok(IsoBuf::Raw(read_binary(path).context(format!("couldn't open ISO at \"{:?}\"", path.as_ref().to_str()))?))
    } else {
        let mut path_buf = PathBuf::new();
        path_buf.push(path);
        path_buf.push("DATA");
        Ok(IsoBuf::Extracted(path_buf))
    }
}

pub fn load_iso<'a>(buf: &'a IsoBuf) -> Result<Directory<'a>, Error> {
    match buf {
        IsoBuf::Raw(buf) => {
            let buf = buf.as_slice();
            let fst_offset = BE::read_u32(&buf[OFFSET_FST_OFFSET..]) as usize;
            let mut pos = fst_offset;
            let num_entries = BE::read_u32(&buf[fst_offset + 8..]) as usize;
            let string_table_offset = num_entries * 0xC;

            let mut fst_entries = Vec::with_capacity(num_entries);
            for _ in 0..num_entries {
                let kind = if buf[pos] == 0 {
                    FstNodeType::File
                } else {
                    FstNodeType::Directory
                };
                pos += 2;

                let cur_pos = pos;
                let string_offset = BE::read_u16(&buf[pos..]) as usize;

                pos = string_offset + string_table_offset + fst_offset;
                let mut end = pos;
                while buf[end] != 0 {
                    end += 1;
                }
                let relative_file_name =
                    str::from_utf8(&buf[pos..end]).context("Couldn't parse the relative file name")?;

                pos = cur_pos + 2;
                let file_offset_parent_dir = BE::read_u32(&buf[pos..]) as usize;
                let file_size_next_dir_index = BE::read_u32(&buf[pos + 4..]) as usize;
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
                .push(Node::File(File::new("iso.hdr", &buf[..HEADER_LENGTH])));
    
            let dol_offset = BE::read_u32(&buf[OFFSET_DOL_OFFSET..]) as usize;

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
                    let mut dir = Directory::new(entry.relative_file_name);

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
        IsoBuf::Extracted(path) => {
            let mut root_dir = Directory::new("root");
            let mut sys_data = Directory::new("&&systemdata");
            let mut disc_data = Directory::new("&&discdata");
            let mut root_data = Directory::new("&&rootdata");

            add_file_to_dir(&path, "cert.bin", "cert.bin", &mut root_data)?;
            add_file_to_dir(&path, "h3.bin", "h3.bin", &mut root_data)?;
            add_file_to_dir(&path, "ticket.bin", "ticket.bin", &mut root_data)?;
            add_file_to_dir(&path, "tmd.bin", "tmd.bin", &mut root_data)?;

            root_dir.children.push(Node::Directory(Box::new(root_data)));
            
            let mut sys_path = path.clone();
            sys_path.push("sys");

            add_file_to_dir(&sys_path, "boot.bin", "iso.hdr", &mut sys_data)?;
            add_file_to_dir(&sys_path, "apploader.img", "AppLoader.ldr", &mut sys_data)?;
            add_file_to_dir(&sys_path, "main.dol", "Start.dol", &mut sys_data)?;
            add_file_to_dir(&sys_path, "fst.bin", "Game.toc", &mut sys_data)?;
            add_file_to_dir(&sys_path, "bi2.bin", "bi2.bin", &mut sys_data)?;
            
            root_dir.children.push(Node::Directory(Box::new(sys_data)));

            let mut disc_path = path.clone();
            disc_path.push("disc");

            add_file_to_dir(&disc_path, "header.bin", "header.bin", &mut disc_data)?;
            add_file_to_dir(&disc_path, "region.bin", "region.bin", &mut disc_data)?;

            root_dir.children.push(Node::Directory(Box::new(disc_data)));

            let mut files_path = path.clone();
            files_path.push("files");

            get_dir_structure_recursive_fs(files_path, &mut root_dir)?;

            Ok(root_dir)
        }
    }
}

fn add_file_to_dir(dir_path: &PathBuf, fs_name: &str, given_name: &str, dir: &mut Directory) -> Result<(), Error> {
    let mut file_path = dir_path.clone();
    file_path.push(fs_name);
    dir.children.push(Node::File(File::new(
        given_name,
        read_binary(&file_path)?,
    )));
    Ok(())
}

fn get_dir_structure_recursive<'a>(
    mut cur_index: usize,
    fst: &[FstEntry<'a>],
    parent_dir: &mut Directory<'a>,
    buf: &'a [u8],
) -> usize {
    let entry = &fst[cur_index];

    if entry.kind == FstNodeType::Directory {
        let mut dir = Directory::new(entry.relative_file_name);

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

fn get_dir_structure_recursive_fs<'a>(path: PathBuf, parent_dir: &mut Directory<'a>) -> Result<(), Error> {
    for entry in path.read_dir().context(format!("Couldn't read dir of \"{:?}\"", path.to_str()))? {
        let entry = entry?;
        let filename = entry.file_name();
        if entry.metadata().context(format!("Couldn't fetch metadata of entry \"{:?}\"", entry.path().to_str()))?.is_dir() {
            let mut dir = Directory::new(filename.to_str().unwrap());
            get_dir_structure_recursive_fs(entry.path(), &mut dir)?;
            parent_dir.children.push(Node::Directory(Box::new(dir)));
        } else {
            parent_dir.children.push(Node::File(File::new(filename.to_str().unwrap(), read_binary(&entry.path())?)));
        }
    }
    Ok(())
}

fn get_file_data<'a>(fst_data: &FstEntry<'a>, buf: &'a [u8]) -> File<'a> {
    let data = &buf[fst_data.file_offset_parent_dir..][..fst_data.file_size_next_dir_index];
    File::new(fst_data.relative_file_name, data)
}

fn read_binary<P: AsRef<Path>>(path: &P) -> Result<Vec<u8>, Error> {
    let mut file = fs::File::open(path).context(format!("Couldn't open \"{:?}\"", path.as_ref().to_str()))?;
    let len = file.metadata().context(format!("Couldn't fetch metadata of \"{:?}\"", path.as_ref().to_str()))?.len();
    let mut buf = Vec::with_capacity(len as usize + 1);
    file.read_to_end(&mut buf).context(format!("Couldn't read \"{:?}\"", path.as_ref().to_str()))?;
    Ok(buf)
}
