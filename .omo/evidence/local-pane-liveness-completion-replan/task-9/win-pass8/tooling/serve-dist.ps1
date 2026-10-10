$ErrorActionPreference='Continue'
$base='C:\Users\sook\ferryx-pane-completion'
$root=Join-Path $base 'source-21dea3c0'
$dist=Join-Path $root 'ui\dist'
$srv=Join-Path $base 'serve-dist.mjs'
@'
import { createServer } from "node:http";
import { readFileSync, existsSync } from "node:fs";
import { join, extname } from "node:path";
const dist = process.argv[2];
const types = { ".html":"text/html; charset=utf-8", ".js":"text/javascript", ".css":"text/css", ".json":"application/json", ".svg":"image/svg+xml", ".png":"image/png", ".ico":"image/x-icon", ".woff2":"font/woff2", ".woff":"font/woff", ".map":"application/json" };
const server = createServer((req, res) => {
  const url = decodeURIComponent((req.url || "/").split("?")[0]);
  let file = join(dist, url === "/" ? "index.html" : url);
  if (!existsSync(file) || url.endsWith("/")) file = join(dist, "index.html");
  if (!existsSync(file)) { res.writeHead(404); res.end("not found"); return; }
  res.writeHead(200, { "content-type": types[extname(file)] || "application/octet-stream" });
  res.end(readFileSync(file));
});
server.listen(5173, "127.0.0.1", () => console.log("FERRYX_DIST_SERVER_READY"));
'@ | Set-Content -Path $srv -Encoding utf8
$c = Get-NetTCPConnection -LocalPort 5173 -State Listen -ErrorAction SilentlyContinue
if ($c) { Write-Output ("ALREADY_LISTENING pid=" + ($c|Select-Object -First 1).OwningProcess) }
else {
  $p = Start-Process -FilePath 'bun' -ArgumentList @($srv, $dist) -PassThru -WindowStyle Hidden
  Write-Output ("SERVER_PID=" + $p.Id)
  Start-Sleep -Seconds 3
}
try { $r = Invoke-WebRequest -Uri 'http://127.0.0.1:5173/' -UseBasicParsing -TimeoutSec 5; Write-Output ("HTTP=" + $r.StatusCode + " bytes=" + $r.RawContentLength) } catch { Write-Output ("HTTP_FAIL: " + $_.Exception.Message) }
Write-Output "SERVE_DIST_DONE"
