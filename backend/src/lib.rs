extern crate byteorder;
extern crate encoding_rs;
#[macro_use]
extern crate failure;
extern crate goblin;
extern crate image;
extern crate regex;
extern crate rustc_demangle;
#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate standalone_syn as syn;
extern crate toml;
extern crate zip;
extern crate wii_crypto;

mod assembler;
mod banner;
mod config;
mod demangle;
mod dol;
mod file_source;
mod framework_map;
pub mod iso;
mod key_val_print;
mod linker;

use assembler::Assembler;
use assembler::Instruction;
use banner::Banner;
use config::Config;
use dol::DolFile;
use failure::{err_msg, Error, ResultExt};
use file_source::{FileSource, FileSystem};
use iso::virtual_file_system::Directory;
use iso::consts::*;
pub use key_val_print::{DontPrint, KeyValPrint, MessageKind};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{prelude::*, BufReader, BufWriter};
use std::iter::Iterator;
use std::mem;
use std::path::PathBuf;
use std::process::Command;
use zip::{write::FileOptions, ZipArchive, ZipWriter};
use wii_crypto::wii_disc::{parse_disc, finalize_iso, Partitions};
use wii_crypto::array_stream::{VecWriter};
use byteorder::{ByteOrder, BE};

pub fn build<P: KeyValPrint>(printer: &P, debug: bool, patch: bool) -> Result<(), Error> {
    let mut toml_buf = String::new();
    File::open("RomHack.toml")
        .context("Couldn't find \"RomHack.toml\".")?
        .read_to_string(&mut toml_buf)
        .context("Failed to read \"RomHack.toml\".")?;

    let config: Config = toml::from_str(&toml_buf).context("Can't parse RomHack.toml")?;

    printer.print(None, "Compiling", "");

    {
        let mut command = Command::new("cargo");
        command
            .args(&["build", "--target", "powerpc-unknown-linux-gnu"])
            .env("RUSTFLAGS", "-C target-feature=+msync,+fres,+frsqrte");

        if !debug {
            command.arg("--release");
        }

        if let Some(ref src_dir) = config.src.src {
            command.current_dir(src_dir);
        }

        let exit_code = command
            .spawn()
            .context("Couldn't build the project")?
            .wait()?;

        ensure!(exit_code.success(), "Couldn't build the project");
    }

    let path_to_compiled_lib =
        find_compiled_library(debug).context("Couldn't find the compiled static library")?;
    let compiled_lib =
        fs::read(path_to_compiled_lib).context("Couldn't read the compiled static library")?;

    if patch {
        build_patch(printer, compiled_lib, config)
    } else {
        build_and_emit_iso(printer, FileSystem, compiled_lib, config)
    }
}

pub fn apply_patch<P: KeyValPrint>(
    printer: &P,
    patch: PathBuf,
    original_game: PathBuf,
    output: PathBuf,
) -> Result<(), Error> {
    printer.print(None, "Parsing", "patch");

    let (zip, compiled_library, mut config) = open_config_from_patch(BufReader::new(
        File::open(patch).context("Couldn't open the patch file")?,
    ))?;

    config.src.iso = original_game;
    config.build.iso = output;

    build_and_emit_iso(printer, zip, compiled_library, config)
}

pub fn build_raw<P: KeyValPrint>(printer: &P, patch: bool) -> Result<(), Error> {
    printer.print(None, "Parsing", "RomHack.toml");

    let mut buffer = Vec::new();

    let mut config: Config = {
        let mut toml_file = File::open("RomHack.toml").context("RomhHack.toml cannot be found")?;
        toml_file
            .read_to_end(&mut buffer)
            .context("Couldn't read RomHack.toml")?;
        toml::from_slice(&buffer).context("Can't parse RomHack.toml")?
    };
    match config.link.libs {
        Some(ref vec) => {
            if vec.len() == 0 {
                bail!("No libraries suplied")
            }
        }
        None => bail!("No libraries suplied"),
    }

    {
        buffer.clear();
        let lib: &PathBuf = config.link.libs.as_ref().unwrap().get(0).clone().unwrap();
        let mut file = File::open(lib).context("Failed to open library cannot be found")?;
        file.read_to_end(&mut buffer)
            .context("Failed to read library")?;
    }

    config.link.libs.as_mut().unwrap().remove(0);
    if patch {
        build_patch(printer, buffer, config)
    } else {
        build_and_emit_iso(printer, FileSystem, buffer, config)
    }
}

pub fn open_config_from_patch<R: Read + Seek>(
    reader: R,
) -> Result<(ZipArchive<R>, Vec<u8>, Config), Error> {
    let mut zip = ZipArchive::new(reader).context("Couldn't parse patch file")?;

    let mut buffer = Vec::new();

    let config: Config = {
        let mut toml_file = zip
            .by_name("RomHack.toml")
            .context("The patch file doesn't contain the patch index")?;

        toml_file
            .read_to_end(&mut buffer)
            .context("Couldn't read the patch index")?;

        toml::from_slice(&buffer).context("Can't parse patch index")?
    };

    {
        let mut compiled_library = zip
            .by_name("libcompiled.a")
            .context("The patch file doesn't contain the compiled library")?;
        buffer.clear();
        compiled_library
            .read_to_end(&mut buffer)
            .context("Couldn't read the compiled library")?;
    }

    Ok((zip, buffer, config))
}

fn add_file_to_zip(
    index: usize,
    iso_path: &String,
    actual_path: &PathBuf,
    zip: &mut ZipWriter<BufWriter<File>>,
    new_map: &mut HashMap<String, PathBuf>
) -> Result<(), Error> {
    let zip_path = format!("replace{}.dat", index);
    new_map.insert(iso_path.clone(), PathBuf::from(&zip_path));
    zip.start_file(zip_path, FileOptions::default())
        .context("Failed creating a new patch file entry")?;

    zip.write_all(&fs::read(actual_path).with_context(|_| {
        format!(
            "Couldn't read the file \"{}\" to store it in the patch.",
            actual_path.display()
        )
    })?)
    .context("Failed storing a file in the patch")?;
    Ok(())
}

fn add_entry_to_zip(
    index: &mut usize,
    iso_path: &String,
    actual_path: &PathBuf,
    zip: &mut ZipWriter<BufWriter<File>>,
    new_map: &mut HashMap<String, PathBuf>
) -> Result<(), Error> {
    if actual_path.is_file() {
        *index = *index + 1;
        add_file_to_zip(*index, iso_path, actual_path, zip, new_map)?;
    } else if actual_path.is_dir() {
        for entry in fs::read_dir(actual_path)? {
            let entry = entry?;
            let entry_path = entry.path();
            let file_name = entry_path.file_name().expect("Entry has no name");
            let iso_path = String::from(iso_path) + &String::from('/') + &String::from(file_name.to_str().unwrap());
            add_entry_to_zip(index, &iso_path, &entry_path, zip, new_map)?;
        }
    }
    Ok(())
}

fn build_patch<P: KeyValPrint>(
    printer: &P,
    compiled_library: Vec<u8>,
    mut config: Config,
) -> Result<(), Error> {
    printer.print(None, "Creating", "patch file");

    config.build.iso.set_extension("patch");
    let mut zip = ZipWriter::new(BufWriter::new(
        File::create(&config.build.iso).context("Couldn't create the patch file")?,
    ));

    printer.print(None, "Storing", "replacement files");

    let mut new_map = HashMap::new();
    let mut index = 0;
    for (iso_path, actual_path) in config.files.iter() {
        add_entry_to_zip(&mut index, iso_path, actual_path, &mut zip, &mut new_map)?;
    }
    config.files = new_map;

    printer.print(None, "Storing", "libraries");

    zip.start_file("libcompiled.a", FileOptions::default())
        .context("Failed creating a new patch file entry for the compiled library")?;
    zip.write_all(&compiled_library)
        .context("Failed storing the compiled library in the patch")?;

    let mut modified_libs = Vec::new();
    for (index, lib_path) in config.link.libs.iter().flat_map(|x| x).enumerate() {
        let zip_path = format!("lib{}.a", index);

        printer.print(
            Some(MessageKind::Warning),
            "Storing",
            &format!("{:?} as {}", lib_path, zip_path),
        );
        modified_libs.push(zip_path.clone());

        zip.start_file(zip_path, FileOptions::default())
            .context("Failed creating a new patch file entry")?;

        let file_buf = fs::read(lib_path).with_context(|_| {
            format!(
                "Couldn't load \"{}\". Did you build the project correctly?",
                lib_path.display()
            )
        })?;
        zip.write_all(&file_buf)
            .context("Failed storing a library in the patch")?;
    }

    if let Some(path) = &mut config.src.patch {
        printer.print(None, "Storing", "patch.asm");

        zip.start_file("patch.asm", FileOptions::default())
            .context("Failed to create the patch.asm file in the patch")?;
        let file_buf = fs::read(&*path).context("Couldn't read the patch.asm file")?;
        zip.write_all(&file_buf)
            .context("Failed storing the patch.asm file in the patch")?;
        *path = PathBuf::from("patch.asm");
    }

    if let Some(path) = &mut config.info.image {
        printer.print(None, "Storing", "banner");

        zip.start_file("banner.dat", FileOptions::default())
            .context("Failed to create the banner file in the patch")?;
        let file_buf = fs::read(&*path).context("Couldn't read the banner file")?;
        zip.write_all(&file_buf)
            .context("Failed storing the banner file in the patch")?;
        *path = PathBuf::from("banner.dat");
    }

    printer.print(None, "Storing", "patch index");

    config.src.iso = PathBuf::new();
    config.build = Default::default();
    zip.start_file("RomHack.toml", FileOptions::default())
        .context("Failed to create the patch index")?;
    if modified_libs.len() > 0 {
        config.link.libs = Some(modified_libs.into_iter().map(|x| PathBuf::from(x)).collect());
    }
    let config = toml::to_vec(&config).context("Couldn't encode the patch index")?;
    zip.write_all(&config)
        .context("Failed storing the patch index")?;

    Ok(())
}

fn add_file_to_iso<F: FileSource>(
    iso_path: &String,
    actual_path: &PathBuf,
    iso: &mut Directory,
    files: &mut F
) -> Result<(), Error> {
    iso.resolve_and_create_path(&iso_path).data = files
        .read_to_vec(actual_path)
        .with_context(|_| {
            format!(
                "Couldn't read the file \"{}\" to store it in the ISO.",
                actual_path.display()
            )
        })?
        .into();
    Ok(())
}

fn is_wii(buf: &[u8]) -> bool {
    BE::read_u32(&buf[OFFSET_WII_MAGIC..]) == 0x5D1C9EA3
}

pub fn build_iso<'a, P: KeyValPrint, F: FileSource>(
    printer: &P,
    mut files: F,
    original_iso: &'a mut [u8],
    compiled_library: Vec<u8>,
    config: &'a mut Config,
) -> Result<Directory<'a>, Error> {
    let mut part_opt: Option<Partitions> = None;
    if is_wii(original_iso) {
        printer.print(None, "Decrypting", "partitions");
        match parse_disc(&mut original_iso[..]) {
            Ok(part_opt_parsed) => {
                part_opt = part_opt_parsed.clone();
            },
            Err(_) => (),
        }
    }
    let mut iso = iso::reader::load_iso(&original_iso[..], &part_opt).context("Couldn't parse the ISO")?;

    printer.print(None, "Replacing", "files");

    for (iso_path, actual_path) in &config.files {
        add_file_to_iso(iso_path, actual_path, &mut iso, &mut files)?;
    }

    let mut original_symbols = HashMap::new();
    if let Some(framework_map) = config.src.map.as_ref().and_then(|m| iso.resolve_path(m)) {
        printer.print(None, "Parsing", "symbol map");
        original_symbols = framework_map::parse(&framework_map.data)
            .context("Couldn't parse the game's symbol map")?;
    } else {
        printer.print(
            Some(MessageKind::Warning),
            "Warning",
            "No symbol map specified or it wasn't found",
        );
    }

    printer.print(None, "Linking", "");

    let mut libs_to_link = Vec::with_capacity(config.link.libs.as_ref().map_or(0, |x| x.len()) + 2);

    libs_to_link.push(compiled_library);

    for lib_path in config.link.libs.iter().flat_map(|x| x) {
        let file_buf = files.read_to_vec(lib_path).with_context(|_| {
            format!(
                "Couldn't load \"{}\". Did you build the project correctly?",
                lib_path.display()
            )
        })?;
        libs_to_link.push(file_buf);
    }

    libs_to_link.push(linker::BASIC_LIB.to_owned());

    let base_address: syn::LitInt =
        syn::parse_str(&config.link.base).context("Invalid Base Address")?;

    let linked = linker::link(
        printer,
        &libs_to_link,
        base_address.value() as u32,
        config.link.entries.clone(),
        &original_symbols,
    )
    .context("Couldn't link the Rom Hack")?;

    printer.print(None, "Creating", "symbol map");

    // TODO NLL bind framework_map to local variable
    framework_map::create(
        &config,
        config
            .src
            .map
            .as_ref()
            .and_then(|m| iso.resolve_path(m))
            .map(|f| &*f.data),
        &linked.sections,
    )
    .context("Couldn't create the new symbol map")?;

    let mut instructions = Vec::new();
    if let Some(patch) = config.src.patch.take() {
        printer.print(None, "Parsing", "patch");

        let asm = files
            .read_to_string(&patch)
            .with_context(|_| format!("Couldn't read the patch file \"{}\".", patch.display()))?;

        let lines = &asm.lines().collect::<Vec<_>>();

        let mut assembler = Assembler::new(linked.symbol_table, &original_symbols);
        instructions = assembler
            .assemble_all_lines(lines)
            .context("Couldn't assemble the patch file lines")?;
    }

    {
        printer.print(None, "Patching", "game");

        let main_dol = iso
            .main_dol_mut()
            .ok_or_else(|| err_msg("Dol file not found"))?;

        let original = DolFile::parse(&main_dol.data);
        main_dol.data = patch_instructions(original, linked.dol, &instructions)
            .context("Couldn't patch the game")?
            .into();
    }
    if iso.is_gamecube_iso() {
        printer.print(None, "Patching", "banner");

        if let Some(banner_file) = iso.banner_mut() {
            // TODO Not always true
            let is_japanese = true;
            let mut banner = Banner::parse(is_japanese, &banner_file.data)
                .context("Couldn't parse the banner")?;

            if let Some(game_name) = config.info.game_name.take() {
                banner.game_name = game_name;
            }
            if let Some(developer_name) = config.info.developer_name.take() {
                banner.developer_name = developer_name;
            }
            if let Some(full_game_name) = config.info.full_game_name.take() {
                banner.full_game_name = full_game_name;
            }
            if let Some(full_developer_name) = config.info.full_developer_name.take() {
                banner.full_developer_name = full_developer_name;
            }
            if let Some(game_description) = config.info.description.take() {
                banner.game_description = game_description;
            }
            if let Some(image_path) = config.info.image.take() {
                let image = files
                    .open_image(image_path)
                    .context("Couldn't open the banner replacement image")?
                    .to_rgba();
                banner.image.copy_from_slice(&image);
            }
            banner_file.data = banner.to_bytes(is_japanese).to_vec().into();
        } else {
            printer.print(Some(MessageKind::Warning), "Warning", "No banner to patch");
        }
    }

    Ok(iso)
}

pub fn build_and_emit_iso<P: KeyValPrint, F: FileSource>(
    printer: &P,
    files: F,
    compiled_library: Vec<u8>,
    mut config: Config,
) -> Result<(), Error> {
    printer.print(None, "Loading", "original game");

    let mut buf = iso::reader::load_iso_buf(&config.src.iso)
        .with_context(|_| format!("Couldn't find \"{}\".", config.src.iso.display()))?;
    let out_path = mem::replace(&mut config.build.iso, Default::default());

    let iso = build_iso(printer, files, &mut buf[..], compiled_library, &mut config)?;

    printer.print(None, "Building", "ISO");

        // File::open(out_path).context("Couldn't open the final ISO")?,
    if !iso.is_wii_iso() {
        iso::writer::write_iso(
            &mut BufWriter::with_capacity(
                4 << 20,
                File::create(out_path.clone()).context("Couldn't create the final ISO")?,
            ),
            &iso,
        )
        .context("Couldn't write the final ISO")?;
    }
    else {
        let mut vec_writer = VecWriter::new();
        iso::writer::write_iso(
            &mut vec_writer,
            &iso,
        )
        .context("Couldn't write the final ISO")?;
        printer.print(None, "Encrypting", "ISO");
        finalize_iso(vec_writer.as_slice(), &mut buf)?;
        printer.print(None, "Writing", "ISO to file");
        let mut file_writer = File::create(out_path.clone()).context("Couldn't create the final ISO")?;
        file_writer.write_all(&buf)?;
    }

    Ok(())
}

pub fn new(name: &str) -> Result<(), Error> {
    let exit_code = Command::new("cargo")
        .args(&["new", "--lib", &name])
        .spawn()
        .context("Couldn't create the cargo project")?
        .wait()?;

    ensure!(exit_code.success(), "Couldn't create the cargo project");

    let mut file = File::create(format!("{}/RomHack.toml", name))
        .context("Couldn't create the RomHack.toml")?;
    write!(
        file,
        r#"[info]
game-name = "{0}"

[src]
iso = "game.iso" # Provide the path of the game's ISO
patch = "src/patch.asm"
# Optionally specify the game's symbol map
# map = "maps/framework.map"

[files]
# You may replace or add new files to the game here
# "path/to/file/in/iso" = "path/to/file/on/harddrive"

[build]
map = "target/framework.map"
iso = "target/{0}.iso"

[link]
entries = ["init"] # Enter the exported function names here
base = "0x8040_1000" # Enter the start address of the Rom Hack's code here
"#,
        name.replace('-', "_"),
    )
    .context("Couldn't write the RomHack.toml")?;

    let mut file = File::create(format!("{}/src/lib.rs", name))
        .context("Couldn't create the lib.rs source file")?;
    write!(
        file,
        "{}",
        r#"#![no_std]

pub mod panic;

#[no_mangle]
pub extern "C" fn init() {}
"#
    )
    .context("Couldn't write the lib.rs source file")?;

    let mut file = File::create(format!("{}/src/panic.rs", name))
        .context("Couldn't create the panic.rs source file")?;
    write!(
        file,
        "{}",
        r#"#[cfg(any(target_arch = "powerpc", target_arch = "wasm32"))]
#[panic_handler]
pub fn panic(_info: &::core::panic::PanicInfo) -> ! {
    loop {}
}
"#
    )
    .context("Couldn't write the panic.rs source file")?;

    let mut file = File::create(format!("{}/src/patch.asm", name))
        .context("Couldn't create the default patch file")?;
    write!(
        file,
        r#"; You can use this to patch the game's code to call into the Rom Hack's code
"#
    )
    .context("Couldn't write the default patch file")?;

    let mut file = OpenOptions::new()
        .append(true)
        .open(format!("{}/Cargo.toml", name))
        .context("Couldn't open the Cargo.toml")?;
    write!(
        file,
        "{}",
        r#"# Comment this in if you want to use the gcn crate in your rom hack.
# It requires the operating system symbols to be resolved via a map.
# gcn = { git = "https://github.com/CryZe/gcn", features = ["panic"] }

[lib]
crate-type = ["staticlib"]

[profile.dev]
panic = "abort"
opt-level = 1

[profile.release]
panic = "abort"
lto = true
"#
    )
    .context("Couldn't write into the Cargo.toml")?;

    let mut file = File::create(format!("{}/.gitignore", name))
        .context("Couldn't create the gitignore file")?;
    write!(
        file,
        r#"/target
**/*.rs.bk
"#
    )
    .context("Couldn't write the gitignore file")?;

    Ok(())
}

fn patch_instructions(
    mut original: DolFile,
    intermediate: DolFile,
    instructions: &[Instruction],
) -> Result<Vec<u8>, Error> {
    original.append(intermediate);
    original
        .patch(instructions)
        .context("Couldn't patch the DOL")?;

    Ok(original.to_bytes())
}

fn find_compiled_library(debug: bool) -> Result<PathBuf, Error> {
    use std::iter::FromIterator;

    let dir = fs::read_dir(PathBuf::from_iter(&[
        "target",
        "powerpc-unknown-linux-gnu",
        if debug { "debug" } else { "release" },
    ]))
    .context("Couldn't list entries of the compiler's target directory")?;

    for entry in dir {
        let entry = entry.context("Couldn't list an entry of the compiler's target directory")?;
        let path = entry.path();
        if path.extension() == Some("a".as_ref()) {
            return Ok(path);
        }
    }

    bail!("None of the files in the compiler's target directory match *.a")
}
