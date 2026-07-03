#!/usr/bin/env bash
# Fetches the prebuilt PDFium library (bblanchon/pdfium-binaries) into
# vendor/pdfium/ for the current OS/arch. Idempotent.
set -euo pipefail

VERSION="${PDFIUM_VERSION:-latest}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/vendor/pdfium"

case "$(uname -s)" in
  Darwin) os="mac" ;;
  Linux) os="linux" ;;
  MINGW* | MSYS* | CYGWIN*) os="win" ;;
  *) echo "unsupported OS: $(uname -s)" >&2; exit 1 ;;
esac

case "$(uname -m)" in
  arm64 | aarch64) arch="arm64" ;;
  x86_64 | AMD64) arch="x64" ;;
  *) echo "unsupported arch: $(uname -m)" >&2; exit 1 ;;
esac

if [ "$VERSION" = "latest" ]; then
  url="https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-${os}-${arch}.tgz"
else
  url="https://github.com/bblanchon/pdfium-binaries/releases/download/${VERSION}/pdfium-${os}-${arch}.tgz"
fi

if [ -d "$DEST/lib" ]; then
  echo "pdfium already present at $DEST (delete to re-fetch)"
  exit 0
fi

echo "fetching $url"
mkdir -p "$DEST"
curl -fsSL "$url" | tar -xz -C "$DEST"
# Windows packages ship the runtime DLL in bin/, but consumers (layout.rs
# and the release bundler) look in lib/ on every OS — unify.
if [ -f "$DEST/bin/pdfium.dll" ]; then
  cp "$DEST/bin/pdfium.dll" "$DEST/lib/"
fi
echo "pdfium installed to $DEST"
ls "$DEST/lib"
