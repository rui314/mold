#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
__attribute__((weak)) int foo() { return 42; }
EOF

echo 'VERSION { global: foo; local: *; };' > $t/a.ver
$CC -B. -shared -o $t/b.so $t/a.o -Wl,--version-script=$t/a.ver

cat <<EOF | $CC -fPIC -c -o $t/c.o -xc -
int foo();
int bar() { return foo(); }
EOF

$CC -B. -shared -o $t/d.so $t/c.o $t/b.so

cat <<EOF | $CC -fPIC -c -o $t/e.o -xc -
void missing();
int foo_impl() { missing(); return -1; }
__asm__(".symver foo_impl,foo@VERSION");
EOF

rm -f $t/f.a
ar rcs $t/f.a $t/e.o

cat <<EOF | $CC -c -o $t/g.o -xc -
int bar();
int main() { return bar(); }
EOF

# d.so references foo@VERSION, so the matching archive member must be extracted.
not $CC -B. -o $t/exe $t/g.o $t/f.a $t/d.so $t/b.so >& $t/log
grep 'undefined symbol: missing' $t/log
