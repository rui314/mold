#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$MACHINE/$testname
mkdir -p $t

cat <<EOF | clang -c -o $t/a.o -xc -
#include <stdio.h>
#include <dlfcn.h>

int main(int argc, char **argv) {
  Dl_info info;

  if (!dladdr((char *)main + 4, &info)) {
    printf("dladdr failed\n");
    return 1;
  }

  printf("fname=%s fbase=%p sname=%s saddr=%p\n",
        info.dli_fname, info.dli_fbase, info.dli_sname, info.dli_saddr);
  return 0;
}
EOF

clang --ld-path=./ld64 -o $t/exe $t/a.o
$t/exe | grep -q sname=main

echo OK
