/* ----------------------------------------------------------------------------
Copyright (c) 2018-2020, Microsoft Research, Daan Leijen
This is free software; you can redistribute it and/or modify it under the
terms of the MIT license. A copy of the license can be found in the file
"LICENSE" at the root of this distribution.
-----------------------------------------------------------------------------*/
#ifndef TESTHELPER_H_
#define TESTHELPER_H_

#include <stdbool.h>
#include <stdio.h>
#include <errno.h>

// ---------------------------------------------------------------------------
// Test macros: CHECK(name,predicate) and CHECK_BODY(name,body)
// ---------------------------------------------------------------------------
static int ok = 0;
static int failed = 0;

static bool check_result(bool result, const char* testname, const char* fname, long lineno) {
  if (!(result)) {
    failed++;
    fprintf(stderr,"\n  FAILED: %s: %s:%ld\n", testname, fname, lineno);
    /* exit(1); */
  }
  else {
    ok++;
    fprintf(stderr, "ok.\n");
  }
  return true;
}

#define CHECK_BODY(name) \
  fprintf(stderr,"test: %s...  ", name ); \
  errno = 0; \
  for(bool done = false, result = true; !done; done = check_result(result,name,__FILE__,__LINE__))

#define CHECK(name,expr)      CHECK_BODY(name){ result = (expr); }

// Print summary of test. Return value can be directly use as a return value for main().
static inline int print_test_summary(void)
{
  fprintf(stderr,"\n\n---------------------------------------------\n"
                 "succeeded: %i\n"
                 "failed   : %i\n\n", ok, failed);
  return failed;
}

#endif // TESTHELPER_H_

// ------------------------------------------------------
// helper to run on threads
// ------------------------------------------------------
typedef bool (*mi_thread_fun_t)(void);

bool mi_run_on_thread(mi_thread_fun_t fun);

typedef struct mi_thread_fun_args_s {
  mi_thread_fun_t fun;
  bool result;
} mi_thread_fun_args_t;

#ifdef _WIN32
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
static DWORD WINAPI mi_win_thread_entry(LPVOID varg) {
  mi_thread_fun_args_t* arg = (mi_thread_fun_args_t*)varg;
  arg->result = arg->fun(); 
  return 0;
}
bool mi_run_on_thread(mi_thread_fun_t fun) {
  mi_thread_fun_args_t arg = { fun, false };
  HANDLE thread = CreateThread(NULL, 0, &mi_win_thread_entry, &arg, 0, NULL);
  if (thread == NULL) return false;
  WaitForSingleObject(thread, INFINITE);
  CloseHandle(thread);
  return arg.result;
}
#else
#include <pthread.h>
static void* mi_pthread_entry(void* varg) {
  mi_thread_fun_args_t* arg = (mi_thread_fun_args_t*)varg;
  arg->result = arg->fun(); 
  return NULL;
}
bool mi_run_on_thread(mi_thread_fun_t fun) {
  mi_thread_fun_args_t arg = { fun, false };
  pthread_t thread;
  if (pthread_create(&thread, NULL, &mi_pthread_entry, &arg) != 0) return false;
  pthread_join(thread, NULL);
  return arg.result;
}
#endif
