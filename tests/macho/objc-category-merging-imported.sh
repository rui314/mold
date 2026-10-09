#!/bin/bash
source "$(dirname "$0")"/common.inc

# The categories on a class another image defines (NSObject, NSString
# here) stay categories the runtime attaches at load, each with its
# methods, protocols and properties. (ld-prime merges them into the
# first of them, but for a class one of whose categories has a +load.)
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
$RUN $t/exe | grep -q '^1 2 3 5 6 7 4 8 1 1 1$'
otool -l $t/exe | grep -A4 'sectname __objc_catlist$' | grep -q 'size 0x0*28$'

# So with -no_objc_category_merging.
$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b.o $t/c.o -framework Foundation \
  -Wl,-no_objc_category_merging
$RUN $t/exe2 | grep -q '^1 2 3 5 6 7 4 8 1 1 1$'
otool -l $t/exe2 | grep -A4 'sectname __objc_catlist$' | grep -q 'size 0x0*28$'
