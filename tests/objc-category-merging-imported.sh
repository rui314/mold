#!/bin/bash
source "$(dirname "$0")"/common.inc

# The categories on a class another image defines (NSObject here) merge
# into the first of them, across objects: its record stays in
# __objc_catlist, pointing at merged lists of each kind another
# category has, named after the class and the categories
# (__OBJC_$_INSTANCE_METHODS_NSObject(A|B|D)); a kind only the first
# has keeps its own list. The categories of a class one of whose
# categories has a +load (NSString) don't merge at all.
cat <<EOF | $CC -o $t/a.o -c -xobjective-c -fno-objc-arc -
#import <Foundation/Foundation.h>
@protocol PB
- (int)b;
@end
@interface NSObject (A)
- (int)a;
+ (int)ca;
@property (readonly) int pa;
@end
@implementation NSObject (A)
- (int)a { return 1; }
+ (int)ca { return 2; }
- (int)pa { return 3; }
@end
@interface NSString (L)
- (int)l;
@end
@implementation NSString (L)
+ (void)load {}
- (int)l { return 4; }
@end
EOF

cat <<EOF | $CC -o $t/b.o -c -xobjective-c -fno-objc-arc -
#import <Foundation/Foundation.h>
@protocol PB
- (int)b;
@end
@interface NSObject (B) <PB>
- (int)b;
@property (class, readonly) int cpb;
@end
@implementation NSObject (B)
- (int)b { return 5; }
+ (int)cpb { return 6; }
@end
@interface NSObject (D)
- (int)d;
@end
@implementation NSObject (D)
- (int)d { return 7; }
@end
@interface NSString (N)
- (int)n;
@end
@implementation NSString (N)
- (int)n { return 8; }
@end
EOF

cat <<EOF | $CC -o $t/c.o -c -xobjective-c -fno-objc-arc -
#import <Foundation/Foundation.h>
#import <objc/runtime.h>
@interface NSObject (Decl)
- (int)a; - (int)b; - (int)d; + (int)ca; - (int)pa; + (int)cpb;
@end
@interface NSString (Decl)
- (int)l; - (int)n;
@end
int main() {
  NSObject *o = [NSObject new];
  Class meta = object_getClass([NSObject class]);
  printf("%d %d %d %d %d %d %d %d %d %d %d\n", [o a], [NSObject ca], [o pa], [o b],
         [NSObject cpb], [o d], [@"x" l], [@"x" n],
         class_conformsToProtocol([NSObject class], objc_getProtocol("PB")),
         class_getProperty([NSObject class], "pa") != NULL,
         class_getProperty(meta, "cpb") != NULL);
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o -framework Foundation
$t/exe | grep -q '^1 2 3 5 6 7 4 8 1 1 1$'
nm $t/exe > $t/nm
grep -q ' __OBJC_$_CATEGORY_NSObject_$_A$' $t/nm
not grep -q '__OBJC_$_CATEGORY_NSObject_$_[BD]$' $t/nm
grep -q ' __OBJC_$_INSTANCE_METHODS_NSObject(A|B|D)$' $t/nm
grep -q ' __OBJC_$_CLASS_METHODS_NSObject(A|B|D)$' $t/nm
grep -q ' __OBJC_CLASS_PROTOCOLS_$_NSObject(A|B|D)$' $t/nm
grep -q ' __OBJC_$_PROP_LIST_NSObject_$_A$' $t/nm
grep -q ' __OBJC_$_CATEGORY_NSString_$_L$' $t/nm
grep -q ' __OBJC_$_CATEGORY_NSString_$_N$' $t/nm
# NSObject(A), NSString(L) and NSString(N).
otool -l $t/exe | grep -A4 'sectname __objc_catlist$' | grep -q 'size 0x0*18$'

# -no_objc_category_merging keeps all five.
$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b.o $t/c.o -framework Foundation \
  -Wl,-no_objc_category_merging
$t/exe2 | grep -q '^1 2 3 5 6 7 4 8 1 1 1$'
otool -l $t/exe2 | grep -A4 'sectname __objc_catlist$' | grep -q 'size 0x0*28$'
