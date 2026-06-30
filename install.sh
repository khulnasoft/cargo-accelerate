#!/usr/bin/env sh
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR"

usage() {
  cat <<'EOF'
Usage: ./install.sh [options]

Options:
  --help        Show this help message
  --with-tools  Install external tooling recommended by cargo-accelerate

This script installs cargo-accelerate from the current repository.
Use --with-tools to also install optional helper tools like sccache, cargo-nextest,
cargo-watch, and a fast linker when available.
EOF
}

install_self() {
  echo "Installing cargo-accelerate from $PROJECT_ROOT..."
  cargo install --path "$PROJECT_ROOT"
}

install_tools() {
  echo "Installing recommended external tools..."

  if command -v brew >/dev/null 2>&1; then
    brew install llvm cargo-nextest cargo-watch sccache mold
    echo "Note: brew installs llvm in /usr/local/opt/llvm or /opt/homebrew/opt/llvm."
  elif command -v apt-get >/dev/null 2>&1; then
    sudo apt-get update
    sudo apt-get install -y llvm clang cargo-watch
    echo "Install cargo-nextest and sccache via cargo after this script if needed."
  elif command -v pacman >/dev/null 2>&1; then
    sudo pacman -Syu --noconfirm llvm clang cargo-watch
    echo "Install cargo-nextest and sccache via cargo after this script if needed."
  else
    echo "No supported package manager found."
    echo "Please install sccache, cargo-nextest, cargo-watch, and a fast linker manually."
    return 1
  fi
}

case "${1-}" in
  --help|-h)
    usage
    exit 0
    ;;
  --with-tools)
    install_self
    install_tools
    ;;
  "")
    install_self
    ;;
  *)
    echo "Unknown option: $1"
    usage
    exit 1
    ;;
esac
