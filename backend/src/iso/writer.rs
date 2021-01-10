use super::virtual_file_system::{Directory, Node, File};
use super::{consts::*, FstEntry, FstNodeType};
use byteorder::{WriteBytesExt, BE};
use failure::{err_msg, Error};
use std::io::{Seek, SeekFrom, Write, Read};
use wii_crypto::wii_disc::{PartHeader, decrypt_title_key, aes_encrypt_inplace};
use std::convert::TryFrom;
use sha1::Sha1;

pub fn write_iso<W, R>(mut writer: W, mut reader: R, root: &Directory) -> Result<(), Error>
where
    W: Write + Seek,
    R: Read + Seek,
{
    let is_wii = root.is_wii_iso();
    let mut part_offset = 0usize;
    let mut part_data_offset = 0usize;
    let mut part_header_opt: Option<PartHeader> = None;
    let (sys_index, sys_dir) = root
        .children
        .iter()
        .enumerate()
        .filter_map(|(i, c)| c.as_directory().map(|d| (i, d)))
        .find(|&(_, d)| d.name == "&&systemdata")
        .ok_or_else(|| err_msg("The virtual file system contains no &&systemdata folder"))?;

    if is_wii {
        // fill with 0s
        print!("filling with 0s...");
        std::io::stdout().flush().unwrap();
        writer.seek(SeekFrom::Start(0))?;
        for _ in 0usize..0x11824 {
            writer.write_all(&[0; 0x10000])?;
        }
        writer.seek(SeekFrom::Start(0))?;
        println!(" done");

        let header = fetch_file(sys_dir, String::from("w_iso.hdr"))?;
        writer.write_all(&header.data)?;
        let part_info = wii_crypto::wii_disc::disc_get_part_info(&header.data[..]);
        for (i, entry) in part_info.entries.iter().enumerate() {
            let filename = format!("w_part{}.prt", i);
            let partition = fetch_file(sys_dir, filename)?;
            writer.seek(SeekFrom::Start(entry.offset as u64))?;
            writer.write_all(&partition.data)?;
            if entry.part_type == 0 && part_data_offset == 0 {
                let part_header = PartHeader::try_from(&partition.data[..0x2C0]).expect("Invalid partition header.");
                part_offset = entry.offset;
                part_data_offset = entry.offset + part_header.data_offset;
                part_header_opt = Some(part_header);
            }
        }
        writer.seek(SeekFrom::Start(part_data_offset as u64))?;
    }

    let header = sys_dir
        .children
        .iter()
        .filter_map(|c| c.as_file())
        .find(|f| f.name == "iso.hdr")
        .ok_or_else(|| err_msg("The &&systemdata folder contains no iso.hdr"))?;
    writer.write_all(&header.data)?;

    let apploader = sys_dir
        .children
        .iter()
        .filter_map(|c| c.as_file())
        .find(|f| f.name == "AppLoader.ldr")
        .ok_or_else(|| err_msg("The &&systemdata folder contains no AppLoader.ldr"))?;
    writer.write_all(&apploader.data)?;

    let dol_offset_without_padding = part_data_offset + header.data.len() + apploader.data.len();
    let dol_offset =
        (dol_offset_without_padding + (DOL_ALIGNMENT - 1)) / DOL_ALIGNMENT * DOL_ALIGNMENT;

    for _ in dol_offset_without_padding..dol_offset {
        writer.write_all(&[0])?;
    }

    let dol = sys_dir
        .children
        .iter()
        .filter_map(|c| c.as_file())
        .find(|f| f.name.ends_with(".dol"))
        .ok_or_else(|| err_msg("The &&systemdata folder contains no dol file"))?;
    writer.write_all(&dol.data)?;

    let fst_list_offset_without_padding = dol_offset + dol.data.len();
    let fst_list_offset =
        (fst_list_offset_without_padding + (FST_ALIGNMENT - 1)) / FST_ALIGNMENT * FST_ALIGNMENT;

    for _ in fst_list_offset_without_padding..fst_list_offset {
        writer.write_all(&[0])?;
    }

    let mut fst_len = 12;
    for (_, node) in root
        .children
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != sys_index)
    {
        fst_len = calculate_fst_len(fst_len, node);
    }

    for _ in 0..fst_len {
        // TODO Seems suboptimal
        // Should not be a problem with BufWriter though
        writer.write_all(&[0])?;
    }

    let root_fst = FstEntry {
        kind: FstNodeType::Directory,
        ..Default::default()
    };

    // Placeholder FST entry for the root
    let mut output_fst = vec![root_fst];
    let mut fst_name_bank = Vec::new();

    for (_, node) in root
        .children
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != sys_index)
    {
        do_output_prep(node, &mut output_fst, &mut fst_name_bank, &mut writer, 0)?;
    }

    let mut data_end = writer.seek(SeekFrom::Current(0))? as usize;
    let size_mod = (data_end - part_data_offset) % 0x1f0000;
    data_end = data_end + if size_mod != 0 {0x1f0000 - size_mod} else {0};

    // Add actual root FST entry
    output_fst[0].file_size_next_dir_index = output_fst.len();

    writer.seek(SeekFrom::Start((part_data_offset + fst_list_offset) as u64))?;

    for entry in &output_fst {
        writer.write_u8(entry.kind as u8)?;
        writer.write_u8(0)?;
        writer.write_u16::<BE>(entry.file_name_offset as u16)?;
        writer.write_i32::<BE>((entry.file_offset_parent_dir >> if is_wii {2} else {0}) as i32)?;
        writer.write_i32::<BE>(entry.file_size_next_dir_index as i32)?;
    }

    writer.write_all(&fst_name_bank)?;

    writer.seek(SeekFrom::Start((part_data_offset + OFFSET_DOL_OFFSET) as u64))?;
    writer.write_u32::<BE>((dol_offset >> if is_wii {2} else {0}) as u32)?;
    writer.write_u32::<BE>((fst_list_offset >> if is_wii {2} else {0}) as u32)?;
    writer.write_u32::<BE>(fst_len as u32)?;
    writer.write_u32::<BE>(fst_len as u32)?;

    // encrypt the file
    if let Some(part_header) = part_header_opt {
        writer.flush()?;
        let n_sectors = 900 * 64;
        let n_clusters = n_sectors / 8;
        let n_groups = n_clusters / 8;
        println!("part offset: {:#010X}; sectors: {}; clusters: {}; groups: {}", part_data_offset, n_sectors, n_clusters, n_groups);
        let mut hasher = Sha1::new();
        // h0
        // We also take this opportunity to spread the data to make space for the hashes
        for i in (0 .. n_sectors).rev() {
            let mut buf = [0u8; 0x7c00];
            let mut hash = [0u8; 0x400];
            print!("{} \r", i);
            std::io::stdout().flush().unwrap();
            reader.seek(SeekFrom::Start((part_data_offset + i * 0x7c00) as u64))?;
            writer.seek(SeekFrom::Start((part_data_offset + i * 0x8000) as u64))?;
            reader.read_exact(&mut buf)?;
            for j in 0 .. 31 {
                hasher.update(&buf[j * 0x400 .. (j + 1) * 0x400]);
                hash[j * 20 .. (j + 1) * 20].copy_from_slice(&hasher.digest().bytes()[..]);
            }
            writer.write_all(&hash)?;
            writer.write_all(&buf)?;
        }
        writer.flush()?;
        println!("");
        // h1
        for i in 0 .. n_clusters {
            let mut buf = [0u8; 0x26c];
            let mut hash = [0u8; 0x0a0];
            for j in 0 .. 8 {
                reader.seek(SeekFrom::Start((part_data_offset + (i * 8 + j) * 0x8000) as u64))?;
                reader.read_exact(&mut buf)?;
                hasher.update(&buf);
                hash[j * 20 .. (j + 1) * 20].copy_from_slice(&hasher.digest().bytes()[..]);
            }
            for j in 0 .. 8 {
                writer.seek(SeekFrom::Start((part_data_offset + (i * 8 + j) * 0x8000 + 0x280) as u64))?;
                writer.write_all(&hash)?;
            }
        }
        writer.flush()?;
        // h2
        for i in 0 .. n_groups {
            let mut buf = [0u8; 0x0A0];
            let mut hash = [0u8; 0x0a0];
            for j in 0 .. 8 {
                reader.seek(SeekFrom::Start((part_data_offset + (i * 64 + j * 8) * 0x8000 + 0x280) as u64))?;
                reader.read_exact(&mut buf)?;
                hasher.update(&buf);
                hash[j * 20 .. (j + 1) * 20].copy_from_slice(&hasher.digest().bytes()[..]);
            }
            for j in 0 .. 64 {
                writer.seek(SeekFrom::Start((part_data_offset + (i * 64 + j) * 0x8000 + 0x340) as u64))?;
                writer.write_all(&hash)?;
            }
        }
        writer.flush()?;
        // h3
        let h3_offset = part_offset + part_header.h3_offset;
        {
            let mut buf = [0u8; 0x0A0];
            let mut hash = vec![0u8; n_groups * 20];
            writer.seek(SeekFrom::Start((h3_offset) as u64))?;
            for i in 0 .. n_groups {
                reader.seek(SeekFrom::Start((part_data_offset + (i * 64) * 0x8000 + 0x340) as u64))?;
                reader.read_exact(&mut buf)?;
                hasher.update(&buf);
                hash[i * 20 .. (i + 1) * 20].copy_from_slice(&hasher.digest().bytes()[..]);
            }
            writer.write_all(&hash)?;
        }
        writer.seek(SeekFrom::Start((0x60) as u64))?;
        writer.write_all(&[1u8, 0])?;

        // set partition data size
        writer.seek(SeekFrom::Start(part_offset as u64))?;
        let mut part_header = part_header;
        part_header.data_size = ((data_end - part_data_offset) / 0x7c00 * 0x8000) >> 2;
        part_header.ticket.sig.copy_from_slice(&[0u8; 0x100]);
        writer.write_all(&<[u8; 0x2C0]>::try_from(&part_header)?)?;

        // encrypt everything
        let part_key = decrypt_title_key(&part_header.ticket);
        for i in 0 .. n_sectors {
            let mut hash = [0u8; 0x400];
            let mut buf = [0u8; 0x7c00];
            reader.seek(SeekFrom::Start((part_data_offset + i * 0x8000) as u64))?;
            writer.seek(SeekFrom::Start((part_data_offset + i * 0x8000) as u64))?;
            reader.read_exact(&mut hash)?;
            reader.read_exact(&mut buf)?;
            let mut iv = [0 as u8; 16];
            aes_encrypt_inplace(&mut hash, &iv, &part_key, 0x400)?;
            iv[..16].copy_from_slice(&hash[0x3D0..][..16]);
            aes_encrypt_inplace(&mut buf, &iv, &part_key, 0x7c00)?;
            writer.write_all(&hash)?;
            writer.write_all(&buf)?;
        }
    }

    Ok(())
}

fn calculate_fst_len(mut cur_value: usize, node: &Node) -> usize {
    match *node {
        Node::Directory(ref dir) => {
            cur_value += 12 + dir.name.len() + 1;

            for child in &dir.children {
                cur_value = calculate_fst_len(cur_value, child);
            }
        }
        Node::File(ref file) => {
            cur_value += 12 + file.name.len() + 1;
        }
    }
    cur_value
}

fn do_output_prep<W>(
    node: &Node,
    output_fst: &mut Vec<FstEntry>,
    fst_name_bank: &mut Vec<u8>,
    writer: &mut W,
    mut cur_parent_dir_index: usize,
) -> Result<(), Error>
where
    W: Write + Seek,
{
    match *node {
        Node::Directory(ref dir) => {
            let fst_ent = FstEntry {
                kind: FstNodeType::Directory,
                file_name_offset: fst_name_bank.len(),
                file_offset_parent_dir: cur_parent_dir_index,
                ..Default::default()
            };

            fst_name_bank.extend_from_slice(dir.name.as_bytes());
            fst_name_bank.push(0);

            cur_parent_dir_index = output_fst.len();

            let this_dir_index = cur_parent_dir_index;

            output_fst.push(fst_ent); // Placeholder for this dir

            for child in &dir.children {
                do_output_prep(
                    child,
                    output_fst,
                    fst_name_bank,
                    writer,
                    cur_parent_dir_index,
                )?;
            }

            let dir_end_index = output_fst.len();
            output_fst[this_dir_index].file_size_next_dir_index = dir_end_index;
        }
        Node::File(ref file) => {
            let mut fst_ent = FstEntry {
                kind: FstNodeType::File,
                file_size_next_dir_index: file.data.len(),
                file_name_offset: fst_name_bank.len(),
                ..Default::default()
            };

            fst_name_bank.extend_from_slice(file.name.as_bytes());
            fst_name_bank.push(0);

            let pos = writer.seek(SeekFrom::Current(0))?;
            let new_pos = pos + (32 - (pos % 32)) % 32;
            writer.seek(SeekFrom::Start(new_pos))?;

            fst_ent.file_offset_parent_dir = new_pos as usize;

            writer.write_all(&file.data)?;

            for _ in 0..(32 - (file.data.len() % 32)) % 32 {
                writer.write_all(&[0])?;
            }

            output_fst.push(fst_ent);
        }
    }

    Ok(())
}

fn fetch_file<'a, 'b>(dir: &'b Directory<'a>, name: String) -> Result<&'b File<'a>, failure::Error> {
    dir.children
       .iter()
       .filter_map(|c| c.as_file())
       .find(|f| f.name == name)
       .ok_or_else(|| err_msg(format!("The {} folder contains no {}", dir.name, name)))
}