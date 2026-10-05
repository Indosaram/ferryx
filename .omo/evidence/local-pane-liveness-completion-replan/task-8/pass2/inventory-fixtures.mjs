import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
const base=os.tmpdir();
for(const name of fs.readdirSync(base)){
if(!name.startsWith('pane-liveness-test-'))continue;
const root=path.join(base,name);
const st=fs.statSync(root);
const files=[];
const scan=(dir,depth)=>{if(depth>3)return;for(const entry of fs.readdirSync(dir,{withFileTypes:true})){const p=path.join(dir,entry.name);if(entry.isDirectory())scan(p,depth+1);else if(entry.name.endsWith('.json')){const text=fs.readFileSync(p,'utf8');if(/run-cancel|run-stall|run-ho|run-so/.test(text))files.push({path:p,contents:text});}}};
scan(root,0);
console.log(JSON.stringify({root,birthtime:st.birthtime.toISOString(),mtime:st.mtime.toISOString(),files}));
}

