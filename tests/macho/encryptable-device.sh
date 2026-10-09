#!/bin/bash
source "$(dirname "$0")"/common.inc

# The test picks the platforms it links for itself.
on_simulator && skip

# The App Store encrypts the code of an app for an iOS, tvOS or visionOS
# device, so every executable, dylib and bundle linked for a device is
# encryptable unless -no_encryption or $LD_NO_ENCRYPT (set to anything)
# says otherwise: its __TEXT sections start a page of their own, which
# LC_ENCRYPTION_INFO_64 names. A -r, -static, -preload or -dylinker
# output isn't, nor a simulator's image or a Mac's; -encryptable makes
# any image but a -r or -preload one encryptable, $LD_NO_ENCRYPT or not.
[ $ARCH = arm64 ] || skip

enc() { otool -l $1 | grep -A3 LC_ENCRYPTION_INFO_64 | awk '$1 == "cryptoff" { print $2 }'; }
text_offset() { otool -l $1 | grep -A4 'sectname __text' | awk '$1 == "offset" { print $2 }'; }

cat <<EOF > $t/a.c
int x = 5;
int *p = &x;
int start(void) { return *p; }
int main(void) { return 0; }
EOF

for p in ios:iphoneos:17.0 tvos:appletvos:17.0 xros:xros:2.0; do
  IFS=: read name sdkname ver <<< "$p"
  sdk=$(xcrun --sdk $sdkname --show-sdk-path 2> /dev/null) || continue
  cc -target arm64-apple-$name$ver -isysroot $sdk -o $t/$name.o -c $t/a.c
  link() {
    $mold -arch arm64 -platform_version $name $ver 27.0 -syslibroot $sdk -o $t/$name.out "$@"
  }

  for kind in -execute -dylib -bundle; do
    link $kind $t/$name.o -lSystem
    [ "$(enc $t/$name.out)" = 16384 ]
    [ "$(text_offset $t/$name.out)" = 16384 ]
  done

  for kind in -r -static -preload -dylinker; do
    link $kind -e _start $t/$name.o
    [ -z "$(enc $t/$name.out)" ]
  done

  link $t/$name.o -lSystem -no_encryption
  [ -z "$(enc $t/$name.out)" ]
  link $t/$name.o -lSystem -no_encryption -encryptable
  [ "$(enc $t/$name.out)" = 16384 ]
  link $t/$name.o -lSystem -encryptable -no_encryption
  [ -z "$(enc $t/$name.out)" ]

  for value in 1 0 ''; do
    LD_NO_ENCRYPT=$value link $t/$name.o -lSystem
    [ -z "$(enc $t/$name.out)" ]
    LD_NO_ENCRYPT=$value link $t/$name.o -lSystem -encryptable
    [ "$(enc $t/$name.out)" = 16384 ]
  done

  # An encryptable image's sections start a page of their own even
  # where -segalign asks for smaller segment alignment.
  link $t/$name.o -lSystem -segalign 0x1000
  [ "$(text_offset $t/$name.out)" = 16384 ]

  # -encryptable is honored in a -static or -dylinker image.
  link -static -e _start $t/$name.o -encryptable
  [ "$(enc $t/$name.out)" = 16384 ]
  link -dylinker -e _start $t/$name.o -encryptable
  [ -n "$(enc $t/$name.out)" ]
  link -preload -e _start $t/$name.o -encryptable
  [ -z "$(enc $t/$name.out)" ]
done

for p in ios-simulator:iphonesimulator:17.0 tvos-simulator:appletvsimulator:17.0 \
  xros-simulator:xrsimulator:2.0; do
  IFS=: read name sdkname ver <<< "$p"
  sdk=$(xcrun --sdk $sdkname --show-sdk-path 2> /dev/null) || continue
  cc -target arm64-apple-${name%-simulator}$ver-simulator -isysroot $sdk -o $t/$name.o -c $t/a.c
  for kind in -execute -dylib -bundle; do
    $mold -arch arm64 -platform_version $name $ver 27.0 -syslibroot $sdk -o $t/$name.out \
      $kind $t/$name.o -lSystem
    [ -z "$(enc $t/$name.out)" ]
  done
done
