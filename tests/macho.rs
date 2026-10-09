//! The Mach-O tests' targets. Tests run natively on the host, on an arm64
//! host also for x86_64 under Rosetta, and for a simulator triple on a
//! simulator device, which runs the programs as QEMU runs a cross
//! target's in the ELF tests.

use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

use crate::simulator::{self, Device, Runtime, Simulator};
use crate::{
    Mode, Options, Target, fail, make_jobs, parse_options, prepare_work_dir, print_inventory,
    print_summary, run_jobs, selected_scripts, usage,
};

/// The simulator triples a run adds to the host's targets by default, and
/// those --all adds too, each when a runtime runs programs of its
/// architecture (an x86_64 one also needs Rosetta). Only an older runtime
/// runs x86_64 programs, iOS 17's.
const DEFAULT_SIMULATORS: &[&str] = &["arm64-apple-ios-simulator"];
const MORE_SIMULATORS: &[&str] =
    &["x86_64-apple-ios-simulator", "arm64-apple-tvos-simulator", "arm64-apple-xros-simulator"];

/// A target, with the simulator runtime that runs its programs if it
/// isn't the host's. A host target builds and runs macOS programs; a
/// simulator target builds for the triple (whose OS version is that of
/// the newest runtime, unless the triple names one) and runs the programs
/// on a device of the runtime.
struct Config {
    target: Target,
    runtime: Option<Runtime>,
}

fn host(arch: &str, linker: &Path) -> Config {
    let target = Target {
        label: arch.to_owned(),
        arch: None,
        env: vec![
            ("mold", Some(linker.display().to_string())),
            ("ARCH", Some(arch.to_owned())),
            ("TRIPLE", None),
            ("SIMULATOR", None),
        ],
    };
    Config { target, runtime: None }
}

fn simulator(simulator: &Simulator, runtime: Runtime, linker: &Path) -> Config {
    let triple = format!("{}-apple-{}{}-simulator", simulator.arch, simulator.os, runtime.version);
    let target = Target {
        label: simulator.label(),
        arch: None,
        env: vec![
            ("mold", Some(linker.display().to_string())),
            ("ARCH", Some(simulator.arch.clone())),
            ("TRIPLE", Some(triple)),
        ],
    };
    Config { target, runtime: Some(runtime) }
}

fn native_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") { "arm64" } else { "x86_64" }
}

/// Whether x86_64 binaries run on this arm64 host.
fn rosetta_available() -> bool {
    Command::new("arch")
        .args(["-x86_64", "/usr/bin/true"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Returns the targets to run and the simulator triples left out for want
/// of a runtime (as on a machine without Xcode's simulators).
fn selected_targets(options: &Options, linker: &Path) -> (Vec<Config>, Vec<String>) {
    let native = native_arch();
    let mut targets = vec![host(native, linker)];
    let mut unavailable = Vec::new();
    match &options.mode {
        Mode::Native | Mode::Default | Mode::All => {
            let rosetta = native == "arm64" && rosetta_available();
            if rosetta {
                targets.push(host("x86_64", linker));
            }
            let simulators: &[&[&str]] = match options.mode {
                Mode::Default => &[DEFAULT_SIMULATORS],
                Mode::All => &[DEFAULT_SIMULATORS, MORE_SIMULATORS],
                _ => &[],
            };
            for triple in simulators.concat() {
                let sim = Simulator::parse(triple).unwrap();
                match simulator::find_runtime(&sim) {
                    Some(runtime) if sim.arch == native || rosetta => {
                        targets.push(simulator(&sim, runtime, linker))
                    }
                    _ => unavailable.push(triple.to_owned()),
                }
            }
        }
        Mode::Triple(triple) => {
            let Some(sim) = Simulator::parse(triple) else {
                eprintln!("mold-tests: {triple}: not a simulator triple");
                usage();
            };
            let Some(runtime) = simulator::find_runtime(&sim) else {
                fail(format!("no simulator runtime for {triple}"));
            };
            targets = vec![simulator(&sim, runtime, linker)];
        }
    }
    (targets, unavailable)
}

/// Boots a device for each simulator target, which its scripts run their
/// programs on. Dropping a device shuts it down.
fn boot_devices(configs: Vec<Config>) -> Result<(Vec<Target>, Vec<Device>), String> {
    let mut targets = Vec::new();
    let mut devices = Vec::new();
    for Config { mut target, runtime } in configs {
        if let Some(runtime) = &runtime {
            let device = Device::boot(runtime)?;
            target.env.push(("SIMULATOR", Some(device.udid.clone())));
            devices.push(device);
        }
        targets.push(target);
    }
    Ok((targets, devices))
}

/// Runs the Mach-O test scripts in `cases_dir` with the linker at `mold`.
/// Command line arguments are substring patterns selecting a subset of
/// tests.
pub fn run(cases_dir: &Path, mold: &Path) -> ExitCode {
    let options = parse_options();
    let work_dir = prepare_work_dir(mold).unwrap_or_else(|err| fail(err));
    let linker = work_dir.join("ld64.mold");
    let (configs, unavailable) = selected_targets(&options, &linker);
    let scripts = selected_scripts(cases_dir, &options.patterns).unwrap_or_else(|err| fail(err));
    if options.list {
        let targets = configs.into_iter().map(|config| config.target).collect();
        return print_inventory(&scripts, &work_dir, targets, &unavailable);
    }
    if !unavailable.is_empty() {
        eprintln!("skipping simulators without a runtime: {}", unavailable.join(", "));
    }

    // The host's targets run together, then each simulator's on its own,
    // its device booted for it alone and shut down once it is done: a
    // booted simulator is a whole OS, and one at a time is load enough.
    // Errors return rather than exit, so that a device is shut down.
    let (hosts, simulators): (Vec<_>, Vec<_>) =
        configs.into_iter().partition(|config| config.runtime.is_none());
    let mut results = Vec::new();
    for phase in std::iter::once(hosts).chain(simulators.into_iter().map(|config| vec![config])) {
        if phase.is_empty() || scripts.is_empty() {
            continue;
        }
        let (targets, devices) = match boot_devices(phase) {
            Ok(booted) => booted,
            Err(err) => {
                eprintln!("mold-tests: {err}");
                return ExitCode::FAILURE;
            }
        };
        let jobs = match make_jobs(&scripts, &work_dir, targets, true) {
            Ok(jobs) => jobs,
            Err(err) => {
                eprintln!("mold-tests: {err}");
                return ExitCode::FAILURE;
            }
        };
        results.extend(run_jobs(&work_dir, jobs, &options));
        drop(devices);
    }
    print_summary(&results)
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::process::Output;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn run_script(body: &str) -> Output {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = env::temp_dir().join(format!(
            "mold-harness-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        let common = Path::new(env!("CARGO_MANIFEST_DIR")).join("macho/common.inc");
        let output = Command::new("bash")
            .args(["-c", &format!("source \"$1\"\n{body}"), "harness-test"])
            .arg(common)
            .env("mold", "unused")
            .current_dir(&dir)
            .output()
            .unwrap();
        fs::remove_dir_all(dir).unwrap();
        output
    }

    #[test]
    fn negated_failure_at_exit_is_not_success() {
        let output = run_script("! true");
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("OK"));
    }

    #[test]
    fn negative_assertion_stops_before_later_commands() {
        let output = run_script("not true\necho reached");
        assert!(!output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(!stdout.contains("reached"));
        assert!(!stdout.contains("OK"));
        assert!(String::from_utf8_lossy(&output.stderr).contains("unexpectedly succeeded"));
    }

    #[test]
    fn explicit_exit_status_is_preserved() {
        assert_eq!(run_script("exit 7").status.code(), Some(7));
    }

    #[test]
    fn expected_failure_and_success_pass() {
        let output = run_script("not false\ntrue");
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("OK"));
    }

    #[test]
    fn skip_remains_successful() {
        let output = run_script("skip");
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("skipped"));
        assert!(!stdout.contains("OK"));
    }
}
