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

#ifndef __TBB_test_common_exception_handling_H
#define __TBB_test_common_exception_handling_H

#include "config.h"

#include <typeinfo>
#include <thread>

#include "oneapi/tbb/task_scheduler_observer.h"

#include "concurrency_tracker.h"

int g_NumThreads = 0;
std::thread::id g_Master = std::this_thread::get_id();
const char * g_Orig_Wakeup_Msg = "Missed wakeup or machine is overloaded?";
const char * g_Wakeup_Msg = g_Orig_Wakeup_Msg;

std::atomic<intptr_t> g_CurExecuted,
                      g_ExecutedAtLastCatch,
                      g_ExecutedAtFirstCatch,
                      g_ExceptionsThrown,
                      g_MasterExecutedThrow,     // number of times external thread entered exception code
                      g_NonMasterExecutedThrow,  // number of times non-external thread entered exception code
                      g_PipelinesStarted;
std::atomic<bool>   g_ExceptionCaught{ false },
                    g_UnknownException{ false };

std::atomic<intptr_t> g_ActualMaxThreads;
std::atomic<intptr_t> g_ActualCurrentThreads;

std::atomic<bool>   g_ThrowException{ true },
                    // g_Flog is true for nested construct tests with catches (exceptions are not allowed to
                    // propagate to the construct itself.)
                    g_Flog{ false },
                    g_MasterExecuted{ false },
                    g_NonMasterExecuted{ false };

bool    g_ExceptionInMaster = false;
bool    g_SolitaryException = false;
bool    g_NestedPipelines   = false;

//! Number of exceptions propagated into the user code (i.e. intercepted by the tests)
std::atomic<intptr_t> g_NumExceptionsCaught;

//-----------------------------------------------------------

class eh_test_observer : public tbb::task_scheduler_observer {
public:
    void on_scheduler_entry(bool is_worker) override {
        if(is_worker) {  // we've already counted the external thread
            intptr_t p = ++g_ActualCurrentThreads;
            intptr_t q = g_ActualMaxThreads;
            while(q < p) {
                g_ActualMaxThreads.compare_exchange_strong(q, p);
            }
        }
    }
    void on_scheduler_exit(bool is_worker) override {
        if(is_worker) {
            --g_ActualCurrentThreads;
        }
    }
};
//-----------------------------------------------------------

inline void ResetEhGlobals ( bool throwException = true, bool flog = false ) {
    utils::ConcurrencyTracker::Reset();
    g_CurExecuted = g_ExecutedAtLastCatch = g_ExecutedAtFirstCatch = 0;
    g_ExceptionCaught = false;
    g_UnknownException = false;
    g_NestedPipelines = false;
    g_ThrowException = throwException;
    g_MasterExecutedThrow = 0;
    g_NonMasterExecutedThrow = 0;
    g_Flog = flog;
    g_MasterExecuted = false;
    g_NonMasterExecuted = false;
    g_ActualMaxThreads = 1;  // count external thread
    g_ActualCurrentThreads = 1;  // count external thread
    g_ExceptionsThrown = g_NumExceptionsCaught = g_PipelinesStarted = 0;
}

#if TBB_USE_EXCEPTIONS
class test_exception : public std::exception {
    const char* my_description;
public:
    test_exception ( const char* description ) : my_description(description) {}

    const char* what() const throw() override { return my_description; }
};

class solitary_test_exception : public test_exception {
public:
    solitary_test_exception ( const char* description ) : test_exception(description) {}
};

using PropagatedException = test_exception;
#define EXCEPTION_NAME(e) typeid(e).name()

#define EXCEPTION_DESCR "Test exception"

#if UTILS_EXCEPTION_HANDLING_SIMPLE_MODE

inline void ThrowTestException () {
    ++g_ExceptionsThrown;
    throw test_exception(EXCEPTION_DESCR);
}

#else /* !UTILS_EXCEPTION_HANDLING_SIMPLE_MODE */

constexpr std::intptr_t Existed = INT_MAX;

inline void ThrowTestException ( intptr_t threshold ) {
    bool inMaster = (std::this_thread::get_id() == g_Master);
    if ( !g_ThrowException ||   // if we're not supposed to throw
            (!g_Flog &&         // if we're not catching throw in bodies and
             (g_ExceptionInMaster ^ inMaster)) ) { // we're the external thread and not expected to throw
              // or are the external thread not the one to throw (??)
        return;
    }
    while ( Existed < threshold )
        utils::yield();
    if ( !g_SolitaryException ) {
        ++g_ExceptionsThrown;
        if(inMaster) ++g_MasterExecutedThrow; else ++g_NonMasterExecutedThrow;
        throw test_exception(EXCEPTION_DESCR);
    }
    // g_SolitaryException == true
    if(g_NestedPipelines) {
        // only throw exception if we have started at least two inner pipelines
        // else return
        if(g_PipelinesStarted >= 3) {
            intptr_t expected = 0;
            if ( g_ExceptionsThrown.compare_exchange_strong(expected, 1) )  {
                if(inMaster) ++g_MasterExecutedThrow; else ++g_NonMasterExecutedThrow;
                throw solitary_test_exception(EXCEPTION_DESCR);
            }
        }
    }
    else {
        intptr_t expected = 0;
        if ( g_ExceptionsThrown.compare_exchange_strong(expected, 1) )  {
            if(inMaster) ++g_MasterExecutedThrow; else ++g_NonMasterExecutedThrow;
            throw solitary_test_exception(EXCEPTION_DESCR);
        }
    }
}
#endif /* !HARNESS_EH_SIMPLE_MODE */

#define UPDATE_COUNTS()     \
    { \
        ++g_CurExecuted; \
        if(g_Master == std::this_thread::get_id()) g_MasterExecuted = true; \
        else g_NonMasterExecuted = true; \
        if( tbb::is_current_task_group_canceling() ) ++g_TGCCancelled; \
    }

#define CATCH()     \
    } catch ( PropagatedException& e ) { \
        intptr_t expected = 0;\
        g_ExecutedAtFirstCatch.compare_exchange_strong(expected , g_CurExecuted); \
        intptr_t curExecuted = g_CurExecuted.load(); \
        expected = g_ExecutedAtLastCatch.load();\
        while (expected < curExecuted) g_ExecutedAtLastCatch.compare_exchange_strong(expected, curExecuted); \
        REQUIRE_MESSAGE(e.what(), "Empty what() string" );  \
        REQUIRE_MESSAGE((strcmp(EXCEPTION_NAME(e), (g_SolitaryException ? typeid(solitary_test_exception) : typeid(test_exception)).name() ) == 0), "Unexpected original exception name"); \
        REQUIRE_MESSAGE((strcmp(e.what(), EXCEPTION_DESCR) == 0), "Unexpected original exception info"); \
        g_ExceptionCaught = l_ExceptionCaughtAtCurrentLevel = true; \
        ++g_NumExceptionsCaught; \
    } catch ( std::exception& ) { \
        REQUIRE_MESSAGE (false, "Unexpected std::exception" ); \
    } catch ( ... ) { \
        g_ExceptionCaught = l_ExceptionCaughtAtCurrentLevel = true; \
        g_UnknownException = unknownException = true; \
    } \
    if ( !g_SolitaryException ) \
        WARN_MESSAGE (true, "Multiple exceptions mode");

#define ASSERT_EXCEPTION() \
    { \
        REQUIRE_MESSAGE ((!g_ExceptionsThrown || g_ExceptionCaught), "throw without catch"); \
        REQUIRE_MESSAGE ((!g_ExceptionCaught  || g_ExceptionsThrown), "catch without throw"); \
        REQUIRE_MESSAGE ((g_ExceptionCaught || (g_ExceptionInMaster && !g_MasterExecutedThrow) || (!g_ExceptionInMaster && !g_NonMasterExecutedThrow)), "no exception occurred"); \
        REQUIRE_MESSAGE (!g_UnknownException, "unknown exception was caught"); \
    }

#define CATCH_AND_ASSERT() \
    CATCH() \
    ASSERT_EXCEPTION()

#else /* !TBB_USE_EXCEPTIONS */

inline void ThrowTestException ( intptr_t ) {}

#endif /* !TBB_USE_EXCEPTIONS */

#if TBB_USE_EXCEPTIONS
#define TRY()   \
    bool l_ExceptionCaughtAtCurrentLevel = false, unknownException = false;    \
    try {

// "l_ExceptionCaughtAtCurrentLevel || unknownException" is used only to "touch" otherwise unused local variables
#define CATCH_AND_FAIL() } catch(...) { \
        REQUIRE_MESSAGE (false, "Cancelling tasks must not cause any exceptions");    \
        (void)(l_ExceptionCaughtAtCurrentLevel && unknownException);                        \
    }
#else
#define TRY() {
#define CATCH_AND_FAIL() }
#endif

const int c_Timeout = 1000000;

#include "oneapi/tbb/task_arena.h"

void WaitUntilConcurrencyPeaks ( int expected_peak ) {
    tbb::task_group tg;
    if ( g_Flog )
        return;
    int n = 0;
retry:
    while ( ++n < c_Timeout && (int)utils::ConcurrencyTracker::PeakParallelism() < expected_peak )
        utils::yield();
#if USE_TASK_SCHEDULER_OBSERVER
    DOCTEST_WARN_MESSAGE( g_NumThreads == g_ActualMaxThreads, "Library did not provide sufficient threads");
#endif
    DOCTEST_WARN_MESSAGE(n < c_Timeout, g_Wakeup_Msg);
    // Workaround in case a missed wakeup takes place
    if ( n == c_Timeout ) {
        tg.run([]{});
        n = 0;
        goto retry;
    }

    TRY();
        tg.wait();
    CATCH_AND_FAIL();
}

inline void WaitUntilConcurrencyPeaks () { WaitUntilConcurrencyPeaks(g_NumThreads); }

inline bool IsMaster() {
    return std::this_thread::get_id() == g_Master;
}

inline bool IsThrowingThread() {
    return g_ExceptionInMaster ^ IsMaster() ? true : false;
}

struct Cancellator {
    static std::atomic<bool> s_Ready;
    tbb::task_group_context &m_groupToCancel;
    intptr_t m_cancellationThreshold;

    void operator()() const {
        utils::ConcurrencyTracker ct;
        s_Ready = true;
        while ( g_CurExecuted < m_cancellationThreshold )
            utils::yield();
        m_groupToCancel.cancel_group_execution();
        g_ExecutedAtLastCatch = g_CurExecuted.load();
    }

    Cancellator( tbb::task_group_context& ctx, intptr_t threshold )
        : m_groupToCancel(ctx), m_cancellationThreshold(threshold)
    {
        s_Ready = false;
    }

    static void Reset () { s_Ready = false; }

    static bool WaitUntilReady () {
        const intptr_t limit = 10000000;
        intptr_t n = 0;
        do {
            utils::yield();
        } while( !s_Ready && ++n < limit );
        // should yield once, then continue if Cancellator is ready.
        REQUIRE( (s_Ready || n == limit) );
        return s_Ready;
    }
};

std::atomic<bool> Cancellator::s_Ready{ false };

template<class LauncherT, class CancellatorT>
void RunCancellationTest ( intptr_t threshold = 1 )
{
    tbb::task_group_context  ctx;
    tbb::task_group tg;

    CancellatorT cancellator(ctx, threshold);
    LauncherT launcher(ctx);

    tg.run(launcher);
    tg.run(cancellator);

    TRY();
        tg.wait();
    CATCH_AND_FAIL();
}

#endif // __TBB_test_common_exception_handling_H
