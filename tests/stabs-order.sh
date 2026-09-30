#!/bin/bash
source "$(dirname "$0")"/common.inc

# A final image's debug notes (stabs) follow its non-debug local
# symbols, open with a closing N_SO of their own, and list each
# compilation unit's symbols by address - functions and data alike -
# as ld-prime does. A global's N_GSYM carries no section or address
# (the debugger looks it up by name), and a tentative definition gets
# one in the first object that declares it.
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

# Each unit's notes by address: an N_GSYM counts at its global's.
awk 'NR == FNR { if ($2 != "-") addr[$3] = $1; next }
     $2 == "-" && $5 == "SO" && $6 == "" { print "--" }
     $2 == "-" && ($5 == "FUN" || $5 == "STSYM") && $6 != "" { print $1, $6 }
     $2 == "-" && $5 == "GSYM" { print addr[$6], $6 }' $t/nm $t/nm > $t/order
awk '$1 == "--" { prev = ""; next }
     { if (prev != "" && ($1 "") < prev) { print "out of order:", $0; exit 1 } prev = $1 "" }' $t/order
grep -q ' _gz$' $t/order
[ "$(grep -c ' _gz$' $t/order)" = 1 ]

# An N_GSYM names no section and no address.
grep ' GSYM ' $t/nm > $t/gsym
not grep -v '^0000000000000000 - 00 0000  GSYM ' $t/gsym
[ "$(awk '$2 == "_gz" { print NR }' $t/order)" -lt "$(awk '$1 == "--" { n++ } n == 2 { print NR; exit }' $t/order)" ]
