//! zkVM guest for the `zkci-example` suite.
//!
//! Runs every unit test in `targets/example/src/tests.rs`, unmodified, and
//! commits a [`SuiteReport`](zkci_harness::SuiteReport) as its public
//! values. Input: the git tree hash of the commit under test.
//!
//! Built for the host it is an ordinary binary (native mode) that runs the
//! same cases, takes the tree hash as its first argument, and prints the
//! report as JSON: the inner loop, and a cross-check that the guest runs
//! exactly what `cargo test -p zkci-example` runs.

#![cfg_attr(target_os = "zkvm", no_main)]

#[cfg(target_os = "zkvm")]
sp1_zkvm::entrypoint!(main);

// The test module starts with `use crate::*;`, so this crate's root has to
// export what the zkci-example root does.
#[allow(unused_imports)]
use zkci_example::*;

#[allow(dead_code, unused_imports)]
mod tests {
    include!(concat!(env!("OUT_DIR"), "/example_tests.rs"));
}

const SUITE: &str = "zkci-example";

#[cfg(target_os = "zkvm")]
pub fn main() {
    let tree: String = sp1_zkvm::io::read();
    let report = zkci_harness::run(SUITE, tree, tests::ZKCI_TESTS, tests::ZKCI_EXCLUDED);
    sp1_zkvm::io::commit(&report);
}

#[cfg(not(target_os = "zkvm"))]
fn main() {
    let tree = std::env::args().nth(1).unwrap_or_default();
    let report = zkci_harness::run(SUITE, tree, tests::ZKCI_TESTS, tests::ZKCI_EXCLUDED);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    if !report.is_pass() {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod native {
    #[test]
    fn every_collected_test_passes_natively() {
        let report = zkci_harness::run(
            super::SUITE,
            String::new(),
            super::tests::ZKCI_TESTS,
            super::tests::ZKCI_EXCLUDED,
        );
        assert!(report.is_pass(), "{report:#?}");
    }
}
