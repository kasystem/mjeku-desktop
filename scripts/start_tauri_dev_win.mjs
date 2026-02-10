import { spawn } from "child_process";
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const desktopRoot = path.resolve(__dirname, "..");

function main() {
  const comspec = process.env.ComSpec || "C:\\\\Windows\\\\System32\\\\cmd.exe";
  // Use a relative path to avoid cmd.exe quoting issues with spaces.
  const cmdFile = "scripts\\\\run_tauri_dev_win.cmd";

  const logPath = path.join(desktopRoot, "tauri-dev.log");
  const logFd = fs.openSync(logPath, "a");

  const child = spawn(comspec, ["/d", "/s", "/c", cmdFile], {
    cwd: desktopRoot,
    detached: true,
    stdio: ["ignore", logFd, logFd]
  });
  child.unref();
  fs.closeSync(logFd);

  // eslint-disable-next-line no-console
  console.log(`Started tauri dev (PID ${child.pid}). Logs: ${logPath}`);
}

main();
