# zkci measurements

Execution only (`zkci-prove execute`). No proof has been generated yet, so
proof time, peak proving RAM and receipt size (M2's other numbers) are still
open.

Machine: 4 vCPU Intel Xeon @ 2.80 GHz, 15 GB RAM, no GPU (a cloud
container). SP1 6.9.0, `succinct` toolchain rustc 1.96.0-dev, guest target
`riscv64im-succinct-zkvm-elf`, release profile.

## api-types suite, 2026-10-13

The guest ELF is a local toolchain build (`ZKCI_LOCAL_BUILD=1`). See
"Reproducibility" for why the Docker build could not run here.

| | |
|---|---|
| Tests run / passed | 24 / 24 |
| Tests excluded | 1 (`fill_rejects_arity_mismatch_in_debug`: `#[should_panic]`, and debug-only) |
| **Cycles (whole run)** | **393,904** |
| SP1 gas | 354,325 |
| Execute wall time | 67–91 ms (3 runs) |
| Guest ELF size | 592,224 bytes |
| Shards at SP1's default 2^24-cycle shard size | 1 |
| Verifying-key derivation (`zkci-prove vkey`, light prover `setup`) | 7.3–7.8 s wall, ~25 s CPU |
| Same 24 tests natively (`cargo test --release -p api-types`) | reported as `0.00s` |

The cycle count is deterministic across runs. It also matches the executor's
own clock: with `RUST_LOG=debug`, SP1 logs `minimal executor finished in N
cycles`.

### Per test

From a separate build: guest feature `cycle-tracker`, `zkci-prove` built
with `--features profiling`. The markers add 50,840 cycles to the run
(444,744 total). Each per-test figure includes up to about 1.1k cycles of
marker overhead; the cheapest test measures 1,118. Treat them as upper
bounds.

| | Cycles |
|---|---|
| Sum over the 24 test regions | 296,056 (≈ 270–296k without markers, about 75% of the run) |
| Fixed cost outside the tests (zkVM start-up, reading input, encoding and committing the report) | ≈ 100–125k |
| Per test: min / median / mean / max | 1,118 / 8,716 / 12,335 / 34,926 |

Most expensive: `payment_option_shape` 34,926,
`node_info_catalog_replication_shape` 29,052,
`peer_info_sources_and_last_seen` 23,913, `service_info_shape` 23,636,
`node_info_carries_start_time_not_uptime` 23,498. All of them build
`serde_json::Value` trees and compare them. The cheapest compare enum
spellings.

## What this says, with the arithmetic left visible

- **This suite costs about 0.4 M cycles.** That is small. It needs one shard,
  so proving cost will be dominated by fixed per-proof work (setup, recursion,
  compression) rather than by the cycles. That fixed cost is the next number
  to measure. The design doc's §4.2 ballpark ("tens of millions of cycles/s
  on a rig") would put this trace at well under a second; that is an
  extrapolation, not a measurement.
- **The fixed overhead per execution is about 25–30% of this run.** Per-test
  proving (design §5) would pay it once per test: roughly 24 × 100k ≈ 2.4 M
  extra cycles, plus a proof each. Proving per file or per cone is the
  better grain until recursion folding is measured.
- **Deriving `H` costs the verifier about 7 s of CPU** on top of the rebuild.
  RISC Zero hashes its image ID straight from the ELF, which is cheaper; SP1
  runs a preprocessing `setup`. Workflows can cache it per ELF hash.
- **A failing test** halts the guest with exit code 1 before it commits
  anything. SP1 6.9's `execute` still returns `Ok` in that case, so
  `zkci-prove` checks the exit code and prints the guest's captured stderr,
  which holds the assertion message. Checked by breaking
  `api_response_ok_shape` locally, without committing the change.
- **Single shard means `sp1-verifier`'s `SP1CompressedVerifierRaw` cannot be
  used.** It rejects single-shard proofs, and it does not check exit codes.
  `zkci-verify` uses the SDK's `verify` for that reason.

## Reproducibility

- **The Docker build (`prove.sh build`) did not run.** Pulling
  `ghcr.io/succinctlabs/sp1:v6.9.0` fails because its layers are served from
  `pkg-containers.githubusercontent.com`, which this environment's egress
  policy blocks. It needs a run on a normal machine.
- **The local build is deterministic at a fixed path.** Two clean builds
  (`rm -rf target/elf-compilation`) gave byte-identical ELFs
  (`sha256 3421a21f…7cd9`).
- **The local build is not portable across paths.** The ELF embeds two
  absolute host paths, both from panic locations: `api-types/src` and the
  build script's `OUT_DIR`. A checkout at a different path therefore yields a
  different ELF and a different verifying key. The Docker build fixes the
  path (`/root/program`). `--remap-path-prefix` would be the fix for local
  builds, if they are ever wanted.
