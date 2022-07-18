#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$MACHINE/$testname
mkdir -p $t

cat <<EOF | clang++ -c -o $t/a.o -xobjective-c++ -
#import <Foundation/Foundation.h>

#include <libunwind.h>
#include <objc/objc-exception.h>

static id exception_processor(id exception) {
  unw_context_t context;
  unw_getcontext(&context);

  unw_cursor_t cursor;
  unw_init_local(&cursor, &context);

  do {
    unw_proc_info_t frame_info;
    if (unw_get_proc_info(&cursor, &frame_info) != UNW_ESUCCESS) {
      NSLog(@"unw_get_proc_info failed");
      continue;
    }

    char proc_name[64] = "";
    unw_word_t offset;
    unw_get_proc_name(&cursor, proc_name, sizeof(proc_name), &offset);

    NSLog(@"proc_name=%s has_handler=%d", proc_name, frame_info.handler != 0);
  } while (unw_step(&cursor) > 0);

  return exception;
}

void throw_exception() {
  [NSException raise:@"foo" format:@"bar"];
}

int main(int argc, char **argv) {
  objc_setExceptionPreprocessor(&exception_processor);
  @try {
    throw_exception();
  } @catch (id exception) {
    NSLog(@"caught an exception");
  }
  return 0;
}
EOF

clang --ld-path=./ld64 -o $t/exe $t/a.o -framework Foundation
$t/exe 2>&1 | grep -q 'proc_name=objc_exception_throw has_handler=0'
$t/exe 2>&1 | grep -q 'proc_name=main has_handler=1'

echo OK
