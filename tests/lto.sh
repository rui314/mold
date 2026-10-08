#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -flto -o $t/a.o -c -xc -
int times2(int x) { return x * 2; }
EOF2

cat <<EOF2 | $CC -flto -o $t/b.o -c -xc -
#include <stdio.h>
int times2(int);
int main() {
  printf("%d\n", times2(21));
}
EOF2

$CC -flto --ld-path=$mold -o $t/exe $t/a.o $t/b.o
$RUN $t/exe | grep '^42$'
# A function only other bitcode calls is not kept external: libLTO
# resolves that call itself, and ld-prime exports only _main.
dyld_info -exports $t/exe > $t/exports
grep -q _main $t/exports
not grep -q _times2 $t/exports

# An -alias base is referenced by the linker, so it stays external.
$CC -flto --ld-path=$mold -o $t/exe3 $t/a.o $t/b.o -Wl,-alias,_times2,_t2
$RUN $t/exe3 | grep '^42$'
dyld_info -exports $t/exe3 > $t/exports3
grep -q _times2 $t/exports3

# A native common that a bitcode definition overrides addresses the
# definition's storage, so the definition must survive LTO.
cat <<EOF2 | $CC -flto -o $t/e.o -c -xc -
#include <stdio.h>
int x = 5;
int getx(void);
int main() {
  printf("%d\n", getx());
}
EOF2
cat <<EOF2 | $CC -fcommon -o $t/f.o -c -xc -
int x;
int getx(void) { return x; }
EOF2
$CC -flto --ld-path=$mold -o $t/exe4 $t/e.o $t/f.o
$RUN $t/exe4 | grep '^5$'

# Mixed bitcode and Mach-O, with bitcode in an archive
cat <<EOF2 | $CC -flto -o $t/c.o -c -xc -
int three() { return 3; }
EOF2
cat <<EOF2 | $CC -o $t/d.o -c -xc -
#include <stdio.h>
int three();
int main() {
  printf("%d\n", three());
}
EOF2
rm -f $t/libfoo.a
ar rcs $t/libfoo.a $t/c.o
$CC -flto --ld-path=$mold -o $t/exe2 $t/d.o $t/libfoo.a
$RUN $t/exe2 | grep '^3$'

# A strong native definition overrides a weak bitcode one, so LTO must
# keep the bitcode one external rather than inline it into its callers.
cat <<EOF2 | $CC -flto -O2 -o $t/g.o -c -xc -
#include <stdio.h>
int pick(void);
int main() {
  printf("%d\n", pick());
}
EOF2
echo '__attribute__((weak)) int pick(void) { return 1; }' | $CC -flto -O2 -o $t/h.o -c -xc -
echo 'int pick(void) { return 2; }' | $CC -o $t/i.o -c -xc -
$CC -flto --ld-path=$mold -o $t/exe5 $t/g.o $t/h.o $t/i.o
$RUN $t/exe5 | grep '^2$'
