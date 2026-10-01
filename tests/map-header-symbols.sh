#!/bin/bash
source "$(dirname "$0")"/common.inc

# The map lists the names the linker gives the mach header that code
# refers to - ___dso_handle, which a C++ static destructor's
# registration passes, and a dylib's or bundle's __mh_*_header - by
# name at the header's address, credited to the linker; those only
# code -dead_strip removed used go among the dead, after the start of
# __TEXT they name, unless the header is a root (an executable's).
cat <<EOF | $CC -c -xc - -o $t/a.o
extern char __dso_handle[], _mh_dylib_header[];
char *p = __dso_handle, *q = _mh_dylib_header;
int f(void) { return 1; }
EOF
$CC --ld-path=$mold -shared -o $t/a.dylib $t/a.o -Wl,-map,$t/map
sed -n '/^# Symbols:/,$p' $t/map | grep '\[  0\]' | head -2 > $t/names
printf '0x00000000\t0x00000000\t[  0] ___dso_handle\n0x00000000\t0x00000000\t[  0] __mh_dylib_header\n' |
  diff - $t/names

$CC --ld-path=$mold -shared -o $t/b.dylib $t/a.o -Wl,-map,$t/map2 -Wl,-dead_strip \
  -Wl,-exported_symbol,_f
sed -n '/^# Dead Stripped Symbols:/,$p' $t/map2 | grep '\[  0\]' > $t/dead
printf '<<dead>>\t0x00000000\t[  0] %s\n' 'segment$start$__TEXT' ___dso_handle __mh_dylib_header |
  diff - $t/dead

cat <<EOF | $CC -c -xc - -o $t/c.o
extern char __dso_handle[];
char *p = __dso_handle;
int main() { return 0; }
EOF
$CC --ld-path=$mold -o $t/exe $t/c.o -Wl,-map,$t/map3
sed -n '/^# Symbols:/,$p' $t/map3 | grep '\[  0\]' | head -2 | cut -f3 > $t/names3
printf '[  0] __mh_execute_header\n[  0] ___dso_handle\n' | diff - $t/names3
