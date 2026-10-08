#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime reads __TEXT,__constructor - where GCC put the constructors
# of code built without dyld, with the assembler's .constructor
# directive - as a list of initializer pointers whatever its type: a
# final image runs them from __init_offsets, in input order among the
# __mod_init_func ones, and a -r output types the section
# S_MOD_INIT_FUNC_POINTERS. __TEXT,__destructor stays data.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
__attribute__((constructor)) static void ctor() { printf("ctor "); }
void init1() { printf("init1 "); }
void init2() { printf("init2 "); }
int main() { printf("main\n"); }
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.constructor
.p2align 3
.quad _init1
.quad _init2
EOF

sect() {
  otool -l $1 | awk -v s=$2 '$1 == "sectname" && $2 == s { f = 1; next }
    f && $1 == "segname" { g = $2 } f && $1 == "flags" { print g, $2; f = 0 }'
}

otool -lv $t/b.o | grep -A10 'sectname __constructor' | grep -q 'type S_REGULAR'

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o
otool -l $t/exe > $t/lc
grep -q __init_offsets $t/lc
not grep -q __constructor $t/lc
$RUN $t/exe | grep -q '^ctor init1 init2 main$'

$CC --ld-path=$mold -o $t/exe2 $t/b.o $t/a.o
$RUN $t/exe2 | grep -q '^init1 init2 ctor main$'

$mold -arch $ARCH -r -o $t/r.o $t/b.o
[ "$(sect $t/r.o __constructor)" = '__TEXT 0x00000009' ]

# Without __init_offsets the pointers stay in __TEXT, where dyld would
# have to rebase them.
not $CC --ld-path=$mold -o $t/exe3 $t/a.o $t/b.o -Wl,-no_fixup_chains 2> $t/log
grep -q 'Found illegal text-relocations' $t/log

cat <<EOF | $CC -o $t/c.o -c -xassembler -
.text
.globl __start
.p2align 2
__start:
  ret
.constructor
.p2align 3
.quad __start
.destructor
.p2align 3
.quad __start
EOF

$mold -arch $ARCH -static -e __start $t/c.o -o $t/exe4
[ "$(sect $t/exe4 __constructor)" = '__TEXT 0x00000009' ]
[ "$(sect $t/exe4 __destructor)" = '__TEXT 0x00000000' ]
