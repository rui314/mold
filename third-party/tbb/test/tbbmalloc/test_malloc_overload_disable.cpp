/*
    Copyright (c) 2018-2022 Intel Corporation

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

#define __TBB_NO_IMPLICIT_LINKAGE 1

#if _WIN32 || _WIN64
#define _CRT_SECURE_NO_WARNINGS
#endif

#include "common/test.h"

#include "common/allocator_overload.h"
#include "common/utils_report.h"
#include "common/utils_env.h"

// Disabling malloc proxy via env variable is available only on Windows for now
#if MALLOC_WINDOWS_OVERLOAD_ENABLED

#define TEST_SYSTEM_COMMAND "test_malloc_overload_disable.exe"

#include "tbb/tbbmalloc_proxy.h"

#include "src/tbb/environment.h"

const size_t SmallObjectSize = 16;
const size_t LargeObjectSize = 2*8*1024;
const size_t HugeObjectSize = 2*1024*1024;

void CheckWindowsProxyDisablingViaMemSize( size_t ObjectSize ) {
    void* ptr = malloc(ObjectSize);
    /*
     * If msize returns 0 - tbbmalloc doesn't contain this object in it`s memory
     * Also msize check that proxy lib is linked
     */
    REQUIRE_MESSAGE(!__TBB_malloc_safer_msize(ptr,nullptr), "Malloc replacement is not deactivated");
    free(ptr);
}

TEST_CASE("Disabling malloc overload") {
    if (!tbb::detail::r1::GetBoolEnvironmentVariable("TBB_MALLOC_DISABLE_REPLACEMENT"))
    {
        utils::SetEnv("TBB_MALLOC_DISABLE_REPLACEMENT","1");
        if ((system(TEST_SYSTEM_COMMAND)) != 0) {
            REPORT("Test error: unable to run the command: %s", TEST_SYSTEM_COMMAND);
            exit(-1);
        }
        // We must execute exit(0) to avoid duplicate "Done" printing.
        exit(0);
    }
    else
    {
        // Check SMALL objects replacement disable
        CheckWindowsProxyDisablingViaMemSize(SmallObjectSize);
        // Check LARGE objects replacement disable
        CheckWindowsProxyDisablingViaMemSize(LargeObjectSize);
        // Check HUGE objects replacement disable
        CheckWindowsProxyDisablingViaMemSize(HugeObjectSize);
    }
}
#endif // MALLOC_WINDOWS_OVERLOAD_ENABLED
