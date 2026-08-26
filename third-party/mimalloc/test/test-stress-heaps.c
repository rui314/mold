/* ----------------------------------------------------------------------------
Copyright (c) 2018-2026 Microsoft Research, Daan Leijen
This is free software; you can redistribute it and/or modify it under the
terms of the MIT license.
-----------------------------------------------------------------------------*/

#define TEST_STRESS          1
#define MI_USE_HEAPS         4

#if !defined(MI_TEST_LIGHT)  // too slow in test integration
#define ALLOW_LARGE          1
#endif

#include "test-stress.c"
