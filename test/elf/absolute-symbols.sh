#!/bin/bash
export LC_ALL=C
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

[ "$(uname -m)" = x86_64 ] || { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -x assembler -
.globl foo
foo = 0x800000
EOF

cat <<EOF | $CC -o $t/b.o -c -fno-PIC -xc -
#define _GNU_SOURCE 1
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <ucontext.h>

void handler(int signum, siginfo_t *info, void *ptr) {
  ucontext_t *u = (ucontext_t *)ptr;
  printf("ip=0x%llx\n", u->uc_mcontext.gregs[REG_RIP]);
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
$t/exe | grep -q '^ip=0x800000$'

echo OK
