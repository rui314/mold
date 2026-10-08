#!/bin/bash
source "$(dirname "$0")"/common.inc

# A live function keeps its LSDA alive through its unwind record, even
# when its subsection's records are not adjacent in __compact_unwind:
# here _f's record and that of its alt entry _f2 have _g's between
# them. The LSDA index must point at the LSDA, not at a stripped one.
rec() { printf '.quad _%s\n.long 1\n.long %s\n.quad 0\n.quad %s\n' $1 $2 $3; }
{
  cat <<EOF
.text
.globl _main, _f, _g
.no_dead_strip _f
_main:
  ret
_f:
  ret
.alt_entry _f2
_f2:
  ret
_g:
  ret
.section __TEXT,__gcc_except_tab
_lsda_f:
  .long 0
.section __LD,__compact_unwind,regular,debug
.p2align 3
EOF
  rec main 0x02000000 0
  rec f 0x42000000 _lsda_f
  rec g 0x02000000 0
  rec f2 0x02001000 0
  echo .subsections_via_symbols
} | $CC -o $t/a.o -c -xassembler -
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-dead_strip

python3 - $t/exe > $t/lsda <<'EOF'
import struct, sys, macho
m = macho.MachO(sys.argv[1])
d = m.contents(m.section('__unwind_info'))
iso, isc = struct.unpack_from('<2I', d, 20)
start = struct.unpack_from('<I', d, iso + 8)[0]
end = struct.unpack_from('<I', d, iso + 12 * (isc - 1) + 8)[0]
for o in range(start, end, 8):
    print(*[hex(0x100000000 + x) for x in struct.unpack_from('<2I', d, o)])
EOF
addr() { nm $1 | awk -v s=$2 '$3 == s { print "0x" $1 }' | sed 's/0x0*/0x/'; }
[ "$(cat $t/lsda)" = "$(addr $t/exe _f) $(addr $t/exe _lsda_f)" ]
