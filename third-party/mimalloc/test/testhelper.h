/* ----------------------------------------------------------------------------
Copyright (c) 2018-2020, Microsoft Research, Daan Leijen
This is free software; you can redistribute it and/or modify it under the
terms of the MIT license. A copy of the license can be found in the file
"LICENSE" at the root of this distribution.
-----------------------------------------------------------------------------*/
#ifndef TESTHELPER_H_
#define TESTHELPER_H_

#include <stdio.h>

// ---------------------------------------------------------------------------
// Test macros: CHECK(name,predicate) and CHECK_BODY(name,body)
// ---------------------------------------------------------------------------
static int ok = 0;
static int failed = 0;

#define CHECK_BODY(name,body) \
 do { \
  fprintf(stderr,"test: %s...  ", name ); \
  bool result = true;                                     \
  do { body } while(false);                                \
  if (!(result)) {                                        \
    failed++; \
    fprintf(stderr,                                       \
            "\n  FAILED: %s:%d:\n  %s\n",                 \
            __FILE__,                                     \
            __LINE__,                                     \
            #body);                                       \
    /* exit(1); */ \
  } \
  else { \
    ok++;                               \
    fprintf(stderr,"ok.\n");                    \
  }                                             \
 } while (false)

#define CHECK(name,expr)      CHECK_BODY(name,{ result = (expr); })

// Print summary of test. Return value can be directly use as a return value for main().
static inline int print_test_summary(void)
{
  fprintf(stderr,"\n\n---------------------------------------------\n"
                 "succeeded: %i\n"
                 "failed   : %i\n\n", ok, failed);
  return failed;
}

#endif // TESTHELPER_H_
