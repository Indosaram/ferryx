import { expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { decodeMachineJson } from "./pairedDaemonContracts";

function fixtures(name: string): { name: string; kind: string; value: unknown }[] {
  const path = resolve(dirname(fileURLToPath(import.meta.url)), "../../../docs/evidence/paired-daemon/fixtures", `${name}.json`);
  return JSON.parse(readFileSync(path, "utf8"));
}
const validFixtures = fixtures("parity-valid");
const invalidFixtures = fixtures("parity-invalid");

it.each(validFixtures)("roundtrips expanded $kind", fixture => {
  expect(decodeMachineJson(fixture.kind, JSON.stringify(fixture.value))).toEqual(fixture.value);
});
it.each(invalidFixtures)("rejects $name", fixture => {
  expect(() => decodeMachineJson(fixture.kind, JSON.stringify(fixture.value))).toThrow();
});
