/*
    Copyright (c) 2005-2024 Intel Corporation

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

#ifndef __TBB_detail__export_H
#define __TBB_detail__export_H

#if defined(__MINGW32__)
    #define __TBB_EXPORT __declspec(dllexport)
#elif defined(_WIN32) // Use .def files for these
    #define __TBB_EXPORT
#elif defined(__unix__) || defined(__APPLE__) // Use .def files for these
    #define __TBB_EXPORT __attribute__ ((visibility ("default")))
#else
    #error "Unknown platform/compiler"
#endif

#if __TBB_BUILD
    #define TBB_EXPORT __TBB_EXPORT
#else
    #define TBB_EXPORT
#endif

#if __TBBMALLOC_BUILD
    #define TBBMALLOC_EXPORT __TBB_EXPORT
#else
    #define TBBMALLOC_EXPORT
#endif

#if __TBBMALLOCPROXY_BUILD
    #define TBBMALLOCPROXY_EXPORT __TBB_EXPORT
#else
    #define TBBMALLOCPROXY_EXPORT
#endif

#if __TBBBIND_BUILD
    #define TBBBIND_EXPORT __TBB_EXPORT
#else
    #define TBBBIND_EXPORT
#endif

#endif
