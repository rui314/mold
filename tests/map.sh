#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fcommon
#include <stdio.h>
int common_sym;
void hello() {
  printf("Hello world\n");
}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -fcommon
void hello();
int common_sym;
int main() {
  hello();
  return common_sym;
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-map,$t/map

# ld64's map: file 0 is "linker synthesized", and the input files follow
# in command line order, dylibs included, then the libraries a dylib
# re-exports that define a symbol the link uses (libSystem's
# libsystem_c). Sections and symbols are tab-separated with a size
# column.
grep -Eq '^\[  0\] linker synthesized$' $t/map
grep -Eq '^\[  1\] .*/a.o$' $t/map
grep -Eq '^\[  2\] .*/b.o$' $t/map
grep -Eq '^\[  3\] .*/libSystem.tbd$' $t/map
grep -Eq '^\[  4\] .*/system/libsystem_c.tbd$' $t/map
grep -Eq $'^0x[0-9A-Fa-f]+\t0x[0-9A-Fa-f]+\t__TEXT\t__text$' $t/map
grep -Eq $'^0x[0-9A-Fa-f]+\t0x[0-9A-Fa-f]+\t\[  0\] __mh_execute_header$' $t/map
grep -Eq $'^0x[0-9A-Fa-f]+\t0x[0-9A-Fa-f]+\t\[  1\] _hello$' $t/map
grep -Eq $'^0x[0-9A-Fa-f]+\t0x[0-9A-Fa-f]+\t\[  2\] _main$' $t/map
not grep -q ltmp $t/map

# The linker's own atoms are file 0's, but for a symbol's stub or GOT
# slot, which counts as the file defining the symbol. A C string is
# known by its contents, and a common symbol belongs to the first object
# that declared it at its size.
grep -Fq $'\t[  4] _printf.stub' $t/map
grep -Fq $'\t[  1] literal string: Hello world\\n' $t/map
grep -Fq $'\t[  0] compact unwind info' $t/map
grep -Fq $'\t[  1] _common_sym' $t/map

# So does a common symbol's GOT slot, whichever object's tentative
# definition won: the one of the largest size, here the second's.
if [ $ARCH = arm64 ]; then
  cat <<'EOF' | $CC -o $t/d.o -c -xassembler -
.globl _main
.p2align 2
_main:
  ret
.section __TEXT,__const
.p2align 2
  .long _big_common@GOT - .
.comm _big_common, 4, 2
EOF
else
  cat <<'EOF' | $CC -o $t/d.o -c -xassembler -
.globl _main
_main:
  addq _big_common@GOTPCREL(%rip), %rax
  ret
.comm _big_common, 4, 2
EOF
fi
echo '.comm _big_common, 16, 4' | $CC -o $t/e.o -c -xassembler -
$CC --ld-path=$mold -o $t/exe3 $t/d.o $t/e.o -Wl,-map,$t/map3
grep -Fq $'\t[  2] _big_common.got' $t/map3
grep -Eq $'\t\\[  2\\] _big_common$' $t/map3

# With -dead_strip, removed atoms are reported in their own section,
# after a blank line, with "<<dead>>" in the address column.
cat <<EOF | $CC -o $t/c.o -c -xc -
void unused_func() {}
const char *unused_str() { return "gone"; }
void hello2() {}
int main() { hello2(); }
EOF
$CC --ld-path=$mold -o $t/exe2 $t/c.o -Wl,-dead_strip -Wl,-map,$t/map2
[ "$(grep -B1 '^# Dead Stripped Symbols:$' $t/map2 | head -1)" = '' ]
grep -Eq $'^<<dead>>\t0x[0-9A-Fa-f]+\t\[  1\] _unused_func$' $t/map2
grep -Fq $'<<dead>>\t0x00000005\t[  1] literal string: gone' $t/map2
grep -Eq $'\t\[  1\] _hello2$' $t/map2
