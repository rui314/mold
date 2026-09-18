#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
__attribute__((weak)) int foo() { return 42; }
EOF

echo 'VER { global: foo; };' > $t/a.ver
$CC -B. -shared -o $t/a.so $t/a.o -Wl,--version-script=$t/a.ver

cat <<EOF | $CC -fPIC -c -o $t/b.o -xc -
int foo();
int bar() { return foo(); }
EOF

$CC -B. -shared -o $t/b.so $t/b.o $t/a.so

cat <<EOF | $CC -c -o $t/c.o -xc -
void missing();
__attribute__((weak)) int foo() { missing(); return 0; }
EOF

rm -f $t/c.a
ar rcs $t/c.a $t/c.o

cat <<EOF | $CC -c -o $t/d.o -xc -
int bar();
int main() { return bar() != 42; }
EOF

# b.so references foo@VER, which must not extract the unversioned foo
# from c.a and introduce an undefined reference to missing.
$CC -B. -o $t/exe $t/d.o $t/c.a $t/b.so $t/a.so
$QEMU $t/exe
