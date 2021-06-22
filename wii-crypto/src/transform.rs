use crate::consts;
use crate::wii_disc::DecryptedBlock;
use crate::wii_disc::*;
use async_recursion::async_recursion;
use byteorder::{ByteOrder, BE};
use const_format::formatcp;
use failure::Fail;
use failure::{Error, ResultExt};
use futures::lock::Mutex;
use futures::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;
use sha1::Sha1;
use std::collections::HashMap;
use std::future::Future;
use std::io::{Error as IOError, ErrorKind, Result as IOResult, SeekFrom};
use std::pin::Pin;
use std::result::Result;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
// use tokio::io::{AsyncBufRead, AsyncRead, AsyncReadExt, AsyncSeekExt, ReadBuf};
// use tokio::sync::Mutex;

type SharedMutex<T> = Arc<Mutex<T>>;

/// Empty reader, should only be used when reading a GC disc as a placeholder for the reader type of the struct Disc.
#[derive(Copy, Clone)]
pub struct Empty {
    _priv: (),
}

pub const fn empty() -> Empty {
    Empty { _priv: () }
}

impl Unpin for Empty {}

impl AsyncRead for Empty {
    #[inline]
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut [u8],
    ) -> Poll<IOResult<usize>> {
        Poll::Ready(Ok(0))
    }
}

impl AsyncSeek for Empty {
    fn poll_seek(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _pos: SeekFrom,
    ) -> Poll<IOResult<u64>> {
        Poll::Ready(Ok(0))
    }
}

impl AsyncBufRead for Empty {
    #[inline]
    fn poll_fill_buf(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<IOResult<&[u8]>> {
        Poll::Ready(Ok(&[]))
    }
    #[inline]
    fn consume(self: Pin<&mut Self>, _n: usize) {}
}

impl std::fmt::Debug for Empty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad("Empty { .. }")
    }
}

impl Default for Empty {
    fn default() -> Self {
        empty()
    }
}

pub(crate) enum AutoReader<R: AsyncRead + Send = Empty> {
    Copy(AutoReaderStatus, SharedMutex<R>),
    #[allow(dead_code)]
    Read(AutoReaderStatus, SharedMutex<R>, SharedMutex<Vec<u8>>),
    Write(AutoReaderStatus, SharedMutex<Box<[u8]>>),
}

pub(crate) struct AutoReaderStatus {
    state: AutoReaderState,
    start_offset: u64,
    end_offset: u64,
    cursor: u64,
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum AutoReaderState {
    Copy,
    #[allow(dead_code)]
    Read,
    Write,
    Done,
}

#[derive(Debug)]
pub(crate) enum AutoReaderError {
    InvalidState(&'static str),
    IOError(IOError),
}

impl std::fmt::Display for AutoReaderError {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AutoReaderError::InvalidState(detail) => {
                write!(fmt, "AutoReader Error: Invalid State: {}", detail)
            }
            AutoReaderError::IOError(ioerr) => {
                write!(fmt, "AutoReader I/O Error: {}", ioerr)
            }
        }
    }
}

impl Fail for AutoReaderError {
    fn cause(&self) -> Option<&(dyn Fail)> {
        match self {
            AutoReaderError::InvalidState(..) => None,
            AutoReaderError::IOError(ioerr) => Some(ioerr),
        }
    }
}

impl<R: AsyncRead + Send + Unpin + 'static> AutoReader<R> {
    pub(crate) fn new_copy(start: u64, end: u64, reader: SharedMutex<R>) -> Self {
        AutoReader::Copy(
            AutoReaderStatus {
                state: AutoReaderState::Copy,
                start_offset: start,
                end_offset: end,
                cursor: start,
            },
            reader,
        )
    }

    #[allow(dead_code)]
    pub(crate) fn new_read(
        start: u64,
        end: u64,
        reader: SharedMutex<R>,
        read_buf: SharedMutex<Vec<u8>>,
    ) -> Self {
        AutoReader::Read(
            AutoReaderStatus {
                state: AutoReaderState::Read,
                start_offset: start,
                end_offset: end,
                cursor: start,
            },
            reader,
            read_buf,
        )
    }

    pub(crate) fn new_write(start: u64, end: u64, write_buf: SharedMutex<Box<[u8]>>) -> Self {
        AutoReader::Write(
            AutoReaderStatus {
                state: AutoReaderState::Write,
                start_offset: start,
                end_offset: end,
                cursor: start,
            },
            write_buf,
        )
    }

    pub(crate) fn status<'a>(self: &'a mut Self) -> &'a mut AutoReaderStatus {
        match self {
            AutoReader::Copy(status, _) => status,
            AutoReader::Read(status, _, _) => status,
            AutoReader::Write(status, _) => status,
        }
    }

    pub(crate) async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        if buf.len() == 0 {
            return Ok(0);
        }
        match self {
            AutoReader::Copy(status, reader) => {
                let cursor: u64;
                let end_offset;
                {
                    cursor = status.cursor;
                    end_offset = status.end_offset;
                }
                if cursor > end_offset {
                    return Err(AutoReaderError::InvalidState(formatcp!(
                        "Invalid offset; cursor ahead of protocol [algorithm error] {}:{}",
                        file!(),
                        line!()
                    ))
                    .into());
                }
                if cursor == end_offset {
                    status.state = AutoReaderState::Done;
                    return Ok(0);
                }
                let req_read_size = std::cmp::min(buf.len(), (end_offset - cursor) as usize);
                let read_size;
                {
                    let mut b = vec![0u8; req_read_size];
                    {
                        let mut r = reader.lock().await;
                        read_size = r.read(&mut b).await?;
                    }
                    buf[..read_size].copy_from_slice(&b[..read_size]);
                }
                if read_size == 0 {
                    return Err(AutoReaderError::IOError(IOError::new(
                        ErrorKind::UnexpectedEof,
                        "Source is to small to be decrypted (Copy)",
                    ))
                    .into());
                }
                {
                    status.cursor += read_size as u64;
                    if status.cursor == end_offset {
                        status.state = AutoReaderState::Done;
                    }
                }
                return Ok(read_size);
            }
            AutoReader::Read(status, reader, read_buf) => {
                let cursor: u64;
                let buf_len: u64;
                let end_offset;
                {
                    let b = read_buf.lock().await;
                    cursor = status.cursor;
                    buf_len = b.len() as u64;
                    end_offset = status.end_offset;
                }
                if cursor + buf_len > end_offset {
                    return Err(AutoReaderError::InvalidState(formatcp!(
                        "Invalid offset; cursor ahead of protocol [algorithm error] {}:{}",
                        file!(),
                        line!()
                    ))
                    .into());
                }
                if cursor + buf_len == end_offset {
                    {
                        status.cursor += buf_len;
                    }
                    status.state = AutoReaderState::Done;
                    return Ok(0);
                }
                let req_read_size =
                    std::cmp::min(buf.len(), (end_offset - (cursor + buf_len)) as usize);
                let mut buf2 = vec![0u8; req_read_size];
                let read_size;
                {
                    let mut r = reader.lock().await;
                    read_size = r.read(&mut buf2).await?;
                }
                buf[..read_size].copy_from_slice(&buf2[..read_size]);
                if read_size == 0 {
                    return Err(AutoReaderError::IOError(IOError::new(
                        ErrorKind::UnexpectedEof,
                        "Source is to small to be decrypted (Read)",
                    ))
                    .into());
                }
                {
                    let mut b = read_buf.lock().await;
                    b.extend(&buf2[..read_size]);
                    if cursor + b.len() as u64 == end_offset {
                        {
                            status.cursor += b.len() as u64;
                        }
                        status.state = AutoReaderState::Done;
                    }
                }
                Ok(read_size)
            }
            AutoReader::Write(status, write_buf) => {
                let mut cursor: u64;
                let write_buf_rem: u64;
                let end_offset;
                {
                    let b = write_buf.lock().await;
                    cursor = status.cursor;
                    write_buf_rem = b.len() as u64 - (cursor - status.start_offset);
                    end_offset = status.end_offset;
                }
                if cursor > end_offset {
                    return Err(AutoReaderError::InvalidState(formatcp!(
                        "Invalid offset; cursor ahead of protocol [algorithm error] {}:{}",
                        file!(),
                        line!()
                    ))
                    .into());
                }
                if write_buf_rem == 0 {
                    return Err(AutoReaderError::InvalidState(formatcp!(
                        "Invalid buffer size; nothing to write [algorithm error] {}:{}",
                        file!(),
                        line!()
                    ))
                    .into());
                }
                let read_size = std::cmp::min(buf.len(), write_buf_rem as usize);
                if read_size == 0 {
                    return Err(AutoReaderError::IOError(IOError::new(
                        ErrorKind::UnexpectedEof,
                        "Source is to small to be decrypted (Write)",
                    ))
                    .into());
                }
                {
                    let b = write_buf.lock().await;
                    buf[..read_size].copy_from_slice(
                        &b[(cursor - status.start_offset) as usize..][..read_size],
                    );
                    status.cursor += read_size as u64;
                    cursor = status.cursor;
                }
                if cursor == end_offset {
                    status.state = AutoReaderState::Done;
                }
                Ok(read_size)
            }
        }
    }
}

struct BPLState<R: AsyncRead + AsyncSeek + Send> {
    reader: R,
    part_data_offset: u64,
    part_key: AesKey,
    buffers: HashMap<usize, DecryptedBlock>,
}

struct BufferedPartitionLoader<'a, R: AsyncRead + AsyncSeek + Send + 'a> {
    cursor: u64,
    decrypted_size: u64,
    state: SharedMutex<BPLState<R>>,
    load_handle: Option<Pin<Box<(dyn Future<Output = IOResult<()>> + 'a)>>>,
    waker: SharedMutex<Option<Waker>>,
}

impl<'a, R: AsyncRead + AsyncSeek + Send + Unpin + 'a> BufferedPartitionLoader<'a, R> {
    pub fn new(reader: R, offset: u64, raw_size: u64, part_key: AesKey) -> Self {
        BufferedPartitionLoader {
            cursor: 0,
            decrypted_size: (raw_size / 0x8000) * 0x7C00,
            state: Arc::from(Mutex::new(BPLState {
                reader,
                part_data_offset: offset,
                part_key: part_key,
                buffers: HashMap::<usize, DecryptedBlock>::new(),
            })),
            load_handle: None,
            waker: Arc::from(Mutex::new(None)),
        }
    }

    async fn load_blocks(
        state: SharedMutex<BPLState<R>>,
        mut r: std::ops::RangeInclusive<usize>,
    ) -> IOResult<()> {
        let mut s = state.lock().await;
        let start_idx = *r.start();
        let end_idx = *r.end();
        if !r.any(|idx| !s.buffers.contains_key(&idx)) {
            return Ok(());
        }
        const STEP: usize = 64 * 8;
        let part_key = s.part_key;
        let part_data_offset = s.part_data_offset;
        s.reader
            .seek(SeekFrom::Start(
                part_data_offset + start_idx as u64 * 0x8000,
            ))
            .await?;
        for i in 0..=(end_idx - start_idx) / STEP {
            let n = std::cmp::min(STEP, end_idx - start_idx + 1 - i * STEP);
            let mut data_pool: Vec<(&mut [u8], bool)> = Vec::with_capacity(n);
            let mut buf = vec![0u8; n * 0x8000];
            s.reader.read_exact(&mut buf[..]).await?;
            let (_, mut data_slice) = buf.split_at_mut(0);
            for j in 0..n {
                let (section, new_data_slice) = data_slice.split_at_mut(consts::WII_SECTOR_SIZE);
                data_slice = new_data_slice;
                data_pool.push((
                    section,
                    !s.buffers.contains_key(&(start_idx + i * STEP + j)),
                ));
            }
            let decrypt_process = move |(data, decrypt): &mut (&mut [u8], bool)| {
                if *decrypt {
                    let mut iv = [0 as u8; consts::WII_KEY_SIZE];
                    iv[..consts::WII_KEY_SIZE].copy_from_slice(
                        &data[consts::WII_SECTOR_IV_OFF..][..consts::WII_KEY_SIZE],
                    );
                    // Decrypt the hash to check if valid (not required here)
                    // aes_decrypt_inplace(&mut data[..consts::WII_SECTOR_HASH_SIZE], &[0 as u8; consts::WII_KEY_SIZE], &part_key).unwrap();
                    aes_decrypt_inplace(
                        &mut data[consts::WII_SECTOR_HASH_SIZE..][..consts::WII_SECTOR_DATA_SIZE],
                        &iv,
                        &part_key,
                    )
                    .unwrap();
                }
            };
            #[cfg(not(target_arch = "wasm32"))]
            data_pool.par_iter_mut().for_each(decrypt_process);
            #[cfg(target_arch = "wasm32")]
            data_pool.iter_mut().for_each(decrypt_process);
            for j in 0..n {
                if !s.buffers.contains_key(&(start_idx + i * STEP + j)) {
                    s.buffers.insert(
                        i * STEP + j + start_idx,
                        DecryptedBlock::from(
                            &buf[j * consts::WII_SECTOR_SIZE + consts::WII_SECTOR_HASH_SIZE
                                ..(j + 1) * consts::WII_SECTOR_SIZE],
                        ),
                    );
                }
            }
        }
        Ok(())
    }
}

impl<R: AsyncRead + AsyncSeek + Send> AsyncSeek for BufferedPartitionLoader<'_, R> {
    fn poll_seek(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        s: SeekFrom,
    ) -> Poll<Result<u64, futures::io::Error>> {
        Poll::Ready(match s {
            SeekFrom::Start(offset) => {
                self.cursor = offset;
                Ok(self.cursor)
            }
            SeekFrom::Current(offset) => {
                if self.cursor as i64 >= -offset {
                    self.cursor = (self.cursor as i64 + offset) as u64;
                    Ok(self.cursor)
                } else {
                    Err(IOError::from(ErrorKind::InvalidInput))
                }
            }
            SeekFrom::End(offset) => {
                if self.decrypted_size as i64 >= -offset {
                    self.cursor = (self.decrypted_size as i64 + offset) as u64;
                    Ok(self.cursor)
                } else {
                    Err(IOError::from(ErrorKind::InvalidInput))
                }
            }
        })
    }
}

impl<'a, R: AsyncRead + AsyncSeek + Unpin + Send> AsyncRead for BufferedPartitionLoader<'a, R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<IOResult<usize>> {
        // If the requested size is 0, or if we are done reading, return without changing buf.
        if buf.len() == 0 || self.cursor >= self.decrypted_size {
            return Poll::Ready(Ok(0));
        }
        // Calculate the size and bounds of what has to be read.
        let read_size = std::cmp::min(buf.len(), (self.decrypted_size - self.cursor) as usize);
        let end_pos = self.cursor + read_size as u64;
        let start_block_idx = (self.cursor / 0x7C00) as usize;
        let end_block_idx = ((end_pos - 1) / 0x7C00) as usize;
        // Queue the waker, and drop the previous one if there was any.
        self.waker.try_lock().unwrap().replace(ctx.waker().clone());
        // Determine what needs to be done.
        let fut = self.load_handle.as_mut();
        let poll;
        if fut.is_none() {
            let waker = Arc::clone(&self.waker);
            let state = Arc::clone(&self.state);
            self.load_handle.replace(Box::pin(async move {
                let result =
                    BufferedPartitionLoader::load_blocks(state, start_block_idx..=end_block_idx)
                        .await;
                let mut w = waker.lock().await;
                if w.is_some() {
                    w.take().unwrap().wake();
                }
                result
            }));
            poll = self.load_handle.as_mut().unwrap().as_mut().poll(ctx);
        } else {
            poll = fut.unwrap().as_mut().poll(ctx);
        }
        if let Poll::Ready(result) = poll {
            self.load_handle.take();
            result?;
        } else {
            return Poll::Pending;
        }
        {
            let buffers = &self.state.try_lock().unwrap().buffers;
            for i in start_block_idx..=end_block_idx {
                let block = buffers
                    .get(&i)
                    .ok_or(IOError::new(ErrorKind::Other, "Unloaded block"))?;
                let block_pos = i as i64 * 0x7C00;
                let buf_write_start = std::cmp::max(0, block_pos - self.cursor as i64) as usize;
                let buf_write_end = std::cmp::min(
                    read_size,
                    ((block_pos + 0x7C00) - self.cursor as i64) as usize,
                );
                let block_read_start = std::cmp::max(0, self.cursor as i64 - block_pos) as usize;
                let block_read_end = std::cmp::min(
                    block.len(),
                    ((self.cursor + read_size as u64) - block_pos as u64) as usize,
                );
                buf[buf_write_start..buf_write_end]
                    .copy_from_slice(&block[block_read_start..block_read_end]);
            }
        }
        self.cursor += read_size as u64;
        Poll::Ready(Ok(read_size))
    }
}

pub struct WiiPartData<'a, R: AsyncRead + AsyncSeek + Unpin + Send> {
    reader: BufferedPartitionLoader<'a, R>,
}

impl<R: Unpin + AsyncRead + AsyncSeek + Send> WiiPartData<'_, R> {
    fn new(reader: R, offset: u64, raw_size: u64, part_key: AesKey) -> Self {
        WiiPartData {
            reader: BufferedPartitionLoader::new(reader, offset, raw_size, part_key),
        }
    }

    pub async fn get(&mut self, pos: u64, len: usize) -> IOResult<Vec<u8>> {
        let mut v = vec![0u8; len];
        self.reader.seek(std::io::SeekFrom::Start(pos)).await?;
        self.reader.read_exact(&mut v).await?;
        Ok(v)
    }
}

pub struct WiiPartition<'a, R: AsyncRead + AsyncSeek + Unpin + Send + 'a> {
    pub part_type: PartitionType,
    pub header: Box<PartHeader>,
    pub tmd: Box<TitleMetaData>,
    pub cert: Box<[u8]>,
    pub data: WiiPartData<'a, R>,
}

pub struct WiiDisc<'a, R: AsyncRead + AsyncSeek + Unpin + Send + 'a> {
    pub header: Box<[u8]>,
    pub region: Box<[u8]>,
    pub partition: WiiPartition<'a, R>,
}

pub enum Disc<'a, R: AsyncRead + AsyncSeek + Unpin + Send + 'a> {
    GameCube(Vec<u8>),
    Wii(WiiDisc<'a, R>),
}

impl Default for Disc<'_, Empty> {
    fn default() -> Self {
        Disc::GameCube(Default::default())
    }
}

impl<'a, R: AsyncRead + AsyncSeek + Unpin + Send + 'a> Disc<'a, R> {
    pub async fn extract(mut reader: R) -> Result<Disc<'a, R>, Error> {
        let mut header = [0u8; 0x400];
        let mut region = [0u8; 0x20];
        reader
            .read_exact(&mut header)
            .await
            .context("Couldn't read disc header")?;
        if BE::read_u32(&header[0x18..]) != 0x5D1C9EA3 {
            let mut result: Vec<u8>;
            if cfg!(not(target_arch = "wasm32")) {
                result = vec![];
                reader.seek(SeekFrom::Start(0)).await?;
                reader
                    .read_to_end(&mut result)
                    .await
                    .context("Couldn't read GameCube disc")?;
            } else {
                result = vec![0u8; 0x400];
                result[..0x400].copy_from_slice(&header);
                const CHUNCK_SIZE: usize = 1usize << 18; // 256KiB
                let mut b = [0u8; CHUNCK_SIZE];
                let mut read_size = reader
                    .read(&mut b)
                    .await
                    .context("Couldn't read GameCube disc")?;
                while read_size > 0 {
                    result.extend(&b[..read_size]);
                    read_size = reader
                        .read(&mut b)
                        .await
                        .context("Couldn't read GameCube disc")?;
                    async_std::task::sleep(std::time::Duration::from_millis(10)).await;
                }
            }
            return Ok(Disc::GameCube(result));
        }

        reader
            .seek(SeekFrom::Start(0x40000))
            .await
            .context("Couldn't seek to partition list")?;
        let mut part_info: PartInfo = Default::default();
        {
            let mut buf = [0u8; 8];
            reader
                .read_exact(&mut buf)
                .await
                .context("Couldn't read partition list offset/size")?;
            let n_part = BE::read_u32(&buf[..]) as usize;
            part_info.offset = (BE::read_u32(&buf[0x4..]) as u64) << 2;
            reader
                .seek(SeekFrom::Start(part_info.offset))
                .await
                .context("Couldn't seek to partition list offset")?;
            part_info.entries.reserve(n_part);
            for i in 0..n_part {
                reader
                    .read_exact(&mut buf)
                    .await
                    .context(format!("Couldn't read parition info #{}", i))?;
                part_info.entries.push(PartInfoEntry {
                    offset: (BE::read_u32(&buf[..]) as u64) << 2,
                    part_type: BE::read_u32(&buf[4..]),
                });
            }
            part_info.entries.sort_by(|a, b| a.offset.cmp(&b.offset));
        }
        reader
            .seek(SeekFrom::Start(0x4E000))
            .await
            .context("Couldn't seek to region")?;
        reader
            .read_exact(&mut region)
            .await
            .context("Couldn't read region")?;

        let mut partition: IOResult<WiiPartition<R>> =
            Err(IOError::new(ErrorKind::Other, "No DATA partition"));
        for part_entry in part_info.entries {
            // If it's not a data partition, we skip it
            if part_entry.part_type != 0 {
                continue;
            }
            // Seek to the current partition
            reader
                .seek(SeekFrom::Start(part_entry.offset))
                .await
                .context("Couldn't seek to partition")?;
            // - Get partition header
            let part_header: Box<PartHeader>;
            {
                let mut buf = [0u8; 0x2C0];
                reader
                    .read_exact(&mut buf)
                    .await
                    .context("Couldn't collect partition header")?;
                part_header = Box::new(PartHeader::from(&buf));
            }
            // - Get TMD
            reader
                .seek(SeekFrom::Start(part_header.tmd_offset + part_entry.offset))
                .await
                .context("Couldn't seek to parition TMD")?;
            let tmd: Box<TitleMetaData>;
            {
                let mut buf = vec![0u8; part_header.tmd_size];
                reader
                    .read_exact(&mut buf)
                    .await
                    .context("Couldn't read parition TMD")?;
                tmd = Box::new(partition_get_tmd(&buf, 0));
            }
            // - Get certification chain
            reader
                .seek(SeekFrom::Start(part_header.cert_offset + part_entry.offset))
                .await
                .context("Couldn't seek to Cert list")?;
            let cert: Box<[u8]>;
            {
                let mut buf = vec![0u8; part_header.cert_size];
                reader
                    .read_exact(&mut buf)
                    .await
                    .context("Couldn't read Cert list")?;
                cert = buf.into_boxed_slice();
            }
            // - Get data
            // Seek to partition data
            reader
                .seek(SeekFrom::Start(part_header.data_offset + part_entry.offset))
                .await
                .context("Couldn't seek to parition Data")?;
            // For each section
            let data = WiiPartData::new(
                reader,
                part_entry.offset + part_header.data_offset,
                part_header.data_size,
                decrypt_title_key(&part_header.ticket),
            );
            partition = Ok(WiiPartition {
                part_type: part_entry.part_type.into(),
                header: part_header,
                tmd,
                cert,
                data,
            });
            break;
        }
        Ok(Disc::Wii(WiiDisc {
            header: Box::new(header),
            region: Box::new(region),
            partition: partition.context("Parition not found")?,
        }))
    }
}

pub struct WiiPatchedPartition {
    pub part_type: PartitionType,
    pub header: Box<PartHeader>,
    pub tmd: Box<TitleMetaData>,
    pub cert: Box<[u8]>,
    pub data: Box<[u8]>,
}

impl<'a, R: AsyncRead + AsyncSeek + Unpin + Send> From<WiiPartition<'a, R>>
    for WiiPatchedPartition
{
    fn from(p: WiiPartition<R>) -> Self {
        WiiPatchedPartition {
            part_type: p.part_type,
            header: p.header,
            tmd: p.tmd,
            cert: p.cert,
            data: Default::default(),
        }
    }
}

pub struct WiiPatchedDisc {
    pub header: Box<[u8]>,
    pub region: Box<[u8]>,
    pub partition: WiiPatchedPartition,
}

impl<'a, R: AsyncRead + AsyncSeek + Unpin + Send> From<WiiDisc<'a, R>> for WiiPatchedDisc {
    fn from(d: WiiDisc<R>) -> Self {
        WiiPatchedDisc {
            header: d.header,
            region: d.region,
            partition: WiiPatchedPartition::from(d.partition),
        }
    }
}

#[derive(Copy, Clone, Debug)]
enum EncryptParseState {
    WriteDiscHeader,
    GetToPartInfo,
    WritePartInfo,
    GetToRegion,
    WriteRegion,
    GetToMagic,
    WriteMagic,
    WriteHeader,
    WriteSectorGroup,

    // FillZero,

    // Final state(s)
    Done,
}

impl Default for EncryptParseState {
    fn default() -> Self {
        EncryptParseState::WriteDiscHeader
    }
}

struct WESState {
    disc: WiiPatchedDisc,
    auto_reader: AutoReader<futures::io::Repeat>,
    state: EncryptParseState,
    part_offset: u64,
    part_key: AesKey,
    part_hashes: Vec<u8>,
    part_block_idx: usize,
    read_buf: Box<[u8]>,
    read_buf_cursor: usize,
}

pub struct WiiEncryptStream<'a> {
    status: SharedMutex<WESState>,

    load_handle: Option<Pin<Box<(dyn Future<Output = IOResult<usize>> + 'a)>>>,
    waker: SharedMutex<Option<Waker>>,
}

fn prepare_header(buf: &mut [u8], status: &mut WESState) {
    let part: &WiiPatchedPartition = &status.disc.partition;
    let part_hashes: &mut Vec<u8> = &mut status.part_hashes;
    let part_key: &mut AesKey = &mut status.part_key;
    buf[..0x2C0].copy_from_slice(&<[u8; 0x2C0]>::from(&*part.header));
    partition_set_tmd(
        &mut buf[part.header.tmd_offset as usize..][..part.header.tmd_size],
        0,
        &*part.tmd,
    );
    buf[part.header.cert_offset as usize..][..part.header.cert_size]
        .copy_from_slice(&*part.cert);

    {
        let n_sectors = (part.header.data_size / 0x8000) as usize;
        let n_clusters = n_sectors / 8;
        let n_groups = n_clusters / 8;
        // h0
        part_hashes.resize(n_sectors * consts::WII_SECTOR_HASH_SIZE, 0);
        let mut data_pool: Vec<(&[u8], &mut [u8])> = Vec::with_capacity(n_sectors);
        let mut data_slice: &[u8] = &part.data[..];
        let mut hash_slice: &mut [u8] = &mut part_hashes[..];
        for _ in 0..n_sectors {
            let (section, new_data_slice) = data_slice.split_at(consts::WII_SECTOR_DATA_SIZE);
            data_slice = new_data_slice;
            let (hash_section, new_hash_slice) =
                hash_slice.split_at_mut(consts::WII_SECTOR_HASH_SIZE);
            hash_slice = new_hash_slice;
            data_pool.push((section, hash_section));
        }
        fn h0_process((data, hash): &mut (&[u8], &mut [u8])) {
            for j in 0..31 {
                hash[j * 20..(j + 1) * 20]
                    .copy_from_slice(&Sha1::from(&data[j * 0x400..][..0x400]).digest().bytes());
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        data_pool.par_iter_mut().for_each(h0_process);
        #[cfg(target_arch = "wasm32")]
        data_pool.iter_mut().for_each(h0_process);
        // h1
        let mut data_pool: Vec<&mut [u8]> = Vec::with_capacity(n_clusters);
        let mut hash_slice: &mut [u8] = &mut part_hashes[..];
        for _ in 0..n_clusters {
            let (hash_section, new_hash_slice) =
                hash_slice.split_at_mut(consts::WII_SECTOR_HASH_SIZE * 8);
            hash_slice = new_hash_slice;
            data_pool.push(hash_section);
        }
        fn h1_process(h: &mut &mut [u8]) {
            let mut hash = [0u8; 0x0a0];
            for j in 0..8 {
                hash[j * 20..(j + 1) * 20]
                    .copy_from_slice(&Sha1::from(&h[j * 0x400..][..0x26c]).digest().bytes()[..]);
            }
            for j in 0..8 {
                h[j * 0x400 + 0x280..][..0xa0].copy_from_slice(&hash);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        data_pool.par_iter_mut().for_each(h1_process);
        #[cfg(target_arch = "wasm32")]
        data_pool.iter_mut().for_each(h1_process);
        // h2
        let mut data_pool: Vec<&mut [u8]> = Vec::with_capacity(n_groups);
        let mut hash_slice: &mut [u8] = &mut part_hashes[..];
        for _ in 0..n_groups {
            let (hash_section, new_hash_slice) =
                hash_slice.split_at_mut(consts::WII_SECTOR_HASH_SIZE * 64);
            hash_slice = new_hash_slice;
            data_pool.push(hash_section);
        }
        fn h2_process(h: &mut &mut [u8]) {
            let mut hash = [0u8; 0x0a0];
            for j in 0..8 {
                hash[j * 20..(j + 1) * 20].copy_from_slice(
                    &Sha1::from(&h[j * 8 * 0x400 + 0x280..][..0xa0])
                        .digest()
                        .bytes()[..],
                );
            }
            for j in 0..64 {
                h[j * 0x400 + 0x340..][..0xa0].copy_from_slice(&hash);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        data_pool.par_iter_mut().for_each(h2_process);
        #[cfg(target_arch = "wasm32")]
        data_pool.iter_mut().for_each(h2_process);
        // h3
        let h3_offset = part.header.h3_offset as usize;
        // zero the h3 table
        buf[h3_offset..][..0x18000].copy_from_slice(&[0u8; 0x18000]);
        // divide and conquer
        let mut data_pool: Vec<(&mut [u8], &[u8])> = Vec::with_capacity(n_groups);
        let mut data_slice = &part_hashes[..];
        let mut h3 = &mut buf[h3_offset..][..0x18000];
        for _ in 0..n_groups {
            let (section, new_data_slice) = data_slice.split_at(consts::WII_SECTOR_HASH_SIZE * 64);
            let (hash_section, new_h3) = h3.split_at_mut(20);
            data_slice = new_data_slice;
            h3 = new_h3;
            data_pool.push((hash_section, section));
        }
        fn h3_process((hash, sector): &mut (&mut [u8], &[u8])) {
            hash[..20].copy_from_slice(&Sha1::from(&sector[0x340..][..0xa0]).digest().bytes()[..]);
        }
        #[cfg(not(target_arch = "wasm32"))]
        data_pool.par_iter_mut().for_each(h3_process);
        #[cfg(target_arch = "wasm32")]
        data_pool.iter_mut().for_each(h3_process);
        // h4 / TMD
        let mut tmd = partition_get_tmd(&buf[..], part.header.tmd_offset as usize);
        let mut tmd_size = 0x1e4 + 36 * tmd.contents.len();
        if tmd.contents.len() > 0 {
            let content = &mut tmd.contents[0];
            content
                .hash
                .copy_from_slice(&Sha1::from(&buf[h3_offset..][..0x18000]).digest().bytes()[..]);
            tmd_size = partition_set_tmd(&mut buf[..], part.header.tmd_offset as usize, &tmd);
        }
        tmd_fake_sign(&mut buf[part.header.tmd_offset as usize..][..tmd_size]);
        ticket_fake_sign(&mut buf[..0x2A4]);
    }
    *part_key = decrypt_title_key(&part.header.ticket);
}

fn prepare_sector_group(
    buf: &mut [u8],
    disc: &WiiPatchedDisc,
    n: usize,
    part_block_idx: usize,
    part_key: AesKey,
    part_hashes: &Vec<u8>,
) {
    let mut data_pool: Vec<(&[u8], &[u8], &mut [u8])> = Vec::with_capacity(n);
    let (_, mut hash_slice) =
        part_hashes.split_at((part_block_idx * consts::WII_SECTOR_HASH_SIZE) as usize);
    let (_, mut data_slice) = disc
        .partition
        .data
        .split_at((part_block_idx as u64 * consts::WII_SECTOR_DATA_SIZE as u64) as usize);
    let (_, mut buf_slice) = buf.split_at_mut(0);
    for _ in 0..n {
        let (hash, new_hash_slice) = hash_slice.split_at(consts::WII_SECTOR_HASH_SIZE);
        let (section, new_data_slice) = data_slice.split_at(consts::WII_SECTOR_DATA_SIZE);
        let (buf, new_buf_slice) = buf_slice.split_at_mut(consts::WII_SECTOR_SIZE);
        hash_slice = new_hash_slice;
        data_slice = new_data_slice;
        buf_slice = new_buf_slice;
        data_pool.push((hash, section, buf));
    }
    let encrypt_process = move |(hash, data, dest): &mut (&[u8], &[u8], &mut [u8])| {
        dest[..consts::WII_SECTOR_HASH_SIZE].copy_from_slice(&hash[..]);
        aes_encrypt_inplace(
            &mut dest[..consts::WII_SECTOR_HASH_SIZE],
            Default::default(),
            part_key,
            consts::WII_SECTOR_HASH_SIZE,
        )
        .expect("Could not encrypt hash sector");
        let iv = AesKey::from(&dest[consts::WII_SECTOR_IV_OFF..][..consts::WII_KEY_SIZE]);
        dest[consts::WII_SECTOR_HASH_SIZE..][..consts::WII_SECTOR_DATA_SIZE]
            .copy_from_slice(&data[..]);
        aes_encrypt_inplace(
            &mut dest[consts::WII_SECTOR_HASH_SIZE..][..consts::WII_SECTOR_DATA_SIZE],
            iv,
            part_key,
            consts::WII_SECTOR_DATA_SIZE,
        )
        .expect("Could not encrypt data sector");
    };
    #[cfg(not(target_arch = "wasm32"))]
    data_pool.par_iter_mut().for_each(encrypt_process);
    #[cfg(target_arch = "wasm32")]
    data_pool.iter_mut().for_each(encrypt_process);
}

impl<'a> WiiEncryptStream<'a> {
    pub fn new(disc: WiiPatchedDisc) -> Self {
        Self::with_capacity(0, disc)
    }

    pub fn with_capacity(_: usize, disc: WiiPatchedDisc) -> Self {
        WiiEncryptStream {
            status: Arc::from(Mutex::new(WESState {
                auto_reader: AutoReader::new_write(
                    0,
                    0x400,
                    Arc::new(Mutex::from(disc.header.clone())),
                ),
                disc,
                state: Default::default(),
                part_offset: 0,
                part_key: Default::default(),
                part_hashes: Vec::new(),
                part_block_idx: 0,
                read_buf: vec![0u8; 1].into_boxed_slice(),
                read_buf_cursor: 0,
            })),

            load_handle: None,
            waker: Arc::from(Mutex::new(None)),
        }
    }

    pub async fn reset(&mut self) {
        let mut status = self.status.lock().await;
        status.state = EncryptParseState::WriteDiscHeader;
        match &mut status.auto_reader {
            AutoReader::Copy(reader_status, _) => {
                reader_status.cursor = 0;
            }
            AutoReader::Read(reader_status, _, buf) => {
                reader_status.cursor = 0;
                buf.lock().await.clear();
            }
            AutoReader::Write(reader_status, buf) => {
                reader_status.cursor = 0;
                std::mem::drop(std::mem::replace(
                    &mut *buf.lock().await,
                    Default::default(),
                ))
            }
        }
    }

    fn get_load_handle<'b, 'c: 'b>(
        self: &'b mut Pin<&'c mut Self>,
    ) -> &'b mut Option<Pin<Box<(dyn Future<Output = IOResult<usize>> + 'a)>>> {
        &mut self.load_handle
    }

    async fn basic_read(
        status: &SharedMutex<WESState>,
    ) -> Result<(usize, AutoReaderState, usize), Error> {
        let read_size; // "read" is in the past tense
        let reader_state;
        let remaining;

        let mut status_ = status.lock().await;
        let mut read_buf = std::mem::take(&mut status_.read_buf);
        let read_cursor = status_.read_buf_cursor;
        let buf_len = read_buf.len();
        let result = status_.auto_reader.read(&mut read_buf[read_cursor..]).await;
        std::mem::drop(std::mem::replace(&mut status_.read_buf, read_buf));
        read_size = result.context("Error while reading for encryption")?;
        status_.read_buf_cursor += read_size;
        remaining = buf_len - status_.read_buf_cursor;
        reader_state = status_.auto_reader.status().state;

        Ok((read_size, reader_state, remaining))
    }

    #[async_recursion]
    async fn handle_disc_sys_section(status: SharedMutex<WESState>) -> Result<usize, Error> {
        use EncryptParseState::*;
        let state;
        {
            let status_ = status.lock().await;
            Mutex::new(2u64);
            state = status_.state;
        }
        match state {
            WriteDiscHeader => {
                let (mut ret, reader_state, remaining) = WiiEncryptStream::basic_read(&status)
                    .await
                    .context("From WiiEncryptStream::WriteDiscHeader read")?;
                if reader_state == AutoReaderState::Done {
                    let mut status_ = status.lock().await;
                    std::mem::drop(std::mem::replace(
                        &mut status_.auto_reader,
                        AutoReader::new_copy(
                            0x400,
                            0x40000,
                            Arc::from(Mutex::new(futures::io::repeat(0))),
                        ),
                    ));
                    status_.state = GetToPartInfo;
                }
                if remaining > 0 {
                    ret += WiiEncryptStream::handle_disc_sys_section(status)
                        .await
                        .context("From WiiEncryptStream::WriteDiscHeader")?;
                }
                Ok(ret)
            }
            GetToPartInfo => {
                let (mut ret, reader_state, remaining) = WiiEncryptStream::basic_read(&status)
                    .await
                    .context("From WiiEncryptStream::GetToPartInfo read")?;
                if reader_state == AutoReaderState::Done {
                    let mut b: Box<[u8]> = Box::new([0u8; 0x28]);
                    let mut status_ = status.lock().await;
                    BE::write_u32(&mut b[..], 1 as u32);
                    BE::write_u32(&mut b[4..], (0x40020 >> 2) as u32);
                    let offset: u64 = 0x50000;
                    let i = 0;
                    let part_type = <u32>::from(status_.disc.partition.part_type);
                    status_.part_offset = offset;
                    BE::write_u32(&mut b[0x20 + (8 * i)..], (offset >> 2) as u32);
                    BE::write_u32(&mut b[0x20 + (8 * i) + 4..], part_type);
                    std::mem::drop(std::mem::replace(
                        &mut status_.auto_reader,
                        AutoReader::new_write(0x40000, 0x40028, Arc::from(Mutex::new(b))),
                    ));
                    status_.state = WritePartInfo;
                }
                if remaining > 0 {
                    ret += WiiEncryptStream::handle_disc_sys_section(status)
                        .await
                        .context("From WiiEncryptStream::GetToPartInfo")?;
                }
                Ok(ret)
            }
            WritePartInfo => {
                let (mut ret, reader_state, remaining) = WiiEncryptStream::basic_read(&status)
                    .await
                    .context("From WiiEncryptStream::WritePartInfo read")?;
                if reader_state == AutoReaderState::Done {
                    let mut status_ = status.lock().await;
                    std::mem::drop(std::mem::replace(
                        &mut status_.auto_reader,
                        AutoReader::new_copy(
                            0x40028,
                            0x4E000,
                            Arc::from(Mutex::new(futures::io::repeat(0))),
                        ),
                    ));
                    status_.state = GetToRegion;
                }
                if remaining > 0 {
                    ret += WiiEncryptStream::handle_disc_sys_section(status)
                        .await
                        .context("From WiiEncryptStream::WritePartInfo")?;
                }
                Ok(ret)
            }
            GetToRegion => {
                let (mut ret, reader_state, remaining) = WiiEncryptStream::basic_read(&status)
                    .await
                    .context("From WiiEncryptStream::GetToRegion read")?;
                if reader_state == AutoReaderState::Done {
                    let mut status_ = status.lock().await;
                    let b = Arc::from(Mutex::new(status_.disc.region.clone()));
                    std::mem::drop(std::mem::replace(
                        &mut status_.auto_reader,
                        AutoReader::new_write(0x4E000, 0x4E020, b),
                    ));
                    status_.state = WriteRegion;
                }
                if remaining > 0 {
                    ret += WiiEncryptStream::handle_disc_sys_section(status)
                        .await
                        .context("From WiiEncryptStream::GetToRegion")?;
                }
                Ok(ret)
            }
            WriteRegion => {
                let (mut ret, reader_state, remaining) = WiiEncryptStream::basic_read(&status)
                    .await
                    .context("From WiiEncryptStream::WriteRegion read")?;
                if reader_state == AutoReaderState::Done {
                    let mut status_ = status.lock().await;
                    std::mem::drop(std::mem::replace(
                        &mut status_.auto_reader,
                        AutoReader::new_copy(
                            0x4E020,
                            0x4FFFC,
                            Arc::from(Mutex::new(futures::io::repeat(0))),
                        ),
                    ));
                    status_.state = GetToMagic;
                }
                if remaining > 0 {
                    ret += WiiEncryptStream::handle_disc_sys_section(status)
                        .await
                        .context("From WiiEncryptStream::WriteRegion")?;
                }
                Ok(ret)
            }
            GetToMagic => {
                let (mut ret, reader_state, remaining) = WiiEncryptStream::basic_read(&status)
                    .await
                    .context("From WiiEncryptStream::GetToMagic read")?;
                if reader_state == AutoReaderState::Done {
                    let mut b: Box<[u8]> = Box::new([0u8; 0x28]);
                    BE::write_u32(&mut b, 0xC3F81A8E);
                    let mut status_ = status.lock().await;
                    std::mem::drop(std::mem::replace(
                        &mut status_.auto_reader,
                        AutoReader::new_write(0x4FFFC, 0x50000, Arc::from(Mutex::new(b))),
                    ));
                    status_.state = WriteMagic;
                }
                if remaining > 0 {
                    ret += WiiEncryptStream::handle_disc_sys_section(status)
                        .await
                        .context("From WiiEncryptStream::GetToMagic")?;
                }
                Ok(ret)
            }
            WriteMagic => {
                let (mut ret, reader_state, remaining) = WiiEncryptStream::basic_read(&status)
                    .await
                    .context("From WiiEncryptStream::WriteMagic read")?;
                if reader_state == AutoReaderState::Done {
                    let mut status_ = status.lock().await;
                    let part_data_offset =
                        status_.part_offset + status_.disc.partition.header.data_offset;
                    let mut b: Box<[u8]> = vec![0u8; part_data_offset as usize].into_boxed_slice();
                    {
                        prepare_header(&mut b, &mut *status_);
                    }
                    std::mem::drop(std::mem::replace(
                        &mut status_.auto_reader,
                        AutoReader::new_write(0x50000, part_data_offset, Arc::from(Mutex::new(b))),
                    ));
                    status_.state = WriteHeader;
                }
                if remaining > 0 {
                    ret += WiiEncryptStream::handle_partition(status)
                        .await
                        .context("From WiiEncryptStream::WriteMagic")?;
                }
                Ok(ret)
            }
            _ => Err(AutoReaderError::InvalidState("Unreachable block was reached").into()),
        }
    }

    #[async_recursion]
    async fn handle_partition(status: SharedMutex<WESState>) -> Result<usize, Error> {
        use EncryptParseState::*;
        const STEP: usize = 64 * 8;
        let state;
        let mut n;
        let part_offset;
        let data_offset;
        let part_block_idx;
        {
            let status_ = status.lock().await;
            Mutex::new(2u64);
            state = status_.state;
            n = std::cmp::min(
                STEP,
                (status_.disc.partition.header.data_size / 0x8000) as usize
                    - status_.part_block_idx,
            );
            part_offset = status_.part_offset;
            data_offset = status_.disc.partition.header.data_offset;
            part_block_idx = status_.part_block_idx;
        }
        match state {
            WriteHeader => {
                let (mut ret, reader_state, remaining) = WiiEncryptStream::basic_read(&status)
                    .await
                    .context("From WiiEncryptStream::WriteHeader read")?;
                if reader_state == AutoReaderState::Done {
                    let mut status_ = status.lock().await;
                    status_.part_block_idx = 0;
                    status_.state = if status_.disc.partition.header.data_size < 0x8000 {
                        Done
                    } else {
                        let mut b: Box<[u8]> = vec![0u8; n * 0x8000].into_boxed_slice();
                        prepare_sector_group(
                            &mut b,
                            &status_.disc,
                            n,
                            status_.part_block_idx,
                            status_.part_key,
                            &status_.part_hashes,
                        );
                        std::mem::drop(std::mem::replace(
                            &mut status_.auto_reader,
                            AutoReader::new_write(
                                part_offset + data_offset,
                                part_offset
                                    + data_offset
                                    + (part_block_idx as u64 + n as u64) * 0x8000,
                                Arc::from(Mutex::new(b)),
                            ),
                        ));
                        WriteSectorGroup
                    }
                }
                if remaining > 0 {
                    ret += WiiEncryptStream::handle_partition(status)
                        .await
                        .context("From WiiEncryptStream::WriteHeader")?;
                }
                Ok(ret)
            }
            WriteSectorGroup => {
                let (mut ret, reader_state, remaining) = WiiEncryptStream::basic_read(&status)
                    .await
                    .context("From WiiEncryptStream::WriteSectorGroup read")?;
                if reader_state == AutoReaderState::Done {
                    let mut status_ = status.lock().await;
                    status_.part_block_idx += STEP;
                    status_.state = if status_.part_block_idx
                        >= (status_.disc.partition.header.data_size / 0x8000) as usize
                    {
                        status_.part_block_idx = 0;
                        Done
                    } else {
                        n = std::cmp::min(
                            STEP,
                            (status_.disc.partition.header.data_size / 0x8000) as usize
                                - status_.part_block_idx,
                        );
                        let mut b: Box<[u8]> = vec![0u8; n * 0x8000].into_boxed_slice();
                        prepare_sector_group(
                            &mut b,
                            &status_.disc,
                            n,
                            status_.part_block_idx,
                            status_.part_key,
                            &status_.part_hashes,
                        );
                        std::mem::drop(std::mem::replace(
                            &mut status_.auto_reader,
                            AutoReader::new_write(
                                part_offset + data_offset + part_block_idx as u64 * 0x8000,
                                part_offset
                                    + data_offset
                                    + (part_block_idx as u64 + n as u64) * 0x8000,
                                Arc::from(Mutex::new(b)),
                            ),
                        ));
                        WriteSectorGroup
                    }
                }
                if remaining > 0 {
                    ret += WiiEncryptStream::handle_partition(status)
                        .await
                        .context("From WiiEncryptStream::WriteSectorGroup")?;
                }
                Ok(ret)
            }
            _ => Err(AutoReaderError::InvalidState("Unreachable block was reached").into()),
        }
    }
}

// #[async_trait]
// impl WiiTransform<EncryptParseState> for WiiEncryptStream<'_, '_> {}

impl<'a> AsyncRead for WiiEncryptStream<'a> {
    // fn read(&mut self, buf: &mut [u8]) -> IOResult<usize> {
    //     use self::EncryptParseState::*;
    //     if buf.len() == 0 {
    //         return Ok(0);
    //     }
    //     let result = match self.status.state {
    //         WriteDiscHeader | GetToPartInfo | WritePartInfo | GetToRegion | WriteRegion
    //         | GetToMagic | WriteMagic => self.handle_disc_sys_section(buf),
    //         WriteHeader | WriteSectorGroup => self.handle_partition(buf),
    //         // FillZero => {
    //         //     let write_size = std::cmp::min(buf.len() as u64, 0x118240000 - self.status.cursor) as usize;
    //         //     for x in &mut buf[.. write_size] { *x = 0; }
    //         //     self.status.cursor += write_size as u64;
    //         //     if self.status.cursor >= 0x118240000 {
    //         //         self.status.state = Done
    //         //     }
    //         //     Ok(write_size)
    //         // },
    //         _ => Ok(0usize),
    //     };
    //     match result {
    //         Ok(size) => Ok(size),
    //         Err(error) => {
    //             if error.kind() != ErrorKind::Interrupted {
    //                 self.status.state = Done;
    //             }
    //             Err(error)
    //         }
    //     }
    // }
    fn poll_read(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<IOResult<usize>> {
        use self::EncryptParseState::*;
        if buf.len() == 0 {
            return Poll::Ready(Ok(0));
        }
        let result: IOResult<usize>;
        // Queue the waker, and drop the previous one if there was any.
        self.waker.try_lock().unwrap().replace(ctx.waker().clone());
        // Determine what needs to be done.
        let fut = self.get_load_handle().as_mut();
        let poll;
        if fut.is_none() {
            let waker = Arc::clone(&self.waker);
            let status = Arc::clone(&self.status);
            {
                let mut status_ = status.try_lock().unwrap();
                status_.read_buf_cursor = 0;
                std::mem::drop(std::mem::replace(
                    &mut status_.read_buf,
                    vec![0u8; buf.len()].into_boxed_slice(),
                ));
            }
            self.load_handle.replace(Box::pin(async move {
                let state;
                {
                    state = status.lock().await.state;
                }
                let result = match state {
                    WriteDiscHeader | GetToPartInfo | WritePartInfo | GetToRegion | WriteRegion
                    | GetToMagic | WriteMagic => {
                        WiiEncryptStream::handle_disc_sys_section(status).await
                    }
                    WriteHeader | WriteSectorGroup => {
                        WiiEncryptStream::handle_partition(status).await
                    }
                    _ => Ok(0usize),
                }
                .or_else(|err| Err(IOError::new(ErrorKind::Other, err)))?;
                let mut w = waker.lock().await;
                if w.is_some() {
                    w.take().unwrap().wake();
                }
                Ok(result)
            }));
            poll = self.load_handle.as_mut().unwrap().as_mut().poll(ctx);
        } else {
            poll = fut.unwrap().as_mut().poll(ctx);
        }
        if let Poll::Ready(res) = poll {
            result = res;
            let mut status_ = self.status.try_lock().unwrap();
            if let Ok(size) = &result {
                buf[..*size].copy_from_slice(&status_.read_buf[..*size]);
            }
            status_.read_buf_cursor = 0;
            std::mem::drop(std::mem::replace(
                &mut status_.read_buf,
                vec![0u8; 1].into_boxed_slice(),
            ));
        } else {
            return Poll::Pending;
        }
        self.load_handle.take();
        match result {
            Ok(size) => Poll::Ready(Ok(size)),
            Err(error) => {
                if error.kind() != ErrorKind::Interrupted {
                    self.status.try_lock().unwrap().state = Done;
                }
                Poll::Ready(Err(error))
            }
        }
    }
}

/// Aligns the `addr` up so that every bits before the `bit`-th bit are 0.
pub fn align_addr<T>(addr: T, bit: usize) -> T
where
    T: std::ops::Add<Output = T>
        + std::ops::Sub<Output = T>
        + std::ops::Shl<usize, Output = T>
        + std::ops::BitAnd<Output = T>
        + std::ops::Not<Output = T>
        + From<u8>,
{
    ((addr - T::from(1)) & !((T::from(1) << bit) - T::from(1))) + (T::from(1) << bit)
}
