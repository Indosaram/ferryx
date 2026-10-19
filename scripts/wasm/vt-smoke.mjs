import fs from 'fs';
const {instance}=await WebAssembly.instantiate(fs.readFileSync(''+(process.argv[2]||'ghostty-vt.wasm')+''),{});
const e=instance.exports; const mem=()=>new DataView(e.memory.buffer);
const pp=e.ghostty_wasm_alloc_opaque(); // pointer-sized slot
console.log('new:',e.ghostty_terminal_new(0,pp,80,24));
const term=mem().getUint32(pp,true);
const data=new TextEncoder().encode("hello\r\n\x1b[31mred world\x1b[0m");
const buf=e.ghostty_alloc?0:0;
const p=e.ghostty_wasm_alloc_opaque(); // scratch; need larger buffer
const big=e.ghostty_alloc(0,data.length); 
new Uint8Array(e.memory.buffer,big,data.length).set(data);
e.ghostty_terminal_vt_write(term,big,data.length);
const out=e.ghostty_wasm_alloc_opaque();
for(const [n,id] of [['cols',1],['rows',2],['cursor_x',3],['cursor_y',4]]){ mem().setUint32(out,0,true); const r=e.ghostty_terminal_get(term,id,out); console.log(n,r,mem().getUint16(out,true)); }
