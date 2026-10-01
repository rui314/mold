#!/bin/bash
source "$(dirname "$0")"/common.inc

# -move_to_rw_segment and -move_to_ro_segment move the atoms of the
# symbols a list file names to another segment, each into the section
# of its name there; -dirty_data_list moves data to __DATA_DIRTY, which
# follows __DATA. The rw option leaves code (and the strings and
# literals that go with it) where it is, the ro one data written at run
# time, with a warning for each symbol named (not matched by a pattern),
# and the first list naming a symbol decides where it goes.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int data1 = 5;
int data2 = 6;
int bss1;
static int sdata = 7;
const int const1 = 9;
int func1(int x) { return x + data1 + sdata; }
int func2(int x) { return x * 2; }
int main() {
  data1++;
  sdata++;
  bss1 = 3;
  printf("%d %d %d %d\n", func1(1), func2(3), data2 + bss1, const1);
}
EOF

segs() {
  otool -l $1 | awk '$1 == "cmd" { c = $2; n = 0 }
    c == "LC_SEGMENT_64" && $1 == "segname" && n++ == 0 { printf "%s ", $2 }'
}
prot() {
  otool -l $1 | awk -v s=$2 '$1 == "segname" { seg = $2 }
    seg == s && $1 == "maxprot" { m = $2 } seg == s && $1 == "initprot" { print m + 0, $2 + 0; exit }'
}

printf '_data1\n_sdata\n_bss1\n_const1\n_func1\n# a comment\n' > $t/rw.txt
$CC --ld-path=$mold -o $t/exe1 $t/a.o -Wl,-move_to_rw_segment,__FOO,$t/rw.txt 2> $t/log1
grep -q "warning: cannot move symbol '_func1' (.*/a.o) to segment '__FOO' because symbol is code (is function)" $t/log1
[ "$(grep -c warning $t/log1)" = 1 ]
nm -m $t/exe1 > $t/nm1
grep -q '(__FOO,__data) external _data1$' $t/nm1
grep -q '(__FOO,__data) non-external _sdata$' $t/nm1
grep -q '(__FOO,__common) external _bss1$' $t/nm1
grep -q '(__FOO,__const) external _const1$' $t/nm1
grep -q '(__TEXT,__text) external _func1$' $t/nm1
grep -q '(__DATA,__data) external _data2$' $t/nm1
segs $t/exe1 | grep -q '__DATA __FOO __LINKEDIT'
[ "$(prot $t/exe1 __FOO)" = '3 3' ]
[ "$($t/exe1)" = '15 6 9 9' ]

# ld-prime gives the segment of moved code the read-write protection of
# any other one it doesn't know, where the code can't run (a Bus error);
# mold makes it executable (and read-only). Under ld-prime, the
# protection check and the run below fail.
printf '_func1\n_func2\n_const1\n_data1\n' > $t/ro.txt
$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-move_to_ro_segment,__BAR,$t/ro.txt 2> $t/log2
grep -q "warning: cannot move symbol '_data1' (.*/a.o) to segment '__BAR' because symbol is not code (is data)" $t/log2
nm -m $t/exe2 > $t/nm2
grep -q '(__BAR,__text) external _func1$' $t/nm2
grep -q '(__BAR,__text) external _func2$' $t/nm2
grep -q '(__BAR,__const) external _const1$' $t/nm2
grep -q '(__DATA,__data) external _data1$' $t/nm2
[ "$(prot $t/exe2 __BAR)" = '5 5' ]
[ "$($t/exe2)" = '15 6 9 9' ]

printf '_data2\n_bss1\n_func2\n' > $t/dirty.txt
$CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-dirty_data_list,$t/dirty.txt 2> $t/log3
not grep -q warning $t/log3
nm -m $t/exe3 > $t/nm3
grep -q '(__DATA_DIRTY,__data) external _data2$' $t/nm3
grep -q '(__DATA_DIRTY,__common) external _bss1$' $t/nm3
grep -q '(__TEXT,__text) external _func2$' $t/nm3
segs $t/exe3 | grep -q '__DATA __DATA_DIRTY __LINKEDIT'
[ "$($t/exe3)" = '15 6 9 9' ]

# A pattern draws no warning; file:name names a symbol of that object
# alone; the first list naming a symbol wins, and -move_to_rw_segment's
# over the other options'.
printf '_func*\n_d*a1\na.o:_data2\nb.o:_const1\n' > $t/pat.txt
printf '_data1\n_data2\n' > $t/second.txt
$CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-move_to_rw_segment,__FOO,$t/pat.txt \
  -Wl,-move_to_rw_segment,__BAZ,$t/second.txt -Wl,-dirty_data_list,$t/second.txt 2> $t/log4
not grep -q warning $t/log4
nm -m $t/exe4 > $t/nm4
grep -q '(__FOO,__data) external _data1$' $t/nm4
grep -q '(__FOO,__data) external _data2$' $t/nm4
grep -q '(__TEXT,__const) external _const1$' $t/nm4
grep -q '(__TEXT,__text) external _func1$' $t/nm4
not grep -q '__BAZ\|__DATA_DIRTY' $t/nm4
[ "$($t/exe4)" = '15 6 9 9' ]

# Moved data with pointers to rebase or bind, in a dylib.
cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
static int value = 42;
int *ptr = &value;
int (*put)(const char *) = puts;
int get(void) { return *ptr; }
EOF
cat <<EOF | $CC -o $t/c.o -c -xc -
int get(void);
extern int (*put)(const char *);
int main() { put("hello"); return get() != 42; }
EOF
printf '_ptr\n_put\n_value\n' > $t/b.txt
$CC --ld-path=$mold -shared -o $t/libb.dylib $t/b.o -Wl,-move_to_rw_segment,__FOO,$t/b.txt
nm -m $t/libb.dylib | grep -q '(__FOO,__data) external _ptr$'
$CC --ld-path=$mold -o $t/exe5 $t/c.o $t/libb.dylib
[ "$($t/exe5)" = hello ]

# The warnings follow ld-prime's walk over the atoms: file by file the
# atoms of an object's sections, common symbols and absolute symbols,
# then those of the thread-local variables' descriptors it makes.
cat <<EOF | $CC -o $t/e.o -c -xassembler -
.data
.globl _f1
_f1: .quad 1
.comm _c1,8,3
.globl _absf
_absf = 7
.subsections_via_symbols
EOF
cat <<EOF | $CC -o $t/f.o -c -xc -
__thread int tv1 = 1;
int e1 = 2;
int *get(void) { return &tv1; }
int main() { return 0; }
EOF
printf '_tv1\n_e1\n_absf\n_c1\n_f1\n' > $t/order.txt
$CC --ld-path=$mold -o $t/exe10 $t/e.o $t/f.o -Wl,-move_to_ro_segment,__BAR,$t/order.txt 2> $t/log10
sed -n "s/.*cannot move symbol '\([^']*\)'.*/\1/p" $t/log10 | tr '\n' ' ' > $t/order10
[ "$(cat $t/order10)" = '_f1 _c1 _absf _e1 _tv1 ' ]

# The Objective-C records the linker rewrites move as the input's would:
# the class data category merging rebuilt, and the method lists in the
# relative form, which ld-prime makes in its own objc-file and counts
# as code - -move_to_rw_segment leaves them with a warning, and
# -move_to_ro_segment takes them to an __objc_methlist of its segment.
cat <<EOF | $CC -o $t/d.o -c -xobjective-c -
#import <Foundation/Foundation.h>
#include <stdio.h>
@interface A : NSObject
- (int)a;
@end
@implementation A
- (int)a { return 1; }
+ (int)ca { return 3; }
@end
@interface A (Cat)
- (int)b;
@end
@implementation A (Cat)
- (int)b { return 2; }
@end
int main() {
  A *a = [A new];
  printf("%d %d %d\n", [a a], [a b], [A ca]);
}
EOF
printf '__OBJC_CLASS_RO_$_A\n__OBJC_$_CLASS_METHODS_A\n' > $t/objc.txt
$CC --ld-path=$mold -o $t/exe8 $t/d.o -framework Foundation -Wl,-objc_relative_method_lists \
  -Wl,-move_to_rw_segment,__FOO,$t/objc.txt 2> $t/log8
grep -q "warning: cannot move symbol '__OBJC_\$_CLASS_METHODS_A' (objc-file) to segment '__FOO' because symbol is code (is objc-method-list)" $t/log8
nm -m $t/exe8 > $t/nm8
grep -q '(__FOO,__objc_const) non-external __OBJC_CLASS_RO_\$_A$' $t/nm8
grep -q '(__TEXT,__objc_methlist) non-external __OBJC_\$_CLASS_METHODS_A$' $t/nm8
[ "$($t/exe8)" = '1 2 3' ]
$CC --ld-path=$mold -o $t/exe9 $t/d.o -framework Foundation -Wl,-objc_relative_method_lists \
  -Wl,-move_to_ro_segment,__FOO,$t/objc.txt -Wl,-trace_symbol_layout > $t/trace9 2> $t/log9
grep -q "warning: cannot move symbol '__OBJC_CLASS_RO_\$_A' (.*/d.o) to segment '__FOO' because symbol is not code (is objc-const)" $t/log9
nm -m $t/exe9 > $t/nm9
grep -q '(__FOO,__objc_methlist) non-external __OBJC_\$_CLASS_METHODS_A$' $t/nm9
grep -q "^symbol '__OBJC_\$_CLASS_METHODS_A', -move_to_ro_segment mapped it to __FOO/__objc_methlist$" $t/trace9
[ "$($t/exe9)" = '1 2 3' ]

# Only a final link lays out segments: ld-prime refuses the two options
# in a -r link (-move_to_rw_segment's first), and ignores
# -dirty_data_list there.
not $mold -r -arch $ARCH -o $t/r.o $t/a.o -move_to_ro_segment __BAR $t/ro.txt \
  -move_to_rw_segment __FOO $t/rw.txt 2> $t/log6
grep -q -- '-move_to_rw_segment not supported with -r' $t/log6
not $mold -r -arch $ARCH -o $t/r.o $t/a.o -move_to_ro_segment __BAR $t/ro.txt 2> $t/log6
grep -q -- '-move_to_ro_segment not supported with -r' $t/log6
$mold -r -arch $ARCH -o $t/r.o $t/a.o -dirty_data_list $t/dirty.txt
not grep -q __DATA_DIRTY <(otool -l $t/r.o)

not $mold -arch $ARCH -o $t/exe7 $t/a.o -move_to_rw_segment __FOO 2> $t/log7
grep -q -- '-move_to_rw_segment <segname> <path>' $t/log7
not $mold -arch $ARCH -o $t/exe7 $t/a.o -move_to_ro_segment '' $t/ro.txt 2> $t/log7
grep -q -- '-move_to_ro_segment <segname> <path>' $t/log7
not $mold -arch $ARCH -o $t/exe7 $t/a.o -dirty_data_list 2> $t/log7
grep -q -- '-dirty_data_list missing <path>' $t/log7
not $mold -arch $ARCH -o $t/exe7 $t/a.o -move_to_rw_segment __FOO $t/none.txt 2> $t/log7
grep -q -- "-move_to_rw_segment file '$t/none.txt' could not be opened, errno=2" $t/log7
# ld-prime names no option for a -dirty_data_list file.
not $mold -arch $ARCH -o $t/exe7 $t/a.o -dirty_data_list $t/none.txt 2> $t/log7
grep -q -- ":  file '$t/none.txt' could not be opened, errno=2" $t/log7
