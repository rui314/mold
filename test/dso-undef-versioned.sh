#!/usr/bin/env bash
. $(dirname $0)/common.inc

is_musl && skip

# libbar.so provides bar@BAR_1.0.
cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
__asm__(".symver bar,bar@BAR_1.0");
int bar(void) { return 42; }
EOF

echo 'BAR_1.0 { global: bar; local: *; };' > $t/b.map

$CC -B. -shared -o $t/libbar.so $t/a.o -Wl,--version-script,$t/b.map

# libfoo.so has a versioned undefined bar@BAR_1.0.
cat <<EOF | $CC -fPIC -c -o $t/c.o -xc -
__asm__(".symver bar,bar@BAR_1.0");
int bar(void);
int foo(void) { return bar(); }
EOF

$CC -B. -shared -o $t/libfoo.so $t/c.o -L$t -lbar -Wl,-rpath,$t

# libbar.a also provides bar@BAR_1.0 (with a different value).
cat <<EOF | $CC -fPIC -c -o $t/d.o -xc -
__asm__(".symver bar,bar@BAR_1.0");
int bar(void) { return 13; }
EOF

rm -f $t/e.a
ar rcs $t/e.a $t/d.o

cat <<EOF | $CC -fPIE -c -o $t/f.o -xc -
int foo(void);
int main(void) { return foo() - 42; }
EOF

# Archive before libfoo.so. It must not be pulled in.
$CC -B. -pie -o $t/exe $t/f.o $t/e.a -L$t -lfoo -Wl,-rpath,$t

nm $t/exe | not grep -E ' bar$'
readelf --dyn-syms $t/exe | not grep -E ' bar$'
$QEMU $t/exe
