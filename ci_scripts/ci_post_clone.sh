#!/bin/zsh
# Xcode Cloud: ekzekutohet automatikisht pas klonimit (ndodhet në
# src-tauri/gen/apple/ci_scripts/ — Apple e kërkon pikërisht aty, si vëlla i .xcodeproj).
# Përgatit mjedisin PARA se Xcode të fillojë ndërtimin: Rust (për cargo-build phase-in
# e gjeneruar nga Tauri) + Node (vetëm këtu, për të ndërtuar UI-në një herë).
set -e

echo "=== Mjeku iOS: ci_post_clone ==="

# Rrënja e repos mjeku-desktop (skripti ndodhet në src-tauri/gen/apple/ci_scripts)
REPO_ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
echo "REPO_ROOT=$REPO_ROOT"

# ── Rust (pattern zyrtar i Apple/Tauri për Xcode Cloud) ─────────────────────
if ! command -v rustup >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
fi
. "$HOME/.cargo/env"
rustup target add aarch64-apple-ios aarch64-apple-ios-sim

# ── Node (Homebrew vjen i instaluar në runner-at e Xcode Cloud) ─────────────
if ! command -v node >/dev/null 2>&1; then
  export HOMEBREW_NO_AUTO_UPDATE=1
  export HOMEBREW_NO_INSTALL_CLEANUP=1
  export NONINTERACTIVE=1
  brew install node@20
  brew link --overwrite --force node@20 || true
  export PATH="/opt/homebrew/opt/node@20/bin:/usr/local/opt/node@20/bin:$PATH"
fi
node --version
npm --version

# ── mjeku-ui (repo simotër — klonohet nëse Xcode Cloud s'e ka bashkangjitur) ─
UI_DIR="$REPO_ROOT/../mjeku-ui"
if [ ! -d "$UI_DIR" ]; then
  echo "mjeku-ui s'u gjet si repo simotër — po e klonoj..."
  export GIT_TERMINAL_PROMPT=0
  if [ -n "$MJEKU_UI_TOKEN" ]; then
    git clone --depth 1 "https://x-access-token:${MJEKU_UI_TOKEN}@github.com/kasystem/mjeku-ui.git" "$UI_DIR"
  else
    echo "KUJDES: ndryshorja MJEKU_UI_TOKEN nuk është vendosur te Xcode Cloud (Environment Variables)."
    echo "Nëse mjeku-ui është repo privat, klonimi tani do të dështojë."
    git clone --depth 1 "https://github.com/kasystem/mjeku-ui.git" "$UI_DIR"
  fi
fi

# ── Varësitë npm ────────────────────────────────────────────────────────────
cd "$UI_DIR" && npm ci
cd "$REPO_ROOT" && npm ci

# Build i UI + seed (i njëjti beforeBuildCommand që përdor edhe desktopi/androidi —
# Xcode Cloud NUK e ekzekuton vetë tauri.conf.json beforeBuildCommand, prandaj e bëjmë këtu).
cd "$UI_DIR" && npm run build
cd "$REPO_ROOT" && node scripts/prepare_ui_seed.mjs

echo "=== ci_post_clone përfundoi me sukses ==="
