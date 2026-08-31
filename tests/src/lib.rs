//! Runs mold's shell tests in parallel for Cargo's test harness.
//!
//! The tests themselves deliberately remain shell scripts so that this port
//! exercises exactly the same inputs and toolchains as C++ mold. The runner
//! owns test discovery, target selection, scheduling, timeouts and reporting.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy)]
struct TargetSpec {
    machine: &'static str,
    triple: &'static str,
    qemu: &'static str,
}

// Keep this list in the same order as mold's test/CMakeLists.txt.
const TARGETS: &[TargetSpec] = &[
    TargetSpec {
        machine: "x86_64",
        triple: "x86_64-linux-gnu",
        qemu: "qemu-x86_64",
    },
    TargetSpec {
        machine: "i686",
        triple: "i686-linux-gnu",
        qemu: "qemu-i386",
    },
    TargetSpec {
        machine: "aarch64",
        triple: "aarch64-linux-gnu",
        qemu: "qemu-aarch64",
    },
    TargetSpec {
        machine: "aarch64_be",
        triple: "aarch64_be-linux-gnu",
        qemu: "qemu-aarch64_be",
    },
    TargetSpec {
        machine: "arm",
        triple: "arm-linux-gnueabihf",
        qemu: "qemu-arm",
    },
    TargetSpec {
        machine: "armeb",
        triple: "armeb-linux-gnueabihf",
        qemu: "qemu-armeb",
    },
    TargetSpec {
        machine: "riscv64",
        triple: "riscv64-linux-gnu",
        qemu: "qemu-riscv64",
    },
    TargetSpec {
        machine: "riscv32",
        triple: "riscv32-linux-gnu",
        qemu: "qemu-riscv32",
    },
    TargetSpec {
        machine: "ppc",
        triple: "powerpc-linux-gnu",
        qemu: "qemu-ppc",
    },
    TargetSpec {
        machine: "ppc64",
        triple: "powerpc64-linux-gnu",
        qemu: "qemu-ppc64",
    },
    TargetSpec {
        machine: "ppc64le",
        triple: "powerpc64le-linux-gnu",
        qemu: "qemu-ppc64le",
    },
    TargetSpec {
        machine: "sparc64",
        triple: "sparc64-linux-gnu",
        qemu: "qemu-sparc64",
    },
    TargetSpec {
        machine: "s390x",
        triple: "s390x-linux-gnu",
        qemu: "qemu-s390x",
    },
    TargetSpec {
        machine: "sh4",
        triple: "sh4-linux-gnu",
        qemu: "qemu-sh4",
    },
    TargetSpec {
        machine: "sh4aeb",
        triple: "sh4aeb-linux-gnu",
        qemu: "qemu-sh4eb",
    },
    TargetSpec {
        machine: "m68k",
        triple: "m68k-linux-gnu",
        qemu: "qemu-m68k",
    },
    TargetSpec {
        machine: "loongarch64",
        triple: "loongarch64-linux-gnu",
        qemu: "qemu-loongarch64",
    },
];

#[derive(Clone, Debug)]
struct Target {
    machine: String,
    triple: Option<String>,
    cpu: Option<String>,
    label: String,
}

impl Target {
    fn native(machine: String) -> Target {
        Target {
            label: machine.clone(),
            machine,
            triple: None,
            cpu: None,
        }
    }

    fn cross(machine: String, triple: String) -> Target {
        Target {
            label: machine.clone(),
            machine,
            triple: Some(triple),
            cpu: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Native,
    All,
    Triple,
}

struct Options {
    jobs: usize,
    mode: Mode,
    triple: Option<String>,
    cpu: Option<String>,
    patterns: Vec<String>,
    timeout: Duration,
    list: bool,
}

#[derive(Clone)]
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
            Outcome::Pass => "pass",
            Outcome::Skip => "skip",
            Outcome::Fail | Outcome::Timeout => "fail",
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

    fn merge(&mut self, other: &Counts) {
        self.pass += other.pass;
        self.skip += other.skip;
        self.fail += other.fail;
    }
}

fn usage() -> ! {
    eprintln!(
        "Usage: cargo test [pattern] [-- [--test-threads N] \
         [--native | --all | --triple TRIPLE] [--cpu CPU] \
         [--timeout SECONDS] [--list]]"
    );
    std::process::exit(2);
}

fn parse_usize(value: Option<String>) -> usize {
    value
        .and_then(|s| s.parse().ok())
        .filter(|&n| n != 0)
        .unwrap_or_else(|| usage())
}

fn parse_options() -> Options {
    let mut jobs = thread::available_parallelism().map_or(1, usize::from);
    let mut mode = Mode::All;
    let mut mode_was_set = false;
    let mut triple = None;
    let mut cpu = None;
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
                triple = None;
            }
            "--all" => {
                mode = Mode::All;
                mode_was_set = true;
                triple = None;
            }
            "--triple" => {
                mode = Mode::Triple;
                mode_was_set = true;
                triple = args.next();
                if triple.is_none() {
                    usage();
                }
            }
            "--cpu" => {
                cpu = args.next();
                if cpu.is_none() {
                    usage();
                }
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

    // Preserve the old runner's TRIPLE/CPU interface for callers that set
    // the cross target in the environment.
    if !mode_was_set {
        if let Some(value) = env::var_os("TRIPLE").filter(|s| !s.is_empty()) {
            mode = Mode::Triple;
            triple = Some(value.to_string_lossy().into_owned());
        }
        if cpu.is_none() {
            cpu = env::var_os("CPU")
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string_lossy().into_owned());
        }
    }

    Options {
        jobs,
        mode,
        triple,
        cpu,
        patterns,
        timeout,
        list,
    }
}

fn canonical_machine(machine: &str) -> String {
    let machine = machine.trim();
    if machine == "amd64" {
        return "x86_64".to_owned();
    }
    if machine.len() == 4 && machine.starts_with('i') && machine.ends_with("86") {
        return "i686".to_owned();
    }
    if machine.starts_with("armeb") {
        return "armeb".to_owned();
    }
    if machine.starts_with("arm") {
        return "arm".to_owned();
    }
    match machine {
        "powerpc" => "ppc".to_owned(),
        "powerpc64" => "ppc64".to_owned(),
        "powerpc64le" => "ppc64le".to_owned(),
        _ => machine.to_owned(),
    }
}

fn machine_from_triple(triple: &str) -> String {
    canonical_machine(triple.split('-').next().unwrap_or(triple))
}

fn native_machine() -> String {
    if let Some(machine) = env::var_os("MACHINE").filter(|s| !s.is_empty()) {
        return canonical_machine(&machine.to_string_lossy());
    }

    if let Ok(output) = Command::new("cc").arg("-dumpmachine").output() {
        if output.status.success() {
            let triple = String::from_utf8_lossy(&output.stdout);
            if !triple.trim().is_empty() {
                return machine_from_triple(&triple);
            }
        }
    }
    canonical_machine(env::consts::ARCH)
}

fn command_exists(command: &str) -> bool {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return path.is_file();
    }
    env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| env::split_paths(&paths).collect::<Vec<_>>())
        .any(|dir| dir.join(command).is_file())
}

fn supports_power10() -> bool {
    if !command_exists("powerpc64le-linux-gnu-gcc") || !command_exists("qemu-ppc64le") {
        return false;
    }

    let compiler = Command::new("powerpc64le-linux-gnu-gcc")
        .args(["-mcpu=power10", "-E", "-x", "c", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    let qemu = Command::new("qemu-ppc64le")
        .args(["-cpu", "help"])
        .output()
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).contains("power10_v2.0"));
    compiler && qemu
}

fn all_targets(native: &str) -> (Vec<Target>, Vec<&'static TargetSpec>) {
    let mut targets = Vec::new();
    let mut unavailable = Vec::new();

    for spec in TARGETS {
        if spec.machine == native {
            targets.push(Target::native(native.to_owned()));
        } else if command_exists(spec.qemu) && command_exists(&format!("{}-gcc", spec.triple)) {
            targets.push(Target::cross(
                spec.machine.to_owned(),
                spec.triple.to_owned(),
            ));
        } else {
            unavailable.push(spec);
        }
    }

    if native == "ppc64le"
        && fs::read_to_string("/proc/cpuinfo").is_ok_and(|s| s.contains("POWER10"))
    {
        let mut target = Target::native(native.to_owned());
        target.cpu = Some("power10".to_owned());
        target.label = "ppc64le-power10".to_owned();
        targets.push(target);
    } else if supports_power10() {
        let mut target = Target::cross("ppc64le".to_owned(), "powerpc64le-linux-gnu".to_owned());
        target.cpu = Some("power10".to_owned());
        target.label = "ppc64le-power10".to_owned();
        targets.push(target);
    }

    (targets, unavailable)
}

fn selected_targets(options: &Options) -> (Vec<Target>, Vec<&'static TargetSpec>) {
    let native = native_machine();
    match options.mode {
        Mode::Native => (vec![Target::native(native)], Vec::new()),
        Mode::All => all_targets(&native),
        Mode::Triple => {
            let triple = options.triple.clone().unwrap_or_else(|| usage());
            let machine = machine_from_triple(&triple);
            let mut target = Target::cross(machine, triple);
            target.cpu.clone_from(&options.cpu);
            if let Some(cpu) = &target.cpu {
                target.label = format!("{}-{cpu}", target.machine);
            }
            (vec![target], Vec::new())
        }
    }
}

fn matches_target(name: &str, machine: &str) -> bool {
    !name.starts_with("arch-") || name.starts_with(&format!("arch-{machine}-"))
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
        if matches!(
            path.extension().and_then(OsStr::to_str),
            Some("log" | "status")
        ) {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn replace_file_link(source: &Path, destination: &Path) -> io::Result<()> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} is a directory", destination.display()),
            ));
        }
        Ok(_) => fs::remove_file(destination)?,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, destination)
    }
    #[cfg(not(unix))]
    {
        fs::copy(source, destination).map(|_| ())
    }
}

fn prepare_work_dir(mold: &Path) -> io::Result<PathBuf> {
    let mold = mold.canonicalize()?;
    let profile_dir = mold.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} has no parent directory", mold.display()),
        )
    })?;
    let wrapper = profile_dir.join("mold-wrapper.so");
    if !wrapper.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} does not exist", wrapper.display()),
        ));
    }

    // C++ mold runs its tests from the build directory, which contains mold,
    // ld and mold-wrapper.so. Give Cargo's test binary the same layout without
    // writing generated files into the source tree.
    let work_dir = profile_dir.join("mold-test");
    fs::create_dir_all(&work_dir)?;
    replace_file_link(&mold, &work_dir.join("mold"))?;
    replace_file_link(&mold, &work_dir.join("ld"))?;
    replace_file_link(&wrapper, &work_dir.join("mold-wrapper.so"))?;
    Ok(work_dir)
}

fn make_jobs(
    cases_dir: &Path,
    work_dir: &Path,
    targets: Vec<Target>,
    patterns: &[String],
    clean: bool,
) -> io::Result<Vec<TestJob>> {
    let scripts = discover_scripts(cases_dir)?;
    let mut jobs = Vec::new();

    for target in targets {
        let target = Arc::new(target);
        let result_dir = work_dir.join("out/test/results").join(&target.label);
        if clean {
            clear_results(&result_dir)?;
        }
        for (name, script) in &scripts {
            if matches_target(name, &target.machine) && matches_patterns(name, patterns) {
                jobs.push(TestJob {
                    target: Arc::clone(&target),
                    script: script.clone(),
                    name: name.clone(),
                    log: result_dir.join(format!("{name}.log")),
                    status_file: result_dir.join(format!("{name}.status")),
                });
            }
        }
    }
    Ok(jobs)
}

fn log_says_skipped(path: &Path) -> bool {
    fs::read(path).is_ok_and(|bytes| {
        bytes
            .split(|&byte| byte == b'\n')
            .any(|line| line.strip_suffix(b"\r").unwrap_or(line) == b"skipped")
    })
}

fn run_job(root: &Path, job: &TestJob, timeout: Duration) -> TestResult {
    let mut outcome = match File::create(&job.log) {
        Err(err) => {
            eprintln!("{}: cannot create {}: {err}", job.name, job.log.display());
            Outcome::Fail
        }
        Ok(log) => {
            let stderr = match log.try_clone() {
                Ok(file) => file,
                Err(err) => {
                    eprintln!("{}: cannot clone {}: {err}", job.name, job.log.display());
                    return TestResult {
                        target: Arc::clone(&job.target),
                        name: job.name.clone(),
                        log: job.log.clone(),
                        outcome: Outcome::Fail,
                    };
                }
            };
            let mut command = Command::new("bash");
            command
                .arg(&job.script)
                .current_dir(root)
                .env("MACHINE", &job.target.machine)
                .stdout(Stdio::from(log))
                .stderr(Stdio::from(stderr));
            match &job.target.triple {
                Some(triple) => {
                    command.env("TRIPLE", triple);
                }
                None => {
                    command.env_remove("TRIPLE");
                }
            }
            match &job.target.cpu {
                Some(cpu) => {
                    command.env("CPU", cpu);
                }
                None => {
                    command.env_remove("CPU");
                }
            }

            // Give every test its own process group, so a timeout also kills
            // compiler or QEMU children rather than leaving them behind.
            #[cfg(unix)]
            command.process_group(0);

            match command.spawn() {
                Err(err) => {
                    eprintln!("{}: cannot run {}: {err}", job.name, job.script.display());
                    Outcome::Fail
                }
                Ok(mut child) => {
                    let start = Instant::now();
                    loop {
                        match child.try_wait() {
                            Ok(Some(status)) => {
                                break if !status.success() {
                                    Outcome::Fail
                                } else if log_says_skipped(&job.log) {
                                    Outcome::Skip
                                } else {
                                    Outcome::Pass
                                };
                            }
                            Ok(None) if start.elapsed() < timeout => {
                                thread::sleep(Duration::from_millis(20))
                            }
                            Ok(None) => {
                                #[cfg(unix)]
                                unsafe {
                                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                                }
                                #[cfg(not(unix))]
                                let _ = child.kill();
                                let _ = child.wait();
                                break Outcome::Timeout;
                            }
                            Err(err) => {
                                eprintln!("{}: cannot wait for test: {err}", job.name);
                                break Outcome::Fail;
                            }
                        }
                    }
                }
            }
        }
    };

    // Keep failed test directories for diagnosis, but do not retain the
    // successful tests' potentially large temporary files.
    if matches!(outcome, Outcome::Pass | Outcome::Skip) {
        let dir = root
            .join("out/test")
            .join(&job.target.label)
            .join(&job.name);
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
        eprintln!(
            "{}: cannot write {}: {err}",
            job.name,
            job.status_file.display()
        );
    }
    TestResult {
        target: Arc::clone(&job.target),
        name: job.name.clone(),
        log: job.log.clone(),
        outcome,
    }
}

fn run_jobs(root: &Path, jobs: Vec<TestJob>, options: &Options) -> Vec<TestResult> {
    if jobs.is_empty() {
        return Vec::new();
    }
    let jobs = Arc::new(jobs);
    let next = Arc::new(AtomicUsize::new(0));
    let (sender, receiver) = mpsc::channel();
    let workers = options.jobs.min(jobs.len());

    thread::scope(|scope| {
        for _ in 0..workers {
            let jobs = Arc::clone(&jobs);
            let next = Arc::clone(&next);
            let sender = sender.clone();
            scope.spawn(move || loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(job) = jobs.get(index) else {
                    break;
                };
                if sender.send(run_job(root, job, options.timeout)).is_err() {
                    break;
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
                    if result.outcome == Outcome::Timeout {
                        " [timeout]"
                    } else {
                        ""
                    },
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

fn print_inventory(jobs: &[TestJob], unavailable: &[&TargetSpec]) {
    let mut counts = BTreeMap::new();
    for job in jobs {
        *counts.entry(job.target.label.as_str()).or_insert(0usize) += 1;
    }
    for (target, count) in counts {
        println!("{target}: tests={count}");
    }
    println!("total: tests={}", jobs.len());
    if !unavailable.is_empty() {
        let targets = unavailable
            .iter()
            .map(|target| target.machine)
            .collect::<Vec<_>>()
            .join(", ");
        println!("unavailable: {targets}");
    }
}

fn print_summary(results: &[TestResult]) -> bool {
    let mut by_target: BTreeMap<&str, Counts> = BTreeMap::new();
    for result in results {
        by_target
            .entry(&result.target.label)
            .or_default()
            .add(result.outcome);
    }

    let mut total = Counts::default();
    for (target, counts) in &by_target {
        println!(
            "{target}: pass={} skip={} fail={}",
            counts.pass, counts.skip, counts.fail
        );
        total.merge(counts);
    }
    if by_target.len() > 1 {
        println!(
            "total: pass={} skip={} fail={}",
            total.pass, total.skip, total.fail
        );
    } else {
        println!(
            "pass={} skip={} fail={}",
            total.pass, total.skip, total.fail
        );
    }
    total.fail == 0
}

pub fn run(cases_dir: &Path, mold: &Path) -> ExitCode {
    let options = parse_options();
    let work_dir = prepare_work_dir(mold).unwrap_or_else(|err| {
        eprintln!("mold-tests: {err}");
        std::process::exit(1);
    });
    let (targets, unavailable) = selected_targets(&options);
    let jobs = make_jobs(
        cases_dir,
        &work_dir,
        targets,
        &options.patterns,
        !options.list,
    )
    .unwrap_or_else(|err| {
        eprintln!("mold-tests: {err}");
        std::process::exit(1);
    });

    if options.list {
        print_inventory(&jobs, &unavailable);
        return ExitCode::SUCCESS;
    }
    if options.mode == Mode::All && !unavailable.is_empty() {
        let targets = unavailable
            .iter()
            .map(|target| target.machine)
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!("skipping targets without both compiler and QEMU: {targets}");
    }

    let results = run_jobs(&work_dir, jobs, &options);
    if print_summary(&results) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_machine_names() {
        assert_eq!(canonical_machine("amd64"), "x86_64");
        assert_eq!(canonical_machine("i386"), "i686");
        assert_eq!(canonical_machine("armv7l"), "arm");
        assert_eq!(canonical_machine("aarch64"), "aarch64");
        assert_eq!(canonical_machine("powerpc64le"), "ppc64le");
    }

    #[test]
    fn selects_generic_and_target_tests() {
        assert!(matches_target("gc-sections", "aarch64"));
        assert!(matches_target("arch-aarch64-reloc", "aarch64"));
        assert!(!matches_target("arch-x86_64-reloc", "aarch64"));
    }
}
