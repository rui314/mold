#!/bin/bash
source "$(dirname "$0")"/common.inc

# -no_weak_imports names each import an object references weakly, and
# -weak_reference_mismatches error each object that references an
# import otherwise than the objects before it, in the order of the
# objects' relocations; weak makes an import weak if any reference is.
cat <<EOF | $CC -o $t/a.o -c -xc -
int aaa() { return 1; }
int bbb() { return 2; }
int zzz() { return 3; }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
extern int zzz(void) __attribute__((weak_import));
extern int aaa(void) __attribute__((weak_import));
int f1() { return zzz() + aaa(); }
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
extern int zzz(void), aaa(void), bbb(void);
int f2() { return zzz() + aaa() + bbb(); }
EOF

$CC --ld-path=$mold -o $t/liba.dylib -shared $t/a.o

# (The messages but the last one have no prefix.)
strip() {
  grep -v '^+' $1 | sed -E 's/^(ld: |mold: (error: )?)//; s|in [^ ]*/|in |'
}

not $mold -dylib -o $t/libb.dylib $t/b.o $t/c.o $t/liba.dylib -no_weak_imports 2> $t/log
cat > $t/expected <<EOF
weak import of symbol '_aaa' not supported because of option: -no_weak_imports
weak import of symbol '_zzz' not supported because of option: -no_weak_imports
weak imports not allowed
EOF
strip $t/log | diff - $t/expected

not $mold -dylib -o $t/libb.dylib $t/b.o $t/c.o $t/liba.dylib \
  -weak_reference_mismatches error 2> $t/log
cat > $t/expected <<EOF
mismatching weak references for symbol: _aaa, found non-weak import in c.o
mismatching weak references for symbol: _zzz, found non-weak import in c.o
weak import mismatches found
EOF
strip $t/log | diff - $t/expected

# By default an import is weak only if every reference is; with weak,
# if any is.
$CC --ld-path=$mold -o $t/libb.dylib -shared $t/b.o $t/c.o $t/liba.dylib
dyld_info -fixups $t/libb.dylib > $t/fixups
not grep -q "\[weak-import\]" $t/fixups
$CC --ld-path=$mold -o $t/libb.dylib -shared $t/b.o $t/c.o $t/liba.dylib \
  -Wl,-weak_reference_mismatches,weak
dyld_info -fixups $t/libb.dylib > $t/fixups
grep -q '_aaa \[weak-import\]' $t/fixups
grep -q '_zzz \[weak-import\]' $t/fixups
grep _bbb $t/fixups > $t/bbb
not grep -q "\[weak-import\]" $t/bbb

not $mold -o $t/exe $t/b.o -weak_reference_mismatches foo 2> $t/log
grep -q 'invalid option to -weak_reference_mismatches \[ error | weak | non-weak \]' $t/log
not $mold -o $t/exe $t/b.o -weak_reference_mismatches 2> $t/log
grep -q -- '-weak_reference_mismatches.*missing' $t/log

# A weakly linked library makes its imports weak, which is fine.
$CC --ld-path=$mold -o $t/libb.dylib -shared $t/c.o -Wl,-weak_library,$t/liba.dylib \
  -Wl,-no_weak_imports
