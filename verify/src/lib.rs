//! Receipt format and checks for the verifiable-CI prototype.
//!
//! [`envelope`] is the file a prover hands over. [`check`] holds the claim
//! checks that run once the evidence itself has verified, kept pure so they
//! are tested without a proof.

pub mod check;
pub mod envelope;
