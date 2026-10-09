#!/bin/bash
source "$(dirname "$0")"/common.inc

# -objc_class_ro_signing_mismatch (and $LD_OBJC_CLASS_RO_SIGNING_MISMATCH)
# say whether objects may disagree on signing class_ro_t pointers (see
# objc-imageinfo-class-ro-signing.sh): they are read as warning (or
# warn) or error.
cat <<EOF | $CC -o $t/a.o -c -xc -
int main() { return 0; }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-objc_class_ro_signing_mismatch,warn
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-objc_class_ro_signing_mismatch,error
not $mold -o $t/exe $t/a.o -objc_class_ro_signing_mismatch suppress 2> $t/log
grep -q -- '-objc_class_ro_signing_mismatch invalid option (warning | error)' $t/log
not $mold -o $t/exe $t/a.o -objc_class_ro_signing_mismatch 2> $t/log
grep -q -- '-objc_class_ro_signing_mismatch.*missing' $t/log

LD_OBJC_CLASS_RO_SIGNING_MISMATCH=warning $CC --ld-path=$mold -o $t/exe $t/a.o
LD_OBJC_CLASS_RO_SIGNING_MISMATCH=1 not $mold -o $t/exe $t/a.o 2> $t/log
grep -q 'LD_OBJC_CLASS_RO_SIGNING_MISMATCH invalid option (warning | error)' $t/log
