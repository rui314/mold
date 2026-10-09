//! Runs mold's shell tests in parallel for Cargo's test harness.
//!
//! The tests themselves deliberately remain shell scripts so that this port
//! exercises exactly the same inputs and toolchains as the system linker.
//! The runner owns test discovery, target selection, scheduling, timeouts
//! and reporting. Tests run natively on the host, on an arm64 host also
//! for x86_64 under Rosetta, and for a simulator triple on a simulator
//! device, which runs the programs as QEMU runs a cross target's in mold's
//! ELF suite.

// The scripts drive Apple's toolchain, so they run only on macOS.
// Elsewhere this crate is empty, which lets the workspace build there.
#![cfg(target_os = "macos")]

mod simulator;

use simulator::{Device, Runtime, Simulator};
use std::collections::BTreeMap;
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io;
use std::iter;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// The simulator configurations a run adds to the host's by default,
/// and those --all adds too, each when a runtime runs programs of its
/// architecture (an x86_64 one also needs Rosetta). Only an older
/// runtime runs x86_64 programs, iOS 17's.
const DEFAULT_SIMULATORS: &[&str] = &["arm64-apple-ios-simulator"];
const MORE_SIMULATORS: &[&str] =
    &["x86_64-apple-ios-simulator", "arm64-apple-tvos-simulator", "arm64-apple-xros-simulator"];

/// A configuration the scripts run in. A host configuration builds and
/// runs macOS programs; a simulator configuration builds for the target
/// triple (whose OS version is that of the newest runtime, unless the
/// triple names one) and runs the programs on a device of the runtime.
struct Target {
    arch: String,
    triple: Option<String>,
    runtime: Option<Runtime>,
    /// The UDID of the booted device that runs the programs.
    device: Option<String>,
    label: String,
}

impl Target {
    fn host(arch: &str) -> Self {
        Self {
            arch: arch.to_owned(),
            triple: None,
            runtime: None,
            device: None,
            label: arch.to_owned(),
        }
    }

    fn simulator(simulator: &Simulator, runtime: Runtime) -> Self {
        let triple =
            format!("{}-apple-{}{}-simulator", simulator.arch, simulator.os, runtime.version);
        Self {
            arch: simulator.arch.clone(),
            triple: Some(triple),
            runtime: Some(runtime),
            device: None,
            label: simulator.label(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Mode {
    /// The host architecture only.
    Native,
    /// The host architecture, plus x86_64 under Rosetta when available.
    Host,
    /// The host's configurations plus DEFAULT_SIMULATORS.
    Default,
    /// The host's configurations plus every simulator's.
    All,
    /// One simulator configuration.
    Triple(String),
}

struct Options {
    jobs: usize,
    mode: Mode,
    patterns: Vec<String>,
    timeout: Duration,
    list: bool,
}

struct TestJob {
    target: Arc<Target>,
    script: PathBuf,
    name: String,
    log: PathBuf,
    status_file: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Pass,
    Skip,
    Fail,
    Timeout,
}

impl Outcome {
    fn status(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Skip => "skip",
            Self::Fail | Self::Timeout => "fail",
        }
    }
}

struct TestResult {
    target: Arc<Target>,
    name: String,
    log: PathBuf,
    outcome: Outcome,
}

#[derive(Default)]
struct Counts {
    pass: usize,
    skip: usize,
    fail: usize,
}

impl Counts {
    fn add(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Pass => self.pass += 1,
            Outcome::Skip => self.skip += 1,
            Outcome::Fail | Outcome::Timeout => self.fail += 1,
        }
    }

    fn merge(&mut self, other: &Self) {
        self.pass += other.pass;
        self.skip += other.skip;
        self.fail += other.fail;
    }
}

fn usage() -> ! {
    eprintln!(
        "Usage: cargo test [pattern] [-- [--test-threads N] \
         [--native | --host | --all | --triple TRIPLE] [--timeout SECONDS] [--list]]\n\
         By default the tests run for macOS (arm64, and x86_64 under Rosetta) and the arm64 \
         iOS simulator. --all adds the x86_64 iOS, tvOS and visionOS simulators; --host runs \
         macOS's alone, --native the host architecture's, --triple one simulator's (e.g. \
         arm64-apple-tvos-simulator)."
    );
    std::process::exit(2);
}

fn parse_usize(value: Option<String>) -> usize {
    value.and_then(|s| s.parse().ok()).filter(|&n| n != 0).unwrap_or_else(|| usage())
}

fn parse_options() -> Options {
    let mut jobs = thread::available_parallelism().map_or(1, usize::from);
    let mut mode = Mode::Default;
    let mut mode_was_set = false;
    let mut patterns = Vec::new();
    let mut timeout = DEFAULT_TIMEOUT;
    let mut list = false;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-j" | "--jobs" | "--test-threads" => jobs = parse_usize(args.next()),
            "--native" => {
                mode = Mode::Native;
                mode_was_set = true;
            }
            "--host" => {
                mode = Mode::Host;
                mode_was_set = true;
            }
            "--all" => {
                mode = Mode::All;
                mode_was_set = true;
            }
            "--triple" => {
                mode = Mode::Triple(args.next().unwrap_or_else(|| usage()));
                mode_was_set = true;
            }
            "--timeout" => timeout = Duration::from_secs(parse_usize(args.next()) as u64),
            "--list" => list = true,
            "--nocapture" | "--show-output" => {}
            "-h" | "--help" => usage(),
            _ if arg.starts_with("-j") && arg.len() > 2 => {
                jobs = parse_usize(Some(arg[2..].to_owned()))
            }
            _ if arg.starts_with("--test-threads=") => {
                jobs = parse_usize(arg.split_once('=').map(|(_, value)| value.to_owned()))
            }
            _ if arg.starts_with('-') => usage(),
            _ => patterns.push(arg),
        }
    }

    // As in mold's ELF suite, TRIPLE in the environment selects the
    // configuration too.
    if !mode_was_set && let Some(triple) = env::var_os("TRIPLE").filter(|s| !s.is_empty()) {
        mode = Mode::Triple(triple.to_string_lossy().into_owned());
    }

    Options { jobs, mode, patterns, timeout, list }
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

/// Returns the configurations to run and the simulator triples that a
/// run skips, silently, for want of a runtime (as on a machine without
/// Xcode's simulators).
fn selected_targets(options: &Options) -> (Vec<Target>, Vec<&'static str>) {
    let native = native_arch();
    let mut targets = vec![Target::host(native)];
    let mut unavailable = Vec::new();
    match &options.mode {
        Mode::Native => {}
        Mode::Host | Mode::Default | Mode::All => {
            let rosetta = native == "arm64" && rosetta_available();
            if rosetta {
                targets.push(Target::host("x86_64"));
            }
            let simulators: &[&[&str]] = match options.mode {
                Mode::Default => &[DEFAULT_SIMULATORS],
                Mode::All => &[DEFAULT_SIMULATORS, MORE_SIMULATORS],
                _ => &[],
            };
            for triple in simulators.concat() {
                let simulator = Simulator::parse(triple).unwrap();
                match simulator::find_runtime(&simulator) {
                    Some(runtime) if simulator.arch == native || rosetta => {
                        targets.push(Target::simulator(&simulator, runtime))
                    }
                    _ => unavailable.push(triple),
                }
            }
        }
        Mode::Triple(triple) => {
            let Some(simulator) = Simulator::parse(triple) else {
                eprintln!("mold-macho-tests: {triple}: not a simulator triple");
                usage();
            };
            let Some(runtime) = simulator::find_runtime(&simulator) else {
                eprintln!("mold-macho-tests: no simulator runtime for {triple}");
                std::process::exit(1);
            };
            targets = vec![Target::simulator(&simulator, runtime)];
        }
    }
    (targets, unavailable)
}

/// Boots a device for each simulator configuration. Dropping a device
/// shuts it down.
fn boot_devices(targets: &mut [Target]) -> Result<Vec<Device>, String> {
    let mut devices = Vec::new();
    for target in targets {
        if let Some(runtime) = &target.runtime {
            let device = Device::boot(runtime)?;
            target.device = Some(device.udid.clone());
            devices.push(device);
        }
    }
    Ok(devices)
}

fn matches_patterns(name: &str, patterns: &[String]) -> bool {
    patterns.is_empty() || patterns.iter().any(|pattern| name.contains(pattern))
}

fn discover_scripts(test_dir: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let mut scripts = Vec::new();
    for entry in fs::read_dir(test_dir)? {
        let path = entry?.path();
        if path.extension() != Some(OsStr::new("sh")) {
            continue;
        }
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        scripts.push((name, path));
    }
    scripts.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(scripts)
}

fn clear_results(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if matches!(path.extension().and_then(OsStr::to_str), Some("log" | "status")) {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

/// Tests run from a directory beside the linker under test and write
/// their outputs under out/test there, so nothing generated lands in the
/// source tree.
fn prepare_work_dir(mold: &Path) -> io::Result<PathBuf> {
    let mold = mold.canonicalize()?;
    let profile_dir = mold.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} has no parent directory", mold.display()),
        )
    })?;
    let work_dir = profile_dir.join("mold-test");
    fs::create_dir_all(&work_dir)?;
    Ok(work_dir)
}

/// The scripts in `cases_dirs` that the patterns select, by name.
fn selected_scripts(
    cases_dirs: &[PathBuf],
    patterns: &[String],
) -> io::Result<Vec<(String, PathBuf)>> {
    let mut scripts = Vec::new();
    for dir in cases_dirs {
        scripts.extend(discover_scripts(dir)?);
    }
    scripts.retain(|(name, _)| matches_patterns(name, patterns));
    scripts.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(scripts)
}

fn make_jobs(
    scripts: &[(String, PathBuf)],
    work_dir: &Path,
    targets: Vec<Target>,
    clean: bool,
) -> io::Result<Vec<TestJob>> {
    let mut jobs = Vec::new();
    for target in targets {
        let target = Arc::new(target);
        let result_dir = work_dir.join("out/test/results").join(&target.label);
        if clean {
            clear_results(&result_dir)?;
        }
        for (name, script) in scripts {
            jobs.push(TestJob {
                target: Arc::clone(&target),
                script: script.clone(),
                name: name.clone(),
                log: result_dir.join(format!("{name}.log")),
                status_file: result_dir.join(format!("{name}.status")),
            });
        }
    }
    Ok(jobs)
}

/// A script that skips itself ends its startup line with "skipped".
fn log_says_skipped(path: &Path) -> bool {
    fs::read(path).is_ok_and(|bytes| {
        bytes
            .split(|&byte| byte == b'\n')
            .any(|line| line.strip_suffix(b"\r").unwrap_or(line).ends_with(b"skipped"))
    })
}

fn run_process(
    root: &Path,
    job: &TestJob,
    linker: &Path,
    timeout: Duration,
) -> Result<Outcome, String> {
    let log = File::create(&job.log)
        .map_err(|err| format!("cannot create {}: {err}", job.log.display()))?;
    let stderr =
        log.try_clone().map_err(|err| format!("cannot clone {}: {err}", job.log.display()))?;
    let mut command = Command::new(&job.script);
    command
        .current_dir(root)
        .env("mold", linker)
        .env("ARCH", &job.target.arch)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr));
    for (name, value) in [("TRIPLE", &job.target.triple), ("SIMULATOR", &job.target.device)] {
        if let Some(value) = value {
            command.env(name, value);
        } else {
            command.env_remove(name);
        }
    }

    // A timeout must also kill compiler children. The test runs in
    // its own process group, which is a background group of the
    // terminal cargo was started from, so it must not inherit that
    // terminal as stdin: a program that reads it or restores its
    // settings on exit (lldb does, even in batch mode) is stopped by
    // SIGTTIN or SIGTTOU and hangs until the timeout.
    command.process_group(0);
    let mut child =
        command.spawn().map_err(|err| format!("cannot run {}: {err}", job.script.display()))?;
    let start = Instant::now();
    loop {
        match child.try_wait().map_err(|err| format!("cannot wait for test: {err}"))? {
            Some(status) => {
                return Ok(if !status.success() {
                    Outcome::Fail
                } else if log_says_skipped(&job.log) {
                    Outcome::Skip
                } else {
                    Outcome::Pass
                });
            }
            None if start.elapsed() < timeout => thread::sleep(Duration::from_millis(20)),
            None => {
                // SAFETY: kill only signals the test's own process group.
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.wait();
                return Ok(Outcome::Timeout);
            }
        }
    }
}

fn run_job(root: &Path, job: &TestJob, linker: &Path, timeout: Duration) -> TestResult {
    let mut outcome = run_process(root, job, linker, timeout).unwrap_or_else(|err| {
        eprintln!("{}: {err}", job.name);
        Outcome::Fail
    });

    // Keep failed test directories for diagnosis, but do not retain the
    // successful tests' potentially large temporary files.
    if matches!(outcome, Outcome::Pass | Outcome::Skip) {
        let dir = root.join("out/test").join(&job.target.label).join(&job.name);
        match fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => {
                eprintln!("{}: cannot remove {}: {err}", job.name, dir.display());
                outcome = Outcome::Fail;
            }
        }
    }

    if let Err(err) = fs::write(&job.status_file, format!("{}\n", outcome.status())) {
        eprintln!("{}: cannot write {}: {err}", job.name, job.status_file.display());
    }
    TestResult {
        target: Arc::clone(&job.target),
        name: job.name.clone(),
        log: job.log.clone(),
        outcome,
    }
}

fn run_jobs(root: &Path, jobs: Vec<TestJob>, linker: &Path, options: &Options) -> Vec<TestResult> {
    if jobs.is_empty() {
        return Vec::new();
    }
    let next = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel();
    let workers = options.jobs.min(jobs.len());

    thread::scope(|scope| {
        for _ in 0..workers {
            let jobs = &jobs;
            let next = &next;
            let sender = sender.clone();
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(index) else {
                        break;
                    };
                    if sender.send(run_job(root, job, linker, options.timeout)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);

        let mut results = Vec::with_capacity(jobs.len());
        while let Ok(result) = receiver.recv() {
            if matches!(result.outcome, Outcome::Fail | Outcome::Timeout) {
                eprintln!(
                    "FAIL {}:{}{} ({})",
                    result.target.label,
                    result.name,
                    if result.outcome == Outcome::Timeout { " [timeout]" } else { "" },
                    result.log.display()
                );
            }
            results.push(result);
            if results.len().is_multiple_of(100) || results.len() == jobs.len() {
                eprintln!("completed {}/{}", results.len(), jobs.len());
            }
        }
        results
    })
}

fn print_inventory(jobs: &[TestJob], unavailable: &[&str]) {
    let mut counts = BTreeMap::new();
    for job in jobs {
        *counts.entry(job.target.label.as_str()).or_insert(0usize) += 1;
    }
    for (target, count) in counts {
        println!("{target}: tests={count}");
    }
    println!("total: tests={}", jobs.len());
    if !unavailable.is_empty() {
        println!("unavailable: {}", unavailable.join(", "));
    }
}

fn print_summary(results: &[TestResult]) -> bool {
    let mut by_target: BTreeMap<&str, Counts> = BTreeMap::new();
    for result in results {
        by_target.entry(&result.target.label).or_default().add(result.outcome);
    }

    let mut total = Counts::default();
    for (target, counts) in &by_target {
        println!("{target}: pass={} skip={} fail={}", counts.pass, counts.skip, counts.fail);
        total.merge(counts);
    }
    if by_target.len() > 1 {
        println!("total: pass={} skip={} fail={}", total.pass, total.skip, total.fail);
    } else {
        println!("pass={} skip={} fail={}", total.pass, total.skip, total.fail);
    }
    total.fail == 0
}

/// Discovers and runs the test scripts in `cases_dirs`, using the linker
/// at `mold`. Command line arguments are substring patterns selecting a
/// subset of tests.
pub fn run(cases_dirs: &[PathBuf], mold: &Path) -> ExitCode {
    let options = parse_options();
    let work_dir = prepare_work_dir(mold).unwrap_or_else(|err| {
        eprintln!("mold-macho-tests: {err}");
        std::process::exit(1);
    });
    let linker = mold.canonicalize().expect("linker not found");
    let (targets, unavailable) = selected_targets(&options);
    let scripts = selected_scripts(cases_dirs, &options.patterns).unwrap_or_else(|err| {
        eprintln!("mold-macho-tests: {err}");
        std::process::exit(1);
    });
    if options.list {
        let jobs = make_jobs(&scripts, &work_dir, targets, false).unwrap_or_else(|err| {
            eprintln!("mold-macho-tests: {err}");
            std::process::exit(1);
        });
        print_inventory(&jobs, &unavailable);
        return ExitCode::SUCCESS;
    }

    // The host's configurations run together, then each simulator's on
    // its own, its device booted for it alone and shut down once it is
    // done: a booted simulator is a whole OS, and one at a time is load
    // enough. Errors return rather than exit, so that a device is shut
    // down.
    let (hosts, simulators): (Vec<_>, Vec<_>) =
        targets.into_iter().partition(|target| target.runtime.is_none());
    let mut results = Vec::new();
    for mut phase in iter::once(hosts).chain(simulators.into_iter().map(|target| vec![target])) {
        if phase.is_empty() || scripts.is_empty() {
            continue;
        }
        let devices = match boot_devices(&mut phase) {
            Ok(devices) => devices,
            Err(err) => {
                eprintln!("mold-macho-tests: {err}");
                return ExitCode::FAILURE;
            }
        };
        let jobs = match make_jobs(&scripts, &work_dir, phase, true) {
            Ok(jobs) => jobs,
            Err(err) => {
                eprintln!("mold-macho-tests: {err}");
                return ExitCode::FAILURE;
            }
        };
        results.extend(run_jobs(&work_dir, jobs, &linker, &options));
        drop(devices);
    }
    if print_summary(&results) { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Output;

    fn run_script(body: &str) -> Output {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = env::temp_dir().join(format!(
            "mold-harness-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        let common = Path::new(env!("CARGO_MANIFEST_DIR")).join("common.inc");
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

    #[test]
    fn selects_tests_by_substring() {
        assert!(matches_patterns("dead-strip", &[]));
        assert!(matches_patterns("dead-strip", &["strip".to_owned()]));
        assert!(!matches_patterns("hello", &["strip".to_owned()]));
    }
}
