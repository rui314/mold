#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld64 merges a category into its class when the class is defined in
# the same image (-objc_category_merging, on by default): the runtime
# then has no category to attach. The merged method list holds the
# categories' methods, last category first, then the class's own; the
# protocol list likewise; the property lists take the categories in
# order, then the class's. The class_ro_t records point at the merged
# lists, the categories leave __objc_catlist, and a class absorbing a
# +load category joins __objc_nlclslist. A category on a class from
# another image stays.
cat <<EOF2 | $CC -o $t/a.o -c -xobjective-c -fno-objc-arc -
#import <Foundation/Foundation.h>
#import <objc/runtime.h>
#import <stdio.h>
@protocol P1 - (int)p1; @end
@protocol P2 - (int)p2; @end
@protocol P3 - (int)p3; @end
@interface Foo : NSObject <P1>
@property int base;
@end
@implementation Foo
- (int)p1 { return 1; }
- (int)m { return 10; }
+ (int)cm { return 20; }
@end
@interface Foo (A) <P2>
@property (readonly) int pa;
- (int)a;
+ (int)ca;
@end
@implementation Foo (A)
+ (void)load { printf("load\n"); }
- (int)pa { return 0; }
- (int)a { return 11; }
+ (int)ca { return 21; }
- (int)p2 { return 2; }
@end
@interface Foo (B) <P3>
@property (readonly) int pb;
- (int)b;
- (int)m;
@end
@implementation Foo (B)
- (int)pb { return 0; }
- (int)b { return 12; }
- (int)m { return 13; }
- (int)p3 { return 3; }
@end
@interface NSObject (Ext)
- (int)ext;
@end
@implementation NSObject (Ext)
- (int)ext { return 7; }
@end
int main() {
  Foo *f = [Foo new];
  unsigned n = 0, cats = 0;
  Class cls = [Foo class];
  Method *ms = class_copyMethodList(cls, &n);
  free(ms);
  Protocol * __unsafe_unretained *ps = class_copyProtocolList(cls, &cats);
  free(ps);
  printf("%d %d %d %d %d %d %d %d %d %u %u\n", [f a], [f b], [f m], [Foo ca], [Foo cm],
         [f p1], [f p2], [f p3], [f ext], n, cats);
  return 0;
}
EOF2
$CC --ld-path=$mold -o $t/exe $t/a.o -framework Foundation -mmacosx-version-min=15.0
$t/exe > $t/out
grep -q '^load$' $t/out
# m resolves to the category's override; 11 methods, 3 protocols.
grep -q '^11 12 13 21 20 1 2 3 7 11 3$' $t/out
otool -l $t/exe > $t/lc
grep -A1 'sectname __objc_catlist' $t/lc | grep -q segname   # NSObject (Ext) stays
otool -s __DATA_CONST __objc_catlist $t/exe | tail -n +3 > $t/catlist
[ "$(wc -l < $t/catlist | tr -d ' ')" = 1 ]
grep -q 'sectname __objc_nlclslist' $t/lc
otool -ov $t/exe > $t/ov
# Merged order: B's methods, A's, then Foo's own.
awk '/baseMethods.*INSTANCE_METHODS_Foo/{f=1} f&&/imp /{print $NF} /baseProtocols/{f=0}' $t/ov | head -11 | tr '\n' ' ' > $t/order
grep -q '^pb\] b\] m\] p3\] pa\] a\] p2\] p1\] m\] base\] setBase:\] $' $t/order
grep -E 'list\[' $t/ov | head -3 | awk '{print $NF}' | tr '\n' ' ' > $t/protos
grep -q '^__OBJC_PROTOCOL_\$_P3 __OBJC_PROTOCOL_\$_P2 __OBJC_PROTOCOL_\$_P1 $' $t/protos
grep -E '^ *name .* (pa|pb|base)$' $t/ov | head -3 | awk '{print $NF}' | tr '\n' ' ' > $t/props
grep -q '^pa pb base $' $t/props
nm $t/exe > $t/nm
not grep -q 'CATEGORY_INSTANCE_METHODS_Foo' $t/nm
not grep -q 'OBJC_\$_CATEGORY_Foo' $t/nm
# ld64 names the merged lists after the class and its categories.
grep -q ' s __OBJC_\$_INSTANCE_METHODS_Foo(A|B)$' $t/nm
grep -q ' s __OBJC_\$_CLASS_METHODS_Foo(A|B)$' $t/nm
grep -q ' s __OBJC_CLASS_PROTOCOLS_\$_Foo(A|B)$' $t/nm
[ "$(grep -c 'INSTANCE_METHODS_Foo' $t/nm)" = 1 ]

# Classic method lists (below macOS 11 on x86-64) merge too.
if [ $ARCH = x86_64 ]; then
  $CC --ld-path=$mold -o $t/exe15 $t/a.o -framework Foundation -mmacosx-version-min=10.15
  $t/exe15 > $t/out15
  grep -q '^11 12 13 21 20 1 2 3 7 11 3$' $t/out15
  otool -s __DATA_CONST __objc_catlist $t/exe15 | tail -n +3 > $t/catlist15
  [ "$(wc -l < $t/catlist15 | tr -d ' ')" = 1 ]
fi

# -no_objc_category_merging keeps the categories (the runtime attaches
# them, so the program behaves the same).
$CC --ld-path=$mold -o $t/exe_no $t/a.o -framework Foundation -Wl,-no_objc_category_merging
$t/exe_no > $t/out_no
grep -q '^11 12 13 21 20 1 2 3 7 11 3$' $t/out_no
otool -s __DATA_CONST __objc_catlist $t/exe_no | tail -n +3 > $t/catlist_no
[ "$(wc -l < $t/catlist_no | tr -d ' ')" = 2 ]
