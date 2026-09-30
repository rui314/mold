#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime reads an object's auto-link options in a row, as a command
# line of library options. It links the libraries, frameworks and
# archives they name (-hidden-l hides an archive's symbols as on the
# command line); it drops unknown words and options missing their
# argument with a warning, then the weak, re-exported and upward forms
# with another, and search paths silently.
cat <<EOF | $CC -o $t/a.o -c -x assembler -
.linker_option "-foo", "bar"
.linker_option "-weak_framework", "Foundation"
.linker_option "-hidden-lbaz"
.linker_option "-L/nonexistent"
.linker_option "-framework"
EOF

cat <<EOF | $CC -o $t/baz.o -c -xc -
int baz(void) { return 3; }
EOF
ar rcs $t/libbaz.a $t/baz.o

cat <<EOF | $CC -o $t/main.o -c -xc -
int baz(void);
int main() { return baz() != 3; }
EOF

$CC --ld-path=$mold -dynamiclib -o $t/libx.dylib $t/main.o $t/a.o -L$t 2> $t/log
grep -q "unknown linker option from object file ignored: '-foo' in .*/a.o" $t/log
grep -q "unknown linker option from object file ignored: 'bar' in .*/a.o" $t/log
grep -q "malformed linker option from object file ignored: '-framework missing <path>', in .*/a.o" $t/log
grep -q "unexpected linker option from object file ignored: '-weak_framework Foundation' in .*/a.o" $t/log
not grep -q -- -L/nonexistent $t/log
nm -m $t/libx.dylib | grep -q 'non-external (was a private external) _baz'

# A -r link reports them too, and keeps the rest.
$mold -r -arch $ARCH -o $t/r.o $t/a.o 2> $t/log2
grep -q "unknown linker option from object file ignored: '-foo'" $t/log2
otool -l $t/r.o | grep -A3 LC_LINKER_OPTION > $t/lc
grep -q 'string #1 -.*lbaz' $t/lc
not grep -q -- '-foo' $t/lc

# An auto-linked library that was not found is mentioned only if
# symbols are left undefined.
cat <<EOF | $CC -o $t/b.o -c -x assembler -
.linker_option "-lnosuchlib"
EOF
cat <<EOF | $CC -o $t/c.o -c -xc -
int nosuch(void);
int main() { return nosuch(); }
EOF
not $CC --ld-path=$mold -o $t/exe $t/b.o $t/c.o 2> $t/log3
grep -q "Could not find or use auto-linked library 'nosuchlib': library 'nosuchlib' not found" $t/log3
$CC --ld-path=$mold -o $t/exe $t/b.o $t/main.o $t/baz.o 2> $t/log4
not grep -q nosuchlib $t/log4
