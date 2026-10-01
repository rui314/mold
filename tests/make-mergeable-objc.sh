#!/bin/bash
source "$(dirname "$0")"/common.inc

# A mergeable dylib records its Objective-C metadata as its link made
# it, as ld-prime does: the method lists in the relative form, a
# category of a class of its own merged into the class, the selector
# references of the objc stubs; and a placeholder at each null list
# pointer of a class or a category, for lists a merging link may add,
# without which ld-prime fails to merge it. (The hook for the classes
# of mergeable libraries is no matter here: -no_merged_libraries_hook.)
cat <<EOF | $CC -o $t/a.o -c -O1 -xobjective-c -
#import <Foundation/Foundation.h>
@protocol Greeter <NSObject>
- (NSString *)greet:(NSString *)name;
@end
@interface Person : NSObject <Greeter>
@property (nonatomic, copy) NSString *name;
@property (nonatomic) int age;
+ (instancetype)personWithName:(NSString *)n;
@end
@implementation Person
+ (instancetype)personWithName:(NSString *)n { Person *p = [self new]; p.name = n; return p; }
- (NSString *)greet:(NSString *)other {
  return [NSString stringWithFormat:@"%@ greets %@ (%d)", self.name, other, self.age];
}
@end
@interface Person (Extra)
- (NSString *)shout;
@end
@implementation Person (Extra)
- (NSString *)shout { return [[self greet:@"world"] uppercaseString]; }
@end
@interface NSString (MergeTest)
- (NSUInteger)doubleLength;
@end
@implementation NSString (MergeTest)
- (NSUInteger)doubleLength { return self.length * 2; }
@end
__attribute__((objc_nonlazy_class))
@interface Eager : NSObject
@end
@implementation Eager
+ (void)load { }
@end
const char *objc_entry(void) {
  Person *p = [Person personWithName:@"Alice"];
  p.age = 30;
  id<Greeter> g = p;
  NSString *s = [NSString stringWithFormat:@"%@ / %@ / %lu %d", [p greet:@"Bob"], [p shout],
                 (unsigned long)[@"abc" doubleLength], [g conformsToProtocol:@protocol(Greeter)]];
  return strdup(s.UTF8String);
}
EOF

cat <<EOF | $CC -o $t/main.o -c -O1 -xc -
#include <stdio.h>
const char *objc_entry(void);
int main() { puts(objc_entry()); }
EOF

$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o -framework Foundation \
  -Wl,-make_mergeable -Wl,-install_name,@rpath/libfoo.dylib
$CC --ld-path=$mold -o $t/exe $t/main.o -L$t -Wl,-merge-lfoo -Wl,-no_merged_libraries_hook
$t/exe | grep -q '^Alice greets Bob (30) / ALICE GREETS WORLD (30) / 6 1$'
$CC -o $t/exe2 $t/main.o -L$t -Wl,-merge-lfoo -Wl,-no_merged_libraries_hook
$t/exe2 | grep -q '^Alice greets Bob (30) / ALICE GREETS WORLD (30) / 6 1$'
otool -L $t/exe2 > $t/libs
not grep -q libfoo $t/libs
grep -q Foundation $t/libs
