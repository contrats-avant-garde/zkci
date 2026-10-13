//! The receipt envelope: a versioned JSON wrapper around the evidence.
//!
//! The `evidence.type` field exists from the first version so the same
//! verifier and check run can later accept other backends (design §6).
//! Only `zkvm` is implemented; `signed` and `tee` parse, and the verifier
//! rejects them as unimplemented.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sp1_sdk::{HashableKey, SP1ProofWithPublicValues, SP1VerifyingKey};

/// The envelope layout this crate writes and reads.
pub const ENVELOPE_VERSION: u32 = 1;

/// The receipt file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    /// [`ENVELOPE_VERSION`] at the time it was written.
    pub version: u32,
    /// What backs the claim.
    pub evidence: Evidence,
}

/// The kind of evidence, tagged by `type`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Evidence {
    /// A zkVM proof whose public values are a `zkci_harness::SuiteReport`.
    Zkvm(ZkvmEvidence),
    /// Reserved: a signed attestation (Sigstore / in-toto).
    Signed,
    /// Reserved: a TEE attestation (SEV-SNP / TDX).
    Tee,
}

/// A zkVM proof and the key it verifies under.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZkvmEvidence {
    /// The proof system. Only `sp1` is supported.
    pub system: String,
    /// The SP1 release that produced the proof.
    pub system_version: String,
    /// `bytes32()` of [`Self::vk`], for people reading the file. The
    /// verifier recomputes it and never trusts this copy.
    pub vk_hash: String,
    /// Hex of the bincode-encoded `SP1VerifyingKey`.
    pub vk: String,
    /// Hex of the bincode-encoded `SP1ProofWithPublicValues`.
    pub proof: String,
}

impl Envelope {
    /// Wraps an SP1 proof and its verifying key.
    pub fn sp1(vk: &SP1VerifyingKey, proof: &SP1ProofWithPublicValues) -> Result<Self> {
        Ok(Self {
            version: ENVELOPE_VERSION,
            evidence: Evidence::Zkvm(ZkvmEvidence {
                system: "sp1".into(),
                system_version: proof.sp1_version.clone(),
                vk_hash: vk.bytes32(),
                vk: hex::encode(bincode::serialize(vk).context("encoding vk")?),
                proof: hex::encode(bincode::serialize(proof).context("encoding proof")?),
            }),
        })
    }

    /// Parses an envelope, rejecting versions this crate does not know.
    pub fn from_json(json: &str) -> Result<Self> {
        let envelope: Self = serde_json::from_str(json).context("parsing receipt envelope")?;
        if envelope.version != ENVELOPE_VERSION {
            bail!(
                "receipt envelope version {} (this verifier reads {ENVELOPE_VERSION})",
                envelope.version
            );
        }
        Ok(envelope)
    }
}

impl ZkvmEvidence {
    /// Decodes the verifying key.
    pub fn decode_vk(&self) -> Result<SP1VerifyingKey> {
        let bytes = hex::decode(&self.vk).context("vk is not hex")?;
        bincode::deserialize(&bytes).context("decoding vk")
    }

    /// Decodes the proof.
    pub fn decode_proof(&self) -> Result<SP1ProofWithPublicValues> {
        let bytes = hex::decode(&self.proof).context("proof is not hex")?;
        bincode::deserialize(&bytes).context("decoding proof")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_type_is_a_tag() {
        let envelope = Envelope {
            version: ENVELOPE_VERSION,
            evidence: Evidence::Zkvm(ZkvmEvidence {
                system: "sp1".into(),
                system_version: "v6.9.0".into(),
                vk_hash: "0x01".into(),
                vk: "00".into(),
                proof: "00".into(),
            }),
        };
        let json = serde_json::to_value(&envelope).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["evidence"]["type"], "zkvm");
        assert_eq!(json["evidence"]["system"], "sp1");
        let back = Envelope::from_json(&json.to_string()).unwrap();
        assert_eq!(back, envelope);
    }

    #[test]
    fn reserved_evidence_types_parse() {
        for (kind, evidence) in [("signed", Evidence::Signed), ("tee", Evidence::Tee)] {
            let json = format!(r#"{{"version":1,"evidence":{{"type":"{kind}"}}}}"#);
            assert_eq!(Envelope::from_json(&json).unwrap().evidence, evidence);
        }
    }

    #[test]
    fn unknown_version_is_rejected() {
        let err = Envelope::from_json(r#"{"version":2,"evidence":{"type":"tee"}}"#).unwrap_err();
        assert!(err.to_string().contains("version 2"), "{err}");
    }

    #[test]
    fn garbage_evidence_fails_to_decode() {
        let evidence = ZkvmEvidence {
            system: "sp1".into(),
            system_version: "v6.9.0".into(),
            vk_hash: String::new(),
            vk: "zz".into(),
            proof: "0102".into(),
        };
        assert!(evidence.decode_vk().is_err());
        assert!(evidence.decode_proof().is_err());
    }
}
