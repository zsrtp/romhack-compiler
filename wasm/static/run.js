import { run } from './run_prelude.js';

new Promise((resolve, reject) => {
  window.addEventListener("load", async () => {
    // let memory = new WebAssembly.Memory({initial:31,maximum:16384,shared:true});
    // let wasm = await wasm_bindgen("./romhack/romhack_bg.wasm", memory);
    let button = document.getElementById("run");
    let onClick = async () => {
      button.disabled = true;
      button.removeEventListener("click", onClick);
      try {
        await run();
      } catch (e) {
        console.error(e);
        throw e;
      }
      finally {
        button.addEventListener("click", onClick);
        button.disabled = false;
      }
    };
    button.addEventListener("click", onClick);
    button.disabled = false;
    resolve();
  });
  window.addEventListener("error", reject);
});
