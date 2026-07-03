/*
    Copyright (c) 2005-2025 Intel Corporation

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

#ifndef __TBB_dynamic_link
#define __TBB_dynamic_link

// Support for dynamic loading entry points from other shared libraries.

#if TBB_DYNAMIC_LINK_WARNING && __STDC_LIB_EXT1__
// Optional TBB-based messaging in case of dynamic link errors was requested. fprintf_s is also
// supported. So, enabling it.
#define __STDC_WANT_LIB_EXT1__ 1
#endif

#include "oneapi/tbb/detail/_config.h"

#include <atomic>
#include <mutex>

#include <cstddef>

#ifdef _WIN32
#include <windows.h>
#else
#include <sys/stat.h>
#endif /* _WIN32 */

namespace tbb {
namespace detail {
namespace r1 {

//! Type definition for a pointer to a void somefunc(void)
typedef void (*pointer_to_handler)();

//! The helper to construct dynamic_link_descriptor structure
// Double cast through the void* in DLD macro is necessary to
// prevent warnings from some compilers (g++ 4.1)
#if __TBB_WEAK_SYMBOLS_PRESENT
#define DLD(s,h) {#s, (pointer_to_handler*)(void*)(&h), (pointer_to_handler)&s}
#define DLD_NOWEAK(s,h) {#s, (pointer_to_handler*)(void*)(&h), nullptr}
#else
#define DLD(s,h) {#s, (pointer_to_handler*)(void*)(&h)}
#define DLD_NOWEAK(s,h) DLD(s,h)
#endif /* __TBB_WEAK_SYMBOLS_PRESENT */
//! Association between a handler name and location of pointer to it.
struct dynamic_link_descriptor {
    //! Name of the handler
    const char* name;
    //! Pointer to the handler
    pointer_to_handler* handler;
#if __TBB_WEAK_SYMBOLS_PRESENT
    //! Weak symbol
    pointer_to_handler ptr;
#endif
};

#if _WIN32
using dynamic_link_handle = HMODULE;
#else
using dynamic_link_handle = void*;
#endif /* _WIN32 */

const int DYNAMIC_LINK_GLOBAL                 = 0x01;
const int DYNAMIC_LINK_LOAD                   = 0x02;
const int DYNAMIC_LINK_WEAK                   = 0x04;
const int DYNAMIC_LINK_LOCAL                  = 0x08;
const int DYNAMIC_LINK_BUILD_ABSOLUTE_PATH    = 0x10;

const int DYNAMIC_LINK_LOCAL_BINDING = DYNAMIC_LINK_BUILD_ABSOLUTE_PATH | DYNAMIC_LINK_LOCAL
                                       | DYNAMIC_LINK_LOAD;
const int DYNAMIC_LINK_DEFAULT       = DYNAMIC_LINK_BUILD_ABSOLUTE_PATH | DYNAMIC_LINK_GLOBAL
                                       | DYNAMIC_LINK_LOAD | DYNAMIC_LINK_WEAK;

//! Fill in dynamically linked handlers.
/** 'library' is the name of the requested library. It should not contain a full
    path. If DYNAMIC_LINK_BUILD_ABSOLUTE_PATH is specified in the 'flags' then
    the function adds the full path to the 'library' name by prepending the path
    from which the runtime itself was loaded. 'required' is the number of the
    initial entries in the array descriptors[] that have to be found in order
    for the call to succeed. If the library and all the required handlers are
    found, then the corresponding handler pointers are set, and the return value
    is true. Otherwise the original array of descriptors is left untouched and
    the return value is false. 'required' is limited by 20 (exceeding of this
    value will result in failure to load the symbols and the return value will
    be false). 'handle' is the handle of the library if it is loaded. Otherwise
    it is left untouched. 'flags' is the set of DYNAMIC_LINK_* flags. Each of
    the DYNAMIC_LINK_* flags allows its corresponding linking stage.
**/
bool dynamic_link( const char* library,
                   const dynamic_link_descriptor descriptors[],
                   std::size_t required,
                   dynamic_link_handle* handle = nullptr,
                   int flags = DYNAMIC_LINK_DEFAULT );

void dynamic_unlink( dynamic_link_handle handle );

void dynamic_unlink_all();

// The enum lists possible errors that can appear during dynamic linking. To
// print detailed information when the errors appear, DYNAMIC_LINK_WARNING macro
// needs to be defined, accepting one of these enum values as its first
// parameter and a variable parameter args. The parameters in this list are
// described below per each error along with the situation when it arises. The
// enumeration starts from '1' to distinguish the error values from no error
// value, which is usually equals to zero.
//
// To use the default implementation for DYNAMIC_LINK_WARNING macro,
// TBB_DYNAMIC_LINK_WARNING macro needs to be set during compilation.
//
// Note: dlerr_t depends on OS: it is char const * on Linux* and macOS*, int on
// Windows*.
enum dynamic_link_error_t : int {
    // Library is not found
    dl_lib_not_found = 1,       // char const * lib, dlerr_t err

    // Symbol is not found
    dl_sym_not_found,           // char const * sym, dlerr_t err

    // System call returned error status
    dl_sys_fail,                // char const * func, dlerr_t err

    // Internal intermediate buffer is too small, consider setting PATH_MAX
    // macro to a larger value
    dl_buff_too_small,          // none

    // An error during library unload
    dl_unload_fail,             // char const * lib, dlerr_t err

    // Library is unsigned or has invalid signature
    dl_lib_unsigned,            // char const * lib

    // Unknown error during signature verification
    dl_sig_err_unknown,         // char const * lib, dlerr_t err

    // Signing certificate is explicitly distrusted by admin or user
    dl_sig_explicit_distrust,   // char const * lib

    // Certificate chain is terminated in a untrusted root certificate
    dl_sig_untrusted_root,      // char const * lib

    // Signature is not trusted
    dl_sig_distrusted,          // char const * lib

    // Hash or publisher was not explicitly trusted and user trust was not
    // allowed
    dl_sig_security_settings,   // char const * lib

    // Other error, 'err' contains system error code
    dl_sig_other_error          // char const * lib, dlerr_t err
}; // dynamic_link_error_t

} // namespace r1
} // namespace detail
} // namespace tbb

#endif /* __TBB_dynamic_link */
