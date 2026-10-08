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
nm -m $t/exe | grep '_mystery (dynamically looked up)'

# -undefined suppress links as dynamic_lookup does, silently
$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-undefined,suppress 2> $t/log
nm -m $t/exe2 | grep '_mystery (dynamically looked up)'
not grep -q _mystery $t/log
