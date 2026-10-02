# -*- mode: sh -*-

# Make sure all commands print out messages in English
export LC_ALL=C

ARCH="${ARCH:-$(uname -m)}"
CC="cc -arch $ARCH"
CXX="c++ -arch $ARCH"

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

test_cflags() {
  echo 'int main() {}' | $CC "$@" -o /dev/null -xc - >& /dev/null
}

# Compares two images as cmp does, but for the order of their symbols:
# the symbol tables must hold the same entries, each with its name. The
# UUID and the code signature, which cover the tables, are left out.
cmp_but_symbol_order() {
  python3 - "$1" "$2" <<'EOF'
import struct, sys
def read(path):
    d = bytearray(open(path, 'rb').read())
    off, cut = 32, []
    for _ in range(struct.unpack_from('<I', d, 16)[0]):
        cmd, size = struct.unpack_from('<II', d, off)
        if cmd == 0x2:
            symoff, nsyms, stroff, strsize = struct.unpack_from('<IIII', d, off + 8)
            cut += [(symoff, nsyms * 16), (stroff, strsize)]
        if cmd == 0x1b:
            cut.append((off + 8, 16))
        if cmd == 0x1d:
            cut.append(struct.unpack_from('<II', d, off + 8))
        off += size
    syms = []
    for i in range(nsyms):
        strx, rest = struct.unpack_from('<I12s', d, symoff + 16 * i)
        syms.append((bytes(d[stroff + strx:d.index(b'\0', stroff + strx)]), rest))
    for start, size in cut:
        d[start:start + size] = bytes(size)
    return d, sorted(syms)
a, b = read(sys.argv[1]), read(sys.argv[2])
sys.exit(0 if a == b else 1)
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
t=out/test/$ARCH/$testname
mkdir -p $t
set -o pipefail
set -x
