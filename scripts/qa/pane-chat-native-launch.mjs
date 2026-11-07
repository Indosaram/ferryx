import { mkdir } from "node:fs/promises";

const root = process.argv[2];
const binary = process.argv[3];
if (!root || !binary) throw new Error("Pass isolated profile and candidate binary");
for (const name of ["home", "data", "runtime", "sessions", "config", "cache", "share", "state"]) {
  await mkdir(`${root}/${name}`, { recursive: true });
}
const child = Bun.spawn([binary], {
  env: {
    ...process.env,
    HOME: `${root}/home`,
    XDG_CONFIG_HOME: `${root}/config`,
    XDG_DATA_HOME: `${root}/share`,
    XDG_CACHE_HOME: `${root}/cache`,
    XDG_STATE_HOME: `${root}/state`,
    XDG_RUNTIME_DIR: "/run/user/1000",
    DISPLAY: ":0",
    WAYLAND_DISPLAY: "wayland-1",
    DBUS_SESSION_BUS_ADDRESS: "unix:path=/run/user/1000/bus",
    FERRYX_DATA_DIR: `${root}/data`,
    FERRYX_RUNTIME_DIR: `${root}/runtime`,
    FERRYX_SESSION_DIR: `${root}/sessions`,
    FERRYX_DAEMON_SUPERVISOR_NAME: "ferryx-qa-pane-chat-native",
    FERRYX_QA_ISOLATED: "1",
  },
  stdout: "inherit", stderr: "inherit",
});
console.log(`QA_GUI_PID=${child.pid}`);
process.exitCode = await child.exited;
