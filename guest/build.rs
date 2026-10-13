use std::{env, path::PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let collected = zkci_harness::build::collect(
        manifest.join("../targets/example/src/tests.rs"),
        "tests",
        out.join("example_tests.rs"),
    );
    assert!(
        !collected.runnable.is_empty(),
        "zkci: no runnable tests found in zkci-example"
    );
}
