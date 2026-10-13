# zkci: verifiable-CI prototype

Milestones M1–M3 of [`docs/zk-ci-design.md`](../docs/zk-ci-design.md). The
goal is to prove that a crate's unit tests passed on commit C, so that a
small verifier, rather than a full CI run, can set the merge gate. This
directory is for measuring cost; nothing here is meant to ship. Measured
numbers are in [`BENCH.md`](BENCH.md).

This is its own Cargo workspace, `exclude`d from the root one, so the SP1 SDK
stays out of the root lockfile and out of root CI.

| Path | Crate | What it is |
|---|---|---|
| `harness/` | `zkci-harness` | M1. Collects existing `#[test]` fns at build time, runs them in order, and returns a `SuiteReport` |
| `guest/` | `zkci-guest` | M2. The SP1 guest. Compiles `api-types/src/tests.rs` unmodified and commits the report. Built for the host, it runs the same cases natively (native mode) |
| `prover/` | `zkci-prove` | M2. Host binary with `execute` (no proof, prints cycles), `vkey`, and `prove` (compressed proof written into a receipt envelope) |
| `verify/` | `zkci-verify` | M3. Verifies a receipt against an expected verifying key (or a rebuilt ELF) and git tree. Prints a JSON verdict and exits 0/1 |
| `prove.sh` | | Driver: `build`, `execute`, `vkey`, `prove`, `verify` |

## Running it

Requires SP1 v6.9.0 (`sp1up --version v6.9.0` installs `cargo-prove` and the
`succinct` toolchain) and Docker for reproducible builds. The version must
match the exact `sp1-*` pins in `Cargo.toml`.

```sh
zkci/prove.sh execute                     # reproducible build in Docker, then execute
ZKCI_LOCAL_BUILD=1 zkci/prove.sh execute  # build with the local toolchain instead
cargo run --manifest-path zkci/Cargo.toml -p zkci-guest -- "$(git rev-parse HEAD^{tree})"   # native mode
cargo test --manifest-path zkci/Cargo.toml --workspace --all-features   # the prototype's own tests
```

`prove` and `verify` are wired but have not been run. This milestone stops
at `execute`.

## Decisions

**SP1 over RISC Zero.** The design's rule was to use SP1 unless its docs make
reproducible guest builds harder than RISC Zero's. They don't. The comparison
below was read from each project's source (SP1 6.9.0, RISC Zero 5.0.0),
because both docs sites are blocked from the environment this was built in.

| | SP1 6.9.0 | RISC Zero 5.0.0 |
|---|---|---|
| Reproducible build | `cargo prove build --docker` in `ghcr.io/succinctlabs/sp1:v<ver>`. `--workspace-directory` mounts a parent directory for path dependencies. `--locked` is opt-in | `cargo risczero build`, or `GuestOptions::use_docker(DockerOptions)` in a build script, in `risczero/risc0-guest-builder`. Always `--locked`. `RISC0_USE_DOCKER` is only a convention inside risc0's own `build.rs`, not a library switch |
| Program ID from an ELF | Verifying key via `setup` (preprocessing) | Image ID hashed straight from the ELF, with no prover needed |
| Guest target | `riscv64im-succinct-zkvm-elf` with `std`. SP1 6 is 64-bit; the design doc's `riscv32im` is from SP1 ≤ 5 | `riscv32im-risc0-zkvm-elf` with `std` |

RISC Zero's cheaper image ID is the one real ergonomic edge. It is not a
build-reproducibility edge, so the rule chooses SP1.

**M0 target: `api-types`.** It is the smallest crate in the repo with unit
tests and no I/O. Its only dependencies are `serde` and `serde_json`, it
already builds for `wasm32-unknown-unknown`, and its 25 tests are pure
serialization assertions. `plugins/api` has fewer lines but pulls in
`service-bundle` (ed25519, sha2).

**Collection at build time, not `#[zkci::test]` or `inventory`.** Outside
`cargo test`, rustc drops `#[test]` functions entirely. The guest's build
script therefore parses the test module with `syn`, removes those attributes,
and generates a `ZKCI_TESTS` list. The test file stays as it is and so do the
test bodies. `inventory` and `linkme` depend on life-before-main or linker
sections, which a zkVM guest doesn't promise to support. An attribute macro
would mean editing every test in the target crate.

The collector reports every test it skips, with the reason (`async`,
`#[should_panic]`, `#[ignore]`, custom test attributes, `cfg`-gated). Those
names are committed in the report, so a proof can never quietly cover fewer
tests than the crate has. For `api-types` that is one test,
`fill_rejects_arity_mismatch_in_debug`, which is `#[should_panic]`; a
`panic = "abort"` guest cannot run that kind of test.

**Public values.** The guest commits one `SuiteReport`: report version,
suite name, the git tree hash it was given, `(name, passed)` per test, and
the excluded tests. In the guest a failing test halts execution with a
non-zero exit code before anything is committed, so no passing proof can
exist for a failing suite. In native mode failures are caught and listed.

**Receipt envelope.** The receipt is versioned JSON with an `evidence.type`
tag (`zkvm` | `signed` | `tee`), as §6 of the design asks. Only `zkvm` is
implemented. It holds the bincode-encoded verifying key and compressed proof
as hex. `zkci-verify` never trusts the key in the envelope on its own: with
`--elf` it derives the key from the rebuilt guest, and with `--expect-vk` it
compares the envelope key's hash against the expected hash first. It then
verifies the proof (SP1 version, zero exit code, public-values digest),
decodes the report strictly (trailing bytes are rejected), and checks the
tree and that every test passed.

**The tree hash `R`** enters the guest as input. On its own it is only a
label. What actually binds a proof to a commit is that the verifier rebuilds
the guest from that commit and gets the same verifying key.

## Gaps and gotchas

- **The Docker build is untested.** The SP1 image's layers are served from
  `pkg-containers.githubusercontent.com`, which the environment this was
  built in cannot reach. The numbers in `BENCH.md` come from a local
  toolchain build of the same source.
- **`zkci-verify` links the full `sp1-sdk`** (its light prover, which
  verifies but never proves), so it is not yet the "tiny verifier". The
  smaller option, `sp1-verifier`'s `SP1CompressedVerifierRaw`, does **not**
  check the guest's exit code. Anyone switching to it must add that check, or
  rely on the guest committing nothing when it fails, which it does today.
  A Groth16 wrap is the other path to milliseconds-scale verification.
- **The guest has its own lockfile.** It compiles `api-types` against
  `zkci/Cargo.lock`, which can resolve different `serde`/`serde_json`
  versions than the root lockfile CI tests with.
- **`#[should_panic]` tests can't run in a guest**, because the guest is
  `panic = "abort"`. Proving them needs either per-test proofs of a non-zero
  exit (design §5), or rewriting them to return `Result`.
