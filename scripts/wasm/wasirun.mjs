import { WASI } from 'node:wasi'; import fs from 'node:fs';
const wasi = new WASI({version:'preview1', args:process.argv.slice(2), env:{...process.env}, preopens:{'/':'/'}, returnOnExit:true});
const m = await WebAssembly.compile(fs.readFileSync(process.argv[2]));
const i = await WebAssembly.instantiate(m, wasi.getImportObject());
process.exit(wasi.start(i));
