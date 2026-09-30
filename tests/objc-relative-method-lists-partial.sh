#!/bin/bash
source "$(dirname "$0")"/common.inc

# A classic method list becomes relative only if each entry's name is a
# whole selector string in the image; otherwise it stays as it is. The
# check comes first, so a list left alone adds no selector reference
# for its other names. (ld-prime converts the list anyway, dropping
# the name's offset and so renaming the method.)
cat <<EOF > $t/a.m
#import <Foundation/Foundation.h>
@interface Foo : NSObject
- (int)uniqueSelA;
- (int)uniqueSelB;
@end
@implementation Foo
- (int)uniqueSelA { return 1; }
- (int)uniqueSelB { return 2; }
@end
int main() { return 0; }
EOF
$CC -S -fno-objc-arc -mmacosx-version-min=14.0 -o $t/a.s $t/a.m
# Point the second method's name one byte into its string.
sed -E 's/(\.quad[[:space:]]+[lL]_OBJC_METH_VAR_NAME_\.1)$/\1+1/' $t/a.s > $t/b.s
[ "$(grep -c 'METH_VAR_NAME_\.1+1$' $t/b.s)" = 1 ]
$CC -c -o $t/b.o $t/b.s
$CC --ld-path=$mold -o $t/exe $t/b.o -framework Foundation -mmacosx-version-min=14.0
$t/exe
otool -ov $t/exe | grep -A1 'baseMethods.*INSTANCE_METHODS_Foo' | grep -q 'entsize 24$'
otool -l $t/exe > $t/lc
not grep -q 'sectname __objc_selrefs' $t/lc
