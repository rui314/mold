#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime splits the Objective-C constant literals (__objc_intobj,
# __objc_doubleobj, __objc_arraydata, __objc_arrayobj, __objc_dictobj
# ...), __cfstring and __ustring into subsections by content, and names
# none of them: their labels, linker-private or not (clang's
# __unnamed_array_storage), are in no image's symbol table. A -r output
# keeps them all, labels and all, for the final link to merge.
cat <<EOF | $CC -o $t/a.o -c -xobjective-c -fno-objc-arc -
#import <Foundation/Foundation.h>
id num(void) { return @42; }
id dbl(void) { return @2.5; }
id arr(void) { return @[@"a", @1]; }
id dict(void) { return @{@"k": @"v"}; }
NSString *str(void) { return @"cfstr"; }
NSString *ustr(void) { return @"été"; }
EOF
otool -l $t/a.o > $t/lc
grep -q 'sectname __objc_arraydata' $t/lc
grep -q 'sectname __ustring' $t/lc
nm $t/a.o | grep -q __unnamed_array_storage

$mold -r -arch $ARCH -o $t/r.o $t/a.o
nm -m $t/r.o > $t/nm
grep -q '(__DATA,__objc_arraydata) non-external .*__unnamed_array_storage$' $t/nm
not grep -q 'l[0-9][0-9][0-9]$' $t/nm

cat <<EOF | $CC -o $t/main.o -c -xobjective-c -fno-objc-arc -
#import <Foundation/Foundation.h>
id num(void); id dbl(void); id arr(void); id dict(void);
NSString *str(void); NSString *ustr(void);
int main() {
  printf("%s\n", [[NSString stringWithFormat:@"%@ %@ %@ %@ %@ %lu", num(), dbl(),
                   [arr() componentsJoinedByString:@","], dict()[@"k"], str(),
                   (unsigned long)[ustr() length]] UTF8String]);
}
EOF
$CC --ld-path=$mold -o $t/exe $t/main.o $t/r.o -framework Foundation
$t/exe | grep -q '^42 2.5 a,1 v cfstr 3$'
nm $t/exe > $t/nm1
not grep -q __unnamed_array_storage $t/nm1
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/a.o -framework Foundation
$t/exe2 | grep -q '^42 2.5 a,1 v cfstr 3$'
nm $t/exe2 > $t/nm2
not grep -q __unnamed_array_storage $t/nm2
