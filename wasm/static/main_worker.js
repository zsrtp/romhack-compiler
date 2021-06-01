let context = {
    cursor: 0,
    read_cursor: BigInt(0),
    len: 0,
    name: "RomHack",
    errorCount: 0,
    file: null,
    buffers: [],
};

async function iso_read(slice) {
    let offset = context.read_cursor;
    let size = Math.min(Number(BigInt(context.file.size) - offset), slice.byteLength);
    // console.debug(`Reading 0x${size.toString(16)} bytes at 0x${offset.toString(16)}...`);
    if (size > 0) {
        let buffer = await context.file.slice(Number(offset) >>> 0, (Number(offset) + size) >>> 0).arrayBuffer();
        slice.set(new Uint8Array(buffer));
    }
    context.read_cursor += BigInt(size);
    // console.debug("Done");
    return size;
}

function iso_seek(kind, offset) {
    if (kind == 0) {
        context.read_cursor = BigInt(offset);
    } else if (kind == 1) {
        context.read_cursor = BigInt(context.file.size) - BigInt(offset);
    } else {
        context.read_cursor += BigInt(offset);
    }
    return context.read_cursor;
}

const WRITE_CHUCK_SIZE = (1 << 31) >>> 0;

function write(slice) {
    let len = slice.byteLength;
    console.debug(`Count Write: len=${len}; cursor=${context.cursor}`);
    const src = slice;
    let start_cursor = context.cursor;
    let start_idx = (context.cursor / WRITE_CHUCK_SIZE) >>> 0;
    let end_idx = ((context.cursor + len - 1) / WRITE_CHUCK_SIZE) >>> 0;
    for (let i = start_idx; i <= end_idx; i++) {
        let array = context.buffers[i];
        let chuck_start = Math.max(context.cursor - i * WRITE_CHUCK_SIZE, 0);
        let read_size = Math.min(WRITE_CHUCK_SIZE - chuck_start, len - i * WRITE_CHUCK_SIZE);
        new Uint8Array(array).set(src.slice(context.cursor - start_cursor), chuck_start);
        context.cursor += read_size;
    }
    return len;
}

function seek(kind, offset) {
    if (kind == 0) {
        context.cursor = offset;
    } else if (kind == 1) {
        context.cursor = context.len - offset;
    } else {
        context.cursor += offset;
    }
    return context.cursor;
}

function count_write(len) {
    context.cursor += len;
    if (context.cursor > context.len) {
        context.len = context.cursor;
    }
    console.debug(`Count Write: len=${len}; cursor=${context.cursor}; context len=${context.len}`)
    return len;
}

function restart() {
    context.cursor = 0;
    context.buffers = new Array((((context.len - 1) / WRITE_CHUCK_SIZE) >>> 0) + 1);
    for (let i = 0; i < context.buffers.length - 1; i++) {
        context.buffers[i] = new ArrayBuffer(WRITE_CHUCK_SIZE);
    }
    context.buffers[context.buffers.length - 1] = new ArrayBuffer(context.len % WRITE_CHUCK_SIZE);
    context.buffer = new ArrayBuffer(context.len);
}

function set_name(str) {
    context.name = str;
}

function keyValPrint(key, val, kind) {
    postMessage([0, "call", [key, val, kind]]);
}

// This is to be able to simply import the module in the browser if we need to use a non-module script.
globalThis.RomHack = {iso_read, iso_seek, write, seek, count_write, restart, set_name, keyValPrint};

importScripts("./romhack/romhack.js");

let function_list = [compileRomhack, set_ctx, get_ctx];

// let memory = new WebAssembly.Memory({initial:31,maximum:16384,shared:true});
let memory;
wasm_bindgen("./romhack/romhack_bg.wasm", memory).then((exports) => {
    memory = exports.memory;
    globalThis.addEventListener("message", onMessage);
});

async function onMessage(ev) {
    let [idx, type, data] = ev.data;
    console.debug("[main worker]" , `data:`, ev.data, `type "${type}";`, `idx: ${idx}`);
    if (type == "call") {
        await function_list[idx].apply(undefined, data);
    }
};

async function compileRomhack(buffer) {
    try {
        let ret = await wasm_bindgen.create_romhack(new Uint8Array(buffer));
        postMessage([0, "call_ret", ret]);
    }
    catch (e) {
        postMessage([0, "call_error", e]);
    }
}

async function set_ctx(ctx) {
    try {
        context = {...ctx};
        postMessage([1, "call_ret", undefined]);
    }
    catch (e) {
        postMessage([1, "call_error", e]);
    }
}

async function get_ctx() {
    try {
        postMessage([2, "call_ret", context], [context]);
    }
    catch (e) {
        postMessage([2, "call_error", e]);
    }
}