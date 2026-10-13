//! `zkci-verify`: checks a receipt and prints a JSON verdict on stdout.
//! Exits 0 only when the verdict is ok.
//!
//! ```sh
//! zkci-verify receipt.json --expect-tree "$(git rev-parse HEAD^{tree})" --elf rebuilt.elf
//! zkci-verify receipt.json --expect-tree "$TREE" --expect-vk "$(cat elf.id)"
//! ```

use std::{fs, path::PathBuf, process::ExitCode};

use anyhow::{anyhow, bail, Context, Result};
use clap::Parser;
use sp1_sdk::{HashableKey, Prover, ProverClient, ProvingKey, SP1VerifyingKey};
use zkci_verify::{
    check::{decode_report, normalize_hex, Verdict},
    envelope::{Envelope, Evidence},
};

#[derive(Parser)]
#[command(about = "Verify a verifiable-CI receipt")]
struct Cli {
    /// The receipt envelope written by `zkci-prove prove`.
    receipt: PathBuf,
    /// The git tree hash the proof must be for (`git rev-parse <commit>^{tree}`).
    #[arg(long)]
    expect_tree: String,
    /// The verifying-key hash the proof must verify under, from
    /// `zkci-prove vkey` on a reproducibly rebuilt guest.
    #[arg(
        long,
        alias = "expect-image-id",
        required_unless_present = "elf",
        conflicts_with = "elf"
    )]
    expect_vk: Option<String>,
    /// A guest ELF rebuilt from the commit. Its verifying key is derived
    /// here, so nothing in the receipt chooses the key.
    #[arg(long)]
    elf: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let verdict = verify(&cli).await.unwrap_or_else(|e| Verdict {
        problems: vec![format!("{e:#}")],
        ..Verdict::default()
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&verdict).expect("verdict serializes")
    );
    if verdict.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

async fn verify(cli: &Cli) -> Result<Verdict> {
    let json = fs::read_to_string(&cli.receipt)
        .with_context(|| format!("reading {}", cli.receipt.display()))?;
    let evidence = match Envelope::from_json(&json)?.evidence {
        Evidence::Zkvm(evidence) => evidence,
        Evidence::Signed => bail!("evidence type `signed` is not implemented yet"),
        Evidence::Tee => bail!("evidence type `tee` is not implemented yet"),
    };
    if evidence.system != "sp1" {
        bail!("unsupported zkVM `{}`", evidence.system);
    }

    let client = ProverClient::builder().light().build().await;
    let vk: SP1VerifyingKey = match (&cli.elf, &cli.expect_vk) {
        (Some(elf), _) => {
            let elf = fs::read(elf).with_context(|| format!("reading {}", elf.display()))?;
            let pk = client
                .setup(elf.into())
                .await
                .map_err(|e| anyhow!("deriving the verifying key from the ELF: {e}"))?;
            pk.verifying_key().clone()
        }
        (None, Some(expect)) => {
            let vk = evidence.decode_vk()?;
            if normalize_hex(&vk.bytes32()) != normalize_hex(expect) {
                bail!("receipt is for program {}, expected {expect}", vk.bytes32());
            }
            vk
        }
        (None, None) => unreachable!("clap requires --elf or --expect-vk"),
    };

    let proof = evidence.decode_proof()?;
    // Checks the proof, the SP1 version, a zero exit code, and that the
    // public values are the ones the proof commits to.
    client
        .verify(&proof, &vk, None)
        .map_err(|e| anyhow!("proof does not verify: {e}"))?;
    let report = decode_report(proof.public_values.as_slice())?;

    Ok(Verdict {
        evidence: "zkvm".into(),
        vk_hash: vk.bytes32(),
        ..Verdict::default()
    }
    .with_report(&report, &cli.expect_tree))
}
