#!/bin/bash
source "$(dirname "$0")"/common.inc

# macOS before 10.12 checks only SHA-1 page hashes. ld-prime signs an
# x86-64 image for such a release, or for firmware, with a SHA-1 code
# directory in the code directory slot and the SHA-256 one as the first
# alternate (slot 0x1000), and so a -static image of either
# architecture; an arm64 image dyld loads gets the SHA-256 one alone.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _start
_start: ret
.data
.p2align 3
_p: .quad _start
EOF

# The hash type of each code directory, in blob index order.
hash_types() {
  python3 - $1 <<'EOF'
import struct, sys
d = open(sys.argv[1], 'rb').read()
off = 32
for _ in range(struct.unpack_from('<I', d, 16)[0]):
    cmd, size = struct.unpack_from('<II', d, off)
    if cmd == 0x1d:
        sig = d[struct.unpack_from('<I', d, off + 8)[0]:]
    off += size
for i in range(struct.unpack_from('>I', sig, 8)[0]):
    slot, pos = struct.unpack_from('>II', sig, 12 + i * 8)
    print('%x:%d' % (slot, sig[pos + 37]), end=' ')
EOF
}

sha1=$'0:1 1000:2 '
sha256=$'0:2 '
[ $ARCH = x86_64 ] && dyld_fw=$sha1 || dyld_fw=$sha256

fw='-platform_version firmware 1.0 1.0'
$mold -arch $ARCH $fw -e _start $t/a.o -o $t/exe1 -adhoc_codesign
[ "$(hash_types $t/exe1)" = "$dyld_fw" ]
codesign -v $t/exe1

$mold -arch $ARCH $fw -e _start $t/a.o -o $t/exe2 -adhoc_codesign -static
[ "$(hash_types $t/exe2)" = "$sha1" ]
codesign -v $t/exe2
codesign -dvvv $t/exe2 2> $t/log2
grep -q 'Hash choices=sha1,sha256' $t/log2

$mold -arch $ARCH -platform_version macos 26.0 26.0 -e _start $t/a.o -o $t/exe3 \
  -adhoc_codesign -static
[ "$(hash_types $t/exe3)" = "$sha1" ]
codesign -v $t/exe3

if [ $ARCH = x86_64 ]; then
  echo 'int main() { return 0; }' | $CC -o $t/b.o -c -xc - -mmacosx-version-min=10.11
  $CC --ld-path=$mold -o $t/exe4 $t/b.o -mmacosx-version-min=10.11 -Wl,-adhoc_codesign
  [ "$(hash_types $t/exe4)" = "$sha1" ]
  codesign -v $t/exe4
  $t/exe4

  echo 'int main() { return 0; }' | $CC -o $t/c.o -c -xc - -mmacosx-version-min=10.12
  $CC --ld-path=$mold -o $t/exe5 $t/c.o -mmacosx-version-min=10.12 -Wl,-adhoc_codesign
  [ "$(hash_types $t/exe5)" = "$sha256" ]
fi
