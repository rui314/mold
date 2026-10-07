//! Simulator devices for the configurations whose programs run on a
//! simulator.
//!
//! mold's ELF suite runs a cross target's programs under QEMU, which
//! needs no setup. A simulator's programs run as host processes, but only
//! on a booted device of the simulator's runtime, which `xcrun simctl
//! spawn` starts them on. The runner keeps a device of its own per
//! runtime, named mold-test- and the runtime, so that the user's devices
//! stay untouched. A run boots it once for all of its tests and shuts it
//! down at the end unless another run (another worktree's) is still using
//! it.

use serde_json::Value;
use std::ffi::CStr;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// The simulator platforms, by their names in target triples and in
/// simctl's runtime list.
const PLATFORMS: &[(&str, &str)] = &[("ios", "iOS"), ("tvos", "tvOS"), ("xros", "xrOS")];

/// A simulator configuration as a target triple names it:
/// `<arch>-apple-<os>[<version>]-simulator`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Simulator {
    pub arch: String,
    pub os: String,
    pub version: Option<String>,
}

impl Simulator {
    pub fn parse(triple: &str) -> Option<Self> {
        let [arch, "apple", os, "simulator"] = *triple.split('-').collect::<Vec<_>>() else {
            return None;
        };
        if !matches!(arch, "arm64" | "x86_64") {
            return None;
        }
        let digits = os.find(|c: char| c.is_ascii_digit()).unwrap_or(os.len());
        let (os, version) = os.split_at(digits);
        PLATFORMS.iter().any(|&(name, _)| name == os).then(|| Self {
            arch: arch.to_owned(),
            os: os.to_owned(),
            version: (!version.is_empty()).then(|| version.to_owned()),
        })
    }

    /// The name of the configuration's results; common.inc names its
    /// test directories alike.
    pub fn label(&self) -> String {
        format!("{}-{}-simulator", self.arch, self.os)
    }

    fn platform(&self) -> &'static str {
        PLATFORMS.iter().find(|&&(name, _)| name == self.os).unwrap().1
    }
}

/// An installed simulator runtime.
pub struct Runtime {
    identifier: String,
    /// The OS version, which the configuration also builds for.
    pub version: String,
    device_type: String,
}

fn simctl(args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("xcrun")
        .arg("simctl")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("cannot run xcrun simctl: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "xcrun simctl {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

fn simctl_list(what: &str) -> Result<Value, String> {
    let stdout = simctl(&["list", "--json", what])?;
    serde_json::from_slice(&stdout)
        .map_err(|err| format!("cannot parse xcrun simctl list {what}: {err}"))
}

fn version_key(version: &str) -> Vec<u32> {
    version.split('.').map(|part| part.parse().unwrap_or(0)).collect()
}

/// Finds the runtime of the triple's version or, without one, the newest
/// runtime that runs programs of its architecture.
pub fn find_runtime(simulator: &Simulator) -> Option<Runtime> {
    let list = simctl_list("runtimes").ok()?;
    let supports_arch = |runtime: &Value| {
        runtime["supportedArchitectures"]
            .as_array()
            .is_some_and(|archs| archs.iter().any(|arch| *arch == *simulator.arch))
    };
    list["runtimes"]
        .as_array()?
        .iter()
        .filter(|runtime| {
            runtime["platform"] == simulator.platform()
                && runtime["isAvailable"] == true
                && supports_arch(runtime)
                && simulator.version.as_ref().is_none_or(|version| runtime["version"] == **version)
        })
        .filter_map(|runtime| {
            Some(Runtime {
                identifier: runtime["identifier"].as_str()?.to_owned(),
                version: runtime["version"].as_str()?.to_owned(),
                device_type: runtime["supportedDeviceTypes"][0]["identifier"].as_str()?.to_owned(),
            })
        })
        .max_by_key(|runtime| version_key(&runtime.version))
}

/// The per-user temporary directory, which is as private to the user as
/// the devices are, whatever TMPDIR says.
fn user_temp_dir() -> io::Result<PathBuf> {
    let mut buf = [0u8; 1024];
    // SAFETY: confstr writes at most `buf.len()` bytes, NUL-terminated.
    let len = unsafe {
        libc::confstr(libc::_CS_DARWIN_USER_TEMP_DIR, buf.as_mut_ptr().cast(), buf.len())
    };
    if len == 0 || len > buf.len() {
        return Err(io::Error::other("cannot find the user's temporary directory"));
    }
    let dir = CStr::from_bytes_until_nul(&buf).unwrap().to_str().unwrap();
    Ok(PathBuf::from(dir))
}

fn open_lock(name: &str, kind: &str) -> io::Result<File> {
    let path = user_temp_dir()?.join(format!("{name}.{kind}.lock"));
    File::options().create(true).truncate(false).write(true).open(path)
}

fn flock(file: &File, operation: libc::c_int) -> io::Result<()> {
    // SAFETY: flock only operates on the descriptor, which `file` owns.
    if unsafe { libc::flock(file.as_raw_fd(), operation) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// A booted device. Runs that share a device coordinate through two lock
/// files: each run holds the users lock shared while it uses the device,
/// and the setup lock exclusively while it boots the device or, at its
/// end, gives up its users lock and shuts the device down if it can then
/// take the users lock exclusively, i.e. if no other run is using it.
pub struct Device {
    pub udid: String,
    name: String,
    users: File,
}

impl Device {
    pub fn boot(runtime: &Runtime) -> Result<Self, String> {
        let suffix = runtime.identifier.rsplit('.').next().unwrap();
        let name = format!("mold-test-{suffix}");
        let lock_error = |err: io::Error| format!("cannot lock {name}: {err}");
        let setup = open_lock(&name, "setup").map_err(lock_error)?;
        flock(&setup, libc::LOCK_EX).map_err(lock_error)?;
        let users = open_lock(&name, "users").map_err(lock_error)?;
        flock(&users, libc::LOCK_SH).map_err(lock_error)?;

        let devices = simctl_list("devices")?;
        let found = devices["devices"][&runtime.identifier].as_array().and_then(|devices| {
            devices.iter().find(|device| device["name"] == *name && device["isAvailable"] == true)
        });
        let (udid, state) = match found {
            Some(device) => (
                device["udid"].as_str().unwrap_or_default().to_owned(),
                device["state"].as_str().unwrap_or_default().to_owned(),
            ),
            None => {
                let udid = simctl(&["create", &name, &runtime.device_type, &runtime.identifier])?;
                (String::from_utf8_lossy(&udid).trim().to_owned(), "Shutdown".to_owned())
            }
        };
        if state == "Shutdown" {
            simctl(&["boot", &udid])?;
        }
        Ok(Self { udid, name, users })
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        let Ok(setup) = open_lock(&self.name, "setup") else {
            return;
        };
        if flock(&setup, libc::LOCK_EX).is_err() || flock(&self.users, libc::LOCK_UN).is_err() {
            return;
        }
        let last = open_lock(&self.name, "users")
            .is_ok_and(|users| flock(&users, libc::LOCK_EX | libc::LOCK_NB).is_ok());
        if last && let Err(err) = simctl(&["shutdown", &self.udid]) {
            eprintln!("mold-macho-tests: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simulator_triples() {
        let simulator = Simulator::parse("arm64-apple-ios26.5-simulator").unwrap();
        assert_eq!(simulator.arch, "arm64");
        assert_eq!(simulator.os, "ios");
        assert_eq!(simulator.version.as_deref(), Some("26.5"));
        assert_eq!(simulator.label(), "arm64-ios-simulator");
        assert_eq!(Simulator::parse("x86_64-apple-tvos-simulator").unwrap().version, None);
        assert!(Simulator::parse("arm64-apple-xros-simulator").is_some());
        assert!(Simulator::parse("arm64-apple-ios17.0").is_none());
        assert!(Simulator::parse("arm64-apple-macos-simulator").is_none());
        assert!(Simulator::parse("riscv64-apple-ios-simulator").is_none());
    }
}
