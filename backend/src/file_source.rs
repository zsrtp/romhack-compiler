use failure::{err_msg, Error};
use image::{self, DynamicImage};
use std::fs;
use std::io::prelude::*;
use std::path::Path;
use zip::ZipArchive;

pub trait FileSource {
    fn read_to_vec<P: AsRef<Path>>(&mut self, path: P) -> Result<Vec<u8>, Error>;
    fn read_to_string<P: AsRef<Path>>(&mut self, path: P) -> Result<String, Error>;
    fn open_image<P: AsRef<Path>>(&mut self, path: P) -> Result<DynamicImage, Error>;
    fn exist<P: AsRef<Path>>(&mut self, path: P) -> Result<bool, Error>;
    fn is_dir<P: AsRef<Path>>(&mut self, path: P) -> Result<bool, Error>;
    fn is_file<P: AsRef<Path>>(&mut self, path: P) -> Result<bool, Error>;
    fn get_names<P: AsRef<Path>>(&mut self, path: P) -> Result<Vec<String>, Error>;
}

pub struct FileSystem;

impl FileSource for FileSystem {
    fn read_to_vec<P: AsRef<Path>>(&mut self, path: P) -> Result<Vec<u8>, Error> {
        Ok(fs::read(path)?)
    }
    fn read_to_string<P: AsRef<Path>>(&mut self, path: P) -> Result<String, Error> {
        Ok(fs::read_to_string(path)?)
    }
    fn open_image<P: AsRef<Path>>(&mut self, path: P) -> Result<DynamicImage, Error> {
        Ok(image::open(path)?)
    }
    fn exist<P: AsRef<Path>>(&mut self, path: P) -> Result<bool, Error> {
        Ok(path.as_ref().exists())
    }
    fn is_dir<P: AsRef<Path>>(&mut self, path: P) -> Result<bool, Error> {
        Ok(path.as_ref().is_dir())
    }
    fn is_file<P: AsRef<Path>>(&mut self, path: P) -> Result<bool, Error> {
        Ok(path.as_ref().is_file())
    }
    fn get_names<P: AsRef<Path>>(&mut self, path: P) -> Result<Vec<String>, Error> {
        let mut names: Vec<String> = Vec::new();
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let entry_path = entry.path();
            let file_name = entry_path.file_name().expect("Entry has no name");
            names.push(String::from(file_name.to_str().unwrap()));
        }
        Ok(names)
    }
}

impl<R: Read + Seek> FileSource for ZipArchive<R> {
    fn read_to_vec<P: AsRef<Path>>(&mut self, path: P) -> Result<Vec<u8>, Error> {
        let mut file = self.by_name(
            path.as_ref()
                .to_str()
                .ok_or_else(|| err_msg("Invalid path"))?,
        )?;
        let mut buf = Vec::with_capacity(file.size() as usize + 1);
        file.read_to_end(&mut buf)?;
        Ok(buf)
    }
    fn read_to_string<P: AsRef<Path>>(&mut self, path: P) -> Result<String, Error> {
        let mut file = self.by_name(
            path.as_ref()
                .to_str()
                .ok_or_else(|| err_msg("Invalid path"))?,
        )?;
        let mut buf = String::with_capacity(file.size() as usize + 1);
        file.read_to_string(&mut buf)?;
        Ok(buf)
    }
    fn open_image<P: AsRef<Path>>(&mut self, path: P) -> Result<DynamicImage, Error> {
        let buf = self.read_to_vec(path)?;
        Ok(image::load_from_memory(&buf)?)
    }
    fn exist<P: AsRef<Path>>(&mut self, path: P) -> Result<bool, Error> {
        Ok(self.by_name(
            path.as_ref()
                .to_str()
                .ok_or_else(|| err_msg("Invalid path"))?
        ).is_ok())
    }
    fn is_dir<P: AsRef<Path>>(&mut self, path: P) -> Result<bool, Error> {
        let file = self.by_name(
            path.as_ref()
                .to_str()
                .ok_or_else(|| err_msg("Invalid path"))?
        )?;
        Ok(file.is_dir())
    }
    fn is_file<P: AsRef<Path>>(&mut self, path: P) -> Result<bool, Error> {
        let file = self.by_name(
            path.as_ref()
                .to_str()
                .ok_or_else(|| err_msg("Invalid path"))?
        )?;
        Ok(file.is_file())
    }
    fn get_names<P: AsRef<Path>>(&mut self, _path: P) -> Result<Vec<String>, Error> {
        Err(failure::err_msg("Unsupported operation on ZipArchive: get_names"))
    }
}
