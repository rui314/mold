#!/bin/bash
source "$(dirname "$0")"/common.inc

# A __compact_unwind record's pointer fields are the function at offset
# 0, the personality at 16 and the LSDA at 24. On x86-64 a 4-byte
# relocation may set one as well as an 8-byte one; a relocation on any
# other field is an error.
cat <<EOF | $CC -o $t/main.o -c -xc -
int main() { return 0; }
EOF

unwind() {
  cat <<EOF | $CC -o $t/$1.o -c -xassembler -
.text
.globl _f
.p2align 2
_f: ret
.globl _g
_g: ret
.section __LD,__compact_unwind,regular,debug
.p2align 3
$2
.long 1
.long 0x02000000
$3
.quad 0
.subsections_via_symbols
EOF
}

unwind a '.quad _f' '.quad _g'
$CC --ld-path=$mold -o $t/a $t/main.o $t/a.o
otool -s __TEXT __unwind_info $t/a | tail -n +2 > $t/a.txt

if [ $ARCH = x86_64 ]; then
  unwind b '.long _f
.long 0' '.long _g
.long 0'
  $CC --ld-path=$mold -o $t/b $t/main.o $t/b.o
  otool -s __TEXT __unwind_info $t/b | tail -n +2 > $t/b.txt
  diff $t/a.txt $t/b.txt

  $mold -arch x86_64 -r -o $t/c.o $t/b.o
  $CC --ld-path=$mold -o $t/c $t/main.o $t/c.o
  otool -s __TEXT __unwind_info $t/c | tail -n +2 > $t/c.txt
  diff $t/a.txt $t/c.txt

  # A -r output keeps such a field 4 bytes (r_length 2), as ld-prime
  # does, and an 8-byte one 8 bytes (r_length 3).
  lengths() {
    otool -r $1 | awk '/^Relocation information/ { s = $3 }
      s == "(__LD,__compact_unwind)" && $1 ~ /^0/ { print $1, $3 }' | sort
  }
  [ "$(lengths $t/c.o)" = "$(printf '00000000 2\n00000010 2')" ]
  $mold -arch x86_64 -r -o $t/a-r.o $t/a.o
  [ "$(lengths $t/a-r.o)" = "$(printf '00000000 3\n00000010 3')" ]
fi

unwind d '.quad _f
.quad _g' ''
not $CC --ld-path=$mold -o $t/d $t/main.o $t/d.o 2> $t/log
grep -q "compact unwind fixup at offset of 8 but expected 16 or 24 in '.*/d.o'" $t/log
not $mold -arch $ARCH -r -o $t/e.o $t/d.o 2> $t/log
grep -q "compact unwind fixup at offset of 8 but expected 16 or 24 in '.*/d.o'" $t/log
