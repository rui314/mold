#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime splits a section of fixed-size records - literals, pointers
# to initializers and terminators, thread-local variable descriptors,
# CFString constants, Objective-C lists and __LD,__compact_unwind - into
# a subsection per record, and rejects the object if the section's size
# is no multiple of the record's. It reads every object so, an archive
# member the link doesn't use too, and names only an object's first
# such section. (A 9-byte __mod_init_func used to crash an arm64 link.)
echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -

# Checks that the object built from SECTION-DIRECTIVE (whose section
# holds nine bytes) fails a final link and -r with "section SEG/SECT
# size 9 is not a multiple of SIZE".
check() {
  printf '%s\n.p2align 3\n.quad 0\n.byte 0\n.text\n.globl _f\n_f: ret\n' "$1" |
    $CC -o $t/a.o -c -xassembler - &&
    not $CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o 2> $t/log &&
    grep -qF "section $2 size 9 is not a multiple of $3 in '$t/a.o'" $t/log &&
    not $mold -r -arch $ARCH -o $t/r.o $t/a.o 2> $t/log &&
    grep -qF "section $2 size 9 is not a multiple of $3 in '$t/a.o'" $t/log
}

check '.section __DATA,__mod_init_func,mod_init_funcs' __DATA/__mod_init_func 8
check '.section __DATA,__mod_term_func,mod_term_funcs' __DATA/__mod_term_func 8
check '.section __DATA,__foo,mod_init_funcs' __DATA/__foo 8
check '.section __TEXT,__literal4,4byte_literals' __TEXT/__literal4 4
check '.section __TEXT,__literal8,8byte_literals' __TEXT/__literal8 8
check '.section __TEXT,__literal16,16byte_literals' __TEXT/__literal16 16
check '.section __DATA,__thread_vars,thread_local_variables' __DATA/__thread_vars 24
check '.section __DATA,__cfstring' __DATA/__cfstring 32
check '.section __DATA,__objc_selrefs,literal_pointers,no_dead_strip' __DATA/__objc_selrefs 8
check '.section __DATA,__objc_classlist,regular,no_dead_strip' __DATA/__objc_classlist 8
check '.section __DATA,__objc_catlist,regular,no_dead_strip' __DATA/__objc_catlist 8
check '.section __DATA,__objc_protorefs,regular,no_dead_strip' __DATA/__objc_protorefs 8
check '.section __LD,__compact_unwind,regular,debug' __LD/__compact_unwind 32

# An archive member the link doesn't use is checked too; an object's
# first bad section is the one named.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __TEXT,__literal8,8byte_literals
.byte 0
.section __DATA,__mod_init_func,mod_init_funcs
.p2align 3
.byte 0
.text
.globl _g
_g: ret
EOF
rm -f $t/lib.a
ar rcs $t/lib.a $t/b.o
not $CC --ld-path=$mold -o $t/exe $t/main.o $t/lib.a 2> $t/log
grep -qF "section __TEXT/__literal8 size 1 is not a multiple of 8 in '$t/lib.a(b.o)'" $t/log
not grep -q __mod_init_func $t/log

# ld-prime reads the sections in order and stops at the bad one: it
# warns of a __cfstring section aligned less than a pointer if it is
# the bad one, not if it comes after it.
cfstring='.section __DATA,__cfstring'
init='.section __DATA,__mod_init_func,mod_init_funcs'
for first in cfstring mod_init_func; do
  if [ $first = cfstring ]; then
    printf '%s\n.long 0\n%s\n.long 0\n' "$cfstring" "$init"
  else
    printf '%s\n.long 0\n%s\n.long 0\n' "$init" "$cfstring"
  fi | $CC -o $t/d.o -c -xassembler -
  not $CC --ld-path=$mold -o $t/exe $t/main.o $t/d.o 2> $t/log
  grep -qF "section __DATA/__$first size 4 is not a multiple of" $t/log
  if [ $first = cfstring ]; then
    grep -q 'section __DATA/__cfstring is not pointer aligned' $t/log
  else
    not grep -q 'not pointer aligned' $t/log
  fi
done

# Where the name doesn't say what the records are, a section of the
# regular type is not one of them.
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.section __DATA_CONST,__cfstring
.byte 0
.section __DATA_CONST,__objc_classlist
.byte 0
.section __TEXT,__lit8
.byte 0
.text
.globl _h
_h: ret
EOF
$CC --ld-path=$mold -o $t/exe $t/main.o $t/c.o

# A CFString constant must have just two relocations, the class pointer
# at offset 0 and the string's at 16; ld-prime refuses another as it
# reads the object, in -r too.
cfstring() {
  printf '.cstring\nL_s: .asciz "hi"\n.section __DATA,__cfstring\n.p2align 3\n'
  printf '.quad %s\n.long 0x7c8\n.long 0\n.quad %s\n.quad %s\n' "$@"
}
check_cf() {
  cfstring $1 $2 $3 | $CC -o $t/cf.o -c -xassembler - &&
    not $CC --ld-path=$mold -o $t/exe $t/main.o $t/cf.o -framework CoreFoundation 2> $t/log &&
    grep -qF "$4 in '/" $t/log &&
    not $mold -r -arch $ARCH -o $t/r.o $t/cf.o 2> $t/log &&
    grep -qF "$4 in '/" $t/log
}
check_cf ___CFConstantStringClassReference 0 2 'cfstring constant does not have two fixups'
check_cf 0 L_s 2 'cfstring constant does not have two fixups'
check_cf ___CFConstantStringClassReference L_s _h 'cfstring constant does not have two fixups'
check_cf 0 L_s _h 'cfstring constant isa not at offset 0 in cfstring object'
check_cf ___CFConstantStringClassReference 0 _h \
  'cfstring constant string-data not at offset 16 in cfstring object'
cfstring ___CFConstantStringClassReference L_s 2 | $CC -o $t/cf.o -c -xassembler -
$CC --ld-path=$mold -o $t/exe $t/main.o $t/cf.o -framework CoreFoundation
