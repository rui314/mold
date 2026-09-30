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

# The private-external absolute symbol stays in the symbol table as a
# local, as ld64 keeps it.
nm -m $t/exe | grep '(absolute) non-external (was a private external) _private_answer'

# So does a local one, but for an assembler-private (l or L) label. The
# absolute symbols follow the symbols of the sections, by value. -x
# drops the locals.
cat <<EOF2 | $CC -c -xassembler - -o $t/local.o
_local_answer = 44
lprivate = 45
.globl _hidden_answer
.private_extern _hidden_answer
_hidden_answer = 41
.data
_data_local: .quad 0
.subsections_via_symbols
EOF2
$CC --ld-path=$mold $t/main.o $t/absolute.o $t/local.o -o $t/exe2
$t/exe2 | grep '^42 43 43 43 1$'
nm -m $t/exe2 | grep -q '(absolute) non-external _local_answer$'
[ "$(nm -p $t/exe2 | awk '$2 ~ /^[a-z]$/ {printf "%s ", $3}')" = \
  '_data_local _hidden_answer _private_answer _local_answer ' ]

$CC --ld-path=$mold $t/main.o $t/absolute.o $t/local.o -o $t/exe3 -Wl,-x
nm $t/exe3 > $t/syms3
not grep -q _local_answer $t/syms3

# A -r output keeps them all, after each object's section symbols in
# the object's order, the demoted private externals last, and with no
# n_desc flags (the assembler marks an absolute symbol no-dead-strip).
$mold -r -arch $ARCH -o $t/r.o $t/local.o
[ "$(nm -p $t/r.o | awk '{printf "%s ", $3}')" = \
  '_data_local _local_answer lprivate _hidden_answer ' ]
nm -m $t/r.o > $t/syms4
grep -q '(absolute) non-external (was a private external) _hidden_answer$' $t/syms4
not grep -q 'no dead strip' $t/syms4
