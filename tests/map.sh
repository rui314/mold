#!/bin/bash
source "$(dirname "$0")"/common.inc

# -map writes ld64's map: the output and its architecture; the files,
# [0] standing for the linker, then the objects in link order and the
# dylibs; the sections, at their addresses and of their sizes; and the
# symbols, tab-separated, a row for each symbol of a subsection at its
# address, of the size up to the subsection's end, with the number of
# the file it came from. A subsection no symbol names at its start is
# "anon", and what the linker makes is file 0's, named after its
# section.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
void hello() { printf("Hello world\n"); }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
void hello();
int data1 = 3;
int main() { hello(); return data1 - 3; }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-map,$t/map
$t/exe | grep -q 'Hello world'

grep -qx "# Path: $t/exe" $t/map
grep -qx "# Arch: $ARCH" $t/map
sed -n '/^# Object files:/,/^# Sections:/p' $t/map | grep '^\[' > $t/files
[ "$(sed -n 1p $t/files)" = '[  0] linker synthesized' ]
[ "$(sed -n 2p $t/files)" = "[  1] $t/a.o" ]
[ "$(sed -n 3p $t/files)" = "[  2] $t/b.o" ]
grep -Eq '^\[  3\] /.*/libSystem.tbd$' $t/files
[ "$(wc -l < $t/files)" -eq 4 ]

# Every section of the image, in order.
otool -l $t/exe | awk '
  $1 == "sectname" { s = $2 }
  $1 == "segname" && s != "" { g = $2 }
  $1 == "addr" && s != "" { a = $2 }
  $1 == "size" && s != "" { print a, $2, g, s; s = "" }' |
  while read -r addr size seg sect; do
    printf '0x%08X\t0x%08X\t%s\t%s\n' $addr $size $seg $sect
  done > $t/sects
sed -n '/^# Sections:/,/^# Symbols:/p' $t/map | grep '^0x' | diff $t/sects -

# The executable's header is the linker's, and comes first.
sed -n '/^# Symbols:/,$p' $t/map | grep '^0x' > $t/syms
[ "$(head -1 $t/syms)" = $'0x100000000\t0x00000000\t[  0] __mh_execute_header' ]

# A symbol's row is at its address, of its subsection's size: here each
# object's whole __text or __data.
row() {
  local addr=$(nm $t/exe | awk -v s=$1 '$3 == s { print $1 }')
  local size=$(otool -l $2 | awk -v s=$3 '$1 == "sectname" { f = ($2 == s) } f && $1 == "size" { print $2; exit }')
  printf '0x%08X\t0x%08X\t[  %d] %s\n' 0x$addr $size $4 $1
}
grep -qxF "$(row _hello $t/a.o __text 1)" $t/syms
grep -qxF "$(row _main $t/b.o __text 2)" $t/syms
grep -qxF "$(row _data1 $t/b.o __data 2)" $t/syms

# The C string is anon, a.o's; the stubs and the unwind info are the
# linker's.
grep -qx $'0x[0-9A-F]*\t0x0000000D\t\\[  1\\] anon' $t/syms
stubs=$(grep $'\t__TEXT\t__stubs$' $t/map | cut -f1,2)
grep -qx "$stubs"$'\t\\[  0\\] __TEXT,__stubs' $t/syms
grep -qx $'0x[0-9A-F]*\t0x[0-9A-F]*\t\\[  0\\] __TEXT,__unwind_info' $t/syms
not grep -q ltmp $t/map

# With -dead_strip, the subsections the output dropped are listed after
# a blank line, of the files they came from, with "<<dead>>" for an
# address; a live one is not.
cat <<EOF | $CC -o $t/c.o -c -xc -
void unused_func() {}
const char *unused_str() { return "gone"; }
void hello2() {}
int main() { hello2(); }
EOF
$CC --ld-path=$mold -o $t/exe2 $t/c.o -Wl,-dead_strip -Wl,-map,$t/map2
[ "$(grep -B1 '^# Dead Stripped Symbols:$' $t/map2 | head -1)" = '' ]
sed -n '/^# Dead Stripped Symbols:/,$p' $t/map2 > $t/dead2
grep -qx $'<<dead>>\t0x000000[0-9A-F][0-9A-F]\t\\[  1\\] _unused_func' $t/dead2
grep -qx $'<<dead>>\t0x000000[0-9A-F][0-9A-F]\t\\[  1\\] _unused_str' $t/dead2
grep -qx $'<<dead>>\t0x00000005\t\\[  1\\] anon' $t/dead2
not grep -q '_hello2\|_main' $t/dead2
grep -q $'\t\\[  1\\] _hello2$' $t/map2

# Without -dead_strip there is no such list, and a dylib has no header
# row.
$CC --ld-path=$mold -shared -o $t/c.dylib $t/c.o -Wl,-map,$t/map3
not grep -q 'Dead Stripped\|__mh_' $t/map3
grep -q $'\t\\[  1\\] _unused_func$' $t/map3
