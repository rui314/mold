#!/bin/bash
source "$(dirname "$0")"/common.inc

# A pointer in a segment mapped without write permission that needs a
# fixup (a rebase or a bind) is a text relocation: the loader would
# have to make the segment writable to apply it. ld-prime lists every
# one, output section by section and each subsection's from the last
# to the first, then fails. An unnamed subsection is "anon-N", the
# object's Nth.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _site
.p2align 3
_site: .quad 0
.quad _ext
.globl _site2
_site2: .quad _ext2
.quad _ext + 8
.section __TEXT,__const
.globl _k
.p2align 3
_k: .quad _ext
.section __DATA_CONST,__const
.globl _dc
.p2align 3
_dc: .quad _ext
.subsections_via_symbols
EOF
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _bsite
.p2align 3
_bsite: .quad _ext
.subsections_via_symbols
EOF
cat <<EOF | $CC -o $t/ext.o -c -xc -
int ext = 42, ext2 = 43;
EOF
$CC --ld-path=$mold -shared -o $t/libext.dylib $t/ext.o
cat <<EOF | $CC -o $t/main.o -c -xc -
int main() { return 0; }
EOF

dir=$t
cat > $t/expected <<EOF
Illegal text-relocations:
  text-relocation in '_site'+0x8 ($dir/a.o) to '_ext'
  text-relocation in '_site2'+0x8 ($dir/a.o) to '_ext'
  text-relocation in '_site2' ($dir/a.o) to '_ext2'
  text-relocation in '_bsite' ($dir/b.o) to '_ext'
Illegal text-relocations:
  text-relocation in '_k' ($dir/a.o) to '_ext'
EOF

# Rebases of local pointers and binds of imported ones alike.
not $CC --ld-path=$mold -o $t/exe1 $t/main.o $t/a.o $t/b.o $t/ext.o 2> $t/log1
grep -E '^(Illegal|  text-relocation)' $t/log1 > $t/list1
diff $t/expected $t/list1
grep -q 'Found illegal text-relocations' $t/log1

not $CC --ld-path=$mold -o $t/exe2 $t/main.o $t/a.o $t/b.o $t/libext.dylib 2> $t/log2
grep -E '^(Illegal|  text-relocation)' $t/log2 > $t/list2
diff $t/expected $t/list2

# Encoding rebase opcodes (without chained fixups), ld-prime lists
# each subsection's in address order, and the first section's only.
cat > $t/expected5 <<EOF
Illegal text-relocations:
  text-relocation in '_site'+0x8 ($dir/a.o) to '_ext'
  text-relocation in '_site2' ($dir/a.o) to '_ext2'
  text-relocation in '_site2'+0x8 ($dir/a.o) to '_ext'
  text-relocation in '_bsite' ($dir/b.o) to '_ext'
EOF
not $CC --ld-path=$mold -o $t/exe5 $t/main.o $t/a.o $t/b.o $t/ext.o -Wl,-no_fixup_chains \
  2> $t/log5
grep -E '^(Illegal|  text-relocation)' $t/log5 > $t/list5
diff $t/expected5 $t/list5

not $CC --ld-path=$mold -shared -o $t/c.dylib $t/b.o $t/ext.o 2> $t/log3
grep -qF "  text-relocation in '_bsite' ($dir/b.o) to '_ext'" $t/log3

# A section-relative pointer names the subsection it points into.
cat <<EOF | $CC -o $t/d.o -c -xassembler -
.text
.globl _d
.p2align 3
_d: .quad L1
.data
.globl _d1
_d1: .quad 0
L1: .quad 0
.subsections_via_symbols
EOF
not $CC --ld-path=$mold -o $t/exe4 $t/main.o $t/d.o 2> $t/log4
grep -qF "  text-relocation in '_d' ($dir/d.o) to '_d1'" $t/log4

# A literal's linker-private label names no subsection (ld64 ignores
# it), so a pointer to it names the literal as the object's Nth
# subsection.
cat <<EOF | $CC -o $t/g.o -c -xassembler -
.text
.globl _g
.p2align 3
_g: .quad lCPI0_0
.literal8
.p2align 3
lCPI0_0: .quad 7
.subsections_via_symbols
EOF
not $CC --ld-path=$mold -o $t/exe17 $t/main.o $t/g.o 2> $t/log17
grep -qF "  text-relocation in '_g' ($dir/g.o) to 'anon-1'" $t/log17

# A segment -segprot makes read-only counts too.
cat <<EOF | $CC -o $t/e.o -c -xassembler -
.section __RO,__ptrs
.globl _ro
.p2align 3
_ro: .quad _ext
.subsections_via_symbols
EOF
$CC --ld-path=$mold -o $t/exe5 $t/main.o $t/e.o $t/ext.o
not $CC --ld-path=$mold -o $t/exe6 $t/main.o $t/e.o $t/ext.o -Wl,-segprot,__RO,r,r 2> $t/log6
grep -qF "  text-relocation in '_ro' ($dir/e.o) to '_ext'" $t/log6

# An image nothing slides has no fixups.
$mold -arch $ARCH -static -e _bsite -o $t/exe7 $t/b.o $t/ext.o

# -read_only_relocs warning or suppress allows them in firmware and in
# an image no dyld loads; error refuses them. Elsewhere ld-prime
# ignores the option with a warning.
not $mold -arch $ARCH -static -pie -e _bsite -o $t/exe8 $t/b.o $t/ext.o 2> $t/log8
grep -q 'Found illegal text-relocations' $t/log8
$mold -arch $ARCH -static -pie -e _bsite -o $t/exe9 $t/b.o $t/ext.o -read_only_relocs suppress
$mold -arch $ARCH -static -pie -e _bsite -o $t/exe10 $t/b.o $t/ext.o -read_only_relocs warning
not $mold -arch $ARCH -static -pie -e _bsite -o $t/exe11 $t/b.o $t/ext.o -read_only_relocs error

not $CC --ld-path=$mold -o $t/exe12 $t/main.o $t/b.o $t/ext.o \
  -Wl,-read_only_relocs,suppress 2> $t/log12
grep -q -- '-read_only_relocs relocs cannot be used in this configuration' $t/log12
grep -q 'Found illegal text-relocations' $t/log12

# ld-prime allows them by default in an x86-64 kext and non-PIE
# executable, not in an arm64 kext, where it also ignores the option.
if [ $ARCH = x86_64 ]; then
  # Subsections count in address order, each C string one (arm64
  # objects name a section's first subsection by an ltmp symbol
  # instead).
  cat <<EOF | $CC -o $t/f.o -c -xassembler -
.text
.p2align 3
.quad 0
.quad L5
.globl _f
_f: ret
.cstring
.asciz "a"
.asciz "b"
.data
.quad 0
L5: .quad 0
.subsections_via_symbols
EOF
  not $CC --ld-path=$mold -o $t/exe16 $t/main.o $t/f.o 2> $t/log16
  grep -qF "  text-relocation in 'anon-0'+0x8 ($dir/f.o) to 'anon-4'" $t/log16

  $mold -arch x86_64 -kext -o $t/kext1 $t/b.o
  not $mold -arch x86_64 -kext -o $t/kext2 $t/b.o -read_only_relocs error 2> $t/log14
  grep -q 'Found illegal text-relocations' $t/log14
  $CC --ld-path=$mold -o $t/exe15 $t/main.o $t/b.o $t/libext.dylib -Wl,-no_pie 2> /dev/null
else
  not $mold -arch arm64 -kext -o $t/kext1 $t/b.o -read_only_relocs suppress 2> $t/log13
  grep -q -- '-read_only_relocs relocs cannot be used in this configuration' $t/log13
  grep -q 'Found illegal text-relocations' $t/log13
fi
