#!/bin/bash
source "$(dirname "$0")"/common.inc
source "$(dirname "$0")"/dtrace.inc

# A probe's argument types are in its symbol's name, as dtrace -h spelled
# them, and the DOF names them as libdtrace's D compiler does, which
# ld-prime has rebuild the provider's script: qualifiers go, a struct,
# union or enum is a struct, a typedef of the D script is an int, and a
# pointer to what is (through typedefs) void, char or int is that
# one's pointer, any other one named after the type it points to.
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
cat > $t/expected <<EOF
dof __dof_types types flags 0xf align 0
attrs 0x05050400 0x01010000 0x01010000 0x05060500 0x07030200
probe floats(double, float) in main: 1 sites, 0 tests
probe ints(int, unsigned, unsigned int, short, long, long long, unsigned char) in main: 1 sites, 0 tests
probe pointers(char *, char *, void *, char **, uint8_t *, int64_t *) in main: 1 sites, 0 tests
probe sized(uint64_t, size_t, uintptr_t, pid_t, int *, char *) in main: 1 sites, 0 tests
probe tagged(struct info *, struct u *, struct e) in main: 1 sites, 0 tests
probe user(myint_t, int *, int *) in main: 1 sites, 0 tests
EOF
diff $t/dof $t/expected
