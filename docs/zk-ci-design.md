# Verifiable CI: proving a local test run instead of re-running it remotely

Design handoff from a chat conversation, 2026-10-13. Intended as the first thing a Claude Code session reads before scaffolding a prototype. Numbers marked *ballpark* are order-of-magnitude estimates, not measurements.

## 1. Problem and motivation

GitHub-hosted CI minutes are metered and run out. The idea: run the test suite locally, produce a succinct proof that "the tests passed on commit C", attach it to the PR, and have a tiny verifier (seconds, free tier) set the merge-gate status instead of a full remote run.

**Baseline to beat:** GitHub does not meter self-hosted runners. If the only requirement is "run CI on my own hardware", that is a config change. The proof-based version earns its keep only when the *verifier does not trust the machine the tests ran on*:

- PRs from strangers (open source, bounties)
- Bounties paid on a green check, possibly settled on-chain
- A closed-source codebase proving it passes a public conformance suite
- Proof that a *failing* input exists (proof of exploit) without revealing it

Design for that niche. The cost argument alone loses to self-hosted runners.

## 2. The statement being proved

> A program with hash `H` was executed on inputs committing to `R` and produced output `PASS` (or a pass/fail vector).

- **Public inputs:** `H` (hash / image ID of the test-runner binary), `R` (source tree root; must match the PR's git tree hash), the result.
- **Witness:** the execution trace. Supplied by the prover, never seen by the verifier.

"Zero-knowledge" is mostly irrelevant for a public repo. What matters is **succinctness** (ms verification) and **soundness**. In zkVMs the privacy property is optional; turn it on only for closed-source repos or secret fixtures. Call the thing *verifiable CI* to avoid over-engineering.

## 3. Architecture

```
developer machine                         GitHub
┌──────────────────────────┐              ┌──────────────────────────────┐
│ source @ commit C        │              │ PR (head = C)                │
│   └─ guest ELF (tests)   │   receipt    │   refs/proofs/<C>  ◄─────────┼── prover pushes
│ prover (GPU) ──────────► │ ───────────► │   verify job / GitHub App:   │
│   receipt {H, R, PASS}   │              │     rebuild guest → H' == H? │
└──────────────────────────┘              │     verify(receipt)          │
                                          │     R == tree(C)?            │
                                          │     → check run: pass/fail   │
                                          └──────────────────────────────┘
```

1. **Guest program.** The test runner compiled to RISC-V for SP1 (`riscv32im-succinct-zkvm-elf`) or RISC Zero. Custom harness (`harness = false`): libtest needs threads; the guest has no threads, network, clock, or filesystem. Everything the tests read enters via host I/O as witness.
2. **Prover.** Local GPU. Emits a receipt. Compressed/Groth16-wrapped for small size and on-chain verifiability.
3. **Delivery.** Prover pushes the receipt to `refs/proofs/<sha>` (or git notes). Avoids the chicken-and-egg of committing the receipt into the tree it attests. Alternative: an external store with the URL in the PR.
4. **Verifier.** A workflow job or, better, a **GitHub App** (immune to PR edits of `.github/workflows/`, consumes zero Actions minutes). Steps: fetch receipt, reproducibly rebuild guest from PR head and compare image ID, verify receipt, check `R` against the tree hash, post a check run.

STARK receipt verification: seconds on CPU. Groth16 wrap: milliseconds, a few hundred bytes.

## 4. The three hard problems

### 4.1 Linking the proof to the source
The receipt proves a *binary* ran. Verifier needs binary ↔ commit.

- **Reproducible guest builds** (RISC Zero: `RISC0_USE_DOCKER=1`; SP1: `cargo prove build --docker` — verify current flags). Verifier rebuilds and compares image ID. Build still happens in CI; savings = test-execution time only. Image IDs cannot be allowlisted because the guest contains the code under test and changes every commit.
- **Prove compilation too.** Research-stage: Ron & Monperrus (KTH, Feb 2026) run ChibiCC inside RISC Zero. Nobody has pushed rustc through a zkVM.
- **Fixed interpreter as guest, source as input.** For Python/JS: `H` is fixed, `R` is checked directly against the git tree. Cleanest linkage, worst performance.

### 4.2 Cost
Frontier (mid-2026): SP1 Hypercube proves 99.7% of Ethereum blocks in <12 s on 16× RTX 5090; ethproofs average ~4 cents/block, down ~45× in a year. *Ballpark:* tens of millions of RISC-V cycles/s on a ~$50k rig vs a few billion native → ~100× slower than native on the rig, ~1000×+ on one consumer GPU. A 1-minute native suite ≈ hours on the rig, a day on a desktop. Precompiles help crypto-heavy code, not ordinary test logic.

### 4.3 Hermeticity
No time, randomness, network, filesystem, threads, env vars. Dependencies that can run inside the guest (SQLite) are fine; a real DB is not. Upside: flaky tests cease to exist by construction.

## 5. Making it tractable: incremental proving (the product)

- Prove each test (or test file) as its own execution.
- Cache key: `(test id, hash of its dependency cone)`.
- On each PR, re-prove only tests whose cone changed; fold cached + new proofs into one PR-level proof via recursion/composition (supported by SP1 and RISC Zero).
- Prerequisite: hermetic, fine-grained dependency tracking (Bazel-style) so "cone unchanged" is itself a verifiable claim.
- The proof cache is the moat.

## 6. Pluggable evidence backends

Same verifier, same check run, trust dial:

| Backend | Trust | Overhead | Status |
|---|---|---|---|
| Signed attestation (Sigstore / in-toto) | contributor keys | ~0 | build today |
| TEE attestation (SEV-SNP / TDX, cheap bare-metal) | AMD/Intel | ~native | build today |
| zkVM proof | math | 100–1000×+ | prototype |

Build the verifier with an `evidence` type field from day one.

## 7. CI workflow changes (small)

Old: checkout → toolchain → build → `cargo test`.
New (compiled language):

```yaml
verify:
  steps:
    - uses: actions/checkout@v4
    - run: git fetch origin refs/proofs/${{ github.event.pull_request.head.sha }}
    - run: cargo prove build --docker            # reproducible guest build
    - run: verifier check receipt.bin \
             --expect-image-id "$(cat elf.id)" \
             --expect-tree "${{ github.event.pull_request.head.sha }}"
```

Plumbing decisions:
- **Receipt transport:** `refs/proofs/<sha>` + `workflow_dispatch` from the prover CLI, or the PR sits in "awaiting proof" until the next sync.
- **Tamper resistance:** verifier runs from the base branch (`pull_request_target`, carefully) or lives outside the repo as a GitHub App. App is preferred.
- **Hybrid split:** `verify-proof` (required, provable subset) + `integration` (conventional/self-hosted runner, everything else). Migrate tests leftward over time.

## 8. Test code changes

Pure unit tests: no body changes. Project-level only: a `[[bin]]`/`[[test]]` target with `harness = false`, a harness dependency, `rust-toolchain.toml`, committed lockfile, Docker build path. The same `#[test]` fns still run natively under `cargo test` for the inner loop.

Five rewrite buckets (mechanical unless `now()`/`rand` are deep in production code):

| Bucket | Change |
|---|---|
| Time | `SystemTime::now()` → injected clock / fixed timestamp as input |
| Randomness | `thread_rng()` → seeded RNG (proptest/quickcheck already seed) |
| Filesystem fixtures | `fs::read` → `include_bytes!` or witness; keep fixtures small or Merkle-commit and load only touched leaves (every byte read costs cycles) |
| Async | `#[tokio::test]` → single-threaded executor (`pollster`/`block_on`) if the future is pure |
| External services | not rewritten; stays in the integration column |

Hard wall: tests that spawn threads or use `rayon`. Mocks: in-memory trait impls fine; `mockall` usually fine; `wiremock`/`testcontainers` → integration column.

Failure reporting: guests are typically `panic = abort`, so a failing `assert!` just means no PASS proof exists. For "47 passed, `test_foo` failed", tests return `Result`, or prove per-test (wanted anyway for §5).

**First step is an audit, not a rewrite:** compile the existing test target for the zkVM and let the linker report which tests pull in `getrandom`, `std::thread`, `std::net`, `std::time`. Sort failures into the buckets.

## 9. Local developer loop

Proving can't block `git push` (hours vs seconds). Shape: a background prover (daemon or detaching pre-push hook) proves the affected cone, pushes the receipt ref when done; PR shows "awaiting proof" meanwhile. This latency is what users will notice, more than any YAML edit.

## 10. Prior art (checked 2026-10-13)

Nobody found building exactly "proof of local test run as merge gate". Adjacent:

- **Proof of compilation:** Ron & Monperrus, *Verifiable Provenance of Software Artifacts with Zero-Knowledge Compilation*, arXiv 2602.11887 (RISC Zero + ChibiCC; OpenSSL/libsodium files). https://www.arxiv.org/abs/2602.11887 — code: https://github.com/ASSERT-KTH/verifiable-compilation
- **TEE-attested CI builds:** Automata Network *TEE Compile* (AWS Nitro as GitHub Actions runner, attestation of build inputs/outputs). https://docs.ata.network/tee-overview/tee-compile
- **TEE + Nix evidence protocol:** Castillo et al., *An Evidence-driven Protocol for Trustworthy CI Pipelines*, EDOC 2026 (Nix + Intel TDX). https://www.alphaxiv.org/abs/2605.21089
- **SGX self-hosted runner pattern:** attestable-mcp-server (Gramine, RA-TLS). https://metorial.com/marketplace/s/co-browser/attestable-mcp-server
- **ZK proof of exploit:** zkPoEX (RISC Zero + SputnikVM, ETHDenver). https://www.risczero.com/blog/zkpoex — Trail of Bits / Matthew Green (2020). https://blog.trailofbits.com/2020/05/21/reinventing-vulnerability-disclosure-using-zero-knowledge-proofs/ — Galois *Cheesecloth*, arXiv 2301.01321. https://ar5iv.labs.arxiv.org/html/2301.01321
- **Proved conformance tests in CI:** eth-act/zkevm-test-monitor PR #61 (prove + verify on OpenVM, ~1 min/test). https://github.com/eth-act/zkevm-test-monitor/pull/61
- **zkVM landscape / numbers:** https://wavect.io/blog/zero-knowledge-proofs-production-2026/ and https://blog.succinct.xyz/real-time-proving-16-gpus/
- A landing page called "VeriSource" (static Hugging Face Space) describes the concept with no code behind it. Not a competitor.

## 11. Prototype plan

Goal: measure, not ship. Get a real cycle count and proof time for a real crate.

**M0 — pick a target crate.** A pure-logic crate from your own projects (parser, protocol logic, crypto helper). Must already have `#[test]` unit tests with no DB/network.

**M1 — harness crate (`zkci-harness`).**
- `#[zkci::test]` attribute (or `inventory`-based collection of existing `#[test]` fns).
- Generates a `main` that runs tests sequentially, records `Vec<(name, Result)>`, commits `(image_id_hint, tree_root, results)` via `sp1_zkvm::io::commit` (or RISC Zero `env::commit`).
- Native mode: same fns run under plain `cargo test`.

**M2 — guest + prover.**
- `guest/` crate targeting SP1 (or RISC Zero), `harness = false`, `panic = abort`.
- `prove` script: `cargo prove build --docker` → run prover → write `receipt.bin` + `elf.id`.
- Record: cycle count, proof time, peak RAM, receipt size. Try `execute` (no proof) first to get cycles cheaply.

**M3 — verifier CLI (`zkci-verify`).**
- Inputs: receipt, expected image ID, expected tree hash.
- Verifies receipt, compares public values, exits 0/1 with a JSON report.
- Evidence-type field in the receipt envelope from the start (`zkvm` | `signed` | `tee`).

**M4 — GitHub glue.**
- Prover pushes to `refs/proofs/<sha>`.
- Workflow: fetch ref → rebuild guest → `zkci-verify` → status. Required check in branch protection.

**M5 — audit tool.** `zkci audit`: build the test target for the zkVM, parse linker/compile errors, bucket by `getrandom` / `std::thread` / `std::net` / `std::time` / `std::fs`, print a migration report.

**Later:** per-test proving + cache keyed by dependency cone; recursion to fold proofs; GitHub App verifier; TEE backend.

**Acceptance for the prototype:** one real crate, one PR, green check produced from a locally generated receipt, with measured numbers in a `BENCH.md`.

## 12. Open questions

- Which zkVM: SP1 vs RISC Zero vs OpenVM? Decide on reproducible-build ergonomics and Rust `std` coverage, not raw speed.
- Receipt format envelope: versioning, evidence type, how to carry per-test results.
- Dependency-cone hashing without Bazel: can `cargo metadata` + file hashes approximate it well enough for the cache?
- Does the GitHub App route need its own hosting, and who pays for verification compute at scale?
- Is there a bounty/escrow product on the other side (pay-on-proof) that makes this worth building beyond a tool?

## Suggested first prompt for Claude Code

> Read docs/zk-ci-design.md. Scaffold M1–M3 as a Cargo workspace (`zkci-harness`, `guest`, `zkci-verify`, plus a `prove.sh`). Use SP1 unless its current docs make reproducible guest builds harder than RISC Zero's; check the docs before choosing. Pick the smallest pure-logic crate in this repo as the M0 target. Don't generate a proof yet — get `execute` working and report the cycle count.
