# -*- mode: sh -*-

# Make sure all commands print out messages in English
export LC_ALL=C

canonical_name() {
  case $1 in
  i?86) echo i686 ;;
  arm*) echo arm ;;
  powerpc) echo ppc ;;
  powerpc64) echo ppc64 ;;
  powerpc64le) echo ppc64le ;;
  *) echo $1
  esac
}

if [ -z "$MACHINE" ]; then
  MACHINE=$(canonical_name $(uname -m))
fi

# Set tool names
if [ -z "$TRIPLE" ]; then
  TESTDIR=out/test/elf/$MACHINE
  CC="${TEST_CC:-cc}"
  CXX="${TEST_CXX:-c++}"
  GCC="${TEST_GCC:-gcc}"
  GXX="${TEST_GXX:-g++}"
  OBJDUMP=objdump
  OBJCOPY=objcopy
  STRIP=strip
  QEMU=
elif [ "$TRIPLE" = powerpc64le-linux-gnu -a "$CPU" = power10 ]; then
  MACHINE=ppc64le
  TESTDIR=out/test/elf/ppc64le-power10
  CC="${TEST_CC:-$TRIPLE-gcc} -mcpu=power10"
  CXX="${TEST_CXX:-$TRIPLE-g++} -mcpu=power10"
  GCC="${TEST_GCC:-$TRIPLE-gcc} -mcpu=power10"
  GXX="${TEST_GXX:-$TRIPLE-g++} -mcpu=power10"
  OBJDUMP="$TRIPLE-objdump"
  OBJCOPY="$TRIPLE-objcopy"
  STRIP="$TRIPLE-strip"
  QEMU="qemu-ppc64le -L /usr/$TRIPLE -cpu power10"
else
  MACHINE=$(canonical_name $(echo $TRIPLE | sed 's/-.*//'))
  TESTDIR=out/test/elf/$MACHINE
  CC="${TEST_CC:-$TRIPLE-gcc}"
  CXX="${TEST_CXX:-$TRIPLE-g++}"
  GCC="${TEST_GCC:-$TRIPLE-gcc}"
  GXX="${TEST_GXX:-$TRIPLE-g++}"
  OBJDUMP="$TRIPLE-objdump"
  OBJCOPY="$TRIPLE-objcopy"
  STRIP="$TRIPLE-strip"

  if [ $MACHINE = i686 ]; then
    QEMU="qemu-i386 -L /usr/$TRIPLE"
  else
    QEMU="qemu-$MACHINE -L /usr/$TRIPLE"
  fi
fi

if [ $MACHINE = x86_64 -o $MACHINE = i686 -o $MACHINE = arm ]; then
  tlsdesc_opt=-mtls-dialect=gnu2
elif [ $MACHINE = aarch64 ]; then
  tlsdesc_opt=-mtls-dialect=desc
fi

# Common functions
test_cflags() {
  echo 'int main() {}' | $CC "$@" -o /dev/null -xc - >& /dev/null
}

test_cxxflags() {
  echo 'int main() {}' | $CXX "$@" -o /dev/null -xc++ - >& /dev/null
}

is_musl() {
  ldd --version 2>&1 | grep -q musl
}

supports_ifunc() {
  ! is_musl && \
    echo 'void x() __attribute__((ifunc("y"))); void *y() { return 0; }' | \
    $CC -c -o /dev/null -xc - >& /dev/null
}

supports_tlsdesc() {
  # musl's tlsdesc on arm32 seems to be broken
  [ $MACHINE = arm ] && is_musl && return 1

  [ -n "$tlsdesc_opt" ]
}

on_qemu() {
  [ "$QEMU" != '' ] || grep -qw qemu /proc/cpuinfo 2> /dev/null
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

# Print out the startup message
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=$TESTDIR/$testname
mkdir -p $t
set -x
