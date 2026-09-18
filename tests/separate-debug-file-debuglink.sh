#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -g -o $t/a.o -xc -
int main() { return 42; }
EOF

cat <<EOF | $CC -c -o $t/b.o -x assembler -
.section .zzz, ""
.globl marker
marker:
.byte 42
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,--separate-debug-file,--no-detach

# A dummy NOBITS .gnu_debuglink in the debug file makes readelf report
# corruption when following the executable's debug link. See issue #1656.
readelf --debug-dump=links --debug-dump=follow-links $t/exe > $t/log
grep -F 'Separate debug info file: exe.dbg' $t/log
readelf -SW $t/exe.dbg | not grep -F .gnu_debuglink

# Removing .gnu_debuglink must not invalidate symbols in later sections.
$OBJDUMP -t $t/exe.dbg | grep -E '\.zzz\b.* marker$'
