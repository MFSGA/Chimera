#!/usr/bin/env bash
# Read-only NixOS development/build preflight. Never changes host networking.
set -u

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
errors=0
warnings=0

ok() { printf '[OK] %s\n' "$1"; }
fail() { printf '[ERROR] %s\n' "$1"; errors=$((errors + 1)); }
warn() { printf '[WARN] %s\n' "$1"; warnings=$((warnings + 1)); }

if [[ -r /etc/os-release ]] && grep -q '^ID=nixos$' /etc/os-release; then
  ok "NixOS host ($(uname -m))"
else
  warn 'Host is not NixOS; Linux dependency checks still apply'
fi

for tool in node pnpm deno cargo rustc pkg-config; do
  if command -v "$tool" >/dev/null 2>&1; then
    ok "$tool available"
  else
    fail "$tool missing: enter the project with nix develop"
  fi
done

if command -v node >/dev/null 2>&1; then
  required_node="$(node -p "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8')).engines.node" "$root/package.json")"
  actual_node="$(node --version)"
  if [[ "$actual_node" == "$required_node" ]]; then
    ok "Node $actual_node matches package.json"
  else
    fail "Node $actual_node does not match required $required_node"
  fi
fi

if command -v pkg-config >/dev/null 2>&1; then
  for dependency in gtk+-3.0 webkit2gtk-4.1 libsoup-3.0 ayatana-appindicator3-0.1 openssl; do
    if pkg-config --exists "$dependency"; then
      ok "$dependency available"
    else
      fail "$dependency missing: use the flake development shell"
    fi
  done
fi

if command -v node >/dev/null 2>&1; then
  mapfile -t sidecars < <(node -e "const fs = require('fs'); const cfg = JSON.parse(fs.readFileSync(process.argv[1], 'utf8')); console.log(cfg.bundle.externalBin.join('\\n'))" "$root/backend/tauri/tauri.conf.json")
  target="$(rustc -vV 2>/dev/null | awk '/^host:/ {print $2}')"
  if [[ -z "$target" ]]; then
    fail 'Could not identify Rust host target'
  else
    for name in "${sidecars[@]}"; do
      binary="$root/backend/tauri/${name}-${target}"
      if [[ -x "$binary" ]]; then
        ok "sidecar ${name##*/} present"
      else
        fail "sidecar ${name##*/} missing: run pnpm prepare:check"
      fi
    done
  fi
fi

if [[ -e /dev/net/tun ]]; then
  ok 'TUN device exists (permissions/runtime behavior not tested)'
else
  warn '/dev/net/tun unavailable; TUN mode needs host configuration'
fi
if [[ -z "${DISPLAY:-}" && -z "${WAYLAND_DISPLAY:-}" ]]; then
  warn 'No graphical display detected in this shell; desktop UI cannot be verified here'
fi

printf '\nPreflight: %d error(s), %d warning(s).\n' "$errors" "$warnings"
if ((errors > 0)); then
  exit 1
fi
