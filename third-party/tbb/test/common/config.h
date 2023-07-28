/*
    Copyright (c) 2005-2021 Intel Corporation

    Licensed under the Apache License, Version 2.0 (the "License");
    you may not use this file except in compliance with the License.
    You may obtain a copy of the License at

        http://www.apache.org/licenses/LICENSE-2.0

    Unless required by applicable law or agreed to in writing, software
    distributed under the License is distributed on an "AS IS" BASIS,
    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
    See the License for the specific language governing permissions and
    limitations under the License.
*/

#ifndef __TBB_test_common_config_H
#define __TBB_test_common_config_H

#if __TBB_CPF_BUILD
#ifndef  TBB_PREVIEW_FLOW_GRAPH_FEATURES
#define TBB_PREVIEW_FLOW_GRAPH_FEATURES 1
#endif
#ifndef TBB_PREVIEW_ALGORITHM_TRACE
#define TBB_PREVIEW_ALGORITHM_TRACE 1
#endif
#ifndef TBB_DEPRECATED_LIMITER_NODE_CONSTRUCTOR
#define TBB_DEPRECATED_LIMITER_NODE_CONSTRUCTOR 1
#endif
#ifndef TBB_PREVIEW_TASK_GROUP_EXTENSIONS
#define TBB_PREVIEW_TASK_GROUP_EXTENSIONS 1
#endif
#ifndef TBB_PREVIEW_CONCURRENT_LRU_CACHE
#define TBB_PREVIEW_CONCURRENT_LRU_CACHE 1
#endif
#ifndef TBB_PREVIEW_VARIADIC_PARALLEL_INVOKE
#define TBB_PREVIEW_VARIADIC_PARALLEL_INVOKE 1
#endif
#ifndef TBB_PREVIEW_BLOCKED_RANGE_ND
#define TBB_PREVIEW_BLOCKED_RANGE_ND 1
#endif
#ifndef TBB_PREVIEW_ISOLATED_TASK_GROUP
#define TBB_PREVIEW_ISOLATED_TASK_GROUP 1
#endif
#endif

#include "oneapi/tbb/detail/_config.h"
#if __FreeBSD__
#include <sys/param.h>  // for __FreeBSD_version
#endif

#if __INTEL_COMPILER
  #define __TBB_CPP14_GENERIC_LAMBDAS_PRESENT (__cplusplus >= 201402L)
#elif __clang__
  #define __TBB_CPP14_GENERIC_LAMBDAS_PRESENT (__has_feature(cxx_generic_lambdas))
#elif __GNUC__
  #define __TBB_CPP14_GENERIC_LAMBDAS_PRESENT           (__cplusplus >= 201402L)
  #define __TBB_GCC_WARNING_IGNORED_ATTRIBUTES_PRESENT  (__TBB_GCC_VERSION >= 60100)
#elif _MSC_VER
  #define __TBB_CPP14_GENERIC_LAMBDAS_PRESENT (_MSC_VER >= 1922)
#endif

// The tuple-based tests with more inputs take a long time to compile.  If changes
// are made to the tuple implementation or any switch that controls it, or if testing
// with a new platform implementation of std::tuple, the test should be compiled with
// MAX_TUPLE_TEST_SIZE >= 10 (or the largest number of elements supported) to ensure
// all tuple sizes are tested.  Expect a very long compile time.
#ifndef MAX_TUPLE_TEST_SIZE
    #define MAX_TUPLE_TEST_SIZE 10
#endif

#if MAX_TUPLE_TEST_SIZE > __TBB_VARIADIC_MAX
    #undef MAX_TUPLE_TEST_SIZE
    #define MAX_TUPLE_TEST_SIZE __TBB_VARIADIC_MAX
#endif

const unsigned MByte = 1024*1024;

#if (_WIN32 && !__TBB_WIN8UI_SUPPORT) || (__linux__ && !__ANDROID__ && !__bg__) || __FreeBSD_version >= 701000
#define __TBB_TEST_SKIP_AFFINITY 0
#else
#define __TBB_TEST_SKIP_AFFINITY 1
#endif

#endif /* __TBB_test_common_config_H */
