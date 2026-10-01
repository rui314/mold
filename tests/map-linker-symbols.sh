#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime's -map lists the symbols it defines once something refers
# to them, as atoms of its own of no size: ___dso_handle (C++ static
# destructors pass it to __cxa_atexit), a dylib's __mh_dylib_header and
# section boundaries, but no segment's. A section -sectcreate makes is
# an atom "l<sect-create>" and the section's name, one only a boundary
# symbol makes is one named by the section alone.
cat <<EOF | $CXX -o $t/a.o -c -xc++ -
struct A { ~A(); };
A::~A() {}
A a;
int main() {}
EOF

printf 'sixteen bytes!!\n' > $t/blob

$CXX --ld-path=$mold -o $t/exe $t/a.o -Wl,-sectcreate,__TEXT,__blob,$t/blob -Wl,-map,$t/map
head -n 2 <(sed -n '/^# Symbols:/,$p' $t/map | grep '^0x') > $t/head
grep -q $'^0x100000000\t0x00000000\t\\[  0\\] __mh_execute_header$' $t/head
grep -q $'^0x100000000\t0x00000000\t\\[  0\\] ___dso_handle$' $t/head
grep -Eq $'^0x[0-9A-F]+\t0x00000010\t\\[  0\\] l<sect-create>__TEXT,__blob$' $t/map

cat <<EOF | $CC -o $t/b.o -c -xc -
extern char _mh_dylib_header;
extern char text_start __asm("section\$start\$__TEXT\$__text");
extern char text_seg __asm("segment\$start\$__TEXT");
extern char mine __asm("section\$start\$__DATA\$__mine");
long f(void) {
  return (long)&_mh_dylib_header + (long)&text_start + (long)&text_seg + (long)&mine;
}
EOF

$CC --ld-path=$mold -o $t/c.dylib -shared $t/b.o -Wl,-map,$t/map2
grep -q $'^0x00000000\t0x00000000\t\\[  0\\] __mh_dylib_header$' $t/map2
grep -Eq $'^0x[0-9A-F]+\t0x00000000\t\\[  0\\] section\\$start\\$__TEXT\\$__text$' $t/map2
grep -Eq $'^0x[0-9A-F]+\t0x00000000\t\\[  0\\] __DATA,__mine$' $t/map2
not grep -q 'segment\$start' $t/map2

# A name -alias gives is the linker's too, of no size, after the
# symbol it aliases.
cat <<EOF | $CC -o $t/c.o -c -xc -
int foo(void) { return 1; }
int data1 = 4;
int main(void) { return foo(); }
EOF
$CC --ld-path=$mold -o $t/exe3 $t/c.o -Wl,-alias,_foo,_bar -Wl,-alias,_data1,_data2 \
  -Wl,-map,$t/map3
sed -n '/^# Symbols:/,$p' $t/map3 > $t/syms3
grep -A1 ' _foo$' $t/syms3 | grep -Eq $'\t0x00000000\t\\[  0\\] _bar$'
grep -A1 ' _data1$' $t/syms3 | grep -Eq $'\t0x00000000\t\\[  0\\] _data2$'
