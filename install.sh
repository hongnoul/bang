#!/bin/sh
# bang installer: versioned store + symlink, same layout the self-updater
# maintains, so `bang update` takes over seamlessly after first install.
set -eu

REPO="hongnoul/bang"
STORE="${XDG_DATA_HOME:-$HOME/.local/share}/bang"
BIN_DIR="${BANG_BIN_DIR:-$HOME/.local/bin}"

os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
case "$arch" in
  arm64) arch="aarch64" ;;
  amd64) arch="x86_64" ;;
esac
asset="bang-${os}-${arch}"

echo "Resolving latest release..."
auth_header=""
if [ -n "${GH_TOKEN:-${GITHUB_TOKEN:-}}" ]; then
  auth_header="Authorization: Bearer ${GH_TOKEN:-$GITHUB_TOKEN}"
fi
release_json=$(curl -fsSL ${auth_header:+-H "$auth_header"} \
  -H "Accept: application/vnd.github+json" \
  "https://api.github.com/repos/${REPO}/releases/latest")
tag=$(printf '%s' "$release_json" | grep -o '"tag_name": *"[^"]*"' | head -1 | cut -d'"' -f4)
[ -n "$tag" ] || { echo "error: could not resolve latest release" >&2; exit 1; }
version=${tag#v}

url="https://github.com/${REPO}/releases/download/${tag}/${asset}"
version_dir="${STORE}/versions/${version}"
mkdir -p "$version_dir" "$BIN_DIR"

echo "Downloading ${asset} ${tag}..."
curl -fsSL "$url" -o "${version_dir}/bang.partial"
if curl -fsSL "${url}.sha256" -o "${version_dir}/bang.sha256" 2>/dev/null; then
  expected=$(awk '{print $1}' "${version_dir}/bang.sha256")
  actual=$(sha256sum "${version_dir}/bang.partial" 2>/dev/null | awk '{print $1}' \
    || shasum -a 256 "${version_dir}/bang.partial" | awk '{print $1}')
  [ "$expected" = "$actual" ] || { echo "error: checksum mismatch" >&2; exit 1; }
  echo "Checksum verified."
fi
chmod +x "${version_dir}/bang.partial"
mv "${version_dir}/bang.partial" "${version_dir}/bang"

ln -sfn "$version_dir" "${STORE}/current"
ln -sfn "${STORE}/current/bang" "${BIN_DIR}/bang"

echo "Installed bang ${tag} -> ${BIN_DIR}/bang"
case ":$PATH:" in
  *":${BIN_DIR}:"*) ;;
  *) echo "note: add ${BIN_DIR} to your PATH" ;;
esac
echo "Try: bang '!w your first search'"
