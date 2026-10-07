# -*- mode: sh -*-

# Make sure all commands print out messages in English
export LC_ALL=C

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
  mold="$(cd "$(dirname "$0")"/.. && pwd)/target/debug/mold"
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

# Prints, for each symbol or address given after an image, the
# __unwind_info encoding the unwinder finds for that address - as
# libunwind looks it up, the last entry at or below it, through the
# first-level index and a regular or compressed page - or "none" past
# the table's end.
unwind_lookup() {
  python3 - "$@" <<'EOF'
import bisect, struct, subprocess, sys
path, keys = sys.argv[1], sys.argv[2:]
lc = subprocess.run(['otool', '-l', path], capture_output=True, text=True).stdout.splitlines()
for i, l in enumerate(lc):
    if l.split() == ['segname', '__TEXT'] and lc[i - 2].split()[-1] == 'LC_SEGMENT_64':
        base = int(lc[i + 1].split()[1], 16)
    if l.split() == ['sectname', '__unwind_info']:
        size, off = int(lc[i + 3].split()[1], 16), int(lc[i + 4].split()[1])
d = open(path, 'rb').read()[off:off + size]
nm = subprocess.run(['nm', path], capture_output=True, text=True).stdout.split('\n')
syms = {f[2]: int(f[0], 16) for f in (l.split() for l in nm) if len(f) == 3}
_, ceo, cec, _, _, iso, isc = struct.unpack_from('<7I', d, 0)
common = struct.unpack_from(f'<{cec}I', d, ceo)
index = [struct.unpack_from('<3I', d, iso + 12 * k) for k in range(isc)]
for key in keys:
    fo = (int(key, 16) if key.startswith('0x') else syms[key]) - base
    k = bisect.bisect_right([e[0] for e in index], fo) - 1
    if k < 0 or k >= isc - 1:
        print('none')
        continue
    first, page, _ = index[k]
    kind = struct.unpack_from('<I', d, page)[0]
    if kind == 3:
        _, eo, ec, eco, ecc = struct.unpack_from('<IHHHH', d, page)
        local = struct.unpack_from(f'<{ecc}I', d, page + eco)
        ents = [(first + (e & 0xffffff), e >> 24) for e in struct.unpack_from(f'<{ec}I', d, page + eo)]
        ents = [(a, common[i] if i < cec else local[i - cec]) for a, i in ents]
    else:
        _, eo, ec = struct.unpack_from('<IHH', d, page)
        ents = [struct.unpack_from('<II', d, page + eo + 8 * j) for j in range(ec)]
    j = bisect.bisect_right([a for a, _ in ents], fo) - 1
    print(hex(ents[j][1]) if j >= 0 else 'none')
EOF
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
  python3 - "$1" <<'EOF'
import struct, sys
d = bytearray(open(sys.argv[1], 'rb').read())
off, found = 32, False
for _ in range(struct.unpack_from('<I', d, 16)[0]):
    cmd, size = struct.unpack_from('<II', d, off)
    if cmd == 0x32:  # LC_BUILD_VERSION
        struct.pack_into('<III', d, off + 8, 13, 0x10000, 0)
        found = True
    off += size
open(sys.argv[1], 'wb').write(d)
sys.exit(not found)
EOF
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
