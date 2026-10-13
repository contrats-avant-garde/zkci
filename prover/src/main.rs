//! `zkci-prove`: runs the guest ELF on the host.
//!
//! - `execute`: runs the guest in the SP1 executor without proving and
//!   prints cycle counts with the decoded report. Cheap, and the number
//!   every cost estimate starts from.
//! - `vkey`: prints the verifying-key hash of an ELF (the design's `H`).
//! - `prove`: generates a compressed proof and writes a receipt envelope.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Instant,
};

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;
use sp1_sdk::{
    Elf, HashableKey, ProveRequest, Prover, ProverClient, ProvingKey, SP1Stdin, SP1VerifyingKey,
};
use tokio::sync::watch;
use zkci_harness::SuiteReport;
use zkci_verify::{check::decode_report, envelope::Envelope};

#[derive(Parser)]
#[command(about = "Execute or prove the verifiable-CI guest")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the guest without proving and print cycle counts as JSON.
    Execute {
        /// The guest ELF.
        #[arg(long)]
        elf: PathBuf,
        /// The git tree hash of the commit under test.
        #[arg(long)]
        tree: String,
    },
    /// Print the verifying-key hash of a guest ELF.
    Vkey {
        /// The guest ELF.
        #[arg(long)]
        elf: PathBuf,
    },
    /// Prove the guest (compressed) and write a receipt envelope.
    Prove {
        /// The guest ELF.
        #[arg(long)]
        elf: PathBuf,
        /// The git tree hash of the commit under test.
        #[arg(long)]
        tree: String,
        /// Where to write the receipt.
        #[arg(long)]
        out: PathBuf,
    },
}

/// `execute` output.
#[derive(Serialize)]
struct Execution {
    suite: String,
    tree: String,
    passed: usize,
    failed: usize,
    excluded: usize,
    /// Executed RISC-V cycles: what proving cost scales with. SP1 6.9's
    /// execute path leaves the report's syscall and memory counters at
    /// zero, so they are not printed.
    cycles: u64,
    gas: Option<u64>,
    elf_bytes: usize,
    execute_ms: u128,
    /// Cycles per test, from a `cycle-tracker` guest run by a
    /// `profiling` build. Empty otherwise.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    per_test: BTreeMap<String, u64>,
}

#[tokio::main]
async fn main() -> ExitCode {
    sp1_sdk::utils::setup_logger();
    match run(Cli::parse().command).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("zkci-prove: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(command: Command) -> Result<ExitCode> {
    match command {
        Command::Execute { elf, tree } => {
            let elf_bytes = read_elf(&elf)?;
            let len = elf_bytes.len();
            let client = ProverClient::builder().light().build().await;
            // Guest stderr carries the panic message of a failing test.
            let (stderr_tx, stderr_rx) = watch::channel(String::new());
            let start = Instant::now();
            let (public_values, report) = client
                .execute(elf_bytes.into(), stdin(&tree))
                .calculate_gas(true)
                .stderr(stderr_tx)
                .await
                .map_err(|e| anyhow!("guest execution failed: {e}"))?;
            let execute_ms = start.elapsed().as_millis();
            // A panic halts the guest with exit code 1 before it commits
            // anything, and execute still returns Ok.
            if report.exit_code != 0 || public_values.as_slice().is_empty() {
                bail!(
                    "guest exited with code {} and committed no report: a test failed, so \
                     no passing proof exists. Guest stderr:\n{}",
                    report.exit_code,
                    stderr_rx.borrow().trim_end()
                );
            }
            let suite = decode_report(public_values.as_slice())?;
            let out = Execution {
                suite: suite.suite.clone(),
                tree: suite.tree.clone(),
                passed: suite.passed(),
                failed: suite.failed(),
                excluded: suite.excluded.len(),
                cycles: report.total_instruction_count() + report.total_syscall_count(),
                gas: report.gas(),
                elf_bytes: len,
                execute_ms,
                per_test: report.cycle_tracker.into_iter().collect(),
            };
            println!("{}", serde_json::to_string_pretty(&out)?);
            Ok(pass_or_fail(&suite))
        }
        Command::Vkey { elf } => {
            let vk = setup_vk(read_elf(&elf)?.into()).await?;
            println!("{}", vk.bytes32());
            Ok(ExitCode::SUCCESS)
        }
        Command::Prove { elf, tree, out } => {
            let client = ProverClient::builder().cpu().build().await;
            let pk = client
                .setup(read_elf(&elf)?.into())
                .await
                .map_err(|e| anyhow!("setup: {e}"))?;
            let proof = client
                .prove(&pk, stdin(&tree))
                .compressed()
                .await
                .map_err(|e| anyhow!("proving: {e}"))?;
            let suite = decode_report(proof.public_values.as_slice())?;
            let envelope = Envelope::sp1(pk.verifying_key(), &proof)?;
            fs::write(&out, serde_json::to_string(&envelope)?)
                .with_context(|| format!("writing {}", out.display()))?;
            eprintln!("zkci-prove: wrote {}", out.display());
            Ok(pass_or_fail(&suite))
        }
    }
}

fn read_elf(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("reading {}", path.display()))
}

fn stdin(tree: &str) -> SP1Stdin {
    let mut stdin = SP1Stdin::new();
    stdin.write(&tree.to_owned());
    stdin
}

async fn setup_vk(elf: Elf) -> Result<SP1VerifyingKey> {
    let client = ProverClient::builder().light().build().await;
    let pk = client.setup(elf).await.map_err(|e| anyhow!("setup: {e}"))?;
    Ok(pk.verifying_key().clone())
}

fn pass_or_fail(suite: &SuiteReport) -> ExitCode {
    if suite.is_pass() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
