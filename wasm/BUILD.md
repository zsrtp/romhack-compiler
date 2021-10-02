# Building for WebAssembly

## Requirements

The requirements for building for WebAssebly are:

- Installing the [Rust Toolchain](https://www.rust-lang.org/tools/install)
- Use the tool `cargo` to install `wasm-pack`:
  ```shell
  $ cargo install wasm-pack
  ```

## Instructions

To build the Romhack Compiler for web use, you need to follow the next instructions:

- In a terminal, go to the folder `/wasm`
- Compile the project using the `wasm-pack` tool:
  ```shell
  $ wasm-pack build
  ```
- From the generated folder (`pkg`), take the file `romhack_bg.wasm` and copy it in the `/wasm/static` folder.
- You can now run an HTTP server in the `/wasm/static` folder and access the `index.html` file.