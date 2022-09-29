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

cat <<EOF | $CC -o $t/a.o -c -xc -fno-PIE -
#include <setjmp.h>
#include <signal.h>
#include <stdio.h>

extern const char readonly[100];
extern char readwrite[100];

static int segv = 0;
static jmp_buf buf;

void handler(int sig) {
  segv = 1;
  longjmp(buf, 1);
}

int main() {
  signal(SIGSEGV, handler);

  readwrite[0] = 5;
  int x = segv;

  if (setjmp(buf) == 0)
    *(char *)readonly = 5;
  int y = segv;

  printf("sigsegv %d %d\n", x, y);
}
EOF

cat <<EOF | $CC -fPIC -shared -o $t/b.so -xc -
const char readonly[100] = "abc";
char readwrite[100] = "abc";
EOF

$CC -B. $t/a.o $t/b.so -o $t/exe -no-pie
$QEMU $t/exe | grep -q '^sigsegv 0 1$'

echo OK
