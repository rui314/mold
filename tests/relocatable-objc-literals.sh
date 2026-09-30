#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime makes the atoms of the Objective-C constant literals
# (__objc_intobj, __objc_doubleobj, __objc_arraydata, __objc_arrayobj,
# __objc_dictobj ...), __cfstring and __ustring by content, and names
# none of them: their labels, linker-private or not (clang's
# __unnamed_array_storage), are in no output's symbol table. A -r
# output names the atoms itself on arm64, l<nnn> with N_PEXT, since
# arm64 relocations must name what they refer to; x86-64 relocations
# refer to them section-relatively, and they get no symbol at all.
cat <<EOF | $CC -o $t/a.o -c -xobjective-c -fno-objc-arc -
#import <Foundation/Foundation.h>
id num(void) { return @42; }
id dbl(void) { return @2.5; }
id arr(void) { return @[@"a", @1]; }
id dict(void) { return @{@"k": @"v"}; }
NSString *str(void) { return @"cfstr"; }
NSString *ustr(void) { return @"été"; }
EOF
otool -l $t/a.o > $t/lc
grep -q 'sectname __objc_arraydata' $t/lc
grep -q 'sectname __ustring' $t/lc
nm $t/a.o | grep -q __unnamed_array_storage

$mold -r -arch $ARCH -o $t/r.o $t/a.o
nm -m $t/r.o > $t/nm
not grep -q '__unnamed\|l_\.str' $t/nm
for s in __objc_intobj __objc_doubleobj __objc_arraydata __objc_arrayobj __objc_dictobj \
  __cfstring __ustring; do
  if [ "$ARCH" = arm64 ]; then
    grep -q ",$s) non-external (was a private external) .*l[0-9][0-9][0-9]\$" $t/nm
  else
    not grep -q ",$s)" $t/nm
  fi
done
if [ "$ARCH" != arm64 ]; then
  otool -rv $t/r.o > $t/relocs
  grep -q 'False  SIGNED  False     [0-9]* (__DATA,__objc_intobj)' $t/relocs
fi

cat <<EOF | $CC -o $t/main.o -c -xobjective-c -fno-objc-arc -
#import <Foundation/Foundation.h>
id num(void); id dbl(void); id arr(void); id dict(void);
NSString *str(void); NSString *ustr(void);
int main() {
  printf("%s\n", [[NSString stringWithFormat:@"%@ %@ %@ %@ %@ %lu", num(), dbl(),
                   [arr() componentsJoinedByString:@","], dict()[@"k"], str(),
                   (unsigned long)[ustr() length]] UTF8String]);
}
EOF
$CC --ld-path=$mold -o $t/exe $t/main.o $t/r.o -framework Foundation
$t/exe | grep -q '^42 2.5 a,1 v cfstr 3$'
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/a.o -framework Foundation
$t/exe2 | grep -q '^42 2.5 a,1 v cfstr 3$'
nm $t/exe2 > $t/nm2
not grep -q __unnamed_array_storage $t/nm2
