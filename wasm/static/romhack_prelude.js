import { context } from './run_prelude.js';
export { keyValPrint } from './run_prelude.js';

export async function iso_read(slice) {
    let offset = context.read_cursor;
    let size = Math.min(Number(BigInt(context.file.size) - offset), slice.byteLength);
    console.info(`Reading 0x${size.toString(16)} bytes at 0x${offset.toString(16)}...`);
    console.debug(new Error());
    if (size > 0) {
        let buffer = await context.file.slice(Number(offset) >>> 0, (Number(offset) + size) >>> 0).arrayBuffer();
        slice.set(new Uint8Array(buffer));
    }
    context.read_cursor += BigInt(size);
    console.info("Done");
    return size;
}

export function iso_seek(kind, offset) {
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

export function write(slice) {
    console.debug(`Count Write: len=${len}; cursor=${context.cursor}; buffer len=${context.buffers.reduce((prev, curr) => prev + curr.byteLength)}`);
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

export function seek(kind, offset) {
    if (kind == 0) {
        context.cursor = offset;
    } else if (kind == 1) {
        context.cursor = context.len - offset;
    } else {
        context.cursor += offset;
    }
    return context.cursor;
}

export function count_write(len) {
    context.cursor += len;
    if (context.cursor > context.len) {
        context.len = context.cursor;
    }
    console.debug(`Count Write: len=${len}; cursor=${context.cursor}; context len=${context.len}`)
    return len;
}

export function restart() {
    context.cursor = 0;
    context.buffers = new Array((((context.len - 1) / WRITE_CHUCK_SIZE) >>> 0) + 1);
    for (let i = 0; i < context.buffers.length - 1; i++) {
        context.buffers[i] = new ArrayBuffer(WRITE_CHUCK_SIZE);
    }
    context.buffers[context.buffers.length - 1] = new ArrayBuffer(context.len % WRITE_CHUCK_SIZE);
    context.buffer = new ArrayBuffer(context.len);
}

export function set_name(str) {
    context.name = str;
}

// This is to be able to simply import the module in the browser if we need to use a non-module script.
globalThis.RomHack = {iso_read, iso_seek, write, seek, count_write, restart, set_name};