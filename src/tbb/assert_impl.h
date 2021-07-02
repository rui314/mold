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

#ifndef __TBB_assert_impl_H
#define __TBB_assert_impl_H

#include "oneapi/tbb/detail/_config.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cstdarg>
#if _MSC_VER && _DEBUG
#include <crtdbg.h>
#endif

#include <mutex>

namespace tbb {
namespace detail {
namespace r1 {

// TODO: consider extension for formatted error description string
static void assertion_failure_impl(const char* location, int line, const char* expression, const char* comment) {

    std::fprintf(stderr, "Assertion %s failed (located in the %s function, line in file: %d)\n",
        expression, location, line);

    if (comment) {
        std::fprintf(stderr, "Detailed description: %s\n", comment);
    }
#if _MSC_VER && _DEBUG
    if (1 == _CrtDbgReport(_CRT_ASSERT, location, line, "tbb_debug.dll", "%s\r\n%s", expression, comment?comment:"")) {
        _CrtDbgBreak();
    } else
#endif
    {
        std::fflush(stderr);
        std::abort();
    }
}

void __TBB_EXPORTED_FUNC assertion_failure(const char* location, int line, const char* expression, const char* comment) {
    static std::once_flag flag;
    std::call_once(flag, [&](){ assertion_failure_impl(location, line, expression, comment); });
}

//! Report a runtime warning.
void runtime_warning( const char* format, ... ) {
    char str[1024]; std::memset(str, 0, 1024);
    va_list args; va_start(args, format);
    vsnprintf( str, 1024-1, format, args);
    va_end(args);
    fprintf(stderr, "TBB Warning: %s\n", str);
}

} // namespace r1
} // namespace detail
} // namespace tbb

#endif // __TBB_assert_impl_H

