use std::borrow::Cow;
use std::cmp;

#[derive(Debug)]
pub enum Node<'a> {
    Directory(Box<Directory<'a>>),
    File(File<'a>),
}

impl<'a> Node<'a> {
    pub fn name(&self) -> &String {
        match self {
            Node::Directory(dir) => &dir.name,
            Node::File(file) => &file.name,
        }
    }

    pub fn as_directory(&self) -> Option<&Directory<'a>> {
        if let &Node::Directory(ref dir) = self {
            Some(dir)
        } else {
            None
        }
    }

    pub fn as_directory_mut(&mut self) -> Option<&mut Directory<'a>> {
        if let &mut Node::Directory(ref mut dir) = self {
            Some(dir)
        } else {
            None
        }
    }

    pub fn as_file(&self) -> Option<&File<'a>> {
        if let &Node::File(ref file) = self {
            Some(file)
        } else {
            None
        }
    }

    pub fn as_file_mut(&mut self) -> Option<&mut File<'a>> {
        if let &mut Node::File(ref mut file) = self {
            Some(file)
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub struct Directory<'a> {
    pub name: String,
    pub children: Vec<Node<'a>>,
}

impl<'a> Default for Directory<'a> {
    fn default() -> Self {
        Directory::new("")
    }
}

impl<'a> Directory<'a> {
    pub fn new(name: &str) -> Directory<'a> {
        Self {
            name: String::from(name),
            children: Vec::new(),
        }
    }

    pub fn main_dol_mut(&mut self) -> Option<&mut File<'a>> {
        let sys_dir = self
            .children
            .iter_mut()
            .filter_map(|c| c.as_directory_mut())
            .find(|d| d.name == "&&systemdata")?;
        let dol = sys_dir
            .children
            .iter_mut()
            .filter_map(|c| c.as_file_mut())
            .find(|f| f.name.ends_with(".dol"))?;
        Some(dol)
    }

    pub fn banner_mut(&mut self) -> Option<&mut File<'a>> {
        let banner = self
            .children
            .iter_mut()
            .filter_map(|c| c.as_file_mut())
            .find(|f| f.name == "opening.bnr")?;
        Some(banner)
    }

    pub fn is_gamecube_iso(&self) -> bool {
        if let Some(file) = self.resolve_path("&&systemdata/iso.hdr") {
            let magic: Vec<u8> = file.data[0x1C..0x20].to_owned();
            let gc_magic: Vec<u8> = vec![0xc2, 0x33, 0x9f, 0x3d];
            cmp::Ordering::Equal == magic.iter()
                .zip(gc_magic)
                .map(|(x, y)| x.cmp(&y))
                .find(|&ord| ord != cmp::Ordering::Equal)
                .unwrap_or(magic.len().cmp(&4))
        } else {
            false
        }
    }

    pub fn is_wii_iso(&self) -> bool {
        if let Some(file) = self.resolve_path("&&systemdata/iso.hdr") {
            let magic: Vec<u8> = file.data[0x18..0x1C].to_owned();
            let wii_magic: Vec<u8> = vec![0x5D, 0x1C, 0x9E, 0xA3];
            cmp::Ordering::Equal == magic.iter()
                .zip(wii_magic)
                .map(|(x, y)| x.cmp(&y))
                .find(|&ord| ord != cmp::Ordering::Equal)
                .unwrap_or(magic.len().cmp(&4))
        } else {
            false
        }
    }

    pub fn resolve_path(&self, path: &str) -> Option<&File<'a>> {
        let mut dir = self;
        let mut segments = path.split('/').peekable();

        while let Some(segment) = segments.next() {
            if segments.peek().is_some() {
                // Must be a folder
                dir = dir
                    .children
                    .iter()
                    .filter_map(|c| c.as_directory())
                    .find(|d| d.name == segment)?;
            } else {
                return dir
                    .children
                    .iter()
                    .filter_map(|c| c.as_file())
                    .find(|f| f.name == segment);
            }
        }
        None
    }

    // TODO NLL This is really bad
    pub fn resolve_and_create_path(&mut self, path: &String) -> &mut File<'a> {
        let mut splits = path.splitn(2, '/');
        if let (Some(folder), Some(sub_path)) = (splits.next(), splits.next()) {
            if !self
                .children
                .iter_mut()
                .filter_map(|c| c.as_directory_mut())
                .any(|d| d.name == folder)
            {
                self.children
                    .push(Node::Directory(Box::new(Directory::new(folder))));
            }
            self.children
                .iter_mut()
                .filter_map(|c| c.as_directory_mut())
                .find(|d| d.name == folder)
                .unwrap()
                .resolve_and_create_path(&String::from(sub_path))
        } else {
            if !self
                .children
                .iter_mut()
                .filter_map(|c| c.as_file_mut())
                .any(|f| f.name == *path)
            {
                self.children.push(Node::File(File::new(path, Vec::new())));
            }
            self.children
                .iter_mut()
                .filter_map(|c| c.as_file_mut())
                .find(|f| f.name == *path)
                .unwrap()
        }
    }
}

pub struct File<'a> {
    pub name: String,
    pub data: Cow<'a, [u8]>,
}

impl<'a> File<'a> {
    pub fn new<A: Into<Cow<'a, [u8]>>>(name: &str, data: A) -> File<'a> {
        Self {
            name: String::from(name),
            data: data.into(),
        }
    }
}

use std::fmt;

impl<'a> fmt::Debug for File<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

// use std::path::Path;
// use std::io::{Read, Result};
// use std::fs::{read_dir, self};

// pub fn import_from_disk<P: AsRef<Path>>(path: P) -> Result<Directory> {
//     let path = path.as_ref();
//     let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();

//     let mut directory = Directory {
//         id: 0,
//         name,
//         children: Vec::new(),
//     };

//     for entry in read_dir(path)? {
//         let entry = entry?;
//         let file_type = entry.file_type()?;
//         if file_type.is_dir() {
//             let child = import_from_disk(entry.path())?;
//             directory.children.push(Node::Directory(Box::new(child)));
//         } else {
//             let name = entry.file_name().to_string_lossy().into_owned();

//             let mut file = fs::File::open(entry.path())?;
//             let len = file.metadata()?.len();
//             let mut data = Vec::with_capacity(len as usize + 1);
//             file.read_to_end(&mut data)?;

//             directory.children.push(Node::File(File {
//                 id: 0,
//                 name,
//                 data,
//             }));
//         }
//     }

//     Ok(directory)
// }
