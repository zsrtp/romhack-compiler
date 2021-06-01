#![feature(async_closure)]

extern crate console_error_panic_hook;
extern crate failure;
extern crate js_sys;
extern crate lazy_static;
extern crate rayon;
extern crate romhack_backend;
extern crate wasm_bindgen;
extern crate wasm_bindgen_futures;
// extern crate wasm_mt;
extern crate web_sys;
extern crate web_worker;
extern crate wee_alloc;
extern crate wii_crypto;

// Use `wee_alloc` as the global allocator.
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

use failure::err_msg;
use failure::{Error, ResultExt};
use futures::io::{AsyncRead, AsyncSeek, AsyncSeekExt, AsyncWrite, BufWriter};
use futures::lock::Mutex;
use futures::task::{Context, Poll};
use romhack_backend::{
    build_iso, iso::writer::write_iso, open_config_from_patch, KeyValPrint, MessageKind,
};
use std::alloc::{alloc as allocate, dealloc as deallocate, Layout};
use std::future::Future;
use std::io::{self, Cursor, SeekFrom};
use std::pin::Pin;
// use std::slice::from_raw_parts;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::Waker;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
// use wasm_mt::prelude::*;
// use serde_closure::FnMut;
use rayon::prelude::*;
use wii_crypto::transform::Disc;

#[wasm_bindgen] //(module = "romhack_prelude")
extern "C" {
    #[wasm_bindgen(js_namespace = RomHack)]
    fn count_write(buf_len: usize);
    #[wasm_bindgen(js_namespace = RomHack, js_name = "seek")]
    fn count_seek(kind: u8, offset: isize) -> usize;
    #[wasm_bindgen(js_namespace = RomHack)]
    fn restart();
    #[wasm_bindgen(js_namespace = RomHack, catch)]
    fn write(buf: &[u8]) -> Result<(), JsValue>;
    #[wasm_bindgen(js_namespace = RomHack)]
    fn seek(kind: u8, offset: isize) -> usize;
    #[wasm_bindgen(js_namespace = RomHack)]
    fn set_name(s: String);

    #[wasm_bindgen(js_namespace = RomHack, js_name = "keyValPrint")]
    fn key_val_print(key: &str, val: &str, kind: Option<String>);

    #[wasm_bindgen(js_namespace = RomHack)]
    fn iso_seek(kind: u8, offset: i64) -> u64;
    #[wasm_bindgen(js_namespace = RomHack, catch)]
    async fn iso_read(buf: &mut [u8]) -> Result<JsValue, JsValue>;
}

struct JSPrinter;

impl KeyValPrint for JSPrinter {
    fn print(&self, kind: Option<MessageKind>, key: &str, val: &str) {
        {
            key_val_print(key.into(), val.into(), kind.and_then(|k| Some(k.into())));
        }
    }
}

#[derive(Default)]
struct RomHackReader;

async fn read_iso_async(buf: &mut [u8]) -> io::Result<usize> {
    iso_read(buf)
        .await
        .or_else(|x| {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!("[{}]{:?}", file!(), x),
            ))
        })
        .and_then(|x| {
            x.as_f64().ok_or(io::Error::new(
                io::ErrorKind::Other,
                "Returned value from iso_read is not a number",
            ))
        })
        .and_then(|x| Ok(x as usize))
}

#[derive(Default)]
struct IsoReadState {
    read_buf: Arc<Mutex<Box<[u8]>>>,
    handle: Option<Pin<Box<(dyn std::future::Future<Output = io::Result<usize>>)>>>,
    waker: Arc<Mutex<Option<Waker>>>,
}

impl AsyncRead for RomHackReader {
    fn poll_read(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        // If the requested size is 0, or if we are done reading, return without changing buf.
        if buf.len() == 0 {
            return Poll::Ready(Ok(0));
        }
        let result: io::Result<usize>;
        // Queue the waker, and drop the previous one if there was any.
        let state = unsafe { ISO_READ_STATE.as_mut().unwrap() };
        state.waker.try_lock().unwrap().replace(ctx.waker().clone());
        // Determine what needs to be done.
        let fut = state.handle.as_mut();
        let poll;
        if fut.is_none() {
            let read_buf_ = Arc::clone(&state.read_buf);
            let waker = Arc::clone(&state.waker);
            {
                let mut rb = state.read_buf.try_lock().unwrap();
                std::mem::drop(std::mem::replace(
                    &mut *rb,
                    vec![0u8; buf.len()].into_boxed_slice(),
                ));
            }
            state.handle.replace(Box::pin(async move {
                let res;
                unsafe {
                    let mut rb = read_buf_.lock().await;
                    res = read_iso_async(&mut rb[..]).await;
                }
                let mut w = waker.lock().await;
                if w.is_some() {
                    w.take().unwrap().wake();
                }
                res
            }));
            poll = state.handle.as_mut().unwrap().as_mut().poll(ctx);
        } else {
            poll = fut.unwrap().as_mut().poll(ctx);
        }
        if let Poll::Ready(res) = poll {
            result = res;
            let mut read_buf = state.read_buf.try_lock().unwrap();
            if let Ok(size) = &result {
                buf[..*size].copy_from_slice(&read_buf[..*size]);
            }
            std::mem::take(&mut *read_buf);
        } else {
            return Poll::Pending;
        }
        state.handle.take();
        Poll::Ready(result)
    }
}

impl AsyncSeek for RomHackReader {
    fn poll_seek(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        pos: SeekFrom,
    ) -> Poll<Result<u64, std::io::Error>> {
        let new_pos = {
            match pos {
                SeekFrom::Start(offset) => iso_seek(0, offset as i64),
                SeekFrom::End(offset) => iso_seek(1, offset),
                SeekFrom::Current(offset) => iso_seek(2, offset),
            }
        };
        Poll::Ready(Ok(new_pos))
    }
}

struct RomHackWriter;

impl AsyncWrite for RomHackWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        write(buf).or_else(|x| {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!("[{}]{:?}", file!(), x),
            ))
        })?;
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        Poll::Ready(Ok(()))
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncSeek for RomHackWriter {
    fn poll_seek(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        pos: SeekFrom,
    ) -> Poll<Result<u64, std::io::Error>> {
        let new_pos = {
            match pos {
                SeekFrom::Start(offset) => seek(0, offset as isize),
                SeekFrom::End(offset) => seek(1, offset as isize),
                SeekFrom::Current(offset) => seek(2, offset as isize),
            }
        };
        Poll::Ready(Ok(new_pos as u64))
    }
}

struct RomHackCounter;

impl AsyncWrite for RomHackCounter {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        count_write(buf.len());
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        Poll::Ready(Ok(()))
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncSeek for RomHackCounter {
    fn poll_seek(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        pos: SeekFrom,
    ) -> Poll<Result<u64, std::io::Error>> {
        let new_pos = {
            match pos {
                SeekFrom::Start(offset) => seek(0, offset as isize),
                SeekFrom::End(offset) => seek(1, offset as isize),
                SeekFrom::Current(offset) => seek(2, offset as isize),
            }
        };
        Poll::Ready(Ok(new_pos as u64))
    }
}

#[no_mangle]
#[wasm_bindgen]
pub extern "C" fn alloc(size: usize) -> *mut u8 {
    unsafe { allocate(Layout::from_size_align_unchecked(size, 1)) }
}

#[no_mangle]
#[wasm_bindgen]
pub extern "C" fn dealloc(ptr: *mut u8, size: usize) {
    unsafe { deallocate(ptr, Layout::from_size_align_unchecked(size, 1)) }
}

static mut ISO_READ_STATE: Option<IsoReadState> = None;
static mut THREAD_POOL: Option<rayon::ThreadPool> = None;
static IS_WORKER: AtomicBool = AtomicBool::new(false);
static mut COMPILER_FUTURE: Option<CompilerFuture> = None;

#[wasm_bindgen(start, catch)]
pub fn main() {
    spawn_local(async move {
        unsafe {
            if IS_WORKER
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                console_error_panic_hook::set_once();
                let r = web_worker::default_thread_pool(Some(
                    js_sys::Reflect::get(
                        &js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("navigator"))
                            .unwrap(),
                        &JsValue::from_str("hardwareConcurrency"),
                    )
                    .unwrap()
                    .as_f64()
                    .unwrap() as usize,
                ))
                .ok_or(JsValue::from("Could not create the thread pool."));
                THREAD_POOL = match r {
                    Ok(pool) => Some(pool),
                    Err(e) => {
                        web_sys::console::error_1(&e);
                        None
                    }
                };
                web_sys::console::log_1(&JsValue::from_str("Hello from the main script!"));
                ISO_READ_STATE = Some(Default::default());
            }
        }
    });
}

static ERROR_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[no_mangle]
#[wasm_bindgen]
pub fn error(message: String) {
    let n_err = ERROR_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if n_err == 0 {
        key_val_print("Error".into(), &message, Some(MessageKind::Error.into()));
    } else {
        key_val_print(
            "Caused by".into(),
            &message,
            Some(MessageKind::Error.into()),
        );
    }
}

static mut PATCH: Option<Box<[u8]>> = None;

#[no_mangle]
#[wasm_bindgen]
pub fn create_romhack(patch: &[u8]) -> js_sys::Promise {
    let mut p = Vec::new();
    p.extend_from_slice(patch);
    unsafe {
        drop(PATCH.replace(p.into_boxed_slice()));
    }
    return js_sys::Promise::new(&mut move |resolve, reject| unsafe {
        if let Some(pool) = &THREAD_POOL {
            spawn_local(async move {
                let fut = CompilerFuture::new();
                let res;
                // fut.running.store(true, Ordering::Relaxed);
                COMPILER_FUTURE = Some(fut);
                web_sys::console::debug_1(&JsValue::from_str("Starting compiler thread..."));
                web_sys::console::debug_1(&JsValue::from_str("thread pool found"));
                pool.install(move || {
                    web_sys::console::log_1(&JsValue::from_str("From worker: Hi!"));
                    let x = [
                        "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth",
                    ];
                    x.par_iter().for_each(|s| {
                        web_sys::console::debug_1(&JsValue::from_str(s));
                    });
                    start_threaded(PATCH.as_ref().unwrap());
                });
                web_sys::console::debug_1(&JsValue::from_str("waiting for worker to finish..."));
                let fut;
                {
                    fut = COMPILER_FUTURE.as_mut().unwrap();
                    res = fut.await;
                }
                web_sys::console::debug_1(&JsValue::from_str("worker finished!"));
                if let Err(e) = res {
                    error(format!("Backtrace: {:?}", e.backtrace()));
                    for cause in e.iter_chain() {
                        error(format!("{:?}", cause));
                    }
                    let _ = js_sys::Function::apply(
                        &reject,
                        &JsValue::undefined(),
                        &js_sys::Array::of1(&JsValue::from_bool(false)),
                    );
                } else {
                    let _ = js_sys::Function::apply(
                        &resolve,
                        &JsValue::undefined(),
                        &js_sys::Array::of1(&JsValue::from_bool(true)),
                    );
                }
            });
        } else {
            web_sys::console::debug_1(&JsValue::from_str("No thread pool found"));
            let p = wasm_bindgen_futures::future_to_promise(async move {
                // COMPILER_FUTURE
                //     .as_ref()
                //     .unwrap()
                //     .running
                //     .store(true, Ordering::Relaxed);
                let r = try_create_romhack(PATCH.as_ref().unwrap()).await;
                // COMPILER_FUTURE
                //     .as_ref()
                //     .unwrap()
                //     .running
                //     .store(false, Ordering::Relaxed);
                if let Err(e) = r {
                    Err(JsValue::from_str(&format!("{}", e)))
                } else {
                    Ok(JsValue::from_bool(true))
                }
            });
            let _ =
                js_sys::Function::apply(&resolve, &JsValue::undefined(), &js_sys::Array::of1(&p));
        }
    });
}

struct CompilerFuture {
    handle: Option<Pin<Box<(dyn Future<Output = ()>)>>>,
    result: Mutex<Option<Error>>,
    running: AtomicBool,
    waker: Option<Waker>,
}

impl CompilerFuture {
    fn new() -> Self {
        CompilerFuture {
            handle: None,
            result: Mutex::new(None),
            running: AtomicBool::new(false),
            waker: None,
        }
    }
}

impl Future for CompilerFuture {
    type Output = Result<(), Error>;
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        ctx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<<Self as futures::Future>::Output> {
        self.waker.replace(ctx.waker().clone());
        let mut res: Result<(), Error> = Ok(());
        if let Some(handle) = self.handle.as_mut() {
            if let Poll::Ready(_) = handle.as_mut().poll(ctx) {
                self.handle.take();
            } else {
                return Poll::Pending;
            }
        }
        if Ok(false)
            == self
                .running
                .compare_exchange(false, false, Ordering::SeqCst, Ordering::SeqCst)
        {
            match self.result.try_lock() {
                Some(mut opt) => {
                    let err = opt.take();
                    if let Some(e) = err {
                        res = Err(e);
                    }
                }
                None => {
                    return Poll::Ready(Ok(()));
                }
            }
        } else {
            return Poll::Pending;
        }
        Poll::Ready(res)
    }
}

fn start_threaded(patch: &'static [u8]) {
    web_sys::console::debug_1(&JsValue::from_str("before spawn_local"));
    unsafe {
        COMPILER_FUTURE
            .as_mut()
            .unwrap()
            .handle
            .replace(Box::pin(async move {
                web_sys::console::debug_1(&JsValue::from_str("in spawn_local"));
                let running;
                let result;
                let mut waker;
                {
                    running = &(COMPILER_FUTURE.as_ref().unwrap().running);
                    result = &(COMPILER_FUTURE.as_ref().unwrap().result);
                    waker = (&(COMPILER_FUTURE.as_ref().unwrap().waker)).as_ref();
                    let res = running.compare_exchange(
                        false,
                        true,
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    );
                    web_sys::console::debug_1(&JsValue::from_str(&format!("{:?}", res)));
                    if let Ok(false) = res {
                        if let Err(e) = try_create_romhack(patch).await {
                            result.lock().await.replace(e);
                        }
                        running.store(false, Ordering::SeqCst);
                        if let Some(w) = waker.take() {
                            w.wake_by_ref();
                        }
                    }
                    else {
                        web_sys::console::error_1(&JsValue::from_str("could not change running state"));
                        result.lock().await.replace(err_msg("could not change running state"));
                    }
                }
                web_sys::console::debug_1(&JsValue::from_str("end of spawn_local"));
            }));
    }
    web_sys::console::debug_1(&JsValue::from_str("after spawn_local"));
}

async fn try_create_romhack(patch: &[u8]) -> Result<(), Error> {
    let (zip, compiled_library, mut config) =
        open_config_from_patch(Cursor::new(patch)).context("Couldn't open patch file")?;
    if let Some(name) = &config.info.game_name {
        set_name(name.clone());
    }

    use romhack_backend::iso;
    use wii_crypto::transform;
    let disc = Disc::extract(RomHackReader)
        .await
        .context("Couldn't load iso into buffer")?;
    match disc {
        transform::Disc::Wii(mut wii_disc) => {
            JSPrinter.print(None, "Extract", "File System");
            let iso = iso::reader::load_iso_wii(&mut wii_disc, &JSPrinter)
                .await
                .context("Couldn't parse the ISO")?;
            JSPrinter.print(None, "Building", "new ISO");
            let iso = build_iso(&JSPrinter, zip, iso, compiled_library, &mut config)
                .context("Couldn't build iso")?;

            let mut patched_disc = transform::WiiPatchedDisc::from(wii_disc);
            let mut vec_writer = futures::io::Cursor::new(Vec::new());
            JSPrinter.print(None, "Writing", "Partition");
            iso::writer::write_iso(&mut vec_writer, &iso)
                .await
                .context("Couldn't write partition")?;
            let mut vec = vec_writer.into_inner();
            patched_disc.partition.header.data_size =
                transform::align_addr((vec.len() as u64 * 0x8000) / 0x7C00, 21);
            vec.resize(patched_disc.partition.header.data_size as usize, 0);
            patched_disc.partition.data = vec.into_boxed_slice();
            JSPrinter.print(None, "Measuring", "Rom Hack File Size");
            RomHackCounter
                .seek(SeekFrom::Start(
                    0x50000
                        + patched_disc.partition.header.data_offset
                        + patched_disc.partition.header.data_size,
                ))
                .await
                .context("Couldn't seek for counter")?;
            restart();
            JSPrinter.print(None, "Encrypting", "ISO");
            futures::io::copy(
                &mut transform::WiiEncryptStream::new(patched_disc),
                &mut BufWriter::new(RomHackWriter),
            )
            .await
            .context("Couldn't encrypt the final ISO")?;
        }
        transform::Disc::GameCube(buf) => {
            let iso = iso::reader::load_iso(&buf[..]).context("Couldn't parse the ISO")?;
            let iso = build_iso(&JSPrinter, zip, iso, compiled_library, &mut config)?;
            JSPrinter.print(None, "Measuring", "Rom Hack File Size");
            write_iso(&mut RomHackCounter, &iso).await?;
            restart();
            JSPrinter.print(None, "Writing", "Rom Hack");
            iso::writer::write_iso(
                &mut BufWriter::new(RomHackWriter), // &mut BufWriter::with_capacity(4 << 20u8, RomHackWriter)
                &iso,
            )
            .await
            .context("Couldn't write the final ISO")?;
        }
    }

    Ok(())
}
