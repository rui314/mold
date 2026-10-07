#!/bin/bash
source "$(dirname "$0")"/common.inc

# Merged Swift code keeps its metadata: its type descriptors and their
# relative pointers (direct, and through a GOT slot for an import),
# protocol conformances, the class metadata ld-prime records as aliases
# into the full metadata, and the DWARF unwind information compact
# unwind can't describe. Its dylibs - the Swift runtime's - come by
# the mergeable dylib's record of them.
command -v swiftc >/dev/null || skip

cat > $t/a.swift <<EOF
public protocol Shape { func area() -> Double; var name: String { get } }
public struct Square: Shape {
  public let side: Double
  public init(side: Double) { self.side = side }
  public func area() -> Double { side * side }
  public var name: String { "square" }
}
public final class Circle: Shape {
  public let r: Double
  public init(r: Double) { self.r = r }
  public func area() -> Double { 3 * r * r }
  public var name: String { "circle" }
}
open class Animal { public init() {}; open func sound() -> String { "..." } }
public class Dog: Animal { public override func sound() -> String { "woof" } }
public enum Color: String { case red, green, blue }
public func total<T: Shape>(_ xs: [T]) -> Double { xs.reduce(0) { \$0 + \$1.area() } }
@_cdecl("swift_entry")
public func swiftEntry() -> Int32 {
  let shapes: [Shape] = [Square(side: 2), Circle(r: 1)]
  var s = 0.0
  for x in shapes { s += x.area(); print(x.name) }
  print(Dog().sound(), Color.green.rawValue, total([Square(side: 3)]))
  return Int32(s)
}
EOF
swiftc -target $ARCH-apple-macos14.0 -parse-as-library -module-name Shapes -O -c \
  -o $t/a.o $t/a.swift

cat <<EOF | $CC -o $t/main.o -c -xc -
int swift_entry(void);
int main() { return swift_entry() == 7 ? 0 : 1; }
EOF

swiftlib=$(xcrun --show-sdk-path)/usr/lib/swift
$CC -mmacosx-version-min=14.0 -shared -o $t/libfoo.dylib $t/a.o -L$swiftlib \
  -Wl,-make_mergeable -Wl,-install_name,@rpath/libfoo.dylib
$CC -mmacosx-version-min=14.0 --ld-path=$mold -o $t/exe $t/main.o -L$t -Wl,-merge-lfoo \
  -L$swiftlib -Wl,-no_merged_libraries_hook
$RUN $t/exe > $t/out
grep -q '^square$' $t/out
grep -q '^circle$' $t/out
grep -q '^woof green 9.0$' $t/out
otool -L $t/exe > $t/libs
not grep -q libfoo $t/libs
grep -q libswiftCore $t/libs
