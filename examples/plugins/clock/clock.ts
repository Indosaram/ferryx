// SDK stdio example. stdout is reserved for daemon requests; diagnostics use stderr.
// A block-render adapter is not wired yet; do not emit HTML on the request stream.

// Bun's stream decoder preserves JSON-lines across arbitrary input chunks.
const decoder = new TextDecoder();
let pending = "";
console.log(JSON.stringify({ type: "getCapabilities" }));
for await (const chunk of Bun.stdin.stream()) {
  pending += decoder.decode(chunk, { stream: true });
  let end: number;
  while ((end = pending.indexOf("\n")) >= 0) {
    const response = JSON.parse(pending.slice(0, end));
    pending = pending.slice(end + 1);
    console.error(`${new Date().toISOString()} clock: ${JSON.stringify(response)}`);
  }
}
