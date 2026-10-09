#!/bin/bash
source "$(dirname "$0")"/common.inc

# -why_live prints, for each live symbol a pattern names, the chain of
# subsections that keeps it alive, each referring to the one above it,
# down to a root and why that is one. ld-prime words and orders its
# report differently (it prints every chain, in its own words), so this
# checks mold's.

cat <<EOF | $CC -o $t/a.o -c -xc -
void leaf() {}
void middle() { leaf(); }
void unused() {}
int main() { middle(); }
EOF

# The chain from _leaf back to the entry point, on stderr.
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-dead_strip -Wl,-why_live,_leaf 2> $t/log
grep -A3 '^_leaf from .*a\.o$' $t/log > $t/chain
cat > $t/chain.expected <<EOF
_leaf from $t/a.o
  _middle from $t/a.o
    _main from $t/a.o
      root: the entry point, -u or -alias
EOF
diff $t/chain.expected $t/chain

# A dead symbol prints nothing; a wildcard matches several.
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-dead_strip -Wl,-why_live,_unused 2> $t/log2
not grep -q _unused $t/log2

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-dead_strip -Wl,-why_live,'_m*' 2> $t/log3
grep -q '^_middle from ' $t/log3
grep -q '^_main from ' $t/log3
not grep -q '^_leaf' $t/log3

# A root says why it is one: the entry point, a -u symbol, an export.
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-dead_strip -Wl,-why_live,_main 2> $t/log4
grep -A1 '^_main from .*a\.o$' $t/log4 | grep -q '^  root: the entry point'

cat <<EOF | $CC -o $t/b.o -c -xc -
int leaf(void) { return 1; }
int middle(void) { return leaf() + 1; }
int other(void) { return leaf() + 2; }
int main(void) { return middle() + other(); }
EOF
cat <<EOF | $CC -o $t/c.o -c -xc -
int leaf(void);
__attribute__((constructor)) static void init(void) { leaf(); }
EOF

$CC --ld-path=$mold -o $t/exe $t/b.o -Wl,-dead_strip,-why_live,_other,-u,_other 2> $t/log5
grep -A1 '^_other from ' $t/log5 | grep -q '^  root: the entry point, -u or -alias'

$CC --ld-path=$mold -o $t/exe $t/b.o -Wl,-dead_strip,-why_live,_main,-export_dynamic 2> $t/log6
grep -A1 '^_main from ' $t/log6 | grep -q '^  root: exported$'

$CC --ld-path=$mold -shared -o $t/lib.dylib $t/b.o -Wl,-dead_strip,-why_live,_leaf 2> $t/log7
grep -A1 '^_leaf from ' $t/log7 | grep -q '^  root: exported$'

# A symbol is reported once, by one chain, however many keep it: here
# an initializer, a root of its own, reaches _leaf first.
$CC --ld-path=$mold -o $t/exe $t/b.o $t/c.o -Wl,-dead_strip,-why_live,_leaf 2> $t/log8
[ "$(grep -c '^_leaf from ' $t/log8)" = 1 ]
grep -A2 '^_leaf from ' $t/log8 > $t/chain8
cat > $t/chain8.expected <<EOF
_leaf from $t/b.o
  _init from $t/c.o
    root: an initializer
EOF
diff $t/chain8.expected $t/chain8

# Every section of an object without subsections is a root.
cat <<EOF | $CC -o $t/d.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  ret
.globl _f1
_f1:
  ret
EOF
$CC --ld-path=$mold -o $t/exe $t/d.o -Wl,-dead_strip,-why_live,_f1 2> $t/log9
grep -A1 '^_f1 from .*d\.o$' $t/log9 | grep -q '^  root: never dead-stripped$'

# An import is kept by the live code that refers to it, and named with
# the dylib's path.
cat <<EOF | $CC -o $t/e.o -c -xc -
int leaf(void);
int main(void) { return leaf(); }
EOF
$CC --ld-path=$mold -o $t/exe $t/e.o $t/lib.dylib -Wl,-dead_strip,-why_live,_leaf 2> $t/log10
grep -A2 '^_leaf from ' $t/log10 > $t/chain10
cat > $t/chain10.expected <<EOF
_leaf from $t/lib.dylib
  _main from $t/e.o
    root: the entry point, -u or -alias
EOF
diff $t/chain10.expected $t/chain10
