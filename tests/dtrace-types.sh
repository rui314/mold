#!/bin/bash
source "$(dirname "$0")"/common.inc
source "$(dirname "$0")"/dtrace.inc

# A probe's argument types are in its symbol's name, as dtrace -h spelled
# them, and the DOF names each of them, a typedef of the D script as an
# int: the type it stood for is gone. (ld-prime has libdtrace's D
# compiler name them, which spells some types otherwise.)
cat > $t/p.d <<EOF
typedef int myint_t;
typedef struct conn conn_t;
struct info { int a; };
union u { int a; };
enum e { E0 };
provider types {
  probe ints(int, unsigned, unsigned int, short, long, long long, unsigned char);
  probe sized(uint64_t, size_t, uintptr_t, pid_t, int32_t *, int8_t *);
  probe pointers(char *, const char *, void *, char **, uint8_t *, int64_t *);
  probe tagged(struct info *, union u *, enum e);
  probe user(myint_t, myint_t *, conn_t *);
  probe floats(double, float);
};
#pragma D attributes Evolving/Evolving/ISA provider types provider
#pragma D attributes Evolving/Stable/Common provider types name
#pragma D attributes Standard/External/Platform provider types args
EOF
dtrace_header p

cat > $t/a.c <<EOF
#include <stdint.h>
#include <sys/types.h>
typedef int myint_t;
typedef struct conn conn_t;
struct info { int a; };
union u { int a; };
enum e { E0 };
#include "p.h"
int main() {
  TYPES_INTS(0, 0, 0, 0, 0, 0, 0);
  TYPES_SIZED(0, 0, 0, 0, 0, 0);
  TYPES_POINTERS(0, 0, 0, 0, 0, 0);
  TYPES_TAGGED(0, 0, 0);
  TYPES_USER(0, 0, 0);
  TYPES_FLOATS(0, 0);
  return 0;
}
EOF
$CC -o $t/a.o -c $t/a.c
$CC --ld-path=$mold -o $t/exe $t/a.o
$t/exe

dof_dump $t/exe > $t/dof
sed -n 's/^probe \([a-z]*\)(\(.*\)) in main: 1 sites, 0 tests$/\1, \2/p' $t/dof |
  awk -F', ' '{ print $1, NF - 1 }' | sort > $t/nargs
cat > $t/expected <<EOF
floats 2
ints 7
pointers 6
sized 6
tagged 3
user 3
EOF
diff $t/nargs $t/expected
grep -q '^probe user([a-z_]*, int \*, int \*) in main' $t/dof
not grep -q conn_t $t/dof
