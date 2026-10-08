#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF > $t/a.c
int foo() { return 42; }
EOF
seq 0 99 | sed 's/.*/int dwarf32_variable_&;/' >> $t/a.c
$CC -c -g -gdwarf32 -o $t/a.o $t/a.c || skip

cat <<EOF > $t/b.c
int main() { return 0; }
EOF
seq 0 99 | sed 's/.*/int dwarf64_variable_&;/' >> $t/b.c
$CC -c -g -gdwarf64 -o $t/b.o $t/b.c || skip

# Compress the objects, not a compiler invocation that stops before linking.
$OBJCOPY --compress-debug-sections=zstd $t/a.o $t/a-zstd.o || skip
$OBJCOPY --compress-debug-sections=zstd $t/b.o $t/b-zstd.o
readelf -WS $t/a-zstd.o | grep -E '\.debug_info .* C '
readelf -WS $t/b-zstd.o | grep -E '\.debug_info .* C '

# MOLD_DEBUG exercises the twelve-byte DWARF probe without a 4 GiB section.
MOLD_DEBUG=1 $CC -B. -o $t/exe $t/b-zstd.o $t/a-zstd.o -Wl,-Map=$t/map
grep -A10 -F '/a-zstd.o:(.debug_info)' $t/map | grep -F '/b-zstd.o:(.debug_info)'
readelf --debug-dump=info $t/exe > $t/debug
grep -F '(32-bit)' $t/debug
grep -F '(64-bit)' $t/debug
$QEMU $t/exe
