#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

# This test crashes only on qemu-sparc64 running on GitHub Actions,
# even though it works on a local x86-64 machine and on an actual
# SPARC machine.
[ $MACHINE = sparc64 ] && { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -x assembler -
.globl foo
foo = 0x800008
EOF

cat <<EOF | $CC -o $t/b.o -c -fno-PIC -xc -
#define _GNU_SOURCE 1
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <ucontext.h>

void handler(int signum, siginfo_t *info, void *ptr) {
  printf("ip=%p\n", info->si_addr);
  exit(0);
}

void foo();

int main() {
  struct sigaction act;
  act.sa_flags = SA_SIGINFO | SA_RESETHAND;
  act.sa_sigaction = handler;
  sigemptyset(&act.sa_mask);
  sigaction(SIGSEGV, &act, 0);
  foo();
}
EOF

$CC -B. -o $t/exe -no-pie $t/a.o $t/b.o
$QEMU $t/exe | grep -q '^ip=0x80000.$'

echo OK
