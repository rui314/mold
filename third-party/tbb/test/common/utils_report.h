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

#ifndef __TBB_test_common_utils_report_H
#define __TBB_test_common_utils_report_H

#include "config.h"

#include <mutex>
#include <cstdarg>

#if __SUNPRO_CC
#include <stdio.h>
#else
#include <cstdio>
#endif

#if defined(MAX_TRACE_SIZE) && MAX_TRACE_SIZE < 1024
    #undef MAX_TRACE_SIZE
#endif
#ifndef MAX_TRACE_SIZE
    #define MAX_TRACE_SIZE  1024
#endif


#if __GLIBC__>2 || ( __GLIBC__==2 && __GLIBC_MINOR__ >= 1) || __APPLE__
    #include <execinfo.h> /*backtrace*/
    #define BACKTRACE_FUNCTION_AVAILABLE 1
#endif

#if defined(_MSC_VER)  &&  _MSC_VER >= 1300  ||  defined(__GNUC__)  ||  defined(__GNUG__)
    #define TRACE_ORIG_INFO __FILE__, __LINE__, __FUNCTION__
#else
    #define TRACE_ORIG_INFO __FILE__, __LINE__, ""
    #define __FUNCTION__ ""
#endif

#if defined(_WIN32) || defined(_WIN64)
    #if (_WIN32_WINNT > 0x0501 && defined(_MSC_VER) && !defined(_M_ARM))
        // Suppress "typedef ignored ... when no variable is declared" warning by vc14
        #pragma warning (push)
        #pragma warning (disable: 4091)
        #ifndef NOMINMAX
            #define NOMINMAX
        #endif
        #include <windows.h>
        #include <dbghelp.h>
        #pragma warning (pop)
        #pragma comment (lib, "dbghelp.lib")
    #endif
#endif

//! printf style tracing macro without automatic new line character adding
#define TRACENL utils::internal::tracer.set_trace_info(0, TRACE_ORIG_INFO)->trace

//! printf style reporting macro
/** On heterogeneous platforms redirects its output to the host side. **/
#define REPORT TRACENL

namespace utils {
namespace internal {

#ifndef TBBReporter
struct TBBReporter {
    void Report ( const char* msg ) {
        printf( "%s", msg );
        fflush(stdout);
#ifdef _WINDOWS_
        OutputDebugStringA(msg);
#endif
    }
};
#endif 

class Tracer {
    int m_flags;
    const char *m_file;
    const char *m_func;
    std::size_t m_line;

    TBBReporter m_reporter;

public:
    enum {
        prefix = 1,
        need_lf = 2
    };

    Tracer(): m_flags(0), m_file(nullptr), m_func(nullptr), m_line(0) {}

    Tracer* set_trace_info( int flags, const char *file, std::size_t line, const char *func ) {
        m_flags = flags;
        m_line = line;
        m_file = file;
        m_func = func;
        return  this;
    }

    void trace( const char* fmt, ... ) {
        char msg[MAX_TRACE_SIZE];
        char msg_fmt_buf[MAX_TRACE_SIZE];
        const char  *msg_fmt = fmt;
        if (m_flags & prefix) {
            snprintf (msg_fmt_buf, MAX_TRACE_SIZE, "[%s] %s", m_func, fmt);
            msg_fmt = msg_fmt_buf;
        }
        std::va_list argptr;
        va_start(argptr, fmt);
        int len = vsnprintf(msg, MAX_TRACE_SIZE, msg_fmt, argptr);
        va_end(argptr);
        if (m_flags & need_lf && len < MAX_TRACE_SIZE - 1  &&  msg_fmt[len-1] != '\n') {
            msg[len] = '\n';
            msg[len + 1] = 0;
        }
        m_reporter.Report(msg);
    }
}; // class Tracer

static Tracer tracer;

template<int>
bool not_the_first_call () {
    static bool first_call = false;
    bool res = first_call;
    first_call = true;
    return res;
}

} // internal

inline void print_call_stack() {
    static std::mutex mutex;
    std::lock_guard<std::mutex> lock(mutex);

    fflush(stdout); fflush(stderr);
    #if BACKTRACE_FUNCTION_AVAILABLE
        const int sz = 100; // max number of frames to capture
        void *buff[sz];
        int n = backtrace(buff, sz);
        REPORT("Call stack info (%d):\n", n);
        backtrace_symbols_fd(buff, n, fileno(stdout));
    #elif __SUNPRO_CC
        REPORT("Call stack info:\n");
        printstack(fileno(stdout));
    #elif _WIN32_WINNT > 0x0501 && _MSC_VER>=1500 && !__TBB_WIN8UI_SUPPORT && !defined(WINAPI_FAMILY)
        const int sz = 62; // XP limitation for number of frames
        void *buff[sz];
        int n = CaptureStackBackTrace(0, sz, buff, nullptr);
        REPORT("Call stack info (%d):\n", n);
        static LONG once = 0;
        if( !InterlockedExchange(&once, 1) )
            SymInitialize(GetCurrentProcess(), nullptr, TRUE);
        const int len = 255; // just some reasonable string buffer size
        union { SYMBOL_INFO sym; char pad[sizeof(SYMBOL_INFO)+len]; };
        sym.MaxNameLen = len;
        sym.SizeOfStruct = sizeof( SYMBOL_INFO );
        DWORD64 offset;
        for(int i = 1; i < n; i++) { // skip current frame
            if(!SymFromAddr( GetCurrentProcess(), DWORD64(buff[i]), &offset, &sym )) {
                sym.Address = ULONG64(buff[i]); offset = 0; sym.Name[0] = 0;
            }
            REPORT("[%d] %016I64X+%04I64X: %s\n", i, sym.Address, offset, sym.Name);
        }
    #endif
}

} // util

#endif // __TBB_test_common_utils_report_H
