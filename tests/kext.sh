#!/bin/bash
source "$(dirname "$0")"/common.inc

# -kext links a kernel extension (MH_KEXT_BUNDLE), which kmutil, not
# dyld, links into the kernel: its undefined symbols are looked up
# among the kernel's exports, and its relocations (LC_DYSYMTAB's) take
# the place of dyld's fixups - external ones where an import's address
# goes, local ones for the pointers that slide. On arm64 its code gets
# a __TEXT_EXEC segment of its own, its read-only data __DATA_CONST
# after __DATA, and it records split info; calls to imports and to its
# weak definitions go through stubs and __got / __weak_got. On x86-64
# it calls imports directly.
cat <<EOF | $CC -o $t/a.o -c -xc -O1 -mkernel -
extern int IOLog(const char *fmt, ...);
extern int kernel_var;
int local_data = 5;
int *local_ptr = &local_data;
int *ext_ptr = &kernel_var;
__attribute__((weak, noinline)) int weakfn(int x) { return x + 1; }
int kext_start(void) { IOLog("x", *ext_ptr); return weakfn(*local_ptr + kernel_var); }
EOF
$mold -arch $ARCH -kext $t/a.o -o $t/kext
otool -hv $t/kext > $t/hdr
grep -q 'KEXTBUNDLE .* NOUNDEFS DYLDLINK TWOLEVEL WEAK_DEFINES BINDS_TO_WEAK' $t/hdr
otool -l $t/kext > $t/lc
awk '$1 == "cmd" { printf "%s ", $2 }' $t/lc > $t/cmds
for cmd in LC_DYLD_INFO_ONLY LC_DYLD_CHAINED_FIXUPS LC_BUILD_VERSION LC_FUNCTION_STARTS \
  LC_DATA_IN_CODE LC_CODE_SIGNATURE LC_ID_DYLIB; do
  not grep -q "$cmd " $t/cmds
done
grep -q 'LC_SYMTAB LC_DYSYMTAB LC_UUID LC_SOURCE_VERSION' $t/cmds
nm -m $t/kext > $t/nm
grep -q '(undefined) external _IOLog (dynamically looked up)' $t/nm
grep -q '(undefined) external _kernel_var (dynamically looked up)' $t/nm

# The relocations count from the image's start (0). kernel_var's GOT
# slot and ext_ptr are external 8-byte UNSIGNED relocations; local_ptr
# is a local one naming the section it points into (__data), and holds
# local_data's address.
a() { printf '%08x' 0x$(nm $t/kext | awk -v s=$1 '$3 == s { print $1 }'); }
otool -r $t/kext > $t/rel
sym() { nm -p $t/kext | awk -v s=$1 '$NF == s { print NR - 1 }'; }
grep -q "^$(a _ext_ptr) 0 *3 *1 *0 *0 *$(sym _kernel_var)\$" $t/rel
data=$(otool -l $t/kext | awk '$1 == "sectname" { n++ } $1 == "sectname" && $2 == "__data" { print n }')
grep -q "^$(a _local_ptr) 0 *3 *0 *0 *0 *$data\$" $t/rel
sect() { otool -l $t/kext | awk -v s=__data -v f=$1 '$1 == "sectname" { n = $2 } n == s && $1 == f { print $2; exit }'; }
off=$(( $(sect offset) + 0x$(a _local_ptr) - $(sect addr) ))
[ $((0x$(od -An -tx8 -j $off -N8 $t/kext | tr -d ' '))) = $((0x$(a _local_data))) ]

if [ $ARCH = arm64 ]; then
  segs=$(awk '$1 == "segname" && !seen[$2]++ { printf "%s ", $2 }' $t/lc)
  [ "$segs" = '__TEXT __TEXT_EXEC __DATA __DATA_CONST __LINKEDIT ' ]
  grep -A8 'segname __TEXT$' $t/lc | grep -q 'maxprot 0x00000001'
  grep -A8 'segname __TEXT_EXEC' $t/lc | grep -q 'maxprot 0x00000005'
  grep -A10 'segname __DATA_CONST' $t/lc | grep -q 'flags 0x0$'
  grep -q 'LC_SOURCE_VERSION LC_SEGMENT_SPLIT_INFO $' $t/cmds
  grep -A1 'sectname __stubs' $t/lc | grep -q __TEXT_EXEC
  grep -A10 'sectname __got' $t/lc | grep -q 'flags 0x00000000'
  otool -Iv $t/kext > $t/isyms
  grep -q 'Indirect symbols for (__TEXT_EXEC,__stubs) 2 entries' $t/isyms
  grep -A1 'sectname __weak_got' $t/lc | grep -q __DATA_CONST

  $mold -arch $ARCH -kext -not_for_dyld_shared_cache $t/a.o -o $t/kext2
  otool -l $t/kext2 > $t/lc2
  not grep -q 'LC_SEGMENT_SPLIT_INFO' $t/lc2
  not grep -q 'segname __DATA_CONST' $t/lc2
else
  # A call to an import is a 4-byte pc-relative external BRANCH.
  grep -Eq "^[0-9a-f]+ 1 +2 +1 +2 +0 +$(sym _IOLog)\$" $t/rel
  not grep -q 'sectname __stubs' $t/lc
  not grep -q 'LC_SEGMENT_SPLIT_INFO' $t/lc
  secs=$(awk '$1 == "sectname" { s = $2 } $1 == "segname" && s { if ($2 == "__DATA") printf "%s ", s; s = "" }' $t/lc)
  [ "$secs" = '__data __got ' ]
fi

# A kext without relocations of a kind has offset 0 for their table.
cat <<EOF2 | $CC -o $t/b.o -c -xc -O1 -mkernel -
int kext_start(void) { return 0; }
EOF2
$mold -arch $ARCH -kext $t/b.o -o $t/kext3
otool -l $t/kext3 | grep -A18 LC_DYSYMTAB > $t/dysymtab
grep -Eq '^ +locreloff 0$' $t/dysymtab
grep -Eq '^ +extreloff 0$' $t/dysymtab

# No dyld maps a kext: a -segalign below a section's alignment leaves
# the alignment be, without a warning, and the section's segment starts
# on it.
$mold -arch $ARCH -kext -segalign 0x1 $t/a.o -o $t/kext4 2> $t/log4
not grep -q 'reducing alignment' $t/log4
otool -l $t/kext4 | awk '$1 == "sectname" { s = $2 } s == "__data" && $1 == "addr" { a = $2 }
  s == "__data" && $1 == "align" { print a, $2; exit }' > $t/data4
read addr align < $t/data4
[ $align = '2^3' ]
[ $((addr % 8)) = 0 ]

# Its constructors stay pointers in __mod_init_func, which the kernel
# runs: ld-prime makes them __init_offsets only with -init_offsets.
cat <<EOF | $CC -o $t/c.o -c -xc -mkernel -
__attribute__((constructor)) static void init(void) {}
int kext_start(void) { return 0; }
EOF
$mold -arch $ARCH -kext $t/c.o -o $t/kext5
otool -l $t/kext5 > $t/lc5
grep -q 'sectname __mod_init_func' $t/lc5
not grep -q 'sectname __init_offsets' $t/lc5
$mold -arch $ARCH -kext -init_offsets $t/c.o -o $t/kext6
otool -l $t/kext6 > $t/lc6
grep -q 'sectname __init_offsets' $t/lc6
