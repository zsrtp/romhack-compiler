export let context = {
    cursor: 0,
    read_cursor: BigInt(0),
    len: 0,
    name: "RomHack",
    errorCount: 0,
    file: null,
    buffers: [],
};

export function resetContext() {
    context.cursor = 0;
    context.read_cursor = BigInt(0);
    context.len = 0;
    context.name = "RomHack";
    context.errorCount = 0;
    context.file = null;
}

export function keyValPrint(key, val, kind) {
    if (kind == null) {
        kind = "normal";
    }
    const text = `${key.padStart(12, " ")} ${val}`;
    console.log(text);
    const log = document.getElementById("log");
    const span = document.createElement("span");
    let is_scrolled = log.scrollHeight - (log.scrollTop + log.clientHeight > 5);
    span.className = `${kind} left`;
    span.appendChild(document.createTextNode(key));
    log.appendChild(span);
    log.appendChild(document.createTextNode(val));
    log.appendChild(document.createElement("br"));
    if (is_scrolled) {
        log.scrollTop = log.scrollHeight;
    }
}

globalThis.mainWorker = null;

let function_list = [keyValPrint];

function main_thread_call(func_idx, args, transfer) {
    if (args === undefined) {
        args = [];
    }
    return new Promise((resolve, reject) => {
        let cb = (ev) => {
            let [idx, type, val] = ev.data;
            if (idx != func_idx) {
                return;
            }
            mainWorker.removeEventListener("message", cb);
            mainWorker.removeEventListener("messageerror", errcb);
            mainWorker.removeEventListener("error", errcb);
            console.debug("[call callback]", ev);
            if (type == "call_ret") {
                resolve(val);
                return;
            }
            if (type == "call_error") {
                reject(val);
                return;
            }
        };
        let errcb = (ev) => {
            mainWorker.removeEventListener("message", cb);
            mainWorker.removeEventListener("messageerror", errcb);
            mainWorker.removeEventListener("error", errcb);
            console.debug("[call messageerror]", ev);
            reject(ev);
        };
        mainWorker.addEventListener("message", cb);
        mainWorker.addEventListener("messageerror", errcb);
        mainWorker.addEventListener("error", errcb);
        mainWorker.postMessage([func_idx, "call", args], transfer);
    });
}

if ("Window" in globalThis && globalThis instanceof globalThis['Window']) {
    if (globalThis.mainWorker === null) {
        globalThis.mainWorker = new Worker("main_worker.js");
        main_thread_call(1, [context]).then(() => {
            globalThis.mainWorker.addEventListener("message", (ev) => {
                let [idx, type, data] = ev.data;
                if (type == "call") {
                    function_list[idx].apply(undefined, data);
                }
            });
        });
    }
}

function exportFile(filename, data) {
    const url = URL.createObjectURL(
        new Blob(data, { type: "application/octet-stream" })
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
    } catch (e) {
        console.error(e);
    } finally {
        URL.revokeObjectURL(url);
    }
}

export async function run() {
    const log = document.getElementById("log");
    while (log.firstChild) {
        log.removeChild(log.firstChild);
    }

    resetContext();

    let files = document.getElementById("iso").files;
    if (files.length < 1 || files[0] == null) {
        throw new Error("No ISO file was provided");
    }
    context.file = files[0];
    await main_thread_call(1, [context]);

    keyValPrint("Opening", "ISO");

    try {
        const isoBuffer = await context.file.slice(0, 6).arrayBuffer();
        let decoder = new TextDecoder("utf-8");
        let gameCode = decoder.decode(isoBuffer);

        let patchUrl;
        let e = document.getElementById("patch");
        let selectedVersion = e.options[e.selectedIndex].text;

        switch (gameCode) {
            case "GZ2E01": {
                patchUrl = releases.get(selectedVersion).get(selectedVersion + '-gcn-ntscu.patch');
                break;
            }
            case "GZ2P01": {
                patchUrl = releases.get(selectedVersion).get(selectedVersion + '-gcn-pal.patch');
                break;
            }
            case "GZ2J01": {
                patchUrl = releases.get(selectedVersion).get(selectedVersion + '-gcn-ntscj.patch');
                break;
            }
            case "RZDE01": {
                patchUrl = releases.get(selectedVersion).get(selectedVersion + '-wii-ntscu-10.patch');
                break;
            }
            case "RZDP01": {
                patchUrl = releases.get(selectedVersion).get(selectedVersion + '-wii-pal.patch');
                break;
            }
            default: {
                console.error("Not a supported ISO.");
                keyValPrint("Error", "Not a supported ISO.", "error");
                return;
            }
        }

        keyValPrint("Opening", "Patch");

        const proxyurl = "https://cors-anywhere.herokuapp.com/";
        // const url = proxyurl + patchUrl;
        const url = "./tpgz.patch"

        let patchPtr;
        let patchLen;
        let returnVal;

        await fetch(url)
            .then((response) => {
                if (!response.ok) {
                    console.error(new Error(`${response.url}: ${response.status} - ${response.statusText}`));
                    throw new Error(`Could not fetch the patch file. [${response.status} (${response.statusText})]`);
                }
                return response.arrayBuffer();
            })
            .then(function (buffer) {
                return main_thread_call(0, [buffer], [buffer]);
            })
            .then(async (ret) => {
                context = {...context, ...await main_thread_call(2)};
                return ret;
            })
            .then(async (returnVal) => {
                if (returnVal == 0) {
                    return Promise.reject(new Error(`Could not compile the romhack`));
                }
                if (returnVal != 0) {
                    keyValPrint("Downloading", "Rom Hack");

                    const { buffers, name } = context;
                    resetContext();
                    await main_thread_call(1, [context]);

                    exportFile(`${name}.iso`, buffers);

                    keyValPrint("Finished", "");
                }
            })
            .catch((e) => { console.error(e); keyValPrint("Aborted", e.toString(), "error") });
    } catch (e) {
        console.error(e);
        keyValPrint("Error", e.message, "error");
        return;
    }
}