#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime reads an object's auto-link options in a row, as a command
# line of library options. It links the libraries, frameworks and
# archives they name (-hidden-l hides an archive's symbols as on the
# command line); it drops unknown words and options missing their
# argument with a warning, the weak, re-exported and upward forms with
# another, and search paths silently.
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
grep -q "malformed linker option from object file ignored: '-framework.*missing.*, in .*/a.o" $t/log
grep -q "unexpected linker option from object file ignored: '-weak_framework Foundation' in .*/a.o" $t/log
not grep -q -- -L/nonexistent $t/log
nm -m $t/libx.dylib | grep -q 'non-external (was a private external) _baz'

# Each object the link loads is warned of once, an archive member only
# if it is loaded. (ld-prime warns twice of an object the link loads
# from the start: as it reads it, and again once it has resolved the
# symbols.)
cat <<EOF | $CC -o $t/e.o -c -x assembler -
.globl _e
_e: ret
.linker_option "-eee"
EOF
cat <<EOF | $CC -o $t/f.o -c -x assembler -
.globl _f
_f: ret
.linker_option "-fff"
EOF
cat <<EOF | $CC -o $t/h.o -c -x assembler -
.globl _h
_h: ret
.linker_option "-hhh"
EOF
rm -f $t/libf.a
ar rcs $t/libf.a $t/f.o $t/h.o
cat <<EOF | $CC -o $t/g.o -c -x assembler -
.data
.p2align 3
.quad _f
EOF
$CC --ld-path=$mold -dynamiclib -o $t/liby.dylib $t/g.o $t/e.o $t/libf.a 2> $t/log6
[ "$(grep -c "unknown linker option from object file ignored: '-eee'" $t/log6)" = 1 ]
[ "$(grep -c "unknown linker option from object file ignored: '-fff'" $t/log6)" = 1 ]
not grep -q -- -hhh $t/log6

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

# A command holds an option and its argument if it takes one: ld-prime
# refuses a command of more strings, or of none.
cat <<EOF | $CC -o $t/d.o -c -x assembler -
.linker_option "-lz", "-lm", "-lc"
EOF
not $CC --ld-path=$mold -o $t/exe $t/d.o $t/main.o $t/baz.o 2> $t/log5
grep -Fq "$t/d.o: LC_LINKER_OPTION has count=3, only 1 or 2 is valid" $t/log5
