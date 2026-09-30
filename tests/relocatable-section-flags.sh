#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r output is input to another link, so ld-prime copies each
# section's type and attributes from its first non-empty input section
# (only __objc_imageinfo and __objc_protolist lose no_dead_strip).
# Normalizing them as for a final image would drop no_dead_strip, and a
# later -dead_strip link would then discard a table nothing references.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __DATA,__keep,regular,no_dead_strip
keep_me: .quad 0x1234
.section __DATA,__coal,coalesced
.quad 2
.section __DATA,__live,regular,live_support
.quad 3
.section __DATA,__nt,regular,no_toc
.quad 4
.subsections_via_symbols
EOF
echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -

attrs() {
  otool -lv $1 | awk -v s=$2 '$1 == "sectname" && $2 == s { f = 1 }
    f && $1 == "type" { t = $2 } f && $1 == "attributes" { print t, $2; exit }'
}

$mold -arch $ARCH -r $t/a.o $t/main.o -o $t/r.o
[ "$(attrs $t/r.o __keep)" = 'S_REGULAR NO_DEAD_STRIP' ]
[ "$(attrs $t/r.o __coal)" = 'S_COALESCED (none)' ]
[ "$(attrs $t/r.o __live)" = 'S_REGULAR LIVE_SUPPORT' ]
[ "$(attrs $t/r.o __nt)" = 'S_REGULAR NO_TOC' ]

# The no_dead_strip table survives a -dead_strip link of the -r output.
$CC --ld-path=$mold -o $t/exe $t/r.o -Wl,-dead_strip
otool -l $t/exe > $t/lc
grep -q 'sectname __keep' $t/lc

# The first member decides, but an empty one doesn't count.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __DATA,__keep,regular
.quad 5
.section __DATA,__empty,regular,no_dead_strip
.subsections_via_symbols
EOF
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.section __DATA,__empty,regular
.quad 6
.subsections_via_symbols
EOF
$mold -arch $ARCH -r $t/b.o $t/a.o -o $t/ba.o
[ "$(attrs $t/ba.o __keep)" = 'S_REGULAR (none)' ]
$mold -arch $ARCH -r $t/b.o $t/c.o -o $t/bc.o
[ "$(attrs $t/bc.o __empty)" = 'S_REGULAR (none)' ]

# Each symbol of a no_dead_strip section is itself marked no-dead-strip,
# so it survives where the output section took another member's
# attributes.
nm -m $t/ba.o > $t/syms-ba
grep -q '\[no dead strip\] keep_me' $t/syms-ba
$CC --ld-path=$mold -o $t/exe2 $t/ba.o $t/main.o -Wl,-dead_strip
nm $t/exe2 > $t/syms-exe2
grep -q keep_me $t/syms-exe2
