#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${1:-0.1.0}"
ARCH="amd64"
PKG_NAME="feathertex"

BIN_PATH="$ROOT_DIR/target/release/feathertex"
if [[ ! -x "$BIN_PATH" ]] && [[ -x "$ROOT_DIR/feathertex" ]]; then
  BIN_PATH="$ROOT_DIR/feathertex"
fi
if [[ ! -x "$BIN_PATH" ]]; then
  echo "Compilazione release in corso..."
  (cd "$ROOT_DIR" && cargo build --release --locked)
  BIN_PATH="$ROOT_DIR/target/release/feathertex"
fi

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

PKG_ROOT="$STAGE/pkg"
mkdir -p \
  "$PKG_ROOT/DEBIAN" \
  "$PKG_ROOT/usr/bin" \
  "$PKG_ROOT/usr/share/applications" \
  "$PKG_ROOT/usr/share/icons/hicolor/scalable/apps" \
  "$PKG_ROOT/usr/share/feathertex/demo/chapters"

install -m755 "$BIN_PATH" "$PKG_ROOT/usr/bin/feathertex"
install -m644 "$ROOT_DIR/packaging/linux/feathertex.desktop" \
  "$PKG_ROOT/usr/share/applications/feathertex.desktop"
install -m644 "$ROOT_DIR/packaging/linux/feathertex.svg" \
  "$PKG_ROOT/usr/share/icons/hicolor/scalable/apps/feathertex.svg"

if [[ -d "$ROOT_DIR/demo" ]]; then
  install -m644 "$ROOT_DIR/demo/main.tex" "$PKG_ROOT/usr/share/feathertex/demo/main.tex"
  install -m644 "$ROOT_DIR/demo/references.bib" "$PKG_ROOT/usr/share/feathertex/demo/references.bib"
  install -m644 "$ROOT_DIR/demo/chapters/intro.tex" "$PKG_ROOT/usr/share/feathertex/demo/chapters/intro.tex"
  install -m644 "$ROOT_DIR/demo/chapters/results.tex" "$PKG_ROOT/usr/share/feathertex/demo/chapters/results.tex"
fi

INSTALLED_KB="$(du -sk "$PKG_ROOT/usr" | awk '{print $1}')"

cat > "$PKG_ROOT/DEBIAN/control" <<EOF
Package: $PKG_NAME
Version: $VERSION
Section: editors
Priority: optional
Architecture: $ARCH
Installed-Size: $INSTALLED_KB
Maintainer: FeatherTex Contributors <noreply@example.invalid>
Depends: libc6 (>= 2.35)
Recommends: libpoppler-glib8 | libpoppler-glib8t64, libcairo2, libglib2.0-0 | libglib2.0-0t64, texlive-latex-base, texlive-latex-recommended, texlive-science, texlive-extra-utils, biber, chktex, hunspell, hunspell-it, hunspell-en-us, latexdiff, poppler-utils
Description: Editor LaTeX leggero e veloce per ricercatori
 FeatherTex combina scrittura fluida con preamboli .fmt precompilati,
 anteprima PDF Poppler+Cairo, ricette di compilazione, Outline multi-file,
 autocompletamento di \cite e \ref, import DOI/arXiv e controllo qualità.
EOF

chmod 0755 "$PKG_ROOT/DEBIAN"
chmod 0644 "$PKG_ROOT/DEBIAN/control"

mkdir -p "$ROOT_DIR/dist"
OUT_DEB="$ROOT_DIR/dist/${PKG_NAME}_${VERSION}_${ARCH}.deb"
rm -f "$OUT_DEB"

if command -v dpkg-deb >/dev/null 2>&1; then
  dpkg-deb --build --root-owner-group "$PKG_ROOT" "$OUT_DEB"
else
  printf '2.0\n' > "$STAGE/debian-binary"
  tar --numeric-owner --owner=0 --group=0 -C "$PKG_ROOT/DEBIAN" -czf "$STAGE/control.tar.gz" .
  tar --numeric-owner --owner=0 --group=0 -C "$PKG_ROOT" --exclude='./DEBIAN' -czf "$STAGE/data.tar.gz" .
  (cd "$STAGE" && ar -rcD "$OUT_DEB" debian-binary control.tar.gz data.tar.gz)
fi

echo "Pacchetto .deb creato: $OUT_DEB"
ls -lh "$OUT_DEB"

