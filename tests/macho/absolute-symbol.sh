#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -xassembler - -o $t/absolute.o
.globl _private_answer, _answer
.private_extern _private_answer
_private_answer = 42
_answer = 43
EOF
cat <<EOF | $CC -c -xc - -o $t/main.o
#include <stdio.h>
#include <stdint.h>
#include <dlfcn.h>
extern char private_answer, answer, _mh_execute_header;
uintptr_t private_value = (uintptr_t)&private_answer;
uintptr_t public_value = (uintptr_t)&answer;
char *header = &_mh_execute_header;
int main() {
  printf("%lu %lu %lu %lu %d\n", private_value, public_value,
         (uintptr_t)&answer, (uintptr_t)dlsym(RTLD_DEFAULT, "answer"),
         header == &_mh_execute_header);
}
EOF
for fixups in -fixup_chains -no_fixup_chains; do
  $CC --ld-path=$mold $t/main.o $t/absolute.o -Wl,$fixups -o $t/exe
  $t/exe | grep '^42 43 43 43 1$'
done
