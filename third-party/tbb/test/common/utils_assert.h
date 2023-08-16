/*
    Copyright (c) 2005-2023 Intel Corporation

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

#endif // __TBB_test_common_utils_assert_H
