#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Create a shared library
cat <<EOF | $CC -o $t/libfoo.so -shared -fPIC -Wl,-soname,libfoo.so -xc -
int foo() { return 42; }
EOF

# Create an object file with a dead function referencing foo(),
# and a main() that does not reference foo().
cat <<EOF | $CC -ffunction-sections -o $t/prog.o -c -xc -
int foo();
int dead() { return foo(); }
int main() { return 0; }
EOF

# Link with --as-needed and --gc-sections (should remove libfoo.so unconditionally now)
$CC -B. -o $t/exe1 $t/prog.o -Wl,--as-needed -Wl,--gc-sections $t/libfoo.so
readelf --dynamic $t/exe1 > $t/log1
not grep -F 'Shared library: [libfoo.so]' $t/log1

# Link with --as-needed but without --gc-sections (should keep libfoo.so because dead() is not GC'ed)
$CC -B. -o $t/exe2 $t/prog.o -Wl,--as-needed -Wl,--no-gc-sections $t/libfoo.so
readelf --dynamic $t/exe2 > $t/log2
grep -F 'Shared library: [libfoo.so]' $t/log2
