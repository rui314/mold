#!/bin/bash
source "$(dirname "$0")"/common.inc

command -v swiftc >/dev/null || skip
[ "$ARCH" = "$(uname -m)" ] || skip

# From macOS 11 on, method lists are rewritten in relative form - class
# methods too, found through the metaclass that the class_t's isa
# points at. A Swift class's class_t lies past its metadata's prefix,
# not at the start of its subsection, so the isa must be read there.
cat <<EOF > $t/a.swift
import Foundation
class Foo: NSObject {
  @objc class func cm1() -> Int { return 1 }
  @objc class func cm2() -> Int { return 2 }
  @objc func im() -> Int { return 3 }
}
print(Foo.cm1() + Foo().im(), Foo.perform(NSSelectorFromString("cm2")) != nil)
EOF
swiftc -module-name main -emit-object -o $t/a.o $t/a.swift
swiftc -use-ld=$mold -o $t/exe $t/a.o
$RUN $t/exe | grep -q '^4 true$'
otool -ov $t/exe > $t/objc
[ "$(grep -A1 'baseMethods.*__CLASS_METHODS__TtC4main3Foo' $t/objc | grep -c 'entsize 12 (relative)')" = 1 ]
[ "$(grep -A1 'baseMethods.*__INSTANCE_METHODS__TtC4main3Foo' $t/objc | grep -c 'entsize 12 (relative)')" = 1 ]
