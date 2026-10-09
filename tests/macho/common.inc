# -*- mode: sh -*-

# Make sure all commands print out messages in English
export LC_ALL=C

# The tests' Python, inline or in a script here, imports macho.py from
# this directory, without writing its bytecode there.
export PYTHONPATH="$(cd "$(dirname "$0")" && pwd)${PYTHONPATH:+:$PYTHONPATH}"
export PYTHONDONTWRITEBYTECODE=1

if [ -z "$TRIPLE" ]; then
  ARCH="${ARCH:-$(uname -m)}"
  TESTDIR=out/test/$ARCH
  PLATFORM=macos
  PLATFORM_NAME=macOS
  PLATFORM_VERSION=
  SDK=$(xcrun --show-sdk-path)
  CC="cc -arch $ARCH"
  CXX="c++ -arch $ARCH"
  SWIFTC=swiftc
  RUN=
  LLDB=lldb
else
  # A simulator triple such as arm64-apple-ios26.5-simulator: programs
  # are built for the triple's OS version with the simulator's SDK and
  # run on the device the runner booted (on a booted one when a script
  # runs by hand), as mold's ELF suite runs a cross target's under QEMU.
  ARCH=${TRIPLE%%-*}
  target_os=$(echo "$TRIPLE" | cut -d- -f3)
  OS_VERSION=${target_os##*[a-z]}
  target_os=${target_os%"$OS_VERSION"}
  PLATFORM=$target_os-simulator
  PLATFORM_VERSION="$PLATFORM $OS_VERSION $OS_VERSION"
  case $target_os in
  ios) sdk_name=iphonesimulator PLATFORM_NAME=iOS-simulator ;;
  tvos) sdk_name=appletvsimulator PLATFORM_NAME=tvOS-simulator ;;
  xros) sdk_name=xrsimulator PLATFORM_NAME=visionOS-simulator ;;
  esac
  SDK=$(xcrun --sdk $sdk_name --show-sdk-path)
  TESTDIR=out/test/$ARCH-$PLATFORM
  CC="cc -target $TRIPLE -isysroot $SDK"
  CXX="c++ -target $TRIPLE -isysroot $SDK"
  SWIFTC="swiftc -target $TRIPLE -sdk $SDK"

  # simctl starts a program in the device's own directory, where the
  # relative paths a test gives (and the install names they become)
  # lead nowhere. As QEMU runs a program in the current directory, a
  # little program of the simulator's changes to the script's directory
  # and executes the program there. (tvOS's headers declare execv
  # unavailable, but the simulator runs it.)
  simctl=$(xcrun -f simctl)
  sim_exec=$PWD/$TESTDIR/exec-$OS_VERSION
  if [ ! -x "$sim_exec" ]; then
    mkdir -p $TESTDIR
    echo 'int chdir(const char *);
int execv(const char *, char *const *);
void perror(const char *);
int main(int argc, char **argv) {
  if (chdir(argv[1]) == 0)
    execv(argv[2], argv + 2);
  perror(argv[2]);
  return 127;
}' | $CC -o "$sim_exec.$$" -xc - && mv "$sim_exec.$$" "$sim_exec"
  fi
  run_on_simulator() {
    "$simctl" spawn "${SIMULATOR:-booted}" "$sim_exec" "$PWD" "$@"
  }
  RUN=run_on_simulator

  # lldb starts a simulator's program on a device of its own choosing,
  # booting it unless it is the one the runner booted: a second whole OS
  # starting, which outlasts a test's timeout on the CI runners. So lldb
  # connects to the runner's device before it loads the program (-O), as
  # a target made first keeps a platform of its own. ($PLATFORM is lldb's
  # name for the simulator's platform too.)
  lldb_on_simulator() {
    if [ -n "$SIMULATOR" ]; then
      lldb -O "platform select $PLATFORM" -O "platform connect $SIMULATOR" "$@"
    else
      lldb "$@"
    fi
  }
  LLDB=lldb_on_simulator
fi

# ${CC/$ARCH/<arch>} compiles for another architecture of the platform.
# $PLATFORM names the platform as -platform_version and .tbd targets do,
# $PLATFORM_NAME as ld-prime's messages do.
# A simulator's $PLATFORM_VERSION holds all of -platform_version's
# arguments, which take the place of the macOS version a script names:
# -platform_version ${PLATFORM_VERSION:-macos 14.0 14.0}.

# The runner sets $mold to the linker under test; default to the debug
# build for running a script by hand.
if [ -z "$mold" ]; then
  mold="$(cd "$(dirname "$0")"/../.. && pwd)/target/debug/mold"
fi

# Common functions
# Unlike shell negation, a failed assertion here reaches the ERR trap
# even when more commands follow it. Use this for expected failures.
not() {
  if "$@"; then
    echo "command unexpectedly succeeded: $*" >&2
    return 1
  fi
  return 0
}

# lldb launching a process under Rosetta hangs on the hosted CI runner
# (debugserver never reports the launch), so scripts run lldb only on
# binaries of the host's own architecture.
native_arch() {
  [ "$ARCH" = "$(uname -m)" ]
}

# A simulator runs a program as a host process, but with the simulator's
# dyld and libraries, and with only the variables simctl's environment
# names SIMCTL_CHILD_<name>.
on_simulator() {
  [ -n "$TRIPLE" ]
}

# Whether a simulator configuration's OS is older than iOS (or tvOS) $1,
# as the x86_64 simulator's iOS 17 runtime is older than the releases
# some features came with (iOS 18 with macOS 15, 27 with macOS 27).
simulator_older_than() {
  on_simulator &&
    [ "$(printf '%s\n' "$OS_VERSION" "$1" | sort -t. -n -k1,1 -k2,2 | head -1)" != "$1" ]
}

test_cflags() {
  echo 'int main() {}' | $CC "$@" -o /dev/null -xc - >& /dev/null
}

# Compares two images whose contents may come in another order: they
# must have the same sections, each of the same size, and define the
# same symbols.
same_sections_and_symbols() {
  local f
  for f in "$1" "$2"; do
    otool -l "$f" | awk '$1 == "sectname" { sect = $2 } $1 == "segname" { seg = $2 }
      $1 == "size" { print seg, sect, $2 }' | sort > "$f.layout"
    nm -jU "$f" | sort >> "$f.layout"
  done
  diff "$1.layout" "$2.layout"
}

# unwind_lookup IMAGE SYMBOL|ADDRESS...: prints the __unwind_info
# encoding the unwinder finds for each (see unwind-lookup.py).
unwind_lookup() {
  python3 "$(dirname "$0")"/unwind-lookup.py "$@"
}

# Looks in a dump of dyld_info -shared_region ($3) for an entry that
# matches the extended regex $1 and is of the kind $2. Xcode 26's
# dyld_info prints no kinds, only where each entry refers from and to,
# so there the kind goes unchecked.
split_has() {
  awk -v re="$1" -v k="$2" '$0 ~ re && (NF == 6 || $NF == k) { n++ } END { exit !n }' "$3"
}

# Marks the object $1 as built for firmware 1.0, as Xcode 27's clang
# makes one for -target <arch>-apple-firmware: platform 13, no SDK. An
# older clang knows no firmware and makes an ELF object for it, so
# tests build a macOS object and mark it.
mark_firmware() {
  python3 -m macho set-build-version "$1" 13 1.0 0
}

# set_section_flags FILE SEG SECT FLAGS [SEG SECT FLAGS...]: rewrites the
# flags of the sections named SEG,SECT in FILE.
set_section_flags() {
  python3 -m macho set-section-flags "$@"
}

# set_section_align FILE SEG SECT P2ALIGN: rewrites the alignment of the
# sections named SEG,SECT in FILE.
set_section_align() {
  python3 -m macho set-section-align "$@"
}

# dtrace_header NAME: makes $t/NAME.h from the D script $t/NAME.d, with
# dtrace -h; a host without it skips the test.
dtrace_header() {
  command -v dtrace > /dev/null || skip
  dtrace -h -s $t/$1.d -o $t/$1.h >& /dev/null || skip
}

# dof_dump FILE: describes each DOF section of an image (see dof-dump.py).
dof_dump() {
  python3 "$(dirname "$0")"/dof-dump.py "$1"
}

skip() {
  echo skipped
  trap - EXIT
  exit 0
}

on_error() {
  code=$?
  echo "command failed: $1: $BASH_COMMAND"
  trap - EXIT
  exit $code
}

on_exit() {
  code=$?
  trap - EXIT
  if [ "$code" -eq 0 ]; then
    echo OK
  fi
  exit "$code"
}

trap 'on_error $LINENO' ERR
trap on_exit EXIT

# Print out the startup message
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=$TESTDIR/$testname
mkdir -p $t
set -o pipefail
set -x
