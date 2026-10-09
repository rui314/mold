#!/bin/bash
source "$(dirname "$0")"/common.inc

# A GOT slot of one of the image's own definitions holds its address,
# rebased - a weak definition the link hid too (_aaa, _wh) - while an
# exported weak definition (_wext) is bound by weak lookup, so that
# another image's copy may replace it, and an import (___stderrp) is
# bound to its library. The slots may come in any order.
[ $ARCH = arm64 ] || skip

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _f
.p2align 2
_f:
  adrp x0, ___stderrp@GOTPAGE
  ldr x0, [x0, ___stderrp@GOTPAGEOFF]
  ret
.section __TEXT,__gcc_except_tab
.p2align 2
_lsda:
  .long _wh@GOT - .
  .long _plain@GOT - .
  .long _wext@GOT - .
  .long _aaa@GOT - .
  .long _zzz@GOT - .
.section __DATA,__const
.globl _zzz, _wh, _plain, _wext, _aaa
.private_extern _zzz, _wh, _plain, _aaa
.weak_definition _wh, _wext, _aaa
_zzz: .quad 5
_wh: .quad 1
_plain: .quad 2
_wext: .quad 3
_aaa: .quad 4
.subsections_via_symbols
EOF

$CC --ld-path=$mold -shared -o $t/b.dylib $t/a.o
nm $t/b.dylib > $t/syms
addr() { echo 0x$(awk -v s=$1 '$3 == s { print $1 }' $t/syms | sed 's/^0*//'); }
dyld_info -fixups $t/b.dylib | grep __got | awk '{ print $NF }' > $t/got
cat > $t/expected <<EOF
$(addr _plain)
$(addr _zzz)
libSystem/___stderrp
$(addr _aaa)
<weak-def-coalesce>/_wext
$(addr _wh)
EOF
sed -E 's/^0x0*/0x/' $t/got | tr 'A-F' 'a-f' | sort > $t/got2
tr 'A-F' 'a-f' < $t/expected | sort > $t/expected2
diff $t/expected2 $t/got2
