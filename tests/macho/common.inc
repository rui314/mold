# -*- mode: sh -*-

# Make sure all commands print out messages in English
export LC_ALL=C

ARCH="${ARCH:-$(uname -m)}"
CC="cc -arch $ARCH"
CXX="c++ -arch $ARCH"

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

test_cflags() {
  echo 'int main() {}' | $CC "$@" -o /dev/null -xc - >& /dev/null
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
