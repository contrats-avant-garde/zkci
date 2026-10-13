#!/usr/bin/env bash
# Verifiable-CI prototype driver (docs/zk-ci-design.md, M2).
#
#   ./prove.sh build     reproducible guest build in Docker -> target/zkci/zkci-guest.elf
#   ./prove.sh execute   build, then run the guest without proving: cycle counts as JSON
#   ./prove.sh vkey      build, then write the verifying-key hash to target/zkci/elf.id
#   ./prove.sh prove     build, vkey, then a compressed proof -> target/zkci/receipt.json
#   ./prove.sh verify    check target/zkci/receipt.json against this repo's tree and the ELF
#
# The code under test is targets/example, in this repository, so the tree a
# proof claims is this repository's HEAD^{tree}.
#
# Environment:
#   ZKCI_LOCAL_BUILD=1  build with the installed `succinct` toolchain instead of
#                       Docker. Faster, not reproducible: a verifier rebuilding in
#                       Docker may derive a different verifying key.
#   ZKCI_SKIP_BUILD=1   reuse target/zkci/zkci-guest.elf as it is.
#   ZKCI_TREE=<hash>    the tree to claim, instead of HEAD^{tree}.
#   ZKCI_ALLOW_DIRTY=1  let `prove` run with uncommitted changes.
#
# Needs SP1 v6.9.0 (`sp1up --version v6.9.0`) and, for reproducible builds,
# Docker. The version must match the sp1-sdk / sp1-zkvm pins in Cargo.toml.
set -euo pipefail

SP1_VERSION=6.9.0

here=$(cd "$(dirname "$0")" && pwd)
out="$here/target/zkci"
elf="$out/zkci-guest.elf"

die() {
  echo "zkci: $*" >&2
  exit 1
}

tree() {
  if [[ -n "${ZKCI_TREE:-}" ]]; then
    echo "$ZKCI_TREE"
  else
    git -C "$here" rev-parse 'HEAD^{tree}'
  fi
}

# The claimed tree is this repository's HEAD, so uncommitted changes mean
# the guest runs code that tree does not contain.
check_clean() {
  [[ -n "${ZKCI_TREE:-}" ]] && return 0
  if [[ -n "$(git -C "$here" status --porcelain)" ]]; then
    if [[ "$1" == strict && "${ZKCI_ALLOW_DIRTY:-}" != 1 ]]; then
      die "uncommitted changes: the proof would claim the HEAD tree for other code (ZKCI_ALLOW_DIRTY=1 to override)"
    fi
    echo "zkci: warning: uncommitted changes; results are for the working tree, not HEAD^{tree}" >&2
  fi
}

# Wall time and peak RSS, when GNU time is installed.
measure() {
  if /usr/bin/time --version 2>&1 | grep -q GNU; then
    /usr/bin/time -f 'zkci: %e s wall, %M KB peak RSS' "$@"
  else
    "$@"
  fi
}

host() {
  local bin=$1
  shift
  cargo run --quiet --release --locked --manifest-path "$here/Cargo.toml" --bin "$bin" -- "$@"
}

build() {
  [[ "${ZKCI_SKIP_BUILD:-}" == 1 ]] && { [[ -f "$elf" ]] || die "no ELF at $elf"; return 0; }
  command -v cargo-prove >/dev/null || die "cargo-prove not found: install SP1 v$SP1_VERSION with sp1up"
  local args=(--locked --output-directory "$out" --elf-name zkci-guest.elf)
  if [[ "${ZKCI_LOCAL_BUILD:-}" != 1 ]]; then
    # The container sees only the workspace directory, which holds
    # targets/example, the crate the guest compiles.
    args+=(--docker --tag "v$SP1_VERSION")
  else
    echo "zkci: local build: the ELF is not reproducible" >&2
  fi
  (cd "$here/guest" && cargo prove build "${args[@]}")
  echo "zkci: guest ELF at $elf" >&2
}

vkey() {
  host zkci-prove vkey --elf "$elf" >"$out/elf.id"
  echo "zkci: verifying key $(cat "$out/elf.id") -> $out/elf.id" >&2
}

case "${1:-}" in
  build)
    build
    ;;
  execute)
    check_clean warn
    build
    host zkci-prove execute --elf "$elf" --tree "$(tree)"
    ;;
  vkey)
    build
    vkey
    ;;
  prove)
    check_clean strict
    build
    vkey
    measure host zkci-prove prove --elf "$elf" --tree "$(tree)" --out "$out/receipt.json"
    ls -l "$out/receipt.json" >&2
    ;;
  verify)
    host zkci-verify "$out/receipt.json" --expect-tree "$(tree)" --elf "$elf"
    ;;
  *)
    sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//' >&2
    exit 2
    ;;
esac
