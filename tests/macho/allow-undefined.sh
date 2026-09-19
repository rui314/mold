#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
extern int mystery();
int (*get_mystery(void))(void) { return mystery; }
int main() { return 0; }
EOF2

# Fails by default
not $CC --ld-path=$mold -o $t/exe $t/a.o 2>/dev/null

# -U allows one specific symbol to stay undefined; running the result
# still needs something (a host process, an inserted library) to
# provide it, so only the link is checked.
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-U,_mystery
nm -m $t/exe | grep -q '_mystery (dynamically looked up)'

# -undefined warning reports but links
$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-undefined,warning 2> $t/log
grep -q 'warning.*_mystery' $t/log
