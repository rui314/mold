#!/bin/bash
source "$(dirname "$0")"/common.inc

# A report the link writes besides its output - -dependency_info, -map
# or -sdk_imports - that ld-prime can't create is only a warning, in
# words of its own for each, given in that order, and the link goes on.
cat <<EOF | $CC -o $t/a.o -c -xc -
int main(void) { return 0; }
EOF
mkdir -p $t/dir
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-map,$t/dir -Wl,-dependency_info,$t/dir \
  -Wl,-sdk_imports,$t/dir 2> $t/log
grep warning: $t/log | sed 's/^[a-z]*: warning: //' > $t/warnings
cat > $t/expected <<EOF
Could not open or create -dependency_info file: $t/dir
could not write map file: $t/dir
can't open SDK imports file for writing at '$t/dir'
EOF
diff $t/expected $t/warnings
$RUN $t/exe
