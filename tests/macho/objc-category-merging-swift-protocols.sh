#!/bin/bash
source "$(dirname "$0")"/common.inc

command -v swiftc >/dev/null || skip
[ "$ARCH" = "$(uname -m)" ] || skip

# Swift gives a class's metaclass ro record a protocol list of its own,
# a copy of the class's (clang's metaclass shares the class's). When a
# category (a Swift extension adopting an @objc protocol) merges into
# the class, both ro records point at the merged list, and both
# superseded lists go, as in ld-prime.
cat <<EOF > $t/a.swift
import Foundation
@objc protocol P1 { func p1() -> Int }
@objc protocol P2 { func p2() -> Int }
class Foo: NSObject, P1 {
  func p1() -> Int { return 1 }
}
extension Foo: P2 {
  func p2() -> Int { return 2 }
}
let f = Foo()
print(f.conforms(to: P1.self), f.conforms(to: P2.self), f.p1() + f.p2())
EOF
$SWIFTC -module-name main -emit-object -o $t/a.o $t/a.swift
nm $t/a.o > $t/nm-in
[ "$(grep -c ' __PROTOCOLS__TtC4main3Foo' $t/nm-in)" = 2 ]

$SWIFTC -use-ld=$mold -o $t/exe $t/a.o
$RUN $t/exe | grep -q '^true true 3$'
nm $t/exe > $t/nm
grep -q '__OBJC_CLASS_PROTOCOLS_\$__TtC4main3Foo(main)$' $t/nm
not grep -q ' __PROTOCOLS__TtC4main3Foo' $t/nm
