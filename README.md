# Mjeku Desktop (Tauri)

Cross-platform desktop app (Windows + macOS) using Tauri v2 + Rust.

## Dev (Recommended)

```bash
cd mjeku-ui
npm install
```

```bash
cd mjeku-desktop
npm install
npm run tauri:dev
```

`tauri:dev` starts the Vite dev server from `../mjeku-ui` and loads it at `http://127.0.0.1:5173`.

## Build

```bash
cd mjeku-desktop
npm install
npm run tauri:build
```

Production loads the UI from the local, downloaded bundle stored in the app data folder.
