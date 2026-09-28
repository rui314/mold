#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int foo();
int main() { return foo(); }
EOF

# An ignored SIGCHLD is inherited across exec. The forked child is then
# reaped automatically, but mold must still report that the link failed.
(trap '' CHLD; not ./mold -o $t/exe $t/a.o)
