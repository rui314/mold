#!/bin/bash
source "$(dirname "$0")"/common.inc

# A final image's debug notes (stabs) follow its non-debug local
# symbols and open with a closing N_SO of their own. Each compilation
# unit notes its functions and data once; ld-prime lists them by
# address, mold in symbol-table order, and neither dsymutil nor lldb
# cares: both map a unit's notes by name. A global's N_GSYM carries no
# section or address (the debugger looks it up by name), and a
# tentative definition gets one in the first object that declares it.
cat <<EOF | $CC -o $t/a.o -c -g -xc -
static int s1 = 1;
int g1 = 2;
static int s2 = 3;
const int c1 = 4;
int gz;
static int sz;
int f2(void);
static int f1(void) { return s1 + s2 + sz + gz; }
int f2(void) { return f1() + g1 + c1; }
int main(void) { return f2(); }
EOF
cat <<EOF | $CC -o $t/b.o -c -g -xc -
int gz;
int f3(void) { return gz; }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o
nm -ap $t/exe > $t/nm

# The locals, then the first stab, an N_SO with an empty name.
awk '{ if ($2 == "-") exit; print $3 }' $t/nm > $t/locals
[ "$(sort $t/locals | tr '\n' ' ')" = '_f1 _s1 _s2 _sz ' ]
awk '$2 == "-" { print $5, $6; exit }' $t/nm | grep -qx 'SO '

# dsymutil's debug map: each unit's symbols, at their addresses in the
# image, the functions with their sizes from the N_FUN pairs.
dsymutil --dump-debug-map $t/exe > $t/map
[ "$(awk '/filename:/ { print "--" } /sym:/ { print $4 }' $t/map | tr -d , | tr '\n' ' ')" = \
  '-- _c1 _f1 _f2 _g1 _gz _main _s1 _s2 _sz -- _f3 ' ]
sed -n 's/.*sym: \([^,]*\),.*binAddr: 0x\([0-9A-F]*\),.*/\1 \2/p' $t/map > $t/map-addrs
[ "$(wc -l < $t/map-addrs)" -eq 10 ]
nm -p $t/exe | awk '{ a = toupper($1); sub(/^0+/, "", a); print $3, a }' > $t/addrs
not grep -vxFf $t/addrs $t/map-addrs
not grep -E 'sym: _(f1|f2|f3|main),.*size: 0x0 ' $t/map

# An N_GSYM names no section and no address.
grep ' GSYM ' $t/nm > $t/gsym
not grep -v '^0000000000000000 - 00 0000  GSYM ' $t/gsym
