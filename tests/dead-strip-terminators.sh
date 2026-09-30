#!/bin/bash
source "$(dirname "$0")"/common.inc

# Dead stripping keeps every terminator (a __mod_term_func pointer, as
# ASan's module destructors are) and the function it names, as it
# keeps every initializer; nothing else refers to either.
cat <<EOF | $CC -o $t/a.o -c -xc -
void fini(void) {}
int main() { return 0; }
EOF
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __DATA,__mod_term_func,mod_term_funcs
.p2align 3
.quad _fini
.subsections_via_symbols
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-dead_strip
$t/exe
nm $t/exe > $t/syms
grep -q ' _fini$' $t/syms
otool -l $t/exe > $t/lc
grep -q 'sectname __mod_term_func' $t/lc
