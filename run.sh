#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
if [[ -f .tools/cargo/bin/cargo.exe ]]; then
  export CARGO_HOME="$(pwd -W)/.tools/cargo"
  export RUSTUP_HOME="$(pwd -W)/.tools/rustup"
  export PATH="$PWD/.tools/cargo/bin:$PWD/.tools/w64devkit/bin:$PATH"
  export RUSTFLAGS='-C link-self-contained=yes'
fi
exec cargo run -- "$@"
