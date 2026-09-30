#!/bin/bash
source "$(dirname "$0")"/common.inc

[ "$ARCH" = arm64 ] || skip

# A dylib calls its weak definition w across 140 MiB of code, through
# w's stub, which dyld binds to the executable's w, the first one in
# load order. A range-extension thunk on the way must jump to the stub
# too, not to the dylib's own w.
cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((weak)) int w(void) { return 2; }
int f(void) { return w(); }
EOF

cat <<'EOF' | $CC -o $t/pad.o -c -xassembler -
.subsections_via_symbols
.macro pad
_pad\@:
  .long \@
  .space 0x100000 - 4
.endm
.rept 140
pad
.endr
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
int w(void);
int g(void) { return w(); }
EOF

$CC --ld-path=$mold -dynamiclib -o $t/libfoo.dylib $t/a.o $t/pad.o $t/b.o

cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
__attribute__((weak)) int w(void) { return 1; }
int f(void);
int g(void);
int main() { printf("%d %d\n", f(), g()); }
EOF

$CC --ld-path=$mold -o $t/exe $t/main.o $t/libfoo.dylib
$t/exe | grep -q '^1 1$'
