#!/bin/bash
source "$(dirname "$0")"/common.inc

# The macOS versions are the point of the test.
on_simulator && skip

# macOS before 10.11.4 checks only SHA-1 page hashes. ld-prime signs an
# image for such a release (of either architecture, though no such
# release runs arm64 code), or an x86-64 one for firmware, with a SHA-1
# code directory in the code directory slot and the SHA-256 one as the
# first alternate (slot 0x1000), and so a -static image of either
# architecture; any other arm64 image gets the SHA-256 one alone.
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
import struct, sys, macho
sig = macho.MachO(sys.argv[1]).linkedit_data(macho.LC_CODE_SIGNATURE)
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
  $RUN $t/exe4

  echo 'int main() { return 0; }' | $CC -o $t/c.o -c -xc - -mmacosx-version-min=10.12
  $CC --ld-path=$mold -o $t/exe5 $t/c.o -mmacosx-version-min=10.12 -Wl,-adhoc_codesign
  [ "$(hash_types $t/exe5)" = "$sha256" ]
fi

sdk=$(xcrun --show-sdk-path)
echo 'int main() { return 0; }' | $CC -o $t/d.o -c -xc -
for v in 10.11 10.12; do
  $mold -arch $ARCH -syslibroot $sdk -platform_version macos $v 27.0 $t/d.o -lSystem \
    -o $t/exe-$v -adhoc_codesign 2> /dev/null
done
[ "$(hash_types $t/exe-10.11)" = "$sha1" ]
[ "$(hash_types $t/exe-10.12)" = "$sha256" ]

# (10.11.4 already checks SHA-256 hashes.)
for v in 10.11.3 10.11.4; do
  $mold -arch $ARCH -syslibroot $sdk -platform_version macos $v 27.0 $t/d.o -lSystem \
    -o $t/exe-$v -adhoc_codesign 2> /dev/null
done
[ "$(hash_types $t/exe-10.11.3)" = "$sha1" ]
[ "$(hash_types $t/exe-10.11.4)" = "$sha256" ]

# iOS and tvOS devices check SHA-256 hashes from 11 on. A simulator's
# image is checked by the Mac's kernel, and visionOS came later: they
# never get a SHA-1 directory.
for p in ios:iphoneos:10.3:11.0 tvos:appletvos:10.2:11.0 ios-simulator:iphonesimulator:9.0:11.0 \
  xros:xros:1.0:2.0; do
  IFS=: read name sdkname old new <<< "$p"
  [ $ARCH = arm64 ] || [ $name = ios-simulator ] || continue
  sdk=$(xcrun --sdk $sdkname --show-sdk-path 2> /dev/null) || continue
  os=${name%-simulator}
  env=${name#$os}
  echo 'int main() { return 0; }' |
    cc -target $ARCH-apple-${os}1.0$env -isysroot $sdk -o $t/$name.o -c -xc -
  for v in $old $new; do
    $mold -arch $ARCH -syslibroot $sdk -platform_version $name $v 27.0 $t/$name.o -lSystem \
      -o $t/$name-$v -adhoc_codesign -no_encryption 2> /dev/null
  done
  if [ $name = ios ] || [ $name = tvos ]; then
    [ "$(hash_types $t/$name-$old)" = "$sha1" ]
  else
    [ "$(hash_types $t/$name-$old)" = "$sha256" ]
  fi
  [ "$(hash_types $t/$name-$new)" = "$sha256" ]
done
