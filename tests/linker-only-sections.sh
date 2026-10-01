#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime reads a regular section by its name as contents only it
# makes, which it has no reader for in an object, and refuses a
# non-empty one: a __TEXT section of stub helpers, Objective-C stubs,
# lazy-load helpers or delay-init stubs or helpers that a symbol is in
# ("unknown symboled section type"; an arm64 assembler puts an ltmpN
# label in each), or __DATA,__lazy_load_got. An empty one, one with no
# symbol, or one of such a name in another segment is any section.
echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -
mk() {
  printf '.section %s\n%s\n.subsections_via_symbols\n' "$2" "$3" | \
    $CC -o $t/$1.o -c -xassembler -
}
mk a __TEXT,__objc_stubs,regular,pure_instructions '_a: .long 0'
mk b __TEXT,__stub_helper,regular '_b: .long 0'
mk c __DATA,__lazy_load_got,regular '.quad 0'
mk d __TEXT,__objc_stubs,regular ''
mk e __FOO,__stub_helper,regular '.long 0'

not $CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o 2> $t/log
grep -F "unknown symboled section type in '$t/a.o'" $t/log
not $mold -r -arch $ARCH -o $t/r.o $t/b.o 2> $t/log
grep -F "unknown symboled section type in '$t/b.o'" $t/log
not $CC --ld-path=$mold -o $t/exe $t/main.o $t/c.o 2> $t/log
grep -F "unknown fixed size section __DATA,__lazy_load_got with content type: lazy-load-GOT in '$t/c.o'" $t/log
$CC --ld-path=$mold -o $t/exe $t/main.o $t/d.o $t/e.o

# The size of the slots of lazy-load or weak GOT sections is checked
# first.
mk g __DATA,__lazy_load_got,regular '.long 0'
not $CC --ld-path=$mold -o $t/exe $t/main.o $t/g.o 2> $t/log
grep -F "section __DATA/__lazy_load_got size 4 is not a multiple of 8 in '$t/g.o'" $t/log
mk h __DATA,__weak_got,non_lazy_symbol_pointers '.long 0'
not $CC --ld-path=$mold -o $t/exe $t/main.o $t/h.o 2> $t/log
grep -F "section __DATA/__weak_got size 4 is not a multiple of 8 in '$t/h.o'" $t/log

# An object without MH_SUBSECTIONS_VIA_SYMBOLS has its sections whole:
# ld-prime takes one of stub helpers with a label in it as data.
printf '.section __TEXT,__stub_helper,regular\n.long 0\n' | $CC -o $t/i.o -c -xassembler -
$CC --ld-path=$mold -o $t/exe $t/main.o $t/i.o

[ $ARCH = x86_64 ] || exit 0
mk f __TEXT,__objc_stubs,regular,pure_instructions '.long 0'
$CC --ld-path=$mold -o $t/exe $t/main.o $t/f.o
