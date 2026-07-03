/*
    Copyright (c) 2025 Intel Corporation
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

#ifndef __TBB_load_tbbbind_H
#define __TBB_load_tbbbind_H

#include "oneapi/tbb/version.h" // __TBB_STRING

#if _WIN32 || _WIN64 || __unix__ || __APPLE__

#if TBB_USE_DEBUG
#define DEBUG_SUFFIX "_debug"
#else
#define DEBUG_SUFFIX
#endif /* TBB_USE_DEBUG */

#if _WIN32 || _WIN64
#define LIBRARY_EXTENSION ".dll"
#define LIBRARY_PREFIX
#elif __APPLE__
#define LIBRARY_EXTENSION __TBB_STRING(.3.dylib)
#define LIBRARY_PREFIX "lib"
#elif __unix__
#define LIBRARY_EXTENSION __TBB_STRING(.so.3)
#define LIBRARY_PREFIX "lib"
#endif /* __unix__ */

#define TBBBIND_NAME            LIBRARY_PREFIX "tbbbind"            DEBUG_SUFFIX LIBRARY_EXTENSION
#define TBBBIND_2_0_NAME        LIBRARY_PREFIX "tbbbind_2_0"        DEBUG_SUFFIX LIBRARY_EXTENSION
#define TBBBIND_2_5_NAME        LIBRARY_PREFIX "tbbbind_2_5"        DEBUG_SUFFIX LIBRARY_EXTENSION

// different versions of TBBbind are tried in that order
static const char* tbbbind_libraries_list[] = {
    TBBBIND_2_5_NAME,
    TBBBIND_2_0_NAME,
    TBBBIND_NAME
};

#endif /* _WIN32 || _WIN64 || __unix__ || __APPLE__ */

#endif /* __TBB_load_tbbbind_H */
