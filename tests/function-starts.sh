#!/bin/bash
source "$(dirname "$0")"/common.inc

# LC_FUNCTION_STARTS lists the start of every atom of every section of
# pure instructions, not only of __text: a subsection with no symbol
# at its start, each label inside one, an alt entry too, but no label
# at a section's end and nothing in a data section, stubs included.
starts() {
  dyld_info -function_starts $1 | awk '$1 ~ /^0x/ { print tolower($1) }'
}
addr() { nm $1 | awk -v s=$2 '$3 == s { print "0x" $1 }' | sed 's/0x0*/0x/'; }
sect_addr() {
  otool -l $1 | awk -v s=$2 '$1 == "sectname" && $2 == s { f = 1 }
    f && $1 == "addr" { print $2; exit }' | sed 's/0x0*/0x/'
}

# C++ static initializers come in __StaticInit, merged into __text.
cat <<EOF | $CXX -c -o $t/a.o -xc++ -
#include <cstdio>
struct S { S() { printf("hi\n"); } };
S s;
int main() { return 0; }
EOF
$CXX --ld-path=$mold -o $t/exe $t/a.o
$t/exe | grep -q hi
starts $t/exe > $t/starts
grep -qx $(addr $t/exe ___cxx_global_var_init) $t/starts
grep -qx $(addr $t/exe _main) $t/starts
not grep -qx $(sect_addr $t/exe __stubs) $t/starts

# An object without subsections: a section is one atom, its labels
# inside it.
cat <<EOF | $CC -c -o $t/b.o -xassembler -
.text
  nop
.globl _main
_main:
  ret
_local:
  nop
  ret
.globl _alt
.alt_entry _alt
_alt:
  ret

.section __TEXT,__foo,regular,pure_instructions
_in_foo:
  ret
_foo_end:

.section __TEXT,__const
_in_const:
  ret
EOF
$CC --ld-path=$mold -o $t/exe2 $t/b.o
starts $t/exe2 > $t/starts2
grep -qx $(sect_addr $t/exe2 __text) $t/starts2
grep -qx $(addr $t/exe2 _main) $t/starts2
grep -qx $(addr $t/exe2 _local) $t/starts2
grep -qx $(addr $t/exe2 _alt) $t/starts2
grep -qx $(addr $t/exe2 _in_foo) $t/starts2
not grep -qx $(addr $t/exe2 _foo_end) $t/starts2
not grep -qx $(addr $t/exe2 _in_const) $t/starts2
[ $(wc -l < $t/starts2) = 5 ]

# The empty __text of an object of only data, which the arm64
# assembler labels (ltmp0), is no function either.
if [ $ARCH = arm64 ]; then
  printf '.data\n.quad 1\n' | $CC -o $t/c.o -c -xassembler -
  echo 'int main() { return 0; }' | $CC -o $t/d.o -c -xc -
  $CC --ld-path=$mold -o $t/exe3 $t/d.o $t/c.o
  starts $t/exe3 > $t/starts3
  [ "$(cat $t/starts3)" = $(addr $t/exe3 _main) ]
fi
