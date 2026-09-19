#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
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

$CC --ld-path=$mold -o $t/exe $t/a.o
$t/exe | grep -q sname=main
