#!/bin/bash
source "$(dirname "$0")"/common.inc

# Segment and section names are bytes, not text: ld-prime takes any
# but NUL from an input object, UTF-8 or not, writes them into the
# output's headers as they came, in a final image and a -r output
# alike, and prints them as they are in -map and -trace_symbol_layout.
# The assembler takes no such names, so they are patched in.
cat <<EOF | $CC -o $t/a0.o -c -xassembler -
.section __DATA,__dQQQ
.globl _x
.p2align 3
_x: .quad 42
.section __SQQQ,__s
.globl _y
.p2align 3
_y: .quad _x
EOF
python3 - $t/a0.o $t/a.o <<'EOF'
import sys
data = open(sys.argv[1], 'rb').read()
data = data.replace(b'__dQQQ', b'__d\xff\xfe\x80').replace(b'__SQQQ', b'__S\xc3(X')
open(sys.argv[2], 'wb').write(data)
EOF
cat <<EOF | $CC -o $t/main.o -c -xc -
extern long x, *y;
int main() { return *y != 42 || x != 42; }
EOF

sect=$'__d\xff\xfe\x80'
seg=$'__S\xc3(X'

$CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o -Wl,-map,$t/map \
  -Wl,-trace_symbol_layout > $t/trace
$RUN $t/exe
otool -l $t/exe > $t/lc
grep -aqx "  sectname $sect" $t/lc
grep -aqx "   segname $seg" $t/lc
grep -aq $'\t__DATA\t'"$sect"'$' $t/map
grep -aq $'\t'"$seg"$'\t__s$' $t/map
grep -aqx "symbol '_x', mapped to __DATA/$sect" $t/trace
grep -aqx "symbol '_y', mapped to $seg/__s" $t/trace

$mold -r -arch $ARCH -o $t/r.o $t/a.o
otool -l $t/r.o > $t/lc-r
grep -aqx "  sectname $sect" $t/lc-r
grep -aqx "   segname $seg" $t/lc-r
