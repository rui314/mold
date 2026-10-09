#!/bin/bash
source "$(dirname "$0")"/common.inc

# An exception a static initializer throws unwinds through the image's
# frames. Uncaught, libc++abi reports it as it terminates the program,
# unless dyld catches it where it calls the initializer: macOS 27's
# does (dyld4::LibSystemHelpers::callInitializer), and aborts with
# nothing on stderr.
cat <<EOF | $CXX -c -o $t/a.o -xc++ -std=c++20 -
#include <exception>

class Error : public std::exception {
public:
  const char *what() const noexcept override {
    return "ERROR STRING";
  }
};

static int foo() {
  throw Error();
  return 1;
}

static inline int bar = foo();

int main() {}
EOF

$CXX --ld-path=$mold -o $t/exe $t/a.o
( set +e; $RUN $t/exe; echo "exit status $?" ) >& $t/log
grep -q 'terminating .* uncaught exception of type Error: ERROR STRING' $t/log ||
  grep -q '^exit status 134$' $t/log

# Caught in the initializer, it gets there through the same frames.
cat <<EOF | $CXX -c -o $t/b.o -xc++ -std=c++20 -
#include <cstdio>
#include <exception>

class Error : public std::exception {
public:
  const char *what() const noexcept override {
    return "ERROR STRING";
  }
};

static int foo() {
  throw Error();
  return 1;
}

static int bar = [] {
  try {
    return foo();
  } catch (const std::exception &e) {
    printf("caught %s\n", e.what());
    return 2;
  }
}();

int main() { return bar == 2 ? 0 : 1; }
EOF

$CXX --ld-path=$mold -o $t/exe2 $t/b.o
$RUN $t/exe2 | grep -q '^caught ERROR STRING$'
