#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
void foo() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -fPIC
__asm__(".symver foo_,foo@@VERSION");
void foo_() {}
EOF

cat <<EOF > $t/c.version
VERSION { global: foo; local: *; };
EOF

$CC -B. -shared -o $t/libfoo.so $t/a.o
$CC -B. -shared -o $t/libbar.so $t/b.o -Wl,--version-script=$t/c.version

cat <<EOF | $CC -o $t/d.o -c -xc -
void foo();
int main() { foo(); }
__asm__(".symver foo,foo@VERSION");
EOF

$CC -B. -o $t/exe $t/d.o -L$t -lfoo -lbar
