#!/bin/bash
source "$(dirname "$0")"/common.inc

# __objc_imageinfo's 0x10 says the image signs its class_ro_t pointers,
# which only objects with classes (a __DATA,__objc_classlist) have: the
# image's flag is set if every such object's is, and one that differs
# from those before it draws a warning, or with
# -objc_class_ro_signing_mismatch error (or its environment variable,
# which wins) fails the link. An object without classes has no say
# unless it sets the flag: then it counts where no object with classes
# came before it, and draws the warning where one without did.
set_flags() {
  python3 - "$@" <<'EOF'
import sys, macho
path, flags = sys.argv[1], int(sys.argv[2], 0)
m = macho.MachO(path)
m.set_u32(m.section('__objc_imageinfo').offset + 4, flags)
m.save(path)
EOF
}

# Whether an image's flags are the 8 hex digits given.
flags_are() {
  local f=$2
  otool -s __DATA_CONST __objc_imageinfo $1 |
    grep -Eq "00000000 $f|00 00 00 00 ${f:6:2} ${f:4:2} ${f:2:2} ${f:0:2}"
}

# Objects with a class each, A's with main, and without classes.
for name in A B; do
  {
    echo "@interface $name"
    echo '@end'
    echo "@implementation $name"
    echo '@end'
    [ $name = B ] || echo 'int main(void) { return 0; }'
  } | $CC -Wno-objc-root-class -o $t/$name.o -c -xobjective-c -
done
for flags in 0x40 0x50; do
  cp $t/A.o $t/A$flags.o
  set_flags $t/A$flags.o $flags
  cp $t/B.o $t/B$flags.o
  set_flags $t/B$flags.o $flags
  {
    echo '.section __DATA,__objc_imageinfo,regular,no_dead_strip'
    echo '.long 0'
    echo ".long $flags"
  } | $CC -o $t/N$flags.o -c -xassembler -
done
link() { $CC --ld-path=$mold -o $t/exe "$@" -lobjc 2> $t/log; }

link $t/A0x50.o $t/B0x50.o
not grep -q class_ro_t $t/log
flags_are $t/exe 00000050

link $t/A0x50.o $t/B0x40.o
grep -q "'.*B0x40.o' was not built with class_ro_t pointer signing enabled, but previous .o file was\$" \
  $t/log
flags_are $t/exe 00000040
link $t/A0x40.o $t/B0x50.o
grep -q "'.*B0x50.o' was built with class_ro_t pointer signing enabled, but previous .o file was not" \
  $t/log
flags_are $t/exe 00000040

link $t/N0x50.o $t/A0x40.o
not grep -q class_ro_t $t/log
flags_are $t/exe 00000040
link $t/N0x40.o $t/A0x40.o $t/N0x50.o
grep -q "'.*N0x50.o' was built with class_ro_t pointer signing enabled, but previous .o file was not" \
  $t/log
flags_are $t/exe 00000040
link $t/A0x50.o $t/N0x40.o
not grep -q class_ro_t $t/log
flags_are $t/exe 00000050
link $t/N0x40.o $t/A0x40.o $t/N0x40.o
flags_are $t/exe 00000040

not link $t/A0x50.o $t/B0x40.o -Wl,-objc_class_ro_signing_mismatch,error
grep -q "'.*B0x40.o' was not built with class_ro_t pointer signing enabled" $t/log
LD_OBJC_CLASS_RO_SIGNING_MISMATCH=warning link $t/A0x50.o $t/B0x40.o \
  -Wl,-objc_class_ro_signing_mismatch,error
not env LD_OBJC_CLASS_RO_SIGNING_MISMATCH=error \
  $CC --ld-path=$mold -o $t/exe $t/A0x50.o $t/B0x40.o -lobjc \
  -Wl,-objc_class_ro_signing_mismatch,warning 2> /dev/null
