#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime aligns each record of a section of fixed-size records at a
# multiple of its alignment, with no modulus, and mostly keeps the
# section's alignment for it. A thread-local variable descriptor goes
# at a multiple of a pointer in an image (clang aligns __thread_vars to
# a byte), and of at least one in a -r output; a selector reference
# keeps its section's, even one an _objc_msgSend$ stub takes over.

# Sets the p2align of section SECT of object IN, writing OUT.
cat > $t/align.py <<'EOF'
import struct, sys
src, dst, sect, p2align = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
d = bytearray(open(src, 'rb').read())
off = 32
for _ in range(struct.unpack_from('<I', d, 16)[0]):
    cmd, size = struct.unpack_from('<II', d, off)
    for i in range(struct.unpack_from('<I', d, off + 64)[0] if cmd == 0x19 else 0):
        s = off + 72 + i * 80
        if d[s:s + 16].rstrip(b'\0').decode() == sect:
            struct.pack_into('<I', d, s + 52, p2align)
    off += size
open(dst, 'wb').write(d)
EOF
sect() { otool -l $1 | grep -A6 "sectname $2\$" | awk '$1 == "size" || $1 == "align" { printf "%s ", $2 }'; }
addr() { nm $1 | awk -v s=$2 '$3 == s { print $1 }'; }

cat <<EOF | $CC -o $t/tlv.o -c -xassembler -
.section __DATA,__thread_data,thread_local_regular
.p2align 3
_a\$tlv\$init: .quad 1
_b\$tlv\$init: .quad 2
.section __DATA,__thread_vars,thread_local_variables
.p2align 4
.globl _a
.globl _b
_a: .quad __tlv_bootstrap
.quad 0
.quad _a\$tlv\$init
_b: .quad __tlv_bootstrap
.quad 0
.quad _b\$tlv\$init
.subsections_via_symbols
EOF
cat <<EOF | $CC -o $t/main.o -c -xc -
extern __thread long a, b;
int main() { return a + b != 3; }
EOF
$CC --ld-path=$mold -o $t/exe $t/main.o $t/tlv.o
$t/exe
[ "$(sect $t/exe __thread_vars)" = '0x0000000000000030 2^3 ' ]
[ $((0x$(addr $t/exe _b) - 0x$(addr $t/exe _a))) = 24 ]

$mold -r -arch $ARCH -o $t/r.o $t/tlv.o
[ "$(sect $t/r.o __thread_vars)" = '0x0000000000000038 2^4 ' ]
[ $((0x$(addr $t/r.o _b) - 0x$(addr $t/r.o _a))) = 32 ]

cat <<EOF | $CC -o $t/a.o -c -xobjective-c -
#import <objc/objc.h>
SEL f(void) { return @selector(foo); }
EOF
python3 $t/align.py $t/a.o $t/a2.o __objc_selrefs 4
cat <<EOF | $CC -o $t/b.o -c -xobjective-c - -Wno-objc-method-access
#import <objc/objc.h>
SEL g(void) { return @selector(bar); }
void h(id x) { [x bar]; }
EOF
python3 $t/align.py $t/b.o $t/b2.o __objc_selrefs 4
echo 'int main() { return 0; }' | $CC -o $t/main2.o -c -xc -

$CC --ld-path=$mold -o $t/exe2 $t/main2.o $t/a2.o -lobjc
[ "$(sect $t/exe2 __objc_selrefs)" = '0x0000000000000008 2^4 ' ]
$CC --ld-path=$mold -o $t/exe3 $t/main2.o $t/b2.o -lobjc
[ "$(sect $t/exe3 __objc_selrefs)" = '0x0000000000000008 2^4 ' ]
