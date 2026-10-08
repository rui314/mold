#!/usr/bin/env bash
. $(dirname $0)/common.inc

command -v swiftc >/dev/null || skip
[ "$ARCH" = "$(uname -m)" ] || skip

# A Swift class's class_ro_t is 80 bytes: RO_HAS_SWIFT_INITIALIZER
# (flag 1 << 6) says a metadata-initializer pointer follows the seven
# standard fields at offset 72, and the runtime calls it while
# realizing the class. Merging an Objective-C category into such a
# class rewrites the ro record; the rewritten record must keep that
# field (NetNewsWire, iTerm2 and OpenEmu all crashed in
# objc_copyClassList / NSClassFromString calling a garbage address).
cat <<EOF2 > $t/foo.swift
import Foundation
// Stored properties of Foundation's resilient value types make the
// class need singleton metadata initialization.
@objc(Foo) public class Foo: NSObject {
  public var when = Date()
  public var where_: URL? = nil
  @objc public func base() -> Int { return 1 }
}
EOF2
$SWIFTC -parse-as-library -module-name M -emit-object -o $t/foo.o $t/foo.swift
# The class_ro_t carries RO_HAS_SWIFT_INITIALIZER (1 << 6).
python3 - $t/foo.o <<'EOF2'
import sys, macho
m = macho.MachO(sys.argv[1])
sym = next(s for s in m.symbols() if s.name == b'__DATA_Foo')
sect = m.sections[sym.sect - 1]
flags = m.u32(sect.offset + sym.value - sect.addr)
assert flags & 0x40, hex(flags)
EOF2

cat <<EOF2 | $CC -o $t/cat.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface Foo : NSObject
- (NSInteger)base;
@end
@interface Foo (Cat)
- (NSInteger)extra;
@end
@implementation Foo (Cat)
- (NSInteger)extra { return 41; }
@end
EOF2

cat <<EOF2 | $CC -o $t/main.o -c -xobjective-c -
#import <Foundation/Foundation.h>
#import <objc/runtime.h>
@interface Foo : NSObject
- (NSInteger)base;
- (NSInteger)extra;
@end
int main() {
  // Realize every class, as XCTest and NSClassFromString do.
  unsigned n = 0;
  Class *classes = objc_copyClassList(&n);
  free(classes);
  Foo *f = [Foo new];
  printf("%ld %ld %u\n", (long)[f base], (long)[f extra], n > 0);
}
EOF2

$SWIFTC -o $t/exe $t/main.o $t/cat.o $t/foo.o -use-ld=$mold -framework Foundation
$RUN $t/exe | grep '^1 41 1$'
# The category was merged into the class.
nm $t/exe > $t/nm
not grep -q 'OBJC_\$_CATEGORY_Foo_\$_Cat$' $t/nm
grep -q 'INSTANCE_METHODS_Foo(Cat)' $t/nm
