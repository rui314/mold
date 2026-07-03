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

#ifndef __TBB_test_common_utils_assert_H
#define __TBB_test_common_utils_assert_H

#include "config.h"
#include "utils_report.h"

#include <cstdlib>
#include <csetjmp>
#include <memory>

#define REPORT_FATAL_ERROR REPORT

namespace utils {

void ReportError( const char* filename, int line, const char* expression, const char * message ) {
    print_call_stack();
    REPORT_FATAL_ERROR("%s:%d, assertion %s: %s\n", filename, line, expression, message ? message : "failed" );

    fflush(stdout); fflush(stderr);

#if _MSC_VER && _DEBUG
    if(1 == _CrtDbgReport(_CRT_ASSERT, filename, line, nullptr, "%s\r\n%s", expression, message?message:""))
        _CrtDbgBreak();
#else
    abort();
#endif
}

//! Compile-time error if x and y have different types
template<typename T>
void AssertSameType( const T& /*x*/, const T& /*y*/ ) {}

} // utils

#define ASSERT_CUSTOM(p,message,file,line)  ((p)?(void)0:utils::ReportError(file,line,#p,message))
#define ASSERT(p,message)                   ASSERT_CUSTOM(p,message,__FILE__,__LINE__)

#if TEST_CUSTOM_ASSERTION_HANDLER_ENABLED

#include "tbb/global_control.h"

 
// Suppress warning about setjmp/longjmp usage in C++ in MSVC.
// It is not guaranteed by the standard that stack-frame objects are destroyed correctly if longjmp is used.
// But since we use it only in testing and only when exceptions are not possible, it should be acceptable.
#if _MSC_VER && !TBB_USE_EXCEPTIONS
#pragma warning(disable: 4611)
#endif

//! Check that expression x raises assertion failure with message containing given substring.
/** Calls utils::SetCustomAssertionHandler to set utils::AssertionFailureHandler as a handler. */
#if TBB_USE_EXCEPTIONS
#define TEST_CUSTOM_ASSERTION_HANDLER(x, substr)                           \
    {                                                                      \
        auto default_handler = utils::SetCustomAssertionHandler();         \
        const char* message = nullptr;                                     \
        bool okay = false;                                                 \
        try {                                                              \
            x;                                                             \
        } catch(utils::AssertionFailure a) {                               \
            okay = true;                                                   \
            message = a.message;                                           \
        }                                                                  \
        utils::CheckAssertionFailure(__LINE__, #x, okay, message, substr); \
        utils::ResetAssertionHandler(default_handler);                     \
    }
#else
#define TEST_CUSTOM_ASSERTION_HANDLER(x, substr)                           \
    {                                                                      \
        auto default_handler = utils::SetCustomAssertionHandler();         \
        const char* message = nullptr;                                     \
        bool okay = false;                                                 \
        if (!setjmp(utils::g_assertion_jmp_buf)) {                         \
            x;                                                             \
        } else {                                                           \
            okay = true;                                                   \
            message = utils::g_assertion_failure->message;                 \
        }                                                                  \
        utils::CheckAssertionFailure(__LINE__, #x, okay, message, substr); \
        utils::ResetAssertionHandler(default_handler);                     \
        utils::g_assertion_failure.reset();                                \
    }
#endif // TBB_USE_EXCEPTIONS

namespace utils {

//! Exception object that holds a message.
struct AssertionFailure {
    const char* message;
    AssertionFailure( const char* filename, int line, const char* expression, const char* comment );
};

AssertionFailure::AssertionFailure( const char* filename, int line,
                                    const char* expression, const char* comment ) :
    message(comment)
{
    REQUIRE_MESSAGE(filename, "missing filename");
    REQUIRE_MESSAGE(0 < line, "line number must be positive");
    // All of our current files have fewer than 4000 lines.
    REQUIRE_MESSAGE(line < 5000, "dubiously high line number");
    REQUIRE_MESSAGE(expression, "missing expression");
}

#if TBB_USE_EXCEPTIONS
void AssertionFailureHandler(const char* filename, int line,
                             const char* expression, const char* comment) {
    throw AssertionFailure(filename, line, expression, comment);
}
#else
static std::jmp_buf g_assertion_jmp_buf;
static std::unique_ptr<AssertionFailure> g_assertion_failure {nullptr};
void AssertionFailureHandler(const char* filename, int line,
                             const char* expression, const char* comment) {

    g_assertion_failure.reset(new AssertionFailure(filename, line, expression, comment));
    std::longjmp(g_assertion_jmp_buf, 1);
}
#endif

tbb::ext::assertion_handler_type SetCustomAssertionHandler() {
    auto default_handler = tbb::ext::set_assertion_handler(AssertionFailureHandler);
    auto custom_handler = tbb::ext::get_assertion_handler();
    REQUIRE_MESSAGE(custom_handler == AssertionFailureHandler,
                    "Custom assertion handler was not set.");
    return default_handler;
}

void CheckAssertionFailure(int line, std::string expression, bool okay,
                           const char* message, std::string substr) {
    REQUIRE_MESSAGE(okay, "Line ", line, ", ", expression, " failed to fail");
    REQUIRE_MESSAGE(message, "Line ", line, ", ", expression, " failed without a message");

    std::string msg_str = message;
    REQUIRE_MESSAGE(msg_str.find(substr) != std::string::npos,
        expression, " failed with message '", msg_str, "' missing substring '", substr, "'");
}

void ResetAssertionHandler(tbb::ext::assertion_handler_type default_handler) {
    auto handler = tbb::ext::set_assertion_handler(nullptr); // Reset to default handler
    REQUIRE_MESSAGE(handler == AssertionFailureHandler,
                    "Previous assertion handler was not returned.");
    REQUIRE_MESSAGE(tbb::ext::get_assertion_handler() == default_handler,
                    "Default assertion handler was not reset.");
}

}

#endif /* TEST_CUSTOM_ASSERTION_HANDLER_ENABLED */

#endif // __TBB_test_common_utils_assert_H
