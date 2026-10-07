#!/bin/bash
source "$(dirname "$0")"/common.inc

# A segment of zero-fill sections alone has no bytes in the file, in
# any kind of image. (ld-prime records its file offset as 0, as it does
# those sections'; mold leaves it where the segment would start, as for
# a segment with only empty regular sections.)
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
.p2align 2
_main: ret
.zerofill __FOO,__bar,_bar,0x1000,4
.zerofill __DATA,__bss,_bss,0x100,4
.section __QUX,__qux
EOF

seg() {
  otool -l $1 | awk -v s=$2 '$1 == "segname" { found = $2 == s }
    found && $1 == "fileoff" { off = $2 } found && $1 == "filesize" { print off, $2; exit }'
}

for kind in '' -dylib -bundle; do
  $mold -arch $ARCH $kind $t/a.o -platform_version ${PLATFORM_VERSION:-macos 13.0 13.0} -syslibroot $SDK \
    -lSystem -o $t/out
  [ "$(seg $t/out __FOO | cut -d' ' -f2)" = 0 ]
  [ "$(seg $t/out __DATA | cut -d' ' -f2)" = 0 ]
  [ "$(seg $t/out __QUX | cut -d' ' -f1)" != 0 ]
done

$mold -arch $ARCH -static -e _main $t/a.o -platform_version ${PLATFORM_VERSION:-macos 13.0 13.0} -o $t/static
[ "$(seg $t/static __FOO | cut -d' ' -f2)" = 0 ]
$mold -arch $ARCH -preload -e _main $t/a.o -platform_version ${PLATFORM_VERSION:-macos 13.0 13.0} -o $t/preload
[ "$(seg $t/preload __DATA | cut -d' ' -f2)" = 0 ]
