#!/bin/bash
source "$(dirname "$0")"/common.inc

# The assembler marks any section it emitted an instruction into with
# S_ATTR_SOME_INSTRUCTIONS. A final image keeps that mark only on code,
# a section of pure instructions, which always carries both; ld-prime
# drops it from any other section, in __TEXT or __DATA. A -r output
# keeps a section's attributes as they came.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __TEXT,__foo
  nop
.section __TEXT,__const
  nop
.section __DATA,__bar
  nop
.section __TEXT,__baz,regular,pure_instructions
.long 0
EOF
cat <<EOF | $CC -o $t/b.o -c -xc -
int main() { return 0; }
EOF

flags() {
  otool -l $1 | awk -v g=$2 -v s=$3 '$1 == "sectname" { n = $2 }
    $1 == "segname" { m = $2 } n == s && m == g && $1 == "flags" { print $2; exit }'
}

[ "$(flags $t/a.o __TEXT __foo)" = 0x00000400 ]
[ "$(flags $t/a.o __DATA __bar)" = 0x00000400 ]

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o
[ "$(flags $t/exe __TEXT __text)" = 0x80000400 ]
[ "$(flags $t/exe __TEXT __foo)" = 0x00000000 ]
[ "$(flags $t/exe __TEXT __const)" = 0x00000000 ]
[ "$(flags $t/exe __DATA __bar)" = 0x00000000 ]
[ "$(flags $t/exe __TEXT __baz)" = 0x80000400 ]
$t/exe

$mold -arch $ARCH -r -o $t/r.o $t/a.o
[ "$(flags $t/r.o __TEXT __foo)" = 0x00000400 ]
[ "$(flags $t/r.o __DATA __bar)" = 0x00000400 ]
