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

#include "TypeDefinitions.h" // Customize.h and proxy.h get included
#include "tbbmalloc_internal_api.h"

#include "../tbb/assert_impl.h" // Out-of-line TBB assertion handling routines are instantiated here.
#include "oneapi/tbb/version.h"
#include "oneapi/tbb/scalable_allocator.h"

#undef UNICODE

#if USE_PTHREAD
#include <dlfcn.h> // dlopen
#elif USE_WINTHREAD
#include <windows.h>
#endif

namespace rml {
namespace internal {

#if TBB_USE_DEBUG
#define DEBUG_SUFFIX "_debug"
#else
#define DEBUG_SUFFIX
#endif /* TBB_USE_DEBUG */

// MALLOCLIB_NAME is the name of the oneTBB memory allocator library.
#if _WIN32||_WIN64
#define MALLOCLIB_NAME "tbbmalloc" DEBUG_SUFFIX ".dll"
#elif __APPLE__
#define MALLOCLIB_NAME "libtbbmalloc" DEBUG_SUFFIX ".dylib"
#elif __FreeBSD__ || __NetBSD__ || __OpenBSD__ || __sun || _AIX || __ANDROID__
#define MALLOCLIB_NAME "libtbbmalloc" DEBUG_SUFFIX ".so"
#elif __unix__
#define MALLOCLIB_NAME "libtbbmalloc" DEBUG_SUFFIX  __TBB_STRING(.so.2)
#else
#error Unknown OS
#endif

void init_tbbmalloc() {
#if __TBB_USE_ITT_NOTIFY
    MallocInitializeITT();
#endif

/* Preventing TBB allocator library from unloading to prevent
   resource leak, as memory is not released on the library unload.
*/
#if USE_WINTHREAD && !__TBB_SOURCE_DIRECTLY_INCLUDED && !__TBB_WIN8UI_SUPPORT
    // Prevent Windows from displaying message boxes if it fails to load library
    UINT prev_mode = SetErrorMode (SEM_FAILCRITICALERRORS);
    HMODULE lib;
    BOOL ret = GetModuleHandleEx(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                                 |GET_MODULE_HANDLE_EX_FLAG_PIN,
                                 (LPCTSTR)&scalable_malloc, &lib);
    MALLOC_ASSERT(lib && ret, "Allocator can't find itself.");
    SetErrorMode (prev_mode);
#endif /* USE_PTHREAD && !__TBB_SOURCE_DIRECTLY_INCLUDED */
}

#if !__TBB_SOURCE_DIRECTLY_INCLUDED
#if USE_WINTHREAD
extern "C" BOOL WINAPI DllMain( HINSTANCE /*hInst*/, DWORD callReason, LPVOID lpvReserved)
{
    if (callReason==DLL_THREAD_DETACH)
    {
        __TBB_mallocThreadShutdownNotification();
    }
    else if (callReason==DLL_PROCESS_DETACH)
    {
        __TBB_mallocProcessShutdownNotification(lpvReserved != nullptr);
    }
    return TRUE;
}
#else /* !USE_WINTHREAD */
struct RegisterProcessShutdownNotification {
// Work around non-reentrancy in dlopen() on Android
    RegisterProcessShutdownNotification() {
        // prevents unloading, POSIX case

        // We need better support for the library pinning
        // when dlopen can't find TBBmalloc library.
        // for example: void *ret = dlopen(MALLOCLIB_NAME, RTLD_NOW);
        // MALLOC_ASSERT(ret, "Allocator can't load itself.");
        dlopen(MALLOCLIB_NAME, RTLD_NOW);
    }
    ~RegisterProcessShutdownNotification() {
        __TBB_mallocProcessShutdownNotification(false);
    }
};

static RegisterProcessShutdownNotification reg;
#endif /* !USE_WINTHREAD */
#endif /* !__TBB_SOURCE_DIRECTLY_INCLUDED */

} } // namespaces

