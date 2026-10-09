#!/bin/bash
source "$(dirname "$0")"/common.inc

# -dylinker links dyld itself (MH_DYLINKER). The kernel maps it next to
# a main executable, anywhere, and starts it at "start" from
# LC_UNIXTHREAD; dyld then slides itself by its own fixups, rebases
# alone, before it loads anything. So it has no __PAGEZERO, __TEXT at
# 0, chained rebases and an export trie as a dylib does, and it names
# itself in LC_ID_DYLINKER. It is bound for the dyld shared cache: it
# records split info, and its __DATA_CONST is SG_READ_ONLY. Its __text
# starts on a 4 KiB boundary, right after the load commands whatever
# -headerpad says.
cat <<EOF | $CC -o $t/a.o -c -xc -O1 -
int counter = 3;
int *ptr = &counter;
int *const cptr = &counter;
__attribute__((weak, noinline)) int weak_fn(int x) { return x + 1; }
int exported_fn(int x) { return x + *ptr + weak_fn(x); }
void start(void) __asm__("start");
void start(void) { for (;;) exported_fn(*cptr); }
EOF

$mold -arch $ARCH -dylinker $t/a.o -o $t/dyld
otool -hv $t/dyld > $t/hdr
grep -Eq ' DYLINKER +[0-9]+ +[0-9]+ +NOUNDEFS DYLDLINK TWOLEVEL WEAK_DEFINES BINDS_TO_WEAK$' $t/hdr

otool -l $t/dyld > $t/lc
cmds() { awk '$1 == "cmd" { printf "%s ", $2 }' $1; }
expected='LC_SEGMENT_64 LC_SEGMENT_64 LC_SEGMENT_64 LC_SEGMENT_64 LC_DYLD_CHAINED_FIXUPS LC_DYLD_EXPORTS_TRIE LC_SYMTAB LC_DYSYMTAB LC_ID_DYLINKER LC_UUID LC_BUILD_VERSION LC_SOURCE_VERSION LC_UNIXTHREAD LC_SEGMENT_SPLIT_INFO LC_FUNCTION_STARTS LC_DATA_IN_CODE '
[ $ARCH = arm64 ] && expected="${expected}LC_CODE_SIGNATURE "
[ "$(cmds $t/lc)" = "$expected" ]
grep -A2 LC_ID_DYLINKER $t/lc | grep -q 'name /usr/lib/dyld '

segs() { awk '$1 == "segname" && !seen[$2]++ { printf "%s ", $2 }' $1; }
[ "$(segs $t/lc)" = '__TEXT __DATA_CONST __DATA __LINKEDIT ' ]
grep -A2 'segname __TEXT$' $t/lc | grep -q 'vmaddr 0x0000000000000000$'
grep -A10 'segname __DATA_CONST$' $t/lc | grep -q 'flags 0x10$'
text() { otool -l $1 | grep -A5 'sectname __text' | awk '$1 == "addr" || $1 == "align" { printf "%s ", $2 }'; }
[ "$(text $t/dyld)" = '0x0000000000001000 2^12 ' ]

# The thread starts at "start".
addr() { nm $1 | awk -v s=$2 '$3 == s { print $1 }'; }
pc() { otool -l $1 | awk '{ for (i = 1; i < NF; i++) if ($i == "pc" || $i == "rip") print $(i + 1) }'; }
[ "$(pc $t/dyld)" = "0x$(addr $t/dyld start)" ]

# Every pointer is a rebase in a fixup chain whose targets count from
# the image's start; there is no import to bind, and a call to a weak
# definition goes straight to it.
dyld_info -fixups -exports -fixup_chains $t/dyld > $t/info
grep -q 'pointer_format:  6 (DYLD_CHAINED_PTR_64_OFFSET)' $t/info
grep -q 'imports_count:    0' $t/info
[ "$(grep -c ' rebase ' $t/info)" = 2 ]
not grep -q ' bind ' $t/info
not grep -Eq 'sectname __(stubs|got)$' $t/lc
grep -Eq '0x0*[0-9A-F]+  _exported_fn$' $t/info
grep -Eq '0x0*[0-9A-F]+  _weak_fn \[weak-def\]$' $t/info
grep -Eq '0x0*[0-9A-F]+  start$' $t/info

# -headerpad leaves __text where it is, and LC_ID_DYLINKER names
# /usr/lib/dyld whatever -install_name or -dylinker_install_name says.
$mold -arch $ARCH -dylinker $t/a.o -o $t/dyld2 -headerpad 0x2000 \
  -install_name /foo -dylinker_install_name /bar
[ "$(text $t/dyld2)" = '0x0000000000001000 2^12 ' ]
otool -l $t/dyld2 | grep -A2 LC_ID_DYLINKER | grep -q 'name /usr/lib/dyld '

# -e names another entry point. -dead_strip keeps every export.
$mold -arch $ARCH -dylinker $t/a.o -o $t/dyld3 -e _exported_fn -dead_strip
[ "$(pc $t/dyld3)" = "0x$(addr $t/dyld3 _exported_fn)" ]
nm $t/dyld3 | grep -q ' start$'

# "start" must be defined.
cat <<EOF | $CC -o $t/b.o -c -xc -
int foo(void) { return 1; }
EOF
not $mold -arch $ARCH -dylinker $t/b.o -o $t/dyld4 2> $t/log4
grep -q start $t/log4

# dyld is what loads dylibs: -l looks for archives only, and a dylib
# named outright is ignored with a warning.
rm -f $t/libfoo.a
ar rcs $t/libfoo.a $t/b.o
$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/b.o
$CC --ld-path=$mold -shared -o $t/libbar.dylib $t/b.o
$mold -arch $ARCH -dylinker $t/a.o -L$t -lfoo -o $t/dyld5 -u _foo
nm -m $t/dyld5 | grep -q '(__TEXT,__text) external _foo'
not $mold -arch $ARCH -dylinker $t/a.o -L$t -lbar -o $t/dyld6 2> $t/log6
grep -q bar $t/log6
$mold -arch $ARCH -dylinker $t/a.o $t/libbar.dylib -o $t/dyld7 2> $t/log7
grep -q "ignoring unexpected dylib '.*libbar.dylib'" $t/log7
not grep -q LC_LOAD_DYLIB <(otool -l $t/dyld7)

# A main executable's options are refused, and dyld, bound for the
# shared cache, may look nothing up dynamically.
not $mold -arch $ARCH -dylinker $t/a.o -o $t/dyld8 -pie 2> $t/log8
grep -q -- '-pie can only be used when linking a main executable' $t/log8
not $mold -arch $ARCH -dylinker $t/a.o -o $t/dyld8 -client_name x 2> $t/log8
grep -q -- '-client_name can only be used when creating a bundle or main executable' $t/log8
not $mold -arch $ARCH -dylinker $t/a.o -o $t/dyld8 -pagezero_size 0x4000 2> $t/log8
grep -q -- '-pagezero_size can only be used when linking a main executable' $t/log8
not $mold -arch $ARCH -dylinker $t/a.o -o $t/dyld8 -undefined dynamic_lookup 2> $t/log8
grep -q "Shared cache eligible dylibs cannot use '-undefined dynamic_lookup'" $t/log8

# Not so its segments' order, but -section_order may order its
# sections (after __text).
$mold -arch $ARCH -dylinker $t/a.o -o $t/dyld9 -section_order __TEXT __unwind_info
sects() { awk '$1 == "sectname" { s = $2 } $1 == "segname" && s { if ($2 == "__TEXT") printf "%s ", s; s = "" }' $1; }
[ "$(sects <(otool -l $t/dyld9))" = '__text __unwind_info ' ]
not $mold -arch $ARCH -dylinker $t/a.o -o $t/dyld9 -segment_order __TEXT:__DATA 2> $t/log9
grep -q -- '-segment_order can only be used with' $t/log9

# -not_for_dyld_shared_cache drops the split info.
$mold -arch $ARCH -dylinker $t/a.o -o $t/dyld10 -not_for_dyld_shared_cache
not grep -q LC_SEGMENT_SPLIT_INFO <(otool -l $t/dyld10)

# The last of -dylinker, -r and the other kinds wins; -static makes a
# static executable only after -dylinker.
$mold -arch $ARCH -r -dylinker $t/a.o -o $t/dyld11
otool -hv $t/dyld11 | grep -q ' DYLINKER '
$mold -arch $ARCH -static -dylinker $t/a.o -o $t/dyld12
otool -hv $t/dyld12 | grep -q ' DYLINKER '
$mold -arch $ARCH -dylinker -static $t/a.o -o $t/dyld13
otool -hv $t/dyld13 | grep -q ' EXECUTE '
$mold -arch $ARCH -dylinker -r $t/a.o -o $t/dyld14.o
otool -hv $t/dyld14.o | grep -q ' OBJECT '
