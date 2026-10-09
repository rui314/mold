#!/bin/bash
source "$(dirname "$0")"/common.inc

# -fixup_chains_section (or -fixup_chains_section_vm) gives a -static or
# -preload image chained fixups whose starts its own loader finds in
# __TEXT,__chain_starts rather than in an LC_DYLD_CHAINED_FIXUPS: the
# pointer format, the number of chains, and each chain's start as an
# offset from the image's address. A chain runs through a segment's
# fixups as far as its 12-bit stride reaches, across pages and
# sections. Its reserved1 says 1 for the first option, 2 for the
# second, which ld-prime refuses to switch between.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _start
.p2align 2
_start:
  ret
.data
.p2align 3
.globl _d0
_d0:
  .quad _start
  .quad _start
  .space 0x3000
  .quad _d0
  .space 0x5000
  .quad _d0
.section __DATA,__const
.p2align 3
  .quad _start
.section __FOO,__bar
.p2align 3
  .quad _d0
EOF

# The words of __TEXT,__chain_starts: the format, the count, and the
# starts as offsets from __DATA's start.
starts() {
  python3 - $1 <<'EOF'
import struct, sys, macho
m = macho.MachO(sys.argv[1])
sect = m.section('__chain_starts')
words = struct.unpack_from('<%dI' % (sect.size // 4), m.data, sect.offset)
base = m.segment('__DATA').vmaddr - m.segment('__TEXT').vmaddr
rest = ['0' if w == 0 else hex(w - base) for w in words[2:]]
print(' '.join([str(words[0]), str(words[1])] + rest))
EOF
}

$mold -arch $ARCH -static -e _start -o $t/exe $t/a.o -fixup_chains_section
otool -hv $t/exe | grep -q PIE
otool -l $t/exe > $t/lc
not grep -q LC_DYLD_CHAINED_FIXUPS $t/lc
if [ $ARCH = arm64 ]; then
  # 0x4000 is beyond the stride from 0x3010; __const and __FOO follow
  # the chain of the last __data fixup and start their own.
  [ "$(starts $t/exe)" = '6 3 0x0 0x8018 0xc000' ]
fi
dyld_info -fixups $t/exe > $t/fixups
[ "$(grep -c rebase $t/fixups)" = 6 ]

grep -A10 'sectname __chain_starts' $t/lc | grep -q 'reserved1 1$'

$mold -arch $ARCH -static -e _start -o $t/exe2 $t/a.o -fixup_chains_section_vm
[ "$(starts $t/exe2)" = "$(starts $t/exe)" ]
otool -l $t/exe2 | grep -A10 'sectname __chain_starts' | grep -q 'reserved1 2$'
not $mold -arch $ARCH -static -e _start -o $t/exe2 $t/a.o -fixup_chains_section_vm \
  -fixup_chains_section 2> $t/log
grep -q -- "-fixup_chains_section can't be used together with other -fixup_chains_section\* options" $t/log

# A later -no_fixup_chains turns it off; a kext and a -kernel image
# ignore it, and -r takes it.
$mold -arch $ARCH -static -e _start -o $t/exe3 $t/a.o -fixup_chains_section -no_fixup_chains
otool -l $t/exe3 > $t/lc3
not grep -q __chain_starts $t/lc3
$mold -arch $ARCH -r -o $t/r.o $t/a.o -fixup_chains_section
$mold -arch $ARCH -kext -o $t/kext $t/a.o -fixup_chains_section
otool -l $t/kext > $t/lc4
not grep -q __chain_starts $t/lc4

# Only an image no dyld loads may have it, nor -rebase_section, which
# only a 32-bit image may have (but kexts and -kernel images ignore).
not $mold -arch $ARCH -dylib -o $t/b.dylib $t/a.o -fixup_chains_section 2> $t/log
grep -q -- "-fixup_chains_section\* can't be used with dynamic binaries" $t/log
not $mold -arch $ARCH -dylib -o $t/b.dylib $t/a.o -rebase_section 2> $t/log
grep -q -- "-rebase_section can't be used with dynamic binaries" $t/log
not $mold -arch $ARCH -static -e _start -o $t/exe4 $t/a.o -rebase_section 2> $t/log
grep -q -- '-rebase_section can only be used on 32-bit architectures' $t/log
not $mold -arch $ARCH -static -e _start -o $t/exe4 $t/a.o -rebase_section \
  -fixup_chains_section 2> $t/log
grep -q -- "-fixup_chains\*, -rebase_section and -threaded_starts_section can't be used together" $t/log
$mold -arch $ARCH -static -kernel -e _start -o $t/exe5 $t/a.o -rebase_section
