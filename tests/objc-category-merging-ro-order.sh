#!/bin/bash
source "$(dirname "$0")"/common.inc

# Category merging writes a class's and its metaclass's class_ro_t
# anew, each where the input had its record: the metaclass's first, as
# clang emits them. So it is when the records are all __objc_const has
# left - the category's lists merged away - as when a class with no
# methods of its own gains a category's.
cat <<EOF | $CC -o $t/a.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface Foo : NSObject @end
@implementation Foo @end
@interface Foo (Cat) - (int)a; @end
@implementation Foo (Cat) - (int)a { return 1; } @end
int main() { return [[Foo new] a]; }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -framework Foundation -Wl,-map,$t/map
grep -E '__OBJC_(META)?CLASS_RO_\$_Foo$' $t/map | cut -f3 > $t/order
diff - $t/order <<'EOF'
[  1] __OBJC_METACLASS_RO_$_Foo
[  1] __OBJC_CLASS_RO_$_Foo
EOF
