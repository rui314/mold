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

#ifndef __TBB_test_common_inject_scheduler_H
#define __TBB_test_common_inject_scheduler_H

#if __TBB_TEST_DEFINE_PRIVATE_PUBLIC
// Include STL headers first to avoid errors
#include <string>
#include <algorithm>
#define private public
#define protected public
#endif // __TBB_TEST_DEFINE_PRIVATE_PUBLIC

// #define __TBB_NO_IMPLICIT_LINKAGE 1 // TODO: check if we need this macro in Malloc or TBB

#define __TBB_BUILD 1

#define __TBB_SOURCE_DIRECTLY_INCLUDED 1
// TODO: uncomment scheduler source files and fix linkage errors
// #include "../../src/tbb/main.cpp"
#include "../../src/tbb/dynamic_link.cpp"
// #include "../../src/tbb/misc_ex.cpp"

// Tasking subsystem files
// #include "../../src/tbb/governor.cpp"
// #include "../../src/tbb/market.cpp"
// #include "../../src/tbb/arena.cpp"
// #include "../../src/tbb/observer_proxy.cpp"
// #include "../../src/tbb/task.cpp"
// #include "../../src/tbb/task_group_context.cpp"

// Other dependencies
// #include "../../src/tbb/private_server.cpp"
#include "../../src/tbb/concurrent_monitor.h"
#if _WIN32 || _WIN64
#include "../../src/tbb/semaphore.cpp"
#endif
#include "../../src/tbb/rml_tbb.cpp"

#if __TBB_TEST_DEFINE_PRIVATE_PUBLIC
#undef protected
#undef private
#endif

#endif // __TBB_test_common_inject_scheduler_H
