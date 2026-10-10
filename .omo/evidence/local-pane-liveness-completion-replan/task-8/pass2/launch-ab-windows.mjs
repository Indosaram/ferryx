import fs from 'node:fs';
import {spawn} from 'node:child_process';
const base='C:/Users/sook/ferryx-pane-completion';
const out=base+'/task8-172baa87';
const text=fs.readFileSync(out+'/logs/full-ui.log','utf8').replace(/\x1b\[[0-9;]*m/g,'');
const files=new Set(['src/components/TerminalSplitView.paneHandleReach.test.tsx','src/lib/pairedDaemonRollout.test.ts']);
for(const match of text.matchAll(/(?:FAIL\s+|❯\s+)(src\/\S+\.(?:test|spec)\.[jt]sx?)/g)) files.add(match[1]);
console.log('AB_FILE_INVENTORY windows '+JSON.stringify([...files]));
const stage=spawn('powershell.exe',['-NoProfile','-File',out+'/stage-ab-windows.ps1'],{stdio:'inherit'});
stage.on('close',code=>{
  if(code!==0){process.exit(code??1);return;}
  const child=spawn(process.execPath,[out+'/ab-runner.mjs',base+'/base-pass2-d82b35e4',out+'/base-ab','windows',...files],{stdio:'inherit'});
  child.on('close',exit=>process.exit(exit??1));
});
