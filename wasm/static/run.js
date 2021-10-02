const compiledModule = fetch("romhack_bg.wasm")
  .then((r) => r.arrayBuffer())
  .then((b) => WebAssembly.compile(b));

let decodeUtf8;
if (typeof window["TextDecoder"] === "undefined") {
  decodeUtf8 = (data) => {
    var str = "",
      i;

    for (i = 0; i < data.length; i++) {
      var value = data[i];

      if (value < 0x80) {
        str += String.fromCharCode(value);
      } else if (value > 0xbf && value < 0xe0) {
        str += String.fromCharCode(
          ((value & 0x1f) << 6) | (data[i + 1] & 0x3f)
        );
        i += 1;
      } else if (value > 0xdf && value < 0xf0) {
        str += String.fromCharCode(
          ((value & 0x0f) << 12) |
            ((data[i + 1] & 0x3f) << 6) |
            (data[i + 2] & 0x3f)
        );
        i += 2;
      } else {
        var charCode =
          (((value & 0x07) << 18) |
            ((data[i + 1] & 0x3f) << 12) |
            ((data[i + 2] & 0x3f) << 6) |
            (data[i + 3] & 0x3f)) -
          0x010000;

        str += String.fromCharCode(
          (charCode >> 10) | 0xd800,
          (charCode & 0x03ff) | 0xdc00
        );
        i += 3;
      }
    }

    return str;
  };
} else {
  const decoder = new TextDecoder("UTF-8");
  decodeUtf8 = (data) => decoder.decode(data);
}

async function allocFile(wasm, elementId) {
  const files = document.getElementById(elementId).files;
  if (files.length < 1 || files[0] == null) {
    return null;
  }
  const file = files[0];
  const contents = await new Promise((resolve) => {
    const reader = new FileReader();
    reader.onload = () => {
      const contents = reader.result;
      resolve(contents);
    };
    reader.readAsArrayBuffer(file);
  });
  const len = contents.byteLength;
  const ptr = wasm.exports.alloc(len);
  const slice = new Uint8Array(wasm.exports.memory.buffer, ptr, len);
  slice.set(new Uint8Array(contents));

  return [ptr, len, contents];
}

function exportFile(filename, data) {
  const url = URL.createObjectURL(
    new Blob([data], { type: "application/octet-stream" })
  );
  try {
    const element = document.createElement("a");
    element.setAttribute("href", url);
    element.setAttribute("download", filename);

    element.style.display = "none";
    document.body.appendChild(element);
    try {
      element.click();
    } finally {
      document.body.removeChild(element);
    }
  } finally {
    URL.revokeObjectURL(url);
  }
}

async function run() {
  const log = document.getElementById("log");
  while (log.firstChild) {
    log.removeChild(log.firstChild);
  }

  let context = {
    cursor: 0,
    len: 0,
    name: "RomHack",
    errorCount: 0,
  };

  function write(ptr, len) {
    const memory = new Uint8Array(context.wasm.exports.memory.buffer);
    const src = memory.slice(ptr, ptr + len);
    new Uint8Array(context.buffer).set(src, context.cursor);
    context.cursor += len;
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

  function countWrite(len) {
    context.cursor += len;
    if (context.cursor > context.len) {
      context.len = context.cursor;
    }
    return len;
  }

  function restart() {
    context.cursor = 0;
    context.buffer = new ArrayBuffer(context.len);
  }

  function setName(ptr, len) {
    context.name = decodeString(ptr, len);
  }

  function error(ptr, len) {
    const message = decodeString(ptr, len);
    const log = document.getElementById("log");
    if (context.errorCount == 0) {
      log.appendChild(document.createElement("br"));
      const span = document.createElement("span");
      span.className = "error left";
      span.appendChild(document.createTextNode("Error"));
      log.appendChild(span);
      const span2 = document.createElement("span");
      span2.appendChild(document.createTextNode(message));
      log.appendChild(span2);
      log.appendChild(document.createElement("br"));
      log.scrollTop = log.scrollHeight;
    } else {
      keyValPrint("Caused by", message, "error");
    }
    context.errorCount += 1;
  }

  async function keyValPrintPtr(kind, keyPtr, keyLen, valPtr, valLen) {
    const key = decodeString(keyPtr, keyLen);
    const val = decodeString(valPtr, valLen);
    switch (kind) {
      case 0:
        kind = "normal";
        break;
      case 1:
        kind = "warning";
        break;
      case 2:
        kind = "error";
        break;
      default:
        break;
    }
    keyValPrint(key, val, kind);
    await new Promise((resolve) => {
      setTimeout(() => {
        resolve();
      }, 0);
    });
  }

  async function keyValPrint(key, val, kind) {
    if (kind == null) {
      kind = "normal";
    }
    const text = `${key.padStart(12, " ")} ${val}`;
    console.log(text);
    const log = document.getElementById("log");
    const span = document.createElement("span");
    span.className = `${kind} left`;
    span.appendChild(document.createTextNode(key));
    log.appendChild(span);
    log.appendChild(document.createTextNode(val));
    log.appendChild(document.createElement("br"));
    log.scrollTop = log.scrollHeight;
  }

  function decodeString(ptr, len) {
    const memory = new Uint8Array(context.wasm.exports.memory.buffer);
    const slice = memory.slice(ptr, ptr + len);
    return decodeUtf8(slice);
  }

  let wasm = await WebAssembly.instantiate(await compiledModule, {
    "./romhack_bg.js": {
      __wbg_keyvalprint_5e5ef5c7eb6e479e: keyValPrintPtr,
      __wbg_setname_02bdb79cade7c440: setName,
      __wbg_countwrite_928e00efa24220c3: countWrite,
      __wbg_countseek_38683a43834f1cc4: seek,
      __wbg_restart_63db13fd1ed677d1: restart,
      __wbg_write_587d3269d1025ba1: write,
      __wbg_seek_7b2147db9ade23b6: seek,
      __wbg_error_b3f2bb3d1fb25132: error,
    },
  });
  context.wasm = wasm;

  keyValPrint("Opening", "ISO");

  const isoFile = await allocFile(wasm, "iso");
  if (isoFile == null) {
    return;
  }
  const [isoPtr, isoLen, isoBuffer] = isoFile;

  let decoder = new TextDecoder("utf-8");
  let gameCode = decoder.decode(isoBuffer.slice(0, 6));

  let patchUrl;
  let e = document.getElementById("patch");
  let selectedVersion = e.options[e.selectedIndex].text;

  switch (gameCode) {
    case "GZ2E01": {
      patchUrl = './patches/'+selectedVersion+'-gcn-ntscu.patch';
      break;
    }
    case "GZ2P01": {
      patchUrl = './patches/'+selectedVersion+'-gcn-pal.patch';
      break;
    }
    case "GZ2J01": {
      patchUrl = './patches/'+selectedVersion+'-gcn-ntscj.patch';
      break;
    }
    default: {
      console.error("Not a supported ISO.");
      return;
    }
  }

  keyValPrint("Opening", "Patch");

  const url = patchUrl;

  let patchPtr;
  let patchLen;
  let returnVal;

  fetch(url)
    .then((response) => response.arrayBuffer())
    .then(function (buffer) {
      patchLen = buffer.byteLength;
      patchPtr = wasm.exports.alloc(patchLen);
      const slice = new Uint8Array(
        wasm.exports.memory.buffer,
        patchPtr,
        patchLen
      );

      slice.set(new Uint8Array(buffer));

      returnVal = wasm.exports.create_romhack(
        patchPtr,
        patchLen,
        isoPtr,
        isoLen
      );

      if (returnVal == 1) {
        keyValPrint("Downloading", "Rom Hack");

        const { buffer, name } = context;
        context = null;
        wasm = null;

        exportFile(`${name}.iso`, buffer);

        keyValPrint("Finished", "");
      }
    })
    .catch((error) => console.error(error));
}
