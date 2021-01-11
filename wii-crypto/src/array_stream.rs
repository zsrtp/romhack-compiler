use std::io::{Write, Seek, Read, SeekFrom, Result as IOResult, Error as IOError, ErrorKind};

pub trait SliceStream {
    fn get_cursor(&self) -> u64;
    fn set_cursor(&mut self, new_cursor: u64);
    fn get_len(&self) -> usize;
}

impl Seek for dyn SliceStream {
    fn seek(&mut self, pos: SeekFrom) -> IOResult<u64> {
        match pos {
            SeekFrom::Current(offset) => {
                if self.get_cursor() as i64 + offset < 0 {
                    return Err(IOError::from(ErrorKind::InvalidInput));
                }
                self.set_cursor((self.get_cursor() as i64 + if (self.get_cursor() as i64 + offset) as usize > self.get_len() {self.get_len() as i64 - self.get_cursor() as i64} else {offset}) as u64);
                Ok(self.get_cursor())
            },
            SeekFrom::Start(offset) => {
                self.set_cursor(if offset as usize > self.get_len() {self.get_len() as u64} else {offset});
                Ok(self.get_cursor())
            },
            SeekFrom::End(offset) => {
                if offset + (self.get_len() as i64) < 0 {
                    return Err(IOError::from(ErrorKind::InvalidInput));
                }
                self.set_cursor(if offset < 0 {(self.get_len() as i64 + offset) as u64} else {self.get_len() as u64});
                Ok(self.get_cursor())
            },
        }
    }
}

pub struct SliceWriter<'a> {
    buffer: &'a mut[u8],
    cursor: u64,
}

impl<'a> SliceWriter<'a> {
    pub fn new(buf: &'a mut[u8]) -> SliceWriter<'a> {
        SliceWriter {buffer: buf, cursor: 0}
    }
}

impl<'a> SliceStream for SliceWriter<'a> {
    fn get_cursor(&self) -> u64 {
        return self.cursor;
    }
    fn set_cursor(&mut self, new_cursor: u64) {
        self.cursor = new_cursor;
    }
    fn get_len(&self) -> usize {
        return self.buffer.len();
    }
}

impl<'a> Write for SliceWriter<'a> {
    fn write(&mut self, buf: &[u8]) -> IOResult<usize> {
        if self.cursor as usize > self.buffer.len() {
            self.cursor = self.buffer.len() as u64;
            return Ok(0);
        }
        let len_to_write = std::cmp::min(buf.len(), self.buffer.len() - self.cursor as usize);
        self.buffer[self.cursor as usize ..][.. len_to_write].copy_from_slice(&buf[.. len_to_write]);
        self.cursor += len_to_write as u64;
        Ok(len_to_write)
    }

    fn flush(&mut self) -> IOResult<()> {
        if self.cursor as usize > self.buffer.len() {
            self.cursor = self.buffer.len() as u64;
        }
        Ok(())
    }
}

pub struct SliceReader<'a> {
    buffer: &'a[u8],
    cursor: u64,
}

impl<'a> SliceReader<'a> {
    pub fn new(buf: &'a[u8]) -> SliceReader<'a> {
        SliceReader {buffer: buf, cursor: 0}
    }
}

impl<'a> SliceStream for SliceReader<'a> {
    fn get_cursor(&self) -> u64 {
        return self.cursor;
    }
    fn set_cursor(&mut self, new_cursor: u64) {
        self.cursor = new_cursor;
    }
    fn get_len(&self) -> usize {
        return self.buffer.len();
    }
}

impl<'a> Read for SliceReader<'a> {
    fn read(&mut self, buf: &mut [u8]) -> IOResult<usize> {
        if self.cursor as usize > self.buffer.len() {
            self.cursor = self.buffer.len() as u64;
            return Ok(0);
        }
        let len_to_read = std::cmp::min(buf.len(), self.buffer.len() - self.cursor as usize);
        buf[.. len_to_read].copy_from_slice(&self.buffer[self.cursor as usize ..][.. len_to_read]);
        self.cursor += len_to_read as u64;
        Ok(len_to_read)
    }
}

#[derive(Clone, Debug)]
pub struct VecWriter {
    buffer: Vec<u8>,
    cursor: usize,
}

impl VecWriter {
    pub const fn new() -> VecWriter {
        VecWriter { buffer: Vec::new(), cursor: 0 }
    }

    pub fn with_capacity(capacity: usize) -> VecWriter {
        VecWriter { buffer: Vec::with_capacity(capacity), cursor: 0 }
    }

    pub fn as_slice<'a> (&'a self) -> &'a[u8] {
        &self.buffer[..]
    }

    pub fn as_slice_mut<'a> (&'a mut self) -> &'a mut [u8] {
        &mut self.buffer[..]
    }
}

impl Seek for VecWriter {
    fn seek(&mut self, pos: SeekFrom) -> IOResult<u64> {
        match pos {
            SeekFrom::Current(offset) => {
                if self.cursor as i64 + offset < 0 {
                    return Err(IOError::from(ErrorKind::InvalidInput));
                }
                let new_pos = (self.cursor as i64 + offset) as usize;
                if new_pos > self.buffer.len() {
                    self.buffer.resize(new_pos, 0);
                }
                self.cursor = new_pos;
                Ok(self.cursor as u64)
            },
            SeekFrom::Start(offset) => {
                let offset = offset as usize;
                if offset > self.buffer.len() {
                    self.buffer.resize(offset, 0);
                }
                self.cursor = offset;
                Ok(self.cursor as u64)
            },
            SeekFrom::End(offset) => {
                if offset + (self.buffer.len() as i64) < 0 {
                    return Err(IOError::from(ErrorKind::InvalidInput));
                }
                let new_pos = (self.buffer.len() as i64 + offset) as usize;
                if new_pos > self.buffer.len() {
                    self.buffer.resize(new_pos, 0);
                }
                self.cursor = new_pos;
                Ok(self.cursor as u64)
            },
        }
    }
}

impl Write for VecWriter {
    fn write(&mut self, buf: &[u8]) -> IOResult<usize> {
        if self.cursor + buf.len() > self.buffer.len() {
            self.buffer.resize(self.cursor + buf.len(), 0);
        }
        let len_to_write = std::cmp::min(buf.len(), self.buffer.len() - self.cursor as usize);
        self.buffer[self.cursor as usize ..][.. len_to_write].copy_from_slice(&buf[.. len_to_write]);
        self.cursor += len_to_write;
        Ok(len_to_write)
    }

    fn flush(&mut self) -> IOResult<()> {
        Ok(())
    }
}