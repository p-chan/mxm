//! Output to a closed pipe (e.g. `mxm service status | head -1`) ends quietly instead of panicking

// `mxm service` prints only on macOS; on other platforms it fails before printing
#![cfg(target_os = "macos")]

use std::process::Command;

/// Runs mxm with stdout connected to a pipe whose read end is already closed
fn run_with_closed_stdout(args: &[&str]) -> std::process::Output {
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    Command::new(env!("CARGO_BIN_EXE_mxm"))
        .args(args)
        .stdout(writer)
        .output()
        .unwrap()
}

#[test]
fn service_status_exits_quietly_when_stdout_is_closed() {
    let out = run_with_closed_stdout(&["service", "status"]);
    assert!(out.status.success(), "{out:?}");
    assert!(out.stderr.is_empty(), "{out:?}");
}

#[test]
fn service_install_print_exits_quietly_when_stdout_is_closed() {
    let out = run_with_closed_stdout(&["service", "install", "--print"]);
    assert!(out.status.success(), "{out:?}");
    assert!(out.stderr.is_empty(), "{out:?}");
}
