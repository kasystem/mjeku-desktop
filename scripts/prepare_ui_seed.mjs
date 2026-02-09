import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

function readJson(p) {
  return JSON.parse(fs.readFileSync(p, "utf8"));
}

function safeJoin(root, subPath) {
  const rel = subPath.replace(/^\/+/, "");
  return path.join(root, rel);
}

function main() {
  const __dirname = path.dirname(fileURLToPath(import.meta.url));
  const desktopRoot = path.resolve(__dirname, "..");
  const uiRoot = path.resolve(desktopRoot, "..", "mjeku-ui");
  const distDir = path.join(uiRoot, "dist");
  const manifestPath = path.join(distDir, "manifest.json");

  if (!fs.existsSync(manifestPath)) {
    throw new Error(`UI manifest not found at ${manifestPath}. Run 'npm --prefix ${uiRoot} run build' first.`);
  }

  const manifest = readJson(manifestPath);
  const version = String(manifest.latestVersion || "").trim();
  const bundlePath = String(manifest.bundlePath || "").trim();
  if (!version || !bundlePath) {
    throw new Error("Invalid manifest.json (missing latestVersion or bundlePath).");
  }

  const bundleAbs = safeJoin(distDir, bundlePath);
  if (!fs.existsSync(bundleAbs)) {
    throw new Error(`UI bundle not found at ${bundleAbs}.`);
  }

  const resourcesDir = path.join(desktopRoot, "src-tauri", "resources");
  fs.mkdirSync(resourcesDir, { recursive: true });

  const outZip = path.join(resourcesDir, "ui-seed.zip");
  const outVer = path.join(resourcesDir, "ui-seed-version.txt");

  fs.copyFileSync(bundleAbs, outZip);
  fs.writeFileSync(outVer, version + "\n", "utf8");

  // eslint-disable-next-line no-console
  console.log(`Prepared UI seed: ${version}`);
}

main();
