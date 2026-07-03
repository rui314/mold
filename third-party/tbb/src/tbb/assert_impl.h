/*
    Copyright (c) 2005-2025 Intel Corporation
    Copyright (c) 2025 UXL Foundation Contributors

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
#include "oneapi/tbb/detail/_utils.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cstdarg>
#if _MSC_VER && _DEBUG
#include <crtdbg.h>
#endif

#include <mutex>

#if __TBBMALLOC_BUILD
namespace rml { namespace internal {
#else
namespace tbb {
namespace detail {
namespace r1 {
#endif

// Do not move the definition into the assertion_failure_impl function because it will require
// "magic statics". It will bring a dependency on C++ runtime on some platforms while assert_impl.h
// is reused in tbbmalloc that should not depend on C++ runtime. For the same reason, we cannot use
// std::call_once here.
static std::atomic<tbb::detail::do_once_state> assertion_state;

// TODO: consider extension for formatted error description string
/* [[noreturn]] */ static void assertion_failure_impl(const char* location, int line,
                                                      const char* expression, const char* comment) {
#if __TBB_MSVC_UNREACHABLE_CODE_IGNORED
    // Workaround for erroneous "unreachable code" during assertion throwing using call_once
    #pragma warning (push)
    #pragma warning (disable: 4702)
#endif
    atomic_do_once([&](){
        std::fprintf(stderr, "Assertion %s failed (located in the %s function, line in file: %d)\n",
            expression, location, line);

        if (comment) {
            std::fprintf(stderr, "Detailed description: %s\n", comment);
        }
#if _MSC_VER && _DEBUG
        if (1 == _CrtDbgReport(_CRT_ASSERT, location, line, "tbb_debug.dll", "%s\r\n%s",
                               expression, comment?comment:"")) {
            _CrtDbgBreak();
        } else
#endif
        {
            std::fflush(stderr);
            std::abort();
        }
    }, assertion_state);
#if __TBB_MSVC_UNREACHABLE_CODE_IGNORED
    #pragma warning (pop)
#endif
}

namespace assertion_handler {
// Initial value is default handler
static std::atomic<assertion_handler_type> handler{assertion_failure_impl};

#if (__TBB_BUILD || __TBBBIND_BUILD) // only TBB and TBBBind use custom handler
static assertion_handler_type set(assertion_handler_type new_handler) noexcept {
    return handler.exchange(new_handler ? new_handler : assertion_failure_impl,
                            std::memory_order_acq_rel);
}
#endif

static assertion_handler_type get() noexcept {
    return handler.load(std::memory_order_acquire);
}
} // namespace assertion_handler

void __TBB_EXPORTED_FUNC assertion_failure(const char* location, int line,
                                           const char* expression, const char* comment) {
    assertion_handler::get()(location, line, expression, comment);
}

//! Report a runtime warning.
void runtime_warning( const char* format, ... ) {
    char str[1024]; std::memset(str, 0, 1024);
    va_list args; va_start(args, format);
    vsnprintf( str, 1024-1, format, args);
    va_end(args);
    fprintf(stderr, "TBB Warning: %s\n", str);
}

#if __TBBMALLOC_BUILD
}} // namespaces rml::internal
#else
} // namespace r1
} // namespace detail
} // namespace tbb
#endif

#endif // __TBB_assert_impl_H

