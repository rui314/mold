#!/bin/bash
source "$(dirname "$0")"/common.inc

# The last of -execute, -dylib, -bundle, -r, -preload and -kext names
# the kind of output, as in ld64. -static makes a static executable of
# anything but a relocatable object or a kext, and a later -execute
# leaves it static.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main, start
.p2align 2
_main:
start:
  ret
EOF

link() { $mold -arch $ARCH -platform_version macos 14.0 14.0 -syslibroot "$SDK" $t/a.o "$@"; }
kind() { otool -hv $1 | tail -1 | awk '{ print $5 }'; }
entry() { otool -l $1 | grep -Eo 'LC_(MAIN|UNIXTHREAD)$'; }

link -r -dylib -lSystem -o $t/out1
[ "$(kind $t/out1)" = DYLIB ]
link -dylib -r -o $t/out2
[ "$(kind $t/out2)" = OBJECT ]
link -execute -lSystem -o $t/out3
[ "$(entry $t/out3)" = LC_MAIN ]
link -dylib -execute -lSystem -o $t/out4
[ "$(kind $t/out4)" = EXECUTE ]

# -dylib after -static makes a dylib, which dyld loads; -static after
# -dylib a static executable, which it does not.
link -static -dylib -lSystem -o $t/out5
[ "$(kind $t/out5)" = DYLIB ]
otool -l $t/out5 | grep -q LC_ID_DYLIB
link -dylib -static -o $t/out6
[ "$(kind $t/out6)" = EXECUTE ]
[ "$(entry $t/out6)" = LC_UNIXTHREAD ]
link -static -execute -o $t/out7
[ "$(entry $t/out7)" = LC_UNIXTHREAD ]
link -preload -static -o $t/out8
[ "$(kind $t/out8)" = EXECUTE ]

# -static leaves a relocatable object and a kext alone.
link -static -r -o $t/out9
[ "$(kind $t/out9)" = OBJECT ]
link -r -static -o $t/out10
[ "$(kind $t/out10)" = OBJECT ]
link -kext -static -o $t/out11
[ "$(kind $t/out11)" = KEXTBUNDLE ]

# -kernel wants the static executable -static makes, which a kext or a
# -preload image is not.
not link -kext -static -kernel -o $t/out12 2> $t/log12
grep -q -- '-kernel must be used with -static' $t/log12
not link -static -preload -kernel -o $t/out13 2> $t/log13
grep -q -- '-kernel must be used with -static' $t/log13
