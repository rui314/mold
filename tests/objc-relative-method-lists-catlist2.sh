#!/bin/bash
source "$(dirname "$0")"/common.inc

command -v swiftc >/dev/null || skip
[ "$ARCH" = "$(uname -m)" ] || skip

# A Swift class whose superclass comes from a resilient module has no
# static class_t: the runtime reaches it through a class stub, and the
# categories of such a class (a Swift extension's @objc members) go
# in __objc_catlist2 rather than __objc_catlist. Their method lists
# are rewritten in relative form like any other, and a selector no
# code references gets a selector reference in the __objc_selrefs
# tail. __objc_catlist2 follows __objc_catlist, and its entries' label
# (_objc_categories_stubs) is not emitted, as for the other lists.
cat <<EOF > $t/base.swift
import Foundation
open class Base: NSObject {
  public override init() {}
}
EOF
swiftc -use-ld=$mold -enable-library-evolution -emit-module -emit-library -module-name Base \
  -emit-module-path $t/Base.swiftmodule -o $t/libBase.dylib $t/base.swift

cat <<EOF > $t/a.swift
import Foundation
import Base
class Sub: Base {}
extension Sub {
  @objc func catIM() -> Int { return 3 }
  @objc class func catCM() -> Int { return 4 }
}
let s = Sub()
print(s.responds(to: NSSelectorFromString("catIM")),
      Sub.responds(to: NSSelectorFromString("catCM")))
EOF
swiftc -I $t -module-name main -emit-object -o $t/a.o $t/a.swift
otool -l $t/a.o | grep -q 'sectname __objc_catlist2'

swiftc -use-ld=$mold -o $t/exe $t/a.o -L$t -lBase -Xlinker -rpath -Xlinker $t
$t/exe | grep -q '^true true$'
otool -ov $t/exe > $t/objc
grep -A1 'instanceMethods.*__CATEGORY_INSTANCE_METHODS__TtC4main3Sub' $t/objc | grep -q 'entsize 12 (relative)'
grep -A1 'classMethods.*__CATEGORY_CLASS_METHODS__TtC4main3Sub' $t/objc | grep -q 'entsize 12 (relative)'
sed -n '/^Contents of (__DATA,__objc_selrefs)/,/^Contents/p' $t/objc > $t/selrefs
grep -q ' catIM$' $t/selrefs
grep -q ' catCM$' $t/selrefs
otool -l $t/exe | grep 'sectname __objc_' > $t/sects
[ "$(grep -A1 __objc_catlist2 $t/sects | tail -1 | awk '{print $2}')" = __objc_imageinfo ]
nm $t/exe > $t/nm
not grep -q _objc_categories_stubs $t/nm

# The same in a -r output.
$mold -r -arch $ARCH -o $t/r.o $t/a.o
otool -l $t/r.o | grep 'sectname __objc_' > $t/sects-r
[ "$(grep -A1 __objc_catlist2 $t/sects-r | tail -1 | awk '{print $2}')" = __objc_imageinfo ]
nm $t/r.o > $t/nm-r
not grep -q _objc_categories_stubs $t/nm-r
