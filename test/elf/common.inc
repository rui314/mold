# -*- mode: sh -*-

# Make sure all commands print out messages in English
export LC_ALL=C

# Set test variables
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
MACHINE="${MACHINE:-$(uname -m)}"

# Canonicalize a target name
case $MACHINE in
arm*) MACHINE=arm ;;
i?86) MACHINE=i386 ;;
esac

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
  echo OK
  exit 0
}

trap 'on_error $LINENO' ERR
trap on_exit EXIT

testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/elf/$MACHINE/$testname
mkdir -p $t
