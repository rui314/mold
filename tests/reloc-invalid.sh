#!/bin/bash
source "$(dirname "$0")"/common.inc

# A relocation the assembler writes for valid-looking source but that
# the linker can't apply fails the link, -r too: a field size or
# pc-relativeness its type doesn't take, an arm64 32-bit pointer, or
# the page offset of an arm64 GOT load on anything but an add or an
# 8-byte load, or of a TLV load on anything but a load.
echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -
cat <<EOF | $CC -o $t/x.o -c -xassembler -
.data
.globl _x, _y
.p2align 3
_x: .quad 0
_y: .quad 0
EOF

# Assembles the lines given and checks that the object fails a final
# link and -r. (A function doesn't inherit the ERR trap, so its steps
# are chained.)
check() {
  printf '%s\n' "$@" | $CC -o $t/a.o -c -xassembler - &&
    not $CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o $t/x.o 2> /dev/null &&
    not $mold -r -arch $ARCH -o $t/r.o $t/a.o 2> /dev/null
}

check .data '.short _x'
check .data '.byte _x'

if [ $ARCH = arm64 ]; then
  check .data '.long _x'
  check .data '.quad _x@GOT'
  check .data '.quad _x@GOT - .'
  check .data '_z: .short _x - _z'
  check .text 'adrp x0, _x@GOTPAGE' 'ldr w0, [x0, _x@GOTPAGEOFF]'
  check .text 'adrp x0, _x@GOTPAGE' 'str x1, [x0, _x@GOTPAGEOFF]'
  check .text 'adrp x0, _x@TLVPPAGE' 'add x0, x0, _x@TLVPPAGEOFF'

  # The forms they take link.
  printf '%s\n' .data '.long _x - _y' '.long _x@GOT - .' .text \
    'adrp x0, _x@GOTPAGE' 'ldr x0, [x0, _x@GOTPAGEOFF]' |
    $CC -o $t/b.o -c -xassembler -
  $CC --ld-path=$mold -o $t/exe $t/main.o $t/b.o $t/x.o
  $mold -r -arch $ARCH -o $t/r.o $t/b.o
else
  check .data '.quad _x@GOTPCREL'

  printf '%s\n' .data '.long _x@GOTPCREL' '.quad _x' |
    $CC -o $t/b.o -c -xassembler -
  $CC --ld-path=$mold -o $t/exe $t/main.o $t/b.o $t/x.o
  $mold -r -arch $ARCH -o $t/r.o $t/b.o
fi
