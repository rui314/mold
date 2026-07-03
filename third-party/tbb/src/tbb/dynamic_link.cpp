/*
    Copyright (c) 2005-2025 Intel Corporation
    Copyright (c) 2026 UXL Foundation Contributors

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

#include "dynamic_link.h"
#include "environment.h"

#include "oneapi/tbb/detail/_template_helpers.h"
#include "oneapi/tbb/detail/_utils.h"
#if DYNAMIC_LINK_FIND_LIB_WITH_TBB_RUNTIME_VERSION
#include "oneapi/tbb/version.h" // to get TBB_runtime_version
#endif

/*
    This file can be used by both TBB and OpenMP RTL. Do not use __TBB_ASSERT() macro
    and runtime_warning() function because they are not available in OpenMP. Use
    __TBB_ASSERT_EX and DYNAMIC_LINK_WARNING instead.
*/

#include <cstdarg>          // va_list etc.
#include <cstring>          // strrchr, memset

#if _WIN32
    // Unify system calls
    #define dlopen( name, flags )   LoadLibraryEx( name, /*reserved*/NULL, flags )
    #define dlsym( handle, name )   GetProcAddress( handle, name )
    // FreeLibrary return bool value that is not used.
    #define dlclose( handle )       (void)( ! FreeLibrary( handle ) )
    #define dlerror()               GetLastError()
#if !__TBB_SKIP_DEPENDENCY_SIGNATURE_VERIFICATION
    #include <Softpub.h>
    #include <wintrust.h>
    #pragma comment (lib, "wintrust")     // Link with the Wintrust.lib file.
#endif
#ifndef PATH_MAX
    #define PATH_MAX                MAX_PATH
#endif
#else /* _WIN32 */
    #include <dlfcn.h>
    #include <unistd.h>

    #include <climits>
    #include <cstdlib>
#endif /* _WIN32 */

#if __TBB_WEAK_SYMBOLS_PRESENT && !__TBB_DYNAMIC_LOAD_ENABLED
    //TODO: use function attribute for weak symbols instead of the pragma.
    #pragma weak dlopen
    #pragma weak dlsym
    #pragma weak dlclose
#endif /* __TBB_WEAK_SYMBOLS_PRESENT && !__TBB_DYNAMIC_LOAD_ENABLED */


#define __USE_STATIC_DL_INIT    ( !__ANDROID__ )


/*
dynamic_link is a common interface for searching for required symbols in an
executable and dynamic libraries.

dynamic_link provides certain guarantees:
  1. Either all or none of the requested symbols are resolved. Moreover, if
  symbols are not resolved, the dynamic_link_descriptor table is not modified;
  2. All returned symbols have secured lifetime: this means that none of them
  can be invalidated until dynamic_unlink is called;
  3. To avoid security issues caused by loading libraries from insecure paths,
  the loading can be made via the full path and/or with validatiion of a
  signature on Windows platforms. In any case, current working directory is
  excluded from the loader consideration.

dynamic_link searches for the requested symbols in three stages, stopping as
soon as all of the symbols have been resolved.

  1. Search the global scope:
    a. On Windows: dynamic_link tries to obtain the handle of the requested
    library and if it succeeds it resolves the symbols via that handle.
    b. On Linux: dynamic_link tries to search for the symbols in the global
    scope via the main program handle. If the symbols are present in the global
    scope their lifetime is not guaranteed (since dynamic_link does not know
    anything about the library from which they are exported). Therefore it
    tries to "pin" the symbols by obtaining the library name and reopening it.
    dlopen may fail to reopen the library in two cases:
       i. The symbols are exported from the executable. Currently dynamic_link
      cannot handle this situation, so it will not find these symbols in this
      step.
      ii. The necessary library has been unloaded and cannot be reloaded. It
      seems there is nothing that can be done in this case. No symbols are
      returned.

  2. Dynamic load: an attempt is made to load the requested library via the
  full path.
    By default, the full path used is that from which the runtime itself was
    loaded. On Windows the full path is determined by using system facilities
    with subsequent signature validation. If the library can be loaded, then an
    attempt is made to resolve the requested symbols in the newly loaded
    library. If the symbols are not found the library is unloaded.

  3. Weak symbols: if weak symbols are available they are returned.
*/

#if __STDC_WANT_LIB_EXT1__
#include <stdio.h>              // fprintf_s
#define TBB_FPRINTF fprintf_s
#else
// fprintf_s is not supported by the implementation, fallback to standard fprintf
#include <cstdio>               // fprintf
#define TBB_FPRINTF std::fprintf
#endif

namespace tbb {
namespace detail {
namespace r1 {

#if __TBB_WEAK_SYMBOLS_PRESENT || __TBB_DYNAMIC_LOAD_ENABLED

#if !defined(DYNAMIC_LINK_WARNING) && !__TBB_WIN8UI_SUPPORT && __TBB_DYNAMIC_LOAD_ENABLED
    // Report runtime errors and continue.
    #define DYNAMIC_LINK_WARNING dynamic_link_warning
#if TBB_DYNAMIC_LINK_WARNING
#define __DYNAMIC_LINK_REPORT_SIGNATURE_ERRORS 1
    // Accepting 'int' instead of 'dynamic_link_error_t' allows to avoid the warning about undefined
    // behavior when an object passed to 'va_start' undergoes default argument promotion. Yet the
    // implicit promotion from 'dynamic_link_error_t' to its underlying type done at the place of a
    // call is supported.
    static void dynamic_link_warning( int code, ... ) {
        const char* prefix = "oneTBB dynamic link warning:";
        const char* str = nullptr;
        // Note: dlerr_t depends on OS: it is char const * on Linux* and macOS*, int on Windows*.
#if _WIN32
#if __INTEL_LLVM_COMPILER || __clang__
// Suppress the incorrect warning about the format specifier for the unsigned long long type.
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wformat"
#endif
        #define DLERROR_SPECIFIER "%ul"
        typedef DWORD dlerr_t;
#else
        #define DLERROR_SPECIFIER "%s"
        typedef const char* dlerr_t;
#endif                          // _WIN32
        dlerr_t error = 0;

        std::va_list args;
        va_start(args, code);

        switch (code) {
        case dl_lib_not_found:
            str = va_arg(args, const char*);
            error = va_arg(args, dlerr_t);
            TBB_FPRINTF(stderr, "%s The module \"%s\" was not found. System error: "
                        DLERROR_SPECIFIER "\n", prefix, str, error);
            break;
        case dl_sym_not_found:     // char const * sym, dlerr_t err:
            str = va_arg(args, const char*);
            error = va_arg(args, dlerr_t);
            TBB_FPRINTF(stderr, "%s The symbol \"%s\" was not resolved. System error: "
                        DLERROR_SPECIFIER "\n", prefix, str, error);
            break;
        case dl_sys_fail:
            str = va_arg(args, const char*);
            error = va_arg(args, dlerr_t);
            TBB_FPRINTF(stderr, "%s A call to \"%s\" failed with error " DLERROR_SPECIFIER "\n",
                        prefix, str, error);
            break;
        case dl_buff_too_small:
            TBB_FPRINTF(stderr, "%s An internal buffer representing a path to dynamically loaded "
                        "module is small. Consider compiling with larger value for PATH_MAX macro.\n",
                        prefix);
            break;
        case dl_unload_fail:
            str = va_arg(args, const char*);
            error = va_arg(args, dlerr_t);
            TBB_FPRINTF(stderr, "%s Error unloading the module \"%s\": " DLERROR_SPECIFIER "\n",
                        prefix, str, error);
            break;
        case dl_lib_unsigned:
            str = va_arg(args, const char*);
            TBB_FPRINTF(stderr, "%s The module \"%s\" is unsigned or has invalid signature.\n",
                        prefix, str);
            break;
        case dl_sig_err_unknown:
            str = va_arg(args, const char*);
            error = va_arg(args, dlerr_t);
            TBB_FPRINTF(stderr, "%s The signature verification of the module \"%s\" results in "
                        "unknown error:" DLERROR_SPECIFIER "\n", prefix, str, error);
            break;
        case dl_sig_explicit_distrust:
            str = va_arg(args, const char*);
            TBB_FPRINTF(stderr, "%s The certificate with which the module \"%s\" is signed is "
                        "explicitly distrusted by an admin or user.\n", prefix, str);
            break;
        case dl_sig_untrusted_root:
            str = va_arg(args, const char*);
            TBB_FPRINTF(stderr, "%s The signature verification process for the module \"%s\" has "
                        "terminated in a root certificate which is not trusted.\n", prefix, str);
            break;
        case dl_sig_distrusted:
            str = va_arg(args, const char*);
            TBB_FPRINTF(stderr, "%s The signature of the module \"%s\" is not trusted.\n", prefix,
                        str);
            break;
        case dl_sig_security_settings:
            str = va_arg(args, const char*);
            TBB_FPRINTF(stderr, "%s The hash or publisher of the module \"%s\" was not explicitly "
                        "trusted and user trust was not allowed.\n", prefix, str);
            break;
        case dl_sig_other_error:
            str = va_arg(args, const char*);
            error = va_arg(args, dlerr_t);
            TBB_FPRINTF(stderr, "%s Signature verification for the module \"%s\" failed. System "
                        "error code: " DLERROR_SPECIFIER "\n", prefix, str, error);
            break;
        }

        va_end(args);
    } // library_warning
#undef DLERROR_SPECIFIER
#if _WIN32 && __INTEL_LLVM_COMPILER
#pragma GCC diagnostic pop
#endif
#else
    static void dynamic_link_warning( int code, ... ) {
        suppress_unused_warning(code);
    } // library_warning
#endif  // TBB_DYNAMIC_LINK_WARNING

#elif defined(DYNAMIC_LINK_WARNING)
#define __DYNAMIC_LINK_REPORT_SIGNATURE_ERRORS 1
#else
#define DYNAMIC_LINK_WARNING(...) (void)0
#endif /* !defined(DYNAMIC_LINK_WARNING) && !__TBB_WIN8UI_SUPPORT && __TBB_DYNAMIC_LOAD_ENABLED */

    static bool resolve_symbols( dynamic_link_handle module, const dynamic_link_descriptor descriptors[], std::size_t required )
    {
        if ( !module )
            return false;

        #if !__TBB_DYNAMIC_LOAD_ENABLED /* only __TBB_WEAK_SYMBOLS_PRESENT is defined */
            if ( !dlsym ) return false;
        #endif /* !__TBB_DYNAMIC_LOAD_ENABLED */

        const std::size_t n_desc=20; // Usually we don't have more than 20 descriptors per library
        __TBB_ASSERT_EX( required <= n_desc, "Too many descriptors is required" );
        if ( required > n_desc ) return false;
        pointer_to_handler h[n_desc];

        for ( std::size_t k = 0; k < required; ++k ) {
            dynamic_link_descriptor const & desc = descriptors[k];
            pointer_to_handler addr = (pointer_to_handler)dlsym( module, desc.name );
            if ( !addr ) {
                DYNAMIC_LINK_WARNING( dl_sym_not_found, desc.name, dlerror() );
                return false;
            }
            h[k] = addr;
        }

        // Commit the entry points.
        // Cannot use memset here, because the writes must be atomic.
        for( std::size_t k = 0; k < required; ++k )
            *descriptors[k].handler = h[k];
        return true;
    }

#if __TBB_WIN8UI_SUPPORT
    bool dynamic_link( const char*  library, const dynamic_link_descriptor descriptors[], std::size_t required, dynamic_link_handle*, int flags ) {
        dynamic_link_handle tmp_handle = nullptr;
        TCHAR wlibrary[256];
        if ( MultiByteToWideChar(CP_UTF8, 0, library, -1, wlibrary, 255) == 0 ) return false;
        if ( flags & DYNAMIC_LINK_LOAD )
            tmp_handle = LoadPackagedLibrary( wlibrary, 0 );
        if (tmp_handle != nullptr){
            return resolve_symbols(tmp_handle, descriptors, required);
        }else{
            return false;
        }
    }
    void dynamic_unlink( dynamic_link_handle ) {}
    void dynamic_unlink_all() {}
#else
#if __TBB_DYNAMIC_LOAD_ENABLED
/*
    There is a security issue on Windows: LoadLibrary() may load and execute malicious code. To
    avoid the issue, we have to exclude working directory from the list of directories in which
    loader searches for the library. This is done by passing LOAD_LIBRARY_SAFE_CURRENT_DIRS flag to
    LoadLibraryEx. To further strengthen the security, library signature is verified.

    Also, the default approach is to load the library via full path. This
    function constructs full path to the specified library (it is assumed the
    library located side-by-side with the tbb.dll.

    The function constructs absolute path for given relative path. Important: Base directory is not
    current one, it is the directory tbb.dll loaded from.

    Example:
        Let us assume "tbb.dll" is located in "c:\program files\common\intel\" directory, e.g.
        absolute path of the library is "c:\program files\common\intel\tbb.dll". Absolute path for
        "tbbmalloc.dll" would be "c:\program files\common\intel\tbbmalloc.dll". Absolute path for
        "malloc\tbbmalloc.dll" would be "c:\program files\common\intel\malloc\tbbmalloc.dll".
*/

    // Struct handle_storage is used by dynamic_link routine to store handles of
    // all loaded or pinned dynamic libraries. When TBB is shut down, it calls
    // dynamic_unlink_all() that unloads modules referenced by handle_storage.
    // This struct should not have any constructors since it may be used before
    // the constructor is called.
    #define MAX_LOADED_MODULES 8 // The number of maximum possible modules which can be loaded

    using atomic_incrementer = std::atomic<std::size_t>;

    static struct handles_t {
        atomic_incrementer my_size;
        dynamic_link_handle my_handles[MAX_LOADED_MODULES];

        void add(const dynamic_link_handle &handle) {
            const std::size_t ind = my_size++;
            __TBB_ASSERT_EX( ind < MAX_LOADED_MODULES, "Too many modules are loaded" );
            my_handles[ind] = handle;
        }

        void free() {
            const std::size_t size = my_size;
            for (std::size_t i=0; i<size; ++i)
                dynamic_unlink( my_handles[i] );
        }
    } handles;

    static std::once_flag init_dl_data_state;

    static struct ap_data_t {
        char _path[PATH_MAX+1];
        std::size_t _len;
    } ap_data;

    static void init_ap_data() {
    #if _WIN32
        // Get handle of our DLL first.
        HMODULE handle;
        BOOL brc = GetModuleHandleEx(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            (LPCSTR)( & dynamic_link ), // any function inside the library can be used for the address
            & handle
            );
        if ( !brc ) { // Error occurred.
            int err = GetLastError();
            DYNAMIC_LINK_WARNING( dl_sys_fail, "GetModuleHandleEx", err );
            return;
        }
        // Now get path to our DLL.
        DWORD drc = GetModuleFileName( handle, ap_data._path, static_cast< DWORD >( PATH_MAX ) );
        if ( drc == 0 ) { // Error occurred.
            int err = GetLastError();
            DYNAMIC_LINK_WARNING( dl_sys_fail, "GetModuleFileName", err );
            return;
        }
        if ( drc >= PATH_MAX ) { // Buffer too short.
            DYNAMIC_LINK_WARNING( dl_buff_too_small );
            return;
        }
        // Find the position of the last backslash.
        char *backslash = std::strrchr( ap_data._path, '\\' );

        if ( !backslash ) {    // Backslash not found.
            __TBB_ASSERT_EX( backslash != nullptr, "Unbelievable.");
            return;
        }
        __TBB_ASSERT_EX( backslash >= ap_data._path, "Unbelievable.");
        ap_data._len = (std::size_t)(backslash - ap_data._path) + 1;
        *(backslash+1) = 0;
    #else
        // There is an use case, when we want to find TBB library, not just some shared object
        // providing "dynamic_link" symbol (it can be shared object that directly includes
        // dynamic_link.cpp). For this case we use a public TBB symbol. Searching for public symbol
        // in every case leads to finding main executable instead of TBB library on some version of
        // Linux.
        #if DYNAMIC_LINK_FIND_LIB_WITH_TBB_RUNTIME_VERSION
        static void *func_from_lib = (void*)&TBB_runtime_version;
        #else
        static void *func_from_lib = (void*)&dynamic_link;
        #endif

        // Get the library path
        Dl_info dlinfo;
        int res = dladdr( func_from_lib, &dlinfo );
        if ( !res ) {
            char const * err = dlerror();
            DYNAMIC_LINK_WARNING( dl_sys_fail, "dladdr", err );
            return;
        } else {
            __TBB_ASSERT_EX( dlinfo.dli_fname!=nullptr, "Unbelievable." );
        }

        char const *slash = std::strrchr( dlinfo.dli_fname, '/' );
        std::size_t fname_len=0;
        if ( slash ) {
            __TBB_ASSERT_EX( slash >= dlinfo.dli_fname, "Unbelievable.");
            fname_len = (std::size_t)(slash - dlinfo.dli_fname) + 1;
        }

        std::size_t rc;
        if ( dlinfo.dli_fname[0]=='/' ) {
            // The library path is absolute
            rc = 0;
            ap_data._len = 0;
        } else {
            // The library path is relative so get the current working directory
            if ( !getcwd( ap_data._path, sizeof(ap_data._path)/sizeof(ap_data._path[0]) ) ) {
                DYNAMIC_LINK_WARNING( dl_buff_too_small );
                return;
            }
            ap_data._len = std::strlen( ap_data._path );
            ap_data._path[ap_data._len++]='/';
            rc = ap_data._len;
        }

        if ( fname_len>0 ) {
            ap_data._len += fname_len;
            if ( ap_data._len>PATH_MAX ) {
                DYNAMIC_LINK_WARNING( dl_buff_too_small );
                ap_data._len=0;
                return;
            }
            std::strncpy( ap_data._path+rc, dlinfo.dli_fname, fname_len );
            ap_data._path[ap_data._len]=0;
        }
    #endif /* _WIN32 */
    }

    static void init_dl_data() {
        init_ap_data();
    }

    /*
        The function constructs absolute path for given relative path. Important: Base directory is not
        current one, it is the directory libtbb.so loaded from.

        Arguments:
        in  name -- Name of a file (may be with relative path; it must not be an absolute one).
        out path -- Buffer to save result (absolute path) to.
        in  len  -- Size of buffer.
        ret      -- 0         -- Error occurred.
                    > len     -- Buffer too short, required size returned.
                    otherwise -- Ok, number of characters (incl. terminating null) written to buffer.
    */
    static std::size_t abs_path( char const * name, char * path, std::size_t len ) {
        if ( !name || ap_data._len == 0 )
            return 0;

        std::size_t name_len = std::strlen( name );
        std::size_t full_len = name_len+ap_data._len;
        if ( full_len < len ) {
            __TBB_ASSERT_EX( ap_data._path[ap_data._len] == 0, nullptr );
            __TBB_ASSERT_EX( std::strlen(ap_data._path) == ap_data._len, nullptr );
            std::strncpy( path, ap_data._path, ap_data._len + 1 );
            __TBB_ASSERT_EX( path[ap_data._len] == 0, nullptr );
            std::strncat( path, name, len - ap_data._len );
            __TBB_ASSERT_EX( std::strlen(path) == full_len, nullptr );
        }
        return full_len+1; // +1 for null character
    }
#endif  // __TBB_DYNAMIC_LOAD_ENABLED
    void init_dynamic_link_data() {
    #if __TBB_DYNAMIC_LOAD_ENABLED
        std::call_once( init_dl_data_state, init_dl_data );
    #endif
    }

    #if __USE_STATIC_DL_INIT
    // ap_data structure is initialized with current directory on Linux.
    // So it should be initialized as soon as possible since the current directory may be changed.
    // static_init_dl_data_t object provides this initialization during library loading.
    static struct static_init_dl_data_t {
        static_init_dl_data_t() {
            init_dynamic_link_data();
        }
    } static_init_dl_data;
    #endif

    #if __TBB_WEAK_SYMBOLS_PRESENT
    static bool weak_symbol_link( const dynamic_link_descriptor descriptors[], std::size_t required )
    {
        // Check if the required entries are present in what was loaded into our process.
        for ( std::size_t k = 0; k < required; ++k )
            if ( !descriptors[k].ptr )
                return false;
        // Commit the entry points.
        for ( std::size_t k = 0; k < required; ++k )
            *descriptors[k].handler = (pointer_to_handler) descriptors[k].ptr;
        return true;
    }
    #else
    static bool weak_symbol_link( const dynamic_link_descriptor[], std::size_t ) {
        return false;
    }
    #endif /* __TBB_WEAK_SYMBOLS_PRESENT */

    void dynamic_unlink( dynamic_link_handle handle ) {
    #if !__TBB_DYNAMIC_LOAD_ENABLED /* only __TBB_WEAK_SYMBOLS_PRESENT is defined */
        if ( !dlclose ) return;
    #endif
        if ( handle ) {
            dlclose( handle );
        }
    }

    void dynamic_unlink_all() {
    #if __TBB_DYNAMIC_LOAD_ENABLED
        handles.free();
    #endif
    }

    static dynamic_link_handle global_symbols_link(const char* library,
                                                   const dynamic_link_descriptor descriptors[],
                                                   std::size_t required )
    {
        dynamic_link_handle library_handle{};
#if _WIN32
        auto res = GetModuleHandleEx(0, library, &library_handle);
        __TBB_ASSERT_EX((res && library_handle) || (!res && !library_handle), nullptr);
#else /* _WIN32 */
    #if !__TBB_DYNAMIC_LOAD_ENABLED /* only __TBB_WEAK_SYMBOLS_PRESENT is defined */
        if ( !dlopen ) return 0;
    #endif /* !__TBB_DYNAMIC_LOAD_ENABLED */
        // RTLD_GLOBAL - to guarantee that old TBB will find the loaded library
        // RTLD_NOLOAD - not to load the library without the full path
        library_handle = dlopen(library, RTLD_LAZY | RTLD_GLOBAL | RTLD_NOLOAD);
#endif /* _WIN32 */
        if (library_handle) {
            if (!resolve_symbols(library_handle, descriptors, required)) {
                dynamic_unlink(library_handle);
                library_handle = nullptr;
            }
        }
        return library_handle;
    }

    static void save_library_handle( dynamic_link_handle src, dynamic_link_handle *dst ) {
        __TBB_ASSERT_EX( src, "The library handle to store must be non-zero" );
        if ( dst )
            *dst = src;
    #if __TBB_DYNAMIC_LOAD_ENABLED
        else
            handles.add( src );
    #endif /* __TBB_DYNAMIC_LOAD_ENABLED */
    }

#if _WIN32
    DWORD loading_flags(int) {
        // Do not search in working directory if it is considered unsafe
        return LOAD_LIBRARY_SAFE_CURRENT_DIRS;
    }
#else
    int loading_flags(int requested_flags) {
        int flags = RTLD_NOW;
        if (requested_flags & DYNAMIC_LINK_LOCAL) {
            flags = flags | RTLD_LOCAL;
#if (__linux__ && __GLIBC__) && !__TBB_USE_SANITIZERS
            if( !GetBoolEnvironmentVariable("TBB_ENABLE_SANITIZERS") ) {
                flags = flags | RTLD_DEEPBIND;
            }
#endif
        } else {
            flags = flags | RTLD_GLOBAL;
        }
        return flags;
    }
#endif

    /**
     * Checks if the file exists and is a regular file.
     */
    bool file_exists(const char* path) {
#if _WIN32
        const DWORD attributes = GetFileAttributesA(path);
        return attributes != INVALID_FILE_ATTRIBUTES && !(attributes & FILE_ATTRIBUTE_DIRECTORY);
#else
        struct stat st;
        return stat(path, &st) == 0 && S_ISREG(st.st_mode);
#endif
    }

#if _WIN32 && !__TBB_SKIP_DEPENDENCY_SIGNATURE_VERIFICATION
    /**
     * Obtains full path to the specified filename and stores it inside passed buffer. Returns the
     * actual length of the buffer required to hold the full path to the specified file, not
     * including the terminating NULL character.
     *
     * If the buffer is too small to hold the full path, the returned value indicates the needed
     * length of the buffer including the terminating NULL character. In case of error, zero is
     * returned.
     */
    unsigned get_module_path(char* path_buffer, const unsigned buffer_length, const char* filename) {
        __TBB_ASSERT_EX(buffer_length > 0, "Cannot write the path to the buffer with zero length");
        const DWORD actual_length = SearchPathA(/*lpPath*/NULL, filename, /*lpExtension*/NULL,
                                                static_cast<DWORD>(buffer_length), path_buffer,
                                                /*lpFilePart*/NULL);
        return actual_length;
    }

#if __DYNAMIC_LINK_REPORT_SIGNATURE_ERRORS
    void report_signature_verification_error(const LONG retval, const char* filepath) {
        switch (retval) {
        case ERROR_SUCCESS:
            // The file is signed:
            //   - Hash representing a file is trusted.
            //   - Trusted publisher without any verification errors.
            //   - No publisher or time stamp chain errors.
            break;
        case TRUST_E_NOSIGNATURE:
        {
            // The file is not signed or has an invalid signature.
            LONG lerr = (LONG)dlerror();
            if (lerr == TRUST_E_NOSIGNATURE || lerr == TRUST_E_SUBJECT_FORM_UNKNOWN ||
                lerr == TRUST_E_PROVIDER_UNKNOWN)
            {
                DYNAMIC_LINK_WARNING( dl_lib_unsigned, filepath );
            } else {
                DYNAMIC_LINK_WARNING( dl_sig_err_unknown, filepath, lerr );
            }
            break;
        }
        case TRUST_E_EXPLICIT_DISTRUST:
            // The hash representing the subject is explicitly disallowed by the admin or user.
            DYNAMIC_LINK_WARNING( dl_sig_explicit_distrust, filepath );
            break;
        case CERT_E_UNTRUSTEDROOT:
            DYNAMIC_LINK_WARNING( dl_sig_untrusted_root, filepath );
            break;
        case TRUST_E_SUBJECT_NOT_TRUSTED:
            DYNAMIC_LINK_WARNING( dl_sig_distrusted, filepath );
            break;
        case CRYPT_E_SECURITY_SETTINGS:
            DYNAMIC_LINK_WARNING( dl_sig_security_settings, filepath );
            break;
        default:
            DYNAMIC_LINK_WARNING( dl_sig_other_error, filepath, retval);
            break;
        }
    }
#else /* __DYNAMIC_LINK_REPORT_SIGNATURE_ERRORS */
    void report_signature_verification_error(const LONG /*retval*/, const char* /*filepath*/) {}
#endif /* __DYNAMIC_LINK_REPORT_SIGNATURE_ERRORS */

    /**
     * Validates signature of specified file.
     *
     * @param filepath Path to a file, whose signature to be validated.
     * @param length Length of the path buffer, including the terminating NULL character.
     * @return 'true' if file signature has been successfully validated. 'false' - if any error
     *         occurs, in which case the error is optionally reported.
     */
    bool has_valid_signature(const char* filepath, const std::size_t length) {
        __TBB_ASSERT_EX(length <= PATH_MAX, "Too small buffer for path conversion");
        wchar_t wfilepath[PATH_MAX] = {0};
        {
            std::mbstate_t state{};
            const char* ansi_filepath = filepath; // mbsrtowcs moves original pointer
            const size_t num_converted = mbsrtowcs(wfilepath, &ansi_filepath, length, &state);
            if (num_converted == std::size_t(-1))
                return false;
        }
        WINTRUST_FILE_INFO fdata;
        std::memset(&fdata, 0, sizeof(fdata));
        fdata.cbStruct       = sizeof(WINTRUST_FILE_INFO);
        fdata.pcwszFilePath  = wfilepath;

        // Check that the certificate used to sign the specified file chains up to a root
        // certificate located in the trusted root certificate store, implying that the identity of
        // the publisher has been verified by a certification authority.
        GUID pgActionID = WINTRUST_ACTION_GENERIC_VERIFY_V2;

        WINTRUST_DATA pWVTData;
        std::memset(&pWVTData, 0, sizeof(pWVTData));
        pWVTData.cbStruct            = sizeof(WINTRUST_DATA);
        pWVTData.dwUIChoice          = WTD_UI_NONE;                    // Disable WVT UI
        pWVTData.fdwRevocationChecks = WTD_REVOKE_WHOLECHAIN;          // Check the whole chain
        pWVTData.dwUnionChoice       = WTD_CHOICE_FILE;                // Verify file signature
        pWVTData.pFile               = &fdata;
        pWVTData.dwStateAction       = WTD_STATEACTION_VERIFY;         // Verify action
        // Perform revocation checking on the entire certificate chain but use only the local cache
        pWVTData.dwProvFlags         = WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_REVOCATION_CHECK_CHAIN;
        pWVTData.dwUIContext         = WTD_UICONTEXT_EXECUTE;          // UI Context to run the file

        const LONG rc = WinVerifyTrust((HWND)INVALID_HANDLE_VALUE, &pgActionID, &pWVTData);
        report_signature_verification_error(rc, filepath);

        pWVTData.dwStateAction = WTD_STATEACTION_CLOSE;       // Release WVT state data
        (void)WinVerifyTrust(NULL, &pgActionID, &pWVTData);

        return ERROR_SUCCESS == rc;
    }
#endif  // _WIN32 && !__TBB_SKIP_DEPENDENCY_SIGNATURE_VERIFICATION

    dynamic_link_handle dynamic_load( const char* library, const dynamic_link_descriptor descriptors[],
                                      std::size_t required, int flags )
    {
        ::tbb::detail::suppress_unused_warning( library, descriptors, required, flags );
        dynamic_link_handle library_handle = nullptr;
#if __TBB_DYNAMIC_LOAD_ENABLED
        const char* path = library;
        std::size_t const len = PATH_MAX + 1;
        char absolute_path[ len ] = {0};
        std::size_t length = 0;
        const bool build_absolute_path = flags & DYNAMIC_LINK_BUILD_ABSOLUTE_PATH;
        if (build_absolute_path) {
            length = abs_path( library, absolute_path, len );
            if (length > len) {
                DYNAMIC_LINK_WARNING( dl_buff_too_small );
                return nullptr;
            } else if (length == 0) {
                // length == 0 means failing of init_ap_data so the warning has already been issued.
                return nullptr;
            } else if (!file_exists(absolute_path)) {
                // Path to a file has been built manually. It is not proven to exist however.
                DYNAMIC_LINK_WARNING( dl_lib_not_found, absolute_path, dlerror() );
                return nullptr;
            }
            path = absolute_path;
        }
#if _WIN32
#if !__TBB_SKIP_DEPENDENCY_SIGNATURE_VERIFICATION
        if (!build_absolute_path) { // Get the path if it is not yet built
            length = get_module_path(absolute_path, len, library);
            if (length == 0) {
                DYNAMIC_LINK_WARNING( dl_lib_not_found, path, dlerror() );
                return library_handle;
            } else if (length >= len) { // The buffer length was insufficient
                DYNAMIC_LINK_WARNING( dl_buff_too_small );
                return library_handle;
            }
            length += 1;   // Count terminating NULL character as part of string length
            path = absolute_path;
        }

        if (!has_valid_signature(path, length))
            return library_handle; // Warning (if any) has already been reported
#endif /* !__TBB_SKIP_DEPENDENCY_SIGNATURE_VERIFICATION */
        // Prevent Windows from displaying silly message boxes if it fails to load library
        // (e.g. because of MS runtime problems - one of those crazy manifest related ones)
        UINT prev_mode = SetErrorMode (SEM_FAILCRITICALERRORS);
#endif /* _WIN32 */
        // The argument of loading_flags is ignored on Windows
        library_handle = dlopen( path, loading_flags(flags) );
#if _WIN32
        SetErrorMode (prev_mode);
#endif /* _WIN32 */
        if( library_handle ) {
            if( !resolve_symbols( library_handle, descriptors, required ) ) {
                // The loaded library does not contain all the expected entry points
                dynamic_unlink( library_handle );
                library_handle = nullptr;
            }
        } else
            DYNAMIC_LINK_WARNING( dl_lib_not_found, path, dlerror() );
#endif /* __TBB_DYNAMIC_LOAD_ENABLED */
        return library_handle;
    }

    bool dynamic_link( const char* library, const dynamic_link_descriptor descriptors[],
                       std::size_t required, dynamic_link_handle* handle, int flags )
    {
        init_dynamic_link_data();

        // TODO: May global_symbols_link find weak symbols?
        dynamic_link_handle library_handle = ( flags & DYNAMIC_LINK_GLOBAL ) ?
            global_symbols_link( library, descriptors, required ) : nullptr;

#if defined(_MSC_VER) && _MSC_VER <= 1900
#pragma warning (push)
// MSVC 2015 warning: 'int': forcing value to bool 'true' or 'false'
#pragma warning (disable: 4800)
#endif
        if ( !library_handle && ( flags & DYNAMIC_LINK_LOAD ) )
            library_handle = dynamic_load( library, descriptors, required, flags );

#if defined(_MSC_VER) && _MSC_VER <= 1900
#pragma warning (pop)
#endif
        if ( !library_handle && ( flags & DYNAMIC_LINK_WEAK ) )
            return weak_symbol_link( descriptors, required );

        if ( library_handle ) {
            save_library_handle( library_handle, handle );
            return true;
        }
        return false;
    }

#endif /*__TBB_WIN8UI_SUPPORT*/
#else /* __TBB_WEAK_SYMBOLS_PRESENT || __TBB_DYNAMIC_LOAD_ENABLED */
    bool dynamic_link( const char*, const dynamic_link_descriptor*, std::size_t, dynamic_link_handle *handle, int ) {
        if ( handle )
            *handle=0;
        return false;
    }
    void dynamic_unlink( dynamic_link_handle ) {}
    void dynamic_unlink_all() {}
#endif /* __TBB_WEAK_SYMBOLS_PRESENT || __TBB_DYNAMIC_LOAD_ENABLED */

} // namespace r1
} // namespace detail
} // namespace tbb
