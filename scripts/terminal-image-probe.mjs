import { readFile } from "node:fs/promises";

const path = process.argv[2];
const pixels = path
  ? await readFile(path)
  : Buffer.from([255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0]);
const format = path ? "f=100" : "f=24,s=2,v=2";
const payload = pixels.toString("base64");
const id = 73421;

process.stdout.write(`Ferryx inline image probe (${path ? "PNG" : "RGB"})\r\n`);
process.stdout.write("\n".repeat(12) + "\x1b[12A\r");
for (let offset = 0; offset < payload.length; offset += 4096) {
  const part = payload.slice(offset, offset + 4096);
  const more = offset + part.length < payload.length ? 1 : 0;
  const options = offset === 0
    ? `a=T,${format},i=${id},c=24,r=12,C=1,q=2,m=${more}`
    : `m=${more}`;
  process.stdout.write(`\x1b_G${options};${part}\x1b\\`);
}
process.stdout.write("\x1b[12B\r\nExpected: red/green above blue/yellow for the default probe.\r\n");
