/*
    Copyright (c) 2005-2021 Intel Corporation

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

// All platform-specific threading support is encapsulated here. */

#ifndef __RML_thread_monitor_H
#define __RML_thread_monitor_H

#if __TBB_USE_WINAPI
#include <windows.h>
#include <process.h>
#include <malloc.h> //_alloca
#include "misc.h" // support for processor groups
#if __TBB_WIN8UI_SUPPORT && (_WIN32_WINNT < 0x0A00)
#include <thread>
#endif
#elif __TBB_USE_POSIX
#include <pthread.h>
#include <cstring>
#include <cstdlib>
#else
#error Unsupported platform
#endif
#include <cstdio>

#include "oneapi/tbb/detail/_template_helpers.h"

#include "itt_notify.h"
#include "semaphore.h"

// All platform-specific threading support is in this header.

#if (_WIN32||_WIN64)&&!__TBB_ipf
// Deal with 64K aliasing.  The formula for "offset" is a Fibonacci hash function,
// which has the desirable feature of spreading out the offsets fairly evenly
// without knowing the total number of offsets, and furthermore unlikely to
// accidentally cancel out other 64K aliasing schemes that Microsoft might implement later.
// See Knuth Vol 3. "Theorem S" for details on Fibonacci hashing.
// The second statement is really does need "volatile", otherwise the compiler might remove the _alloca.
#define AVOID_64K_ALIASING(idx)                       \
    std::size_t offset = (idx+1) * 40503U % (1U<<16);      \
    void* volatile sink_for_alloca = _alloca(offset); \
    __TBB_ASSERT_EX(sink_for_alloca, "_alloca failed");
#else
// Linux thread allocators avoid 64K aliasing.
#define AVOID_64K_ALIASING(idx) tbb::detail::suppress_unused_warning(idx)
#endif /* _WIN32||_WIN64 */

namespace tbb {
namespace detail {
namespace r1 {

// Forward declaration: throws std::runtime_error with what() returning error_code description prefixed with aux_info
void handle_perror(int error_code, const char* aux_info);

namespace rml {
namespace internal {

#if __TBB_USE_ITT_NOTIFY
static const ::tbb::detail::r1::tchar *SyncType_RML = _T("%Constant");
static const ::tbb::detail::r1::tchar *SyncObj_ThreadMonitor = _T("RML Thr Monitor");
#endif /* __TBB_USE_ITT_NOTIFY */

//! Monitor with limited two-phase commit form of wait.
/** At most one thread should wait on an instance at a time. */
class thread_monitor {
public:
    class cookie {
        friend class thread_monitor;
        std::atomic<std::size_t> my_epoch{0};
    };
    thread_monitor() : skipped_wakeup(false), my_sema() {
        ITT_SYNC_CREATE(&my_sema, SyncType_RML, SyncObj_ThreadMonitor);
    }
    ~thread_monitor() {}

    //! If a thread is waiting or started a two-phase wait, notify it.
    /** Can be called by any thread. */
    void notify();

    //! Begin two-phase wait.
    /** Should only be called by thread that owns the monitor.
        The caller must either complete the wait or cancel it. */
    void prepare_wait( cookie& c );

    //! Complete a two-phase wait and wait until notification occurs after the earlier prepare_wait.
    void commit_wait( cookie& c );

    //! Cancel a two-phase wait.
    void cancel_wait();

#if __TBB_USE_WINAPI
    typedef HANDLE handle_type;

    #define __RML_DECL_THREAD_ROUTINE unsigned WINAPI
    typedef unsigned (WINAPI *thread_routine_type)(void*);

    //! Launch a thread
    static handle_type launch( thread_routine_type thread_routine, void* arg, std::size_t stack_size, const size_t* worker_index = NULL );

#elif __TBB_USE_POSIX
    typedef pthread_t handle_type;

    #define __RML_DECL_THREAD_ROUTINE void*
    typedef void*(*thread_routine_type)(void*);

    //! Launch a thread
    static handle_type launch( thread_routine_type thread_routine, void* arg, std::size_t stack_size );
#endif /* __TBB_USE_POSIX */

    //! Join thread
    static void join(handle_type handle);

    //! Detach thread
    static void detach_thread(handle_type handle);
private:
    cookie my_cookie; // epoch counter
    std::atomic<bool> in_wait{false};
    bool skipped_wakeup;
    binary_semaphore my_sema;
#if __TBB_USE_POSIX
    static void check( int error_code, const char* routine );
#endif
};

#if __TBB_USE_WINAPI

#ifndef STACK_SIZE_PARAM_IS_A_RESERVATION
#define STACK_SIZE_PARAM_IS_A_RESERVATION 0x00010000
#endif

// _beginthreadex API is not available in Windows 8 Store* applications, so use std::thread instead
#if __TBB_WIN8UI_SUPPORT && (_WIN32_WINNT < 0x0A00)
inline thread_monitor::handle_type thread_monitor::launch( thread_routine_type thread_function, void* arg, std::size_t, const std::size_t*) {
//TODO: check that exception thrown from std::thread is not swallowed silently
    std::thread* thread_tmp=new std::thread(thread_function, arg);
    return thread_tmp->native_handle();
}
#else
inline thread_monitor::handle_type thread_monitor::launch( thread_routine_type thread_routine, void* arg, std::size_t stack_size, const std::size_t* worker_index ) {
    unsigned thread_id;
    int number_of_processor_groups = ( worker_index ) ? NumberOfProcessorGroups() : 0;
    unsigned create_flags = ( number_of_processor_groups > 1 ) ? CREATE_SUSPENDED : 0;
    HANDLE h = (HANDLE)_beginthreadex( NULL, unsigned(stack_size), thread_routine, arg, STACK_SIZE_PARAM_IS_A_RESERVATION | create_flags, &thread_id );
    if( !h ) {
        handle_perror(0, "thread_monitor::launch: _beginthreadex failed\n");
    }
    if ( number_of_processor_groups > 1 ) {
        MoveThreadIntoProcessorGroup( h, FindProcessorGroupIndex( static_cast<int>(*worker_index) ) );
        ResumeThread( h );
    }
    return h;
}
#endif //__TBB_WIN8UI_SUPPORT && (_WIN32_WINNT < 0x0A00)

void thread_monitor::join(handle_type handle) {
#if TBB_USE_ASSERT
    DWORD res =
#endif
        WaitForSingleObjectEx(handle, INFINITE, FALSE);
    __TBB_ASSERT( res==WAIT_OBJECT_0, NULL );
#if TBB_USE_ASSERT
    BOOL val =
#endif
        CloseHandle(handle);
    __TBB_ASSERT( val, NULL );
}

void thread_monitor::detach_thread(handle_type handle) {
#if TBB_USE_ASSERT
    BOOL val =
#endif
        CloseHandle(handle);
    __TBB_ASSERT( val, NULL );
}

#endif /* __TBB_USE_WINAPI */

#if __TBB_USE_POSIX
inline void thread_monitor::check( int error_code, const char* routine ) {
    if( error_code ) {
        handle_perror(error_code, routine);
    }
}

inline thread_monitor::handle_type thread_monitor::launch( void* (*thread_routine)(void*), void* arg, std::size_t stack_size ) {
    // FIXME - consider more graceful recovery than just exiting if a thread cannot be launched.
    // Note that there are some tricky situations to deal with, such that the thread is already
    // grabbed as part of an OpenMP team.
    pthread_attr_t s;
    check(pthread_attr_init( &s ), "pthread_attr_init has failed");
    if( stack_size>0 )
        check(pthread_attr_setstacksize( &s, stack_size ), "pthread_attr_setstack_size has failed" );
    pthread_t handle;
    check( pthread_create( &handle, &s, thread_routine, arg ), "pthread_create has failed" );
    check( pthread_attr_destroy( &s ), "pthread_attr_destroy has failed" );
    return handle;
}

void thread_monitor::join(handle_type handle) {
    check(pthread_join(handle, NULL), "pthread_join has failed");
}

void thread_monitor::detach_thread(handle_type handle) {
    check(pthread_detach(handle), "pthread_detach has failed");
}
#endif /* __TBB_USE_POSIX */

inline void thread_monitor::notify() {
    my_cookie.my_epoch.store(my_cookie.my_epoch.load(std::memory_order_acquire) + 1, std::memory_order_release);
    bool do_signal = in_wait.exchange( false );
    if( do_signal )
        my_sema.V();
}

inline void thread_monitor::prepare_wait( cookie& c ) {
    if( skipped_wakeup ) {
        // Lazily consume a signal that was skipped due to cancel_wait
        skipped_wakeup = false;
        my_sema.P(); // does not really wait on the semaphore
    }
    // Former c = my_cookie
    c.my_epoch.store(my_cookie.my_epoch.load(std::memory_order_acquire), std::memory_order_release);
    in_wait.store( true, std::memory_order_seq_cst );
}

inline void thread_monitor::commit_wait( cookie& c ) {
    bool do_it = ( c.my_epoch.load(std::memory_order_relaxed) == my_cookie.my_epoch.load(std::memory_order_relaxed) );
    if( do_it ) my_sema.P();
    else        cancel_wait();
}

inline void thread_monitor::cancel_wait() {
    // if not in_wait, then some thread has sent us a signal;
    // it will be consumed by the next prepare_wait call
    skipped_wakeup = ! in_wait.exchange( false );
}

} // namespace internal
} // namespace rml
} // namespace r1
} // namespace detail
} // namespace tbb

#endif /* __RML_thread_monitor_H */
