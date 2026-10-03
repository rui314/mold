#!/bin/bash
source "$(dirname "$0")"/common.inc

# What a diagnostic names from an input - an archive member's name, an
# install name, an auto-link option - it prints as the bytes it read,
# UTF-8 or not, as ld-prime does. (A path can't be made here: APFS
# takes only UTF-8 names.)

cat <<'EOF' | $CC -o $t/main.o -c -xc -
int dup(void) { return 1; }
int foo(void);
int main() { return foo(); }
EOF

cat <<'EOF' | $CC -o $t/mZ.o -c -xc -
int dup(void) { return 2; }
int foo(void) { return 0; }
EOF

# An archive member named m\xff.o.
rm -f $t/libm.a
ar rcs $t/libm.a $t/mZ.o
python3 - $t/libm.a <<'EOF'
import sys
data = open(sys.argv[1], 'rb').read()
assert data.count(b'mZ.o') == 1
open(sys.argv[1], 'wb').write(data.replace(b'mZ.o', b'm\xff.o'))
EOF
not $CC --ld-path=$mold -o $t/exe $t/main.o -Wl,-force_load,$t/libm.a 2> $t/log
grep -q $'libm.a(m\xff.o)$' $t/log

# A dylib's install name.
$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/mZ.o -mmacos-version-min=15.0 \
  -Wl,-install_name,$'/tmp/libf\xffo.dylib'
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/libfoo.dylib -mmacos-version-min=14.0 2> $t/log2
grep -q $'with dylib \'/tmp/libf\xffo.dylib\' which was built for newer version 15.0' $t/log2

# An object's auto-link options.
cat <<'EOF' | sed $'s/@/\xff/g' | $CC -o $t/opts.o -c -xassembler -
.linker_option "-foo@"
.linker_option "-framework", "F@"
.globl _bar
_bar:
  ret
EOF
not $CC --ld-path=$mold -o $t/exe3 $t/main.o $t/opts.o 2> $t/log3
grep -q $'unknown linker option from object file ignored: \'-foo\xff\'' $t/log3
grep -q $'auto-linked framework \'F\xff\': framework \'F\xff\' not found' $t/log3
