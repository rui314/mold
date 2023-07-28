/*
    Copyright (c) 2005-2022 Intel Corporation

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

#ifndef __TBB_test_common_utils_env_H
#define __TBB_test_common_utils_env_H

#include "test.h"

#include <cstdlib> // getenv/setenv

namespace utils {

int SetEnv( const char *envname, const char *envval ) {
    CHECK_MESSAGE( (envname && envval), "Harness::SetEnv() requires two valid C strings" );
#if __TBB_WIN8UI_SUPPORT
    CHECK_MESSAGE( false, "Harness::SetEnv() should not be called in code built for win8ui" );
    return -1;
#elif !(_MSC_VER || __MINGW32__ || __MINGW64__)
    // On POSIX systems use setenv
    return setenv(envname, envval, /*overwrite=*/1);
#elif __STDC_SECURE_LIB__>=200411
    // this macro is set in VC & MinGW if secure API functions are present
    return _putenv_s(envname, envval);
#else
    // If no secure API on Windows, use _putenv
    size_t namelen = strlen(envname), valuelen = strlen(envval);
    char* buf = new char[namelen+valuelen+2];
    strncpy(buf, envname, namelen);
    buf[namelen] = '=';
    strncpy(buf+namelen+1, envval, valuelen);
    buf[namelen+1+valuelen] = char(0);
    int status = _putenv(buf);
    delete[] buf;
    return status;
#endif
}

char* GetEnv(const char *envname) {
    CHECK_MESSAGE(envname, "Harness::GetEnv() requires a valid C string");
#if __TBB_WIN8UI_SUPPORT
    return nullptr;
#else
    return std::getenv(envname);
#endif
}

} // namespace utils

#endif // __TBB_test_common_utils_env_H

