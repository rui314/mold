#!/bin/bash
source "$(dirname "$0")"/common.inc

# -alias whose base symbol is imported from a dylib. ld64 makes the
# new name an indirect symbol (N_INDR) and an export trie entry that
# re-exports the dylib's symbol under that name. Xcode links every app
# extension's debug dylib with
#   -alias _NSExtensionMain ___debug_main_executable_dylib_entry_point
# and _NSExtensionMain, from Foundation, is otherwise unreferenced.
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo(void) { return 42; }
EOF

$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o -Wl,-alias,_puts,_my_puts
nm -m $t/libfoo.dylib > $t/nm
grep -q 'indirect.*_my_puts.*for _puts' $t/nm
grep -q 'undefined.*_puts (from libSystem)' $t/nm
dyld_info -exports $t/libfoo.dylib > $t/exports
grep -q 're-export.*_my_puts.*_puts' $t/exports

# A client binds to the alias and reaches puts through it.
cat <<EOF | $CC -o $t/main.o -c -xc -
int my_puts(const char *);
int main() { my_puts("hello via alias"); }
EOF
$CC --ld-path=$mold -o $t/exe $t/main.o $t/libfoo.dylib
$t/exe | grep -q 'hello via alias'
