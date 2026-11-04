// SDK stdio example. stdout is reserved for daemon requests; diagnostics use stderr.
// Rendering is scriptless. The process publishes a fresh document every second.

// Bun's stream decoder preserves JSON-lines across arbitrary input chunks.
const decoder = new TextDecoder();
let pending = "";
console.log(JSON.stringify({ type: "getCapabilities" }));
const render = () => console.log(JSON.stringify({ type: "pluginRender", view: "main",
  html: `<main><h1>Clock</h1><time>${new Date().toISOString()}</time></main>` }));
render();
const timer = setInterval(render, 1000);
try {
for await (const chunk of Bun.stdin.stream()) {
  pending += decoder.decode(chunk, { stream: true });
  let end: number;
  while ((end = pending.indexOf("\n")) >= 0) {
    const response = JSON.parse(pending.slice(0, end));
    pending = pending.slice(end + 1);
    console.error(`${new Date().toISOString()} clock: ${JSON.stringify(response)}`);
  }
}
} finally { clearInterval(timer); }
