#!/bin/bash
source "$(dirname "$0")"/common.inc

# Merged Objective-C code keeps its classes, categories, protocols,
# selectors and literal strings. ld-prime records the metadata as its
# link optimized it: a category of a class of the dylib merged into the
# class, the method lists in the relative form, and every list pointer
# that is NULL bound to a placeholder. An image that merges a library
# that defines classes gets ld-prime's hook for them (so that
# +[NSBundle bundleForClass:] finds the library's bundle), unless
# -no_merged_libraries_hook; mold has no such hook yet.
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

$CC -shared -o $t/libfoo.dylib $t/a.o -framework Foundation -Wl,-make_mergeable \
  -Wl,-install_name,@rpath/libfoo.dylib
$CC --ld-path=$mold -o $t/exe $t/main.o -L$t -Wl,-merge-lfoo -Wl,-no_merged_libraries_hook
$t/exe | grep -q '^Alice greets Bob (30) / ALICE GREETS WORLD (30) / 6 1$'
otool -L $t/exe > $t/libs
not grep -q libfoo $t/libs
grep -q Foundation $t/libs

if $mold -v 2>&1 | grep -q mold-macho; then
  not $CC --ld-path=$mold -o $t/exe2 $t/main.o -L$t -Wl,-merge-lfoo 2> $t/log
  grep -q "the hook for the classes of mergeable libraries is not supported ('$t/libfoo.dylib' defines _OBJC_CLASS_\$_Person); use -no_merged_libraries_hook" $t/log
else
  $CC --ld-path=$mold -o $t/exe2 $t/main.o -L$t -Wl,-merge-lfoo
  nm $t/exe2 | grep -q imageNameHook
fi
