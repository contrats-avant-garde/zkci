# zkci measurements

Execution only (`zkci-prove execute`). No proof has been generated yet, so
proof time, peak proving RAM and receipt size (M2's other numbers) are still
open.

Machine: 4 vCPU Intel Xeon @ 2.10 GHz, 15 GB RAM, no GPU (a cloud
container). SP1 6.9.0, `succinct` toolchain rustc 1.96.0-dev, guest target
`riscv64im-succinct-zkvm-elf`, release profile.

## zkci-example suite, 2026-10-13

The guest ELF is a local toolchain build (`ZKCI_LOCAL_BUILD=1`). See
"Reproducibility" for why the Docker build could not run here.

| | |
|---|---|
| Tests run / passed | 21 / 21 |
| Tests excluded | 1 (`bump_major_past_u64_max_panics`: `#[should_panic]`) |
| **Cycles (whole run)** | **2,971,867** |
| SP1 gas | 2,532,094 |
| Execute wall time | 124–157 ms (4 runs) |
| Guest ELF size | 321,224 bytes |
| Shards at SP1's default 2^24-cycle shard size | 1 |
| Verifying-key derivation (`zkci-prove vkey`, light prover `setup`) | 7.2 s wall, ~24.5 s CPU (3 runs) |
| Same 21 tests natively (`cargo test --release -p zkci-example -- --skip bump_major`) | reported as `0.00s` (0.08–0.10 s with the `#[should_panic]` test, nearly all of it that one test) |
| Same 21 cases in native mode (`zkci-guest` built for the host, one thread) | 4–5 ms for the whole process |

The cycle count is deterministic across runs. It also matches the executor's
own clock: with `RUST_LOG=debug`, SP1 logs `minimal executor finished in N
cycles`.

### Per test

From a separate build: guest feature `cycle-tracker`, `zkci-prove` built
with `--features profiling`. The markers add 37,443 cycles to the run
(3,009,310 total). Each per-test figure includes up to about 1.5k cycles of
marker overhead; the cheapest test measures 1,511. Treat them as upper
bounds.

| | Cycles |
|---|---|
| Sum over the 21 test regions | 2,885,045 (≈ 2.85–2.89 M without markers, 96–97% of the run) |
| Fixed cost outside the tests (zkVM start-up, reading input, encoding and committing the report) | ≈ 90–120k |
| Per test: min / median / mean / max | 1,511 / 11,386 / 137,383 / 1,877,064 |

Most expensive: `sorts_a_shuffled_set` 1,877,064,
`comparators_agree_with_precedence` 680,448, `display_round_trips` 45,095,
`max_satisfying_picks_the_newest_match` 40,873,
`requirement_display_round_trips` 35,521. The top two build their inputs by
formatting and parsing every generated version (320 and 135 of them);
`display_round_trips`, which parses and prints 9 versions, puts that at
about 5k cycles a version. The cheapest, `parses_a_release`, parses one
version and compares it.

## What this says, with the arithmetic left visible

- **This suite costs about 3 M cycles.** That fits in one shard (2^24 ≈
  16.8 M). Whether proving it is dominated by the cycles or by fixed
  per-proof work (setup, recursion, compression) is the next number to
  measure. The design doc's §4.2 ballpark ("tens of millions of RISC-V
  cycles/s" on a rig) would put this trace at well under a second; that is
  an extrapolation, not a measurement.
- **The fixed overhead per execution is about 90–120k cycles, 3–4% of this
  run.** Per-test proving (design §5) would pay it once per test: roughly
  21 × 100k ≈ 2.1 M extra cycles, about 70% on top of the whole suite, plus
  a proof each. The median test (11k cycles) costs about a tenth of that
  overhead, so for most tests a per-test proof would mostly prove start-up
  and report encoding. Proving per file or per cone is the better grain until
  recursion folding is measured.
- **The cost sits in two tests.** They take 2.56 M of the 2.89 M test
  cycles (89%); the other 19 take 0.33 M together. With §5's cache keyed by
  dependency cone, a change outside those two tests' cones would re-prove
  about a ninth of the test cycles.
- **The executor runs this trace at about 19–24 M cycles/s** (2.97 M cycles
  in 124–157 ms), 25–40× the 4–5 ms the same cases take as a native
  process. That keeps `execute` cheap enough to be the inner loop for cost
  estimates.
- **Deriving `H` costs the verifier about 7 s of wall time and 24 s of CPU**
  on top of the rebuild. RISC Zero hashes its image ID straight from the ELF,
  which is cheaper; SP1 runs a preprocessing `setup`. Workflows can cache it
  per ELF hash.
- **A failing test** halts the guest with exit code 1 before it commits
  anything. SP1 6.9's `execute` still returns `Ok` in that case, so
  `zkci-prove` checks the exit code and prints the guest's captured stderr,
  which holds the assertion message. Checked by breaking `parses_a_release`
  locally, without committing the change.
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
  (`sha256 e4a62305…32e0`, verifying key `0x00107628…5f72`).
- **The local build is not portable across paths.** The ELF embeds absolute
  host paths from panic locations: the build script's `OUT_DIR`, which lies
  under the checkout, and dependency sources under `$CARGO_HOME`. The
  workspace's own crates, the example included, are recorded relative to the
  workspace. The same source built at another path gave a 321,232-byte ELF
  with a different verifying key (`0x00a983cd…fb46`). The Docker build fixes
  the path (`/root/program`). `--remap-path-prefix` would be the fix for
  local builds, if they are ever wanted.
