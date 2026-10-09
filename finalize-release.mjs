import { mkdirSync } from "node:fs";
import { finalizeMacosBundle } from "./scripts/lib/release-platforms.mjs";

const appPath = process.argv[2];
if (!appPath) throw new Error("Expected retrieved app path");
const workspaceDir = new URL("./notary/", import.meta.url).pathname;
mkdirSync(workspaceDir, { recursive: true });
const result = finalizeMacosBundle({
  appPath,
  workspaceDir,
  signingIdentity: "Developer ID Application: Indo Yoon (5DUM8WPB4C)",
  notaryProfile: "FerryxNotary",
  approveNotarization: true,
});
console.log(JSON.stringify(result));
console.log("NOTARIZED_RELEASE_OK");
