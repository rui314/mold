/*
    Copyright (c) 2005-2025 Intel Corporation
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

#include "common/test.h"

#if _MSC_VER
#pragma warning (push)
#if __TBB_MSVC_UNREACHABLE_CODE_IGNORED
    // Suppress pointless "unreachable code" warning.
    #pragma warning (disable: 4702)
#endif
#endif //#if _MSC_VER

#include "common/utils.h"
#include "oneapi/tbb/detail/_config.h"
#include "tbb/global_control.h"

#include "tbb/task_group.h"

#include "common/concurrency_tracker.h"

#include <atomic>
#include <stdexcept>
#include <unordered_map>

//! \file test_task_group.cpp
//! \brief Test for [scheduler.task_group scheduler.task_group_status] specification

unsigned g_MaxConcurrency = 4;
using atomic_t = std::atomic<std::uintptr_t>;
unsigned MinThread = 1;
unsigned MaxThread = 4;

//------------------------------------------------------------------------
// Tests for the thread safety of the task_group manipulations
//------------------------------------------------------------------------

#include "common/spin_barrier.h"

enum SharingMode {
    VagabondGroup = 1,
    ParallelWait = 2
};

template<typename task_group_type>
class SharedGroupBodyImpl : utils::NoCopy, utils::NoAfterlife {
    static const std::uintptr_t c_numTasks0 = 4096,
                        c_numTasks1 = 1024;

    const std::uintptr_t m_numThreads;
    const std::uintptr_t m_sharingMode;

    task_group_type *m_taskGroup;
    atomic_t m_tasksSpawned,
             m_threadsReady;
    utils::SpinBarrier m_barrier;

    static atomic_t s_tasksExecuted;

    struct TaskFunctor {
        SharedGroupBodyImpl *m_pOwner;
        void operator () () const {
            if ( m_pOwner->m_sharingMode & ParallelWait ) {
                while ( utils::ConcurrencyTracker::PeakParallelism() < m_pOwner->m_numThreads )
                    utils::yield();
            }
            ++s_tasksExecuted;
        }
    };

    TaskFunctor m_taskFunctor;

    void Spawn ( std::uintptr_t numTasks ) {
        for ( std::uintptr_t i = 0; i < numTasks; ++i ) {
            ++m_tasksSpawned;
            utils::ConcurrencyTracker ct;
            m_taskGroup->run( m_taskFunctor );
        }
        ++m_threadsReady;
    }

    void DeleteTaskGroup () {
        delete m_taskGroup;
        m_taskGroup = nullptr;
    }

    void Wait () {
        while ( m_threadsReady != m_numThreads )
            utils::yield();
        const std::uintptr_t numSpawned = c_numTasks0 + c_numTasks1 * (m_numThreads - 1);
        CHECK_MESSAGE( m_tasksSpawned == numSpawned, "Wrong number of spawned tasks. The test is broken" );
        INFO("Max spawning parallelism is " << utils::ConcurrencyTracker::PeakParallelism() << "out of " << g_MaxConcurrency);
        if ( m_sharingMode & ParallelWait ) {
            m_barrier.wait( &utils::ConcurrencyTracker::Reset );
            {
                utils::ConcurrencyTracker ct;
                m_taskGroup->wait();
            }
            if ( utils::ConcurrencyTracker::PeakParallelism() == 1 ) {
                const char* msg = "Warning: No parallel waiting detected in TestParallelWait";
                WARN( msg );
            }
            m_barrier.wait();
        }
        else
            m_taskGroup->wait();
        CHECK_MESSAGE( m_tasksSpawned == numSpawned, "No tasks should be spawned after wait starts. The test is broken" );
        CHECK_MESSAGE( s_tasksExecuted == numSpawned, "Not all spawned tasks were executed" );
    }

public:
    SharedGroupBodyImpl ( std::uintptr_t numThreads, std::uintptr_t sharingMode = 0 )
        : m_numThreads(numThreads)
        , m_sharingMode(sharingMode)
        , m_taskGroup(nullptr)
        , m_barrier(numThreads)
    {
        CHECK_MESSAGE( m_numThreads > 1, "SharedGroupBody tests require concurrency" );
        if ((m_sharingMode & VagabondGroup) && m_numThreads != 2) {
            CHECK_MESSAGE(false, "In vagabond mode SharedGroupBody must be used with 2 threads only");
        }
        utils::ConcurrencyTracker::Reset();
        s_tasksExecuted = 0;
        m_tasksSpawned = 0;
        m_threadsReady = 0;
        m_taskFunctor.m_pOwner = this;
    }

    void Run ( std::uintptr_t idx ) {
        AssertLive();
        if ( idx == 0 ) {
            if (m_taskGroup || m_tasksSpawned) {
                CHECK_MESSAGE(false, "SharedGroupBody must be reset before reuse");
            }
            m_taskGroup = new task_group_type;
            Spawn( c_numTasks0 );
            Wait();
            if ( m_sharingMode & VagabondGroup )
                m_barrier.wait();
            else
                DeleteTaskGroup();
        }
        else {
            while ( m_tasksSpawned == 0 )
                utils::yield();
            CHECK_MESSAGE ( m_taskGroup, "Task group is not initialized");
            Spawn (c_numTasks1);
            if ( m_sharingMode & ParallelWait )
                Wait();
            if ( m_sharingMode & VagabondGroup ) {
                CHECK_MESSAGE ( idx == 1, "In vagabond mode SharedGroupBody must be used with 2 threads only" );
                m_barrier.wait();
                DeleteTaskGroup();
            }
        }
        AssertLive();
    }
};

template<typename task_group_type>
atomic_t SharedGroupBodyImpl<task_group_type>::s_tasksExecuted;

template<typename task_group_type>
class  SharedGroupBody : utils::NoAssign, utils::NoAfterlife {
    bool m_bOwner;
    SharedGroupBodyImpl<task_group_type> *m_pImpl;
public:
    SharedGroupBody ( std::uintptr_t numThreads, std::uintptr_t sharingMode = 0 )
        : utils::NoAssign()
        , utils::NoAfterlife()
        , m_bOwner(true)
        , m_pImpl( new SharedGroupBodyImpl<task_group_type>(numThreads, sharingMode) )
    {}
    SharedGroupBody ( const SharedGroupBody& src )
        : utils::NoAssign()
        , utils::NoAfterlife()
        , m_bOwner(false)
        , m_pImpl(src.m_pImpl)
    {}
    ~SharedGroupBody () {
        if ( m_bOwner )
            delete m_pImpl;
    }
    void operator() ( std::uintptr_t idx ) const {
        // Wrap the functior into additional task group to enforce bounding.
        task_group_type tg;
        tg.run_and_wait([&] { m_pImpl->Run(idx); });
    }
};

template<typename task_group_type>
class RunAndWaitSyncronizationTestBody : utils::NoAssign {
    utils::SpinBarrier& m_barrier;
    std::atomic<bool>& m_completed;
    task_group_type& m_tg;
public:
    RunAndWaitSyncronizationTestBody(utils::SpinBarrier& barrier, std::atomic<bool>& completed, task_group_type& tg)
        : m_barrier(barrier), m_completed(completed), m_tg(tg) {}

    void operator()() const {
        m_barrier.wait();
        utils::doDummyWork(100000);
        m_completed = true;
    }

    void operator()(int id) const {
        if (id == 0) {
            m_tg.run_and_wait(*this);
        } else {
            m_barrier.wait();
            m_tg.wait();
            CHECK_MESSAGE(m_completed, "A concurrent waiter has left the wait method earlier than work has finished");
        }
    }
};

template<typename task_group_type>
void TestParallelSpawn () {
    NativeParallelFor( g_MaxConcurrency, SharedGroupBody<task_group_type>(g_MaxConcurrency) );
}

template<typename task_group_type>
void TestParallelWait () {
    NativeParallelFor( g_MaxConcurrency, SharedGroupBody<task_group_type>(g_MaxConcurrency, ParallelWait) );

    utils::SpinBarrier barrier(g_MaxConcurrency);
    std::atomic<bool> completed;
    completed = false;
    task_group_type tg;
    RunAndWaitSyncronizationTestBody<task_group_type> b(barrier, completed, tg);
    NativeParallelFor( g_MaxConcurrency, b );
}

// Tests non-stack-bound task group (the group that is allocated by one thread and destroyed by the other)
template<typename task_group_type>
void TestVagabondGroup () {
    NativeParallelFor( 2, SharedGroupBody<task_group_type>(2, VagabondGroup) );
}

#include "common/memory_usage.h"

template<typename task_group_type>
void TestThreadSafety() {
    auto tests = [] {
        for (int trail = 0; trail < 10; ++trail) {
            TestParallelSpawn<task_group_type>();
            TestParallelWait<task_group_type>();
            TestVagabondGroup<task_group_type>();
        }
    };

    // Test and warm up allocator.
    tests();

    // Ensure that consumption is stabilized.
    std::size_t initial = utils::GetMemoryUsage();
    for (;;) {
        tests();
        std::size_t current = utils::GetMemoryUsage();
        if (current <= initial) {
            return;
        }
        initial = current;
    }
}
//------------------------------------------------------------------------
// Common requisites of the Fibonacci tests
//------------------------------------------------------------------------

const std::uintptr_t N = 20;
const std::uintptr_t F = 6765;

atomic_t g_Sum;

#define FIB_TEST_PROLOGUE() \
    const unsigned numRepeats = g_MaxConcurrency * 4;    \
    utils::ConcurrencyTracker::Reset()

#define FIB_TEST_EPILOGUE(sum) \
    CHECK(utils::ConcurrencyTracker::PeakParallelism() <= g_MaxConcurrency); \
    CHECK( sum == numRepeats * F );


// Fibonacci tasks specified as functors
template<class task_group_type>
class FibTaskBase : utils::NoAssign, utils::NoAfterlife {
protected:
    std::uintptr_t* m_pRes;
    mutable std::uintptr_t m_Num;
    virtual void impl() const = 0;
public:
    FibTaskBase( std::uintptr_t* y, std::uintptr_t n ) : m_pRes(y), m_Num(n) {}
    void operator()() const {
        utils::ConcurrencyTracker ct;
        AssertLive();
        if( m_Num < 2 ) {
            *m_pRes = m_Num;
        } else {
            impl();
        }
    }
    virtual ~FibTaskBase() {}
};

template<class task_group_type>
class FibTaskAsymmetricTreeWithFunctor : public FibTaskBase<task_group_type> {
public:
    FibTaskAsymmetricTreeWithFunctor( std::uintptr_t* y, std::uintptr_t n ) : FibTaskBase<task_group_type>(y, n) {}
    virtual void impl() const override {
        std::uintptr_t x = ~0u;
        task_group_type tg;
        tg.run( FibTaskAsymmetricTreeWithFunctor(&x, this->m_Num-1) );
        this->m_Num -= 2; tg.run_and_wait( *this );
        *(this->m_pRes) += x;
    }
};

template<class task_group_type>
class FibTaskSymmetricTreeWithFunctor : public FibTaskBase<task_group_type> {
public:
    FibTaskSymmetricTreeWithFunctor( std::uintptr_t* y, std::uintptr_t n ) : FibTaskBase<task_group_type>(y, n) {}
    virtual void impl() const override {
        std::uintptr_t x = ~0u,
               y = ~0u;
        task_group_type tg;
        tg.run( FibTaskSymmetricTreeWithFunctor(&x, this->m_Num-1) );
        tg.run( FibTaskSymmetricTreeWithFunctor(&y, this->m_Num-2) );
        tg.wait();
        *(this->m_pRes) = x + y;
    }
};

// Helper functions
template<class fib_task>
std::uintptr_t RunFibTask(std::uintptr_t n) {
    std::uintptr_t res = ~0u;
    fib_task(&res, n)();
    return res;
}

template<typename fib_task>
void RunFibTest() {
    FIB_TEST_PROLOGUE();
    std::uintptr_t sum = 0;
    for( unsigned i = 0; i < numRepeats; ++i )
        sum += RunFibTask<fib_task>(N);
    FIB_TEST_EPILOGUE(sum);
}

template<typename fib_task>
void FibFunctionNoArgs() {
    g_Sum += RunFibTask<fib_task>(N);
}

template<typename task_group_type>
void TestFibWithLambdas() {
    FIB_TEST_PROLOGUE();
    atomic_t sum;
    sum = 0;
    task_group_type tg;
    for( unsigned i = 0; i < numRepeats; ++i )
        tg.run( [&](){sum += RunFibTask<FibTaskSymmetricTreeWithFunctor<task_group_type> >(N);} );
    tg.wait();
    FIB_TEST_EPILOGUE(sum);
}

template<typename task_group_type>
void TestFibWithFunctor() {
    RunFibTest<FibTaskAsymmetricTreeWithFunctor<task_group_type> >();
    RunFibTest< FibTaskSymmetricTreeWithFunctor<task_group_type> >();
}

template<typename task_group_type>
void TestFibWithFunctionPtr() {
    FIB_TEST_PROLOGUE();
    g_Sum = 0;
    task_group_type tg;
    for( unsigned i = 0; i < numRepeats; ++i )
        tg.run( &FibFunctionNoArgs<FibTaskSymmetricTreeWithFunctor<task_group_type> > );
    tg.wait();
    FIB_TEST_EPILOGUE(g_Sum);
}

template<typename task_group_type>
void RunFibonacciTests() {
    TestFibWithLambdas<task_group_type>();
    TestFibWithFunctor<task_group_type>();
    TestFibWithFunctionPtr<task_group_type>();
}

class test_exception : public std::exception
{
    const char* m_strDescription;
public:
    test_exception ( const char* descr ) : m_strDescription(descr) {}

    const char* what() const noexcept override { return m_strDescription; }
};

using TestException = test_exception;

#include <string.h>

#define NUM_CHORES      512
#define NUM_GROUPS      64
#define SKIP_CHORES     (NUM_CHORES/4)
#define SKIP_GROUPS     (NUM_GROUPS/4)
#define EXCEPTION_DESCR1 "Test exception 1"
#define EXCEPTION_DESCR2 "Test exception 2"

atomic_t g_ExceptionCount;
atomic_t g_TaskCount;
unsigned g_ExecutedAtCancellation;
bool g_Rethrow;
bool g_Throw;

class ThrowingTask : utils::NoAssign, utils::NoAfterlife {
    atomic_t &m_TaskCount;
public:
    ThrowingTask( atomic_t& counter ) : m_TaskCount(counter) {}
    void operator() () const {
        utils::ConcurrencyTracker ct;
        AssertLive();
        if ( g_Throw ) {
            if ( ++m_TaskCount == SKIP_CHORES )
                TBB_TEST_THROW(test_exception(EXCEPTION_DESCR1));
            utils::yield();
        }
        else {
            ++g_TaskCount;
            while( !tbb::is_current_task_group_canceling() )
                utils::yield();
        }
    }
};

inline void ResetGlobals ( bool bThrow, bool bRethrow ) {
    g_Throw = bThrow;
    g_Rethrow = bRethrow;
    g_ExceptionCount = 0;
    g_TaskCount = 0;
    utils::ConcurrencyTracker::Reset();
}

template<typename task_group_type>
void LaunchChildrenWithFunctor () {
    atomic_t count;
    count = 0;
    task_group_type g;
    for (unsigned i = 0; i < NUM_CHORES; ++i) {
        if (i % 2 == 1) {
            g.run(g.defer(ThrowingTask(count)));
        } else
        {
            g.run(ThrowingTask(count));
        }
    }
#if TBB_USE_EXCEPTIONS
    tbb::task_group_status status = tbb::not_complete;
    bool exceptionCaught = false;
    try {
        status = g.wait();
    } catch ( TestException& e ) {
        CHECK_MESSAGE( e.what(), "Empty what() string" );
        CHECK_MESSAGE( strcmp(e.what(), EXCEPTION_DESCR1) == 0, "Unknown exception" );
        exceptionCaught = true;
        ++g_ExceptionCount;
    } catch( ... ) { CHECK_MESSAGE( false, "Unknown exception" ); }
    if (g_Throw && !exceptionCaught && status != tbb::canceled) {
        CHECK_MESSAGE(false, "No exception in the child task group");
    }
    if ( g_Rethrow && g_ExceptionCount > SKIP_GROUPS ) {
        throw test_exception(EXCEPTION_DESCR2);
    }
#else
    g.wait();
#endif
}

// Tests for cancellation and exception handling behavior
template<typename task_group_type>
void TestManualCancellationWithFunctor () {
    ResetGlobals( false, false );
    task_group_type tg;
    for (unsigned i = 0; i < NUM_GROUPS; ++i) {
        // TBB version does not require taking function address
        if (i % 2 == 0) {
            auto h = tg.defer(&LaunchChildrenWithFunctor<task_group_type>);
            tg.run(std::move(h));
        } else
        {
            tg.run(&LaunchChildrenWithFunctor<task_group_type>);
        }
    }
    CHECK_MESSAGE ( !tbb::is_current_task_group_canceling(), "Unexpected cancellation" );
    while ( g_MaxConcurrency > 1 && g_TaskCount == 0 )
        utils::yield();
    tg.cancel();
    g_ExecutedAtCancellation = int(g_TaskCount);
    tbb::task_group_status status = tg.wait();
    CHECK_MESSAGE( status == tbb::canceled, "Task group reported invalid status." );
    CHECK_MESSAGE( g_TaskCount <= NUM_GROUPS * NUM_CHORES, "Too many tasks reported. The test is broken" );
    CHECK_MESSAGE( g_TaskCount < NUM_GROUPS * NUM_CHORES, "No tasks were cancelled. Cancellation model changed?" );
    CHECK_MESSAGE( g_TaskCount <= g_ExecutedAtCancellation + utils::ConcurrencyTracker::PeakParallelism(), "Too many tasks survived cancellation" );
}

#if TBB_USE_EXCEPTIONS
template<typename task_group_type>
void TestExceptionHandling1 () {
    ResetGlobals( true, false );
    task_group_type tg;
    for( unsigned i = 0; i < NUM_GROUPS; ++i )
        // TBB version does not require taking function address
        tg.run( &LaunchChildrenWithFunctor<task_group_type> );
    try {
        tg.wait();
    } catch ( ... ) {
        CHECK_MESSAGE( false, "Unexpected exception" );
    }
    CHECK_MESSAGE( g_ExceptionCount <= NUM_GROUPS, "Too many exceptions from the child groups. The test is broken" );
    CHECK_MESSAGE( g_ExceptionCount == NUM_GROUPS, "Not all child groups threw the exception" );
}

template<typename task_group_type>
void TestExceptionHandling2 () {
    ResetGlobals( true, true );
    task_group_type tg;
    bool exceptionCaught = false;
    for( unsigned i = 0; i < NUM_GROUPS; ++i ) {
        // TBB version does not require taking function address
        tg.run( &LaunchChildrenWithFunctor<task_group_type> );
    }
    try {
        tg.wait();
    } catch ( TestException& e ) {
        CHECK_MESSAGE( e.what(), "Empty what() string" );
        CHECK_MESSAGE( strcmp(e.what(), EXCEPTION_DESCR2) == 0, "Unknown exception" );
        exceptionCaught = true;
    } catch( ... ) { CHECK_MESSAGE( false, "Unknown exception" ); }
    CHECK_MESSAGE( exceptionCaught, "No exception thrown from the root task group" );
    CHECK_MESSAGE( g_ExceptionCount >= SKIP_GROUPS, "Too few exceptions from the child groups. The test is broken" );
    CHECK_MESSAGE( g_ExceptionCount <= NUM_GROUPS - SKIP_GROUPS, "Too many exceptions from the child groups. The test is broken" );
    CHECK_MESSAGE( g_ExceptionCount < NUM_GROUPS - SKIP_GROUPS, "None of the child groups was cancelled" );
}

template <typename task_group_type>
void TestExceptionHandling3() {
    task_group_type tg;
    try {
        tg.run_and_wait([]() {
            volatile bool suppress_unreachable_code_warning = true;
            if (suppress_unreachable_code_warning) {
                throw 1;
            }
        });
    } catch (int error) {
        CHECK(error == 1);
    } catch ( ... ) {
        CHECK_MESSAGE( false, "Unexpected exception" );
    }
}

template<typename task_group_type>
class LaunchChildrenDriver {
public:
    void Launch(task_group_type& tg) {
        ResetGlobals(false, false);
        for (unsigned i = 0; i < NUM_GROUPS; ++i) {
            tg.run(LaunchChildrenWithFunctor<task_group_type>);
        }
        CHECK_MESSAGE(!tbb::is_current_task_group_canceling(), "Unexpected cancellation");
        while (g_MaxConcurrency > 1 && g_TaskCount == 0)
            utils::yield();
    }

    void Finish() {
        CHECK_MESSAGE(g_TaskCount <= NUM_GROUPS * NUM_CHORES, "Too many tasks reported. The test is broken");
        CHECK_MESSAGE(g_TaskCount < NUM_GROUPS * NUM_CHORES, "No tasks were cancelled. Cancellation model changed?");
        CHECK_MESSAGE(g_TaskCount <= g_ExecutedAtCancellation + g_MaxConcurrency, "Too many tasks survived cancellation");
    }
}; // LaunchChildrenWithTaskHandleDriver

template<typename task_group_type, bool Throw>
void TestMissingWait () {
    bool exception_occurred = false,
         unexpected_exception = false;
    LaunchChildrenDriver<task_group_type> driver;
    try {
        task_group_type tg;
        driver.Launch( tg );
        volatile bool suppress_unreachable_code_warning = Throw;
        if (suppress_unreachable_code_warning) {
            throw int(); // Initiate stack unwinding
        }
    }
    catch ( const tbb::missing_wait& e ) {
        CHECK_MESSAGE( e.what(), "Error message is absent" );
        exception_occurred = true;
        unexpected_exception = Throw;
    }
    catch ( int ) {
        exception_occurred = true;
        unexpected_exception = !Throw;
    }
    catch ( ... ) {
        exception_occurred = unexpected_exception = true;
    }
    CHECK( exception_occurred );
    CHECK( !unexpected_exception );
    driver.Finish();
}
#endif

template<typename task_group_type>
void RunCancellationAndExceptionHandlingTests() {
    TestManualCancellationWithFunctor<task_group_type>();
#if TBB_USE_EXCEPTIONS
    TestExceptionHandling1<task_group_type>();
    TestExceptionHandling2<task_group_type>();
    TestExceptionHandling3<task_group_type>();
    TestMissingWait<task_group_type, true>();
    TestMissingWait<task_group_type, false>();
#endif
}

void EmptyFunction () {}

struct TestFunctor {
    void operator()() { CHECK_MESSAGE( false, "Non-const operator called" ); }
    void operator()() const { /* library requires this overload only */ }
};

template<typename task_group_type>
void TestConstantFunctorRequirement() {
    task_group_type g;
    TestFunctor tf;
    g.run( tf ); g.wait();
    g.run_and_wait( tf );
}

//------------------------------------------------------------------------
namespace TestMoveSemanticsNS {
    struct TestFunctor {
        void operator()() const {};
    };

    struct MoveOnlyFunctor : utils::MoveOnly, TestFunctor {
        MoveOnlyFunctor() : utils::MoveOnly() {};
        MoveOnlyFunctor(MoveOnlyFunctor&& other) : utils::MoveOnly(std::move(other)) {};
    };

    struct MovePreferableFunctor : utils::Movable, TestFunctor {
        MovePreferableFunctor() : utils::Movable() {};
        MovePreferableFunctor(MovePreferableFunctor&& other) : utils::Movable(std::move(other)) {};
        MovePreferableFunctor(const MovePreferableFunctor& other) : utils::Movable(other) {};
    };

    struct NoMoveNoCopyFunctor : utils::NoCopy, TestFunctor {
        NoMoveNoCopyFunctor() : utils::NoCopy() {};
        // mv ctor is not allowed as cp ctor from parent utils::NoCopy
    private:
        NoMoveNoCopyFunctor(NoMoveNoCopyFunctor&&);
    };

     template<typename task_group_type>
    void TestBareFunctors() {
        task_group_type tg;
        MovePreferableFunctor mpf;
        // run_and_wait() doesn't have any copies or moves of arguments inside the impl
        tg.run_and_wait( NoMoveNoCopyFunctor() );

        tg.run( MoveOnlyFunctor() );
        tg.wait();

        tg.run( mpf );
        tg.wait();
        CHECK_MESSAGE(mpf.alive, "object was moved when was passed by lval");
        mpf.Reset();

        tg.run( std::move(mpf) );
        tg.wait();
        CHECK_MESSAGE(!mpf.alive, "object was copied when was passed by rval");
        mpf.Reset();
    }
}

template<typename task_group_type>
void TestMoveSemantics() {
    TestMoveSemanticsNS::TestBareFunctors<task_group_type>();
}
//------------------------------------------------------------------------

// TODO: TBB_REVAMP_TODO - enable when ETS is available
#if TBBTEST_USE_TBB && TBB_PREVIEW_ISOLATED_TASK_GROUP
namespace TestIsolationNS {
    class DummyFunctor {
    public:
        DummyFunctor() {}
        void operator()() const {
            for ( volatile int j = 0; j < 10; ++j ) {}
        }
    };

    template<typename task_group_type>
    class ParForBody {
        task_group_type& m_tg;
        std::atomic<bool>& m_preserved;
        tbb::enumerable_thread_specific<int>& m_ets;
    public:
        ParForBody(
            task_group_type& tg,
            std::atomic<bool>& preserved,
            tbb::enumerable_thread_specific<int>& ets
        ) : m_tg(tg), m_preserved(preserved), m_ets(ets) {}

        void operator()(int) const {
            if (++m_ets.local() > 1) m_preserved = false;

            for (int i = 0; i < 1000; ++i)
                m_tg.run(DummyFunctor());
            m_tg.wait();
            m_tg.run_and_wait(DummyFunctor());

            --m_ets.local();
        }
    };

    template<typename task_group_type>
    void CheckIsolation(bool isolation_is_expected) {
        task_group_type tg;
        std::atomic<bool> isolation_is_preserved;
        isolation_is_preserved = true;
        tbb::enumerable_thread_specific<int> ets(0);

        tbb::parallel_for(0, 100, ParForBody<task_group_type>(tg, isolation_is_preserved, ets));

        ASSERT(
            isolation_is_expected == isolation_is_preserved,
            "Actual and expected isolation-related behaviours are different"
        );
    }

    // Should be called only when > 1 thread is used, because otherwise isolation is guaranteed to take place
    void TestIsolation() {
        CheckIsolation<tbb::task_group>(false);
        CheckIsolation<tbb::isolated_task_group>(true);
    }
}
#endif

#if __TBB_USE_ADDRESS_SANITIZER
//! Test for thread safety for the task_group
//! \brief \ref error_guessing \ref resource_usage
TEST_CASE("Memory leaks test is not applicable under ASAN\n" * doctest::skip(true)) {}
#elif !EMSCRIPTEN
//! Emscripten requires preloading of the file used to determine memory usage, hence disabled.
//! Test for thread safety for the task_group
//! \brief \ref error_guessing \ref resource_usage
TEST_CASE("Thread safety test for the task group") {
    if (tbb::this_task_arena::max_concurrency() < 2) {
        // The test requires more than one thread to check thread safety
        return;
    }
    for (unsigned p=MinThread; p <= MaxThread; ++p) {
        if (p < 2) {
            continue;
        }
        tbb::global_control limit(tbb::global_control::max_allowed_parallelism, p);
        g_MaxConcurrency = p;
        TestThreadSafety<tbb::task_group>();
    }
}
#endif

//! Fibonacci test for task group
//! \brief \ref interface \ref requirement
TEST_CASE("Fibonacci test for the task group") {
    for (unsigned p=MinThread; p <= MaxThread; ++p) {
        tbb::global_control limit(tbb::global_control::max_allowed_parallelism, p);
        tbb::task_arena a(p);
        g_MaxConcurrency = p;
        a.execute([] {
            RunFibonacciTests<tbb::task_group>();
        });
    }
}

//! Cancellation and exception test for the task group
//! \brief \ref interface \ref requirement
TEST_CASE("Cancellation and exception test for the task group") {
    for (unsigned p = MinThread; p <= MaxThread; ++p) {
        tbb::global_control limit(tbb::global_control::max_allowed_parallelism, p);
        tbb::task_arena a(p);
        g_MaxConcurrency = p;
        a.execute([] {
            RunCancellationAndExceptionHandlingTests<tbb::task_group>();
        });
    }
}

//! Constant functor test for the task group
//! \brief \ref interface \ref negative
TEST_CASE("Constant functor test for the task group") {
    for (unsigned p=MinThread; p <= MaxThread; ++p) {
        tbb::global_control limit(tbb::global_control::max_allowed_parallelism, p);
        g_MaxConcurrency = p;
        TestConstantFunctorRequirement<tbb::task_group>();
    }
}

//! Move semantics test for the task group
//! \brief \ref interface \ref requirement
TEST_CASE("Move semantics test for the task group") {
    for (unsigned p=MinThread; p <= MaxThread; ++p) {
        tbb::global_control limit(tbb::global_control::max_allowed_parallelism, p);
        g_MaxConcurrency = p;
        TestMoveSemantics<tbb::task_group>();
    }
}

#if TBB_PREVIEW_ISOLATED_TASK_GROUP

#if __TBB_USE_ADDRESS_SANITIZER
//! Test for thread safety for the isolated_task_group
//! \brief \ref error_guessing
TEST_CASE("Memory leaks test is not applicable under ASAN\n" * doctest::skip(true)) {}
#elif !EMSCRIPTEN
//! Test for thread safety for the isolated_task_group
//! \brief \ref error_guessing
TEST_CASE("Thread safety test for the isolated task group") {
    if (tbb::this_task_arena::max_concurrency() < 2) {
        // The test requires more than one thread to check thread safety
        return;
    }
    for (unsigned p=MinThread; p <= MaxThread; ++p) {
        if (p < 2) {
            continue;
        }
        tbb::global_control limit(tbb::global_control::max_allowed_parallelism, p);
        g_MaxConcurrency = p;
        tbb::task_arena a(p);
        a.execute([] {
            TestThreadSafety<tbb::isolated_task_group>();
        });
    }
}
#endif

//! Cancellation and exception test for the isolated task group
//! \brief \ref interface \ref requirement
TEST_CASE("Fibonacci test for the isolated task group") {
    for (unsigned p=MinThread; p <= MaxThread; ++p) {
        tbb::global_control limit(tbb::global_control::max_allowed_parallelism, p);
        g_MaxConcurrency = p;
        tbb::task_arena a(p);
        a.execute([] {
            RunFibonacciTests<tbb::isolated_task_group>();
        });
    }
}

//! Cancellation and exception test for the isolated task group
//! \brief \ref interface \ref requirement
TEST_CASE("Cancellation and exception test for the isolated task group") {
    for (unsigned p=MinThread; p <= MaxThread; ++p) {
        tbb::global_control limit(tbb::global_control::max_allowed_parallelism, p);
        g_MaxConcurrency = p;
        tbb::task_arena a(p);
        a.execute([] {
            RunCancellationAndExceptionHandlingTests<tbb::isolated_task_group>();
        });
    }
}

//! Constant functor test for the isolated task group.
//! \brief \ref interface \ref negative
TEST_CASE("Constant functor test for the isolated task group") {
    for (unsigned p=MinThread; p <= MaxThread; ++p) {
        tbb::global_control limit(tbb::global_control::max_allowed_parallelism, p);
        g_MaxConcurrency = p;
        tbb::task_arena a(p);
        a.execute([] {
            TestConstantFunctorRequirement<tbb::isolated_task_group>();
        });
    }
}

//! Move semantics test for the isolated task group.
//! \brief \ref interface \ref requirement
TEST_CASE("Move semantics test for the isolated task group") {
    for (unsigned p=MinThread; p <= MaxThread; ++p) {
        tbb::global_control limit(tbb::global_control::max_allowed_parallelism, p);
        g_MaxConcurrency = p;
        tbb::task_arena a(p);
        a.execute([] {
            TestMoveSemantics<tbb::isolated_task_group>();
        });
    }
}

//TODO: add test void isolated_task_group::run(d2::task_handle&& h) and isolated_task_group::::run_and_wait(d2::task_handle&& h)
#endif /* TBB_PREVIEW_ISOLATED_TASK_GROUP */

void run_deep_stealing(tbb::task_group& tg1, tbb::task_group& tg2, int num_tasks, std::atomic<int>& tasks_executed) {
    for (int i = 0; i < num_tasks; ++i) {
        tg2.run([&tg1, &tasks_executed] {
            volatile char consume_stack[1000]{};
            ++tasks_executed;
            tg1.wait();
            utils::suppress_unused_warning(consume_stack);
        });
    }
}

// ASan uses fake stack for stack-use-after-scope detection that makes stack overflow
// avoidance mechanism incompatible because it is based on assumption that local variables are located
// on the real stack.
#if !__TBB_USE_ADDRESS_SANITIZER
// TODO: move to the conformance test
//! Test for stack overflow avoidance mechanism.
//! \brief \ref requirement
TEST_CASE("Test for stack overflow avoidance mechanism") {
    if (tbb::this_task_arena::max_concurrency() < 2) {
        return;
    }

    tbb::global_control thread_limit(tbb::global_control::max_allowed_parallelism, 2);
    tbb::task_group tg1;
    tbb::task_group tg2;
    std::atomic<int> tasks_executed{};
    tg1.run_and_wait([&tg1, &tg2, &tasks_executed] {
        run_deep_stealing(tg1, tg2, 10000, tasks_executed);
        while (tasks_executed < 100) {
            // Some stealing is expected to happen.
            utils::yield();
        }
        CHECK(tasks_executed < 10000);
    });
    tg2.wait();
    CHECK(tasks_executed == 10000);
}

//! Test for stack overflow avoidance mechanism.
//! \brief \ref error_guessing
TEST_CASE("Test for stack overflow avoidance mechanism within arena") {
    if (tbb::this_task_arena::max_concurrency() < 2) {
        return;
    }

    tbb::task_arena a1(2, 1);
    a1.execute([] {
        tbb::task_group tg1;
        tbb::task_group tg2;
        std::atomic<int> tasks_executed{};

        // Determine nested task execution limit.
        int second_thread_executed{};
        tg1.run_and_wait([&tg1, &tg2, &tasks_executed, &second_thread_executed] {
            run_deep_stealing(tg1, tg2, 10000, tasks_executed);
            do {
                second_thread_executed = tasks_executed;
                utils::Sleep(10);
            } while (second_thread_executed < 100 || second_thread_executed != tasks_executed);
            CHECK(tasks_executed < 10000);
        });
        tg2.wait();
        CHECK(tasks_executed == 10000);

        tasks_executed = 0;
        tbb::task_arena a2(2, 2);
        tg1.run_and_wait([&a2, &tg1, &tg2, &tasks_executed, second_thread_executed] {
            run_deep_stealing(tg1, tg2, second_thread_executed - 1, tasks_executed);
            while (tasks_executed < second_thread_executed - 1) {
                // Wait until the second thread near the limit.
                utils::yield();
            }
            tg2.run([&a2, &tg1, &tasks_executed] {
                a2.execute([&tg1, &tasks_executed] {
                    volatile char consume_stack[1000]{};
                    ++tasks_executed;
                    tg1.wait();
                    utils::suppress_unused_warning(consume_stack);
                });
            });
            while (tasks_executed < second_thread_executed) {
                // Wait until the second joins the arena.
                utils::yield();
            }
            a2.execute([&tg1, &tg2, &tasks_executed] {
                run_deep_stealing(tg1, tg2, 10000, tasks_executed);
            });
            int currently_executed{};
            do {
                currently_executed = tasks_executed;
                utils::Sleep(10);
            } while (currently_executed != tasks_executed);
            CHECK(tasks_executed < 10000 + second_thread_executed);
        });
        a2.execute([&tg2] {
            tg2.wait();
        });
        CHECK(tasks_executed == 10000 + second_thread_executed);
    });
}
#endif // !__TBB_USE_ADDRESS_SANITIZER

//! Test checks that we can submit work to task_group asynchronously with waiting.
//! \brief \ref regression
TEST_CASE("Async task group") {
    int num_threads = tbb::this_task_arena::max_concurrency();
    if (num_threads < 3) {
        // The test requires at least 2 worker threads
        return;
    }
    tbb::task_arena a(2*num_threads, num_threads);
    utils::SpinBarrier barrier(num_threads + 2);
    tbb::task_group tg[2];
    std::atomic<bool> finished[2]{};
    finished[0] = false; finished[1] = false;
    for (int i = 0; i < 2; ++i) {
        a.enqueue([i, &tg, &finished, &barrier] {
            barrier.wait();
            for (int j = 0; j < 10000; ++j) {
                tg[i].run([] {});
                utils::yield();
            }
            finished[i] = true;
        });
    }
    utils::NativeParallelFor(num_threads, [&](int idx) {
        barrier.wait();
        a.execute([idx, &tg, &finished] {
            std::size_t counter{};
            while (!finished[idx%2]) {
                tg[idx%2].wait();
                if (counter++ % 16 == 0) utils::yield();
            }
            tg[idx%2].wait();
        });
    });
}

struct SelfRunner {
    tbb::task_group& m_tg;
    std::atomic<unsigned>& count;
    void operator()() const {
        unsigned previous_count = count.fetch_sub(1);
        if (previous_count > 1)
            m_tg.run( *this );
    }
};

//! Submit work to single task_group instance from inside the work
//! \brief \ref error_guessing
TEST_CASE("Run self using same task_group instance") {
    const unsigned num = 10;
    std::atomic<unsigned> count{num};
    tbb::task_group tg;
    SelfRunner uf{tg, count};
    tg.run( uf );
    tg.wait();
    CHECK_MESSAGE(
        count == 0,
        "Not all tasks were spawned from inside the functor running within task_group."
    );
}

//TODO: move to some common place to share with conformance tests
namespace accept_task_group_context {

template <typename TaskGroup, typename CancelF, typename WaitF>
void run_cancellation_use_case(CancelF&& cancel, WaitF&& wait) {
    std::atomic<bool> outer_cancelled{false};
    std::atomic<unsigned> count{13};

    tbb::task_group_context inner_ctx(tbb::task_group_context::isolated);
    TaskGroup inner_tg(inner_ctx);

    tbb::task_group outer_tg;
    auto outer_tg_task = [&] {
        inner_tg.run([&] {
            utils::SpinWaitUntilEq(outer_cancelled, true);
            inner_tg.run( SelfRunner{inner_tg, count} );
        });

        utils::try_call([&] {
            std::forward<CancelF>(cancel)(outer_tg);
        }).on_completion([&] {
            outer_cancelled = true;
        });
    };

    auto check = [&] {
        tbb::task_group_status outer_status = tbb::task_group_status::not_complete;
        outer_status = std::forward<WaitF>(wait)(outer_tg);
        CHECK_MESSAGE(
            outer_status == tbb::task_group_status::canceled,
            "Outer task group should have been cancelled."
        );

        tbb::task_group_status inner_status = inner_tg.wait();
        CHECK_MESSAGE(
            inner_status == tbb::task_group_status::complete,
            "Inner task group should have completed despite the cancellation of the outer one."
        );

        CHECK_MESSAGE(0 == count, "Some of the inner group tasks were not executed.");
    };

    outer_tg.run(outer_tg_task);
    check();
}

template <typename TaskGroup>
void test() {
    run_cancellation_use_case<TaskGroup>(
        [](tbb::task_group& outer) { outer.cancel(); },
        [](tbb::task_group& outer) { return outer.wait(); }
    );

#if TBB_USE_EXCEPTIONS
    run_cancellation_use_case<TaskGroup>(
        [](tbb::task_group& /*outer*/) {
            volatile bool suppress_unreachable_code_warning = true;
            if (suppress_unreachable_code_warning) {
                throw int();
            }
        },
        [](tbb::task_group& outer) {
            try {
                outer.wait();
                return tbb::task_group_status::complete;
            } catch(const int&) {
                return tbb::task_group_status::canceled;
            }
        }
    );
#endif
}

} // namespace accept_task_group_context

//! Respect task_group_context passed from outside
//! \brief \ref interface \ref requirement
TEST_CASE("Respect task_group_context passed from outside") {
#if TBB_PREVIEW_ISOLATED_TASK_GROUP
    accept_task_group_context::test<tbb::isolated_task_group>();
#endif
}

#if __TBB_PREVIEW_TASK_GROUP_EXTENSIONS
//! The test for task_handle inside other task waiting with run
//! \brief \ref requirement
TEST_CASE("Task handle for scheduler bypass"){
    tbb::task_group tg;
    std::atomic<bool> run {false};

    tg.run([&]{
        return tg.defer([&]{
            run = true;
        });
    });

    tg.wait();
    CHECK_MESSAGE(run == true, "task handle returned by user lambda (bypassed) should be run");

    // Test returning an empty handle
    run = false;
    tg.run([&] {
        run = true;
        return tbb::task_handle{};
    });

    tg.wait();
    CHECK(run == true);
}

//! The test for task_handle inside other task waiting with run_and_wait
//! \brief \ref requirement
TEST_CASE("Task handle for scheduler bypass via run_and_wait"){
    tbb::task_group tg;
    std::atomic<bool> run {false};

    tg.run_and_wait([&]{
        return tg.defer([&]{
            run = true;
        });
    });

    CHECK_MESSAGE(run == true, "task handle returned by user lambda (bypassed) should be run");

    // Test returning an empty handle
    run = false;
    tg.run_and_wait([&] {
        run = true;
    });
    CHECK(run == true);
}
#endif //__TBB_PREVIEW_TASK_GROUP_EXTENSIONS

#if TBB_USE_EXCEPTIONS
#if __TBB_PREVIEW_TASK_GROUP_EXTENSIONS && __TBB_GCC_VERSION && !__clang__ && !__INTEL_COMPILER
// GCC issues a warning in task_handle_task::has_dependencies for empty task_handle
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wstringop-overflow"
#endif
//As these tests are against behavior marked by spec as undefined, they can not be put into conformance tests

//! The test for error in scheduling empty task_handle
//! \brief \ref requirement
TEST_CASE("Empty task_handle cannot be scheduled"
        * doctest::should_fail()    //Test needs to revised as implementation uses assertions instead of exceptions
        * doctest::skip()           //skip the test for now, to not pollute the test log
){
    tbb::task_group tg;

    CHECK_THROWS_WITH_AS(tg.run(tbb::task_handle{}), "Attempt to schedule empty task_handle", std::runtime_error);
}

//! The test for error in task_handle being scheduled into task_group different from one it was created from
//! \brief \ref requirement
TEST_CASE("task_handle cannot be scheduled into different task_group"
        * doctest::should_fail()    //Test needs to revised as implementation uses assertions instead of exceptions
        * doctest::skip()           //skip the test for now, to not pollute the test log
){
    tbb::task_group tg;
    tbb::task_group tg1;

    CHECK_THROWS_WITH_AS(tg1.run(tg.defer([]{})), "Attempt to schedule task_handle into different task_group", std::runtime_error);
}

//! The test for error in task_handle being scheduled into task_group different from one it was created from
//! \brief \ref requirement
TEST_CASE("task_handle cannot be scheduled into other task_group of the same context"
        * doctest::should_fail()    //Implementation is no there yet, as it is not clear that is the expected behavior
        * doctest::skip()           //skip the test for now, to not pollute the test log
)
{
    tbb::task_group_context ctx;

    tbb::task_group tg(ctx);
    tbb::task_group tg1(ctx);

    CHECK_NOTHROW(tg.run(tg.defer([]{})));
    CHECK_THROWS_WITH_AS(tg1.run(tg.defer([]{})), "Attempt to schedule task_handle into different task_group", std::runtime_error);
}

//! Test for thread-safe exception handling in concurrent wait scenarios
//! \brief \ref error_guessing \ref interface
TEST_CASE("Concurrent exception handling in task_group wait") {
    const int num_waiters = 4;
    const int num_iterations = 5;
    
    for (int iter = 0; iter < num_iterations; ++iter) {
        tbb::task_group tg;
        utils::SpinBarrier barrier(num_waiters + 1); // +1 for main thread
        std::atomic<int> exceptions_caught{0};
        std::atomic<int> normal_completions{0};
        std::atomic<bool> task_thrown{false};
        
        // Launch a task that will throw an exception
        tg.run([&] {
            barrier.wait(); // Wait for all waiters to be ready
            utils::yield(); // Give waiters a chance to enter wait state
            task_thrown = true;
            throw test_exception("Concurrent wait exception test");
        });
        
        // Launch multiple threads that will concurrently call wait()
        std::vector<std::thread> waiters;
        for (int i = 0; i < num_waiters; ++i) {
            waiters.emplace_back([&] {
                barrier.wait(); // Synchronize start
                try {
                    tbb::task_group_status status = tg.wait();
                    if (status != tbb::not_complete)
                        ++normal_completions;
                } catch (const test_exception& e) {
                    CHECK_MESSAGE(std::string(e.what()) == "Concurrent wait exception test", 
                                "Unexpected exception message");
                    ++exceptions_caught;
                } catch (...) {
                    CHECK_MESSAGE(false, "Unexpected exception type caught");
                }
            });
        }
                
        try {
            tbb::task_group_status status = tg.wait();
            if (status != tbb::not_complete)
                ++normal_completions;
        } catch (const test_exception& e) {
            CHECK_MESSAGE(std::string(e.what()) == "Concurrent wait exception test", 
                        "Unexpected exception message");
            ++exceptions_caught;
        } catch (...) {
            CHECK_MESSAGE(false, "Unexpected exception type caught");
        }
        
        // Wait for all threads to complete
        for (auto& waiter : waiters) {
            waiter.join();
        }
        
        // Verify that the exception was properly handled
        CHECK_MESSAGE(task_thrown.load(), "Task should have thrown an exception");
        
        // At least one thread should have caught the exception
        CHECK_MESSAGE(exceptions_caught.load() > 0, 
                     "At least one waiter should have caught the exception");
        CHECK_MESSAGE(normal_completions.load() + exceptions_caught.load() == num_waiters + 1,
                     "The total caught or completed is num_waiters + 1");
    }
}

//! \brief \ref error_guessing
TEST_CASE("Arena exception handling race condition test") { 
    const int num_iterations = 5;
    const int num_threads = std::thread::hardware_concurrency();
    
    for (int iter = 0; iter < num_iterations; ++iter) {
        bool throw_to_arena = (iter < num_iterations/2) ? false : true;
        utils::SpinBarrier barrier(num_threads);
        std::atomic<int> exception_count{0};
        std::atomic<int> expected_task_group_caught{0};
        std::atomic<int> expected_arena_caught{0};
        std::atomic<int> successful_executions{0};
        std::vector<std::thread> threads;
        
        tbb::task_arena arena{num_threads};
        for (int t = 0; t < num_threads; ++t) {
            threads.emplace_back([throw_to_arena, &barrier, &arena, &exception_count, 
                                  &expected_task_group_caught, &expected_arena_caught,
                                  &successful_executions, t]() {
                try {
                    barrier.wait();
                    arena.execute([throw_to_arena, &exception_count, &expected_task_group_caught, t]() {
                        // Create nested task groups that will throw exceptions simultaneously
                        tbb::task_group tg;
                        
                        // Launch multiple tasks that throw at the same time
                        for (int i = 0; i < 10; ++i) {
                            tg.run([&exception_count, t, i]() {
                                // Small delay to increase chance of simultaneous exceptions
                                std::this_thread::sleep_for(std::chrono::microseconds(1));
                                exception_count.fetch_add(1);
                                throw std::runtime_error("Test exception " + std::to_string(t) + "_" + std::to_string(i));
                            });
                        }
                        
                        // This wait() should trigger the arena exception handling code
                        try {
                            tg.wait();
                        } catch (const std::runtime_error&) {
                            expected_task_group_caught.fetch_add(1);
							if (throw_to_arena) throw;
                        }
                    });
                    successful_executions.fetch_add(1);
                } catch (const std::runtime_error&) {
                    expected_arena_caught.fetch_add(1);
                    successful_executions.fetch_add(1);
                }
            });
        }
        
        // Wait for all threads
        for (auto& thread : threads) {
            thread.join();
        }
        
        // Verify we didn't crash and all threads completed
        CHECK_MESSAGE(successful_executions.load() == num_threads, "All executions should finish");
        CHECK_MESSAGE(exception_count.load(), "Should have thrown at least one exception");
        CHECK_MESSAGE(expected_task_group_caught.load(), "Should have caught at least one exception");
        CHECK_MESSAGE(expected_task_group_caught.load() <= exception_count.load(), "Should not catch more exceptions than are thrown");
		if (throw_to_arena) {
			CHECK_MESSAGE(expected_arena_caught.load(), "Should have caught exceptions in arena");
			CHECK_MESSAGE(expected_arena_caught.load() <= expected_task_group_caught.load(), "Should not catch more exceptions than are thrown");
		} else {
			CHECK_MESSAGE(expected_arena_caught.load() == 0, "Should not catch exceptions in arena when not throwing to arena");
		}
    }
}

//! \brief \ref error_guessing
TEST_CASE("Task dispatcher exception handling race condition test") {
    if (tbb::this_task_arena::max_concurrency() < 2) return;
    
    const int num_iterations = 50;
    const int tasks_to_throw = 20;
    const int num_concurrent_waits = 8;
    
    for (int iter = 0; iter < num_iterations; ++iter) {
        utils::SpinBarrier barrier(num_concurrent_waits);
        std::atomic<int> exceptions_thrown{0};
        std::atomic<int> expected_exceptions_caught{0};
        std::atomic<int> unexpected_exceptions_caught{0};
        std::vector<std::thread> threads;
        
        // Create a shared task group that will have exceptions
        tbb::task_group shared_tg;
        
        // Launch tasks that will throw exceptions
        for (int i = 0; i < tasks_to_throw; ++i) {
            shared_tg.run([&exceptions_thrown, i]() {
                // Stagger the exceptions slightly
                if (i % 3 == 0) {
                    std::this_thread::sleep_for(std::chrono::microseconds(1));
                }
                exceptions_thrown.fetch_add(1);
                throw std::runtime_error("Dispatcher test exception " + std::to_string(i));
            });
        }
        
        // Launch multiple threads that will wait on the same task group simultaneously
        // This will trigger concurrent calls to task_dispatcher::execute_and_wait
        for (int t = 0; t < num_concurrent_waits; ++t) {
            threads.emplace_back([&barrier, &shared_tg, 
                                  &expected_exceptions_caught, 
                                  &unexpected_exceptions_caught]() {
                try {
                    barrier.wait();

                    // Create a nested task group for additional complexity
                    tbb::task_group nested_tg;
                    
                    // Add some non-throwing tasks to the nested group
                    for (int i = 0; i < 5; ++i) {
                        nested_tg.run([i]() {
                            // Some work that doesn't throw
                            volatile int x = i * i;
                            (void)x;
                        });
                    }
                    
                    // Wait on both groups - this should trigger the exception handling
                    // in task_dispatcher::execute_and_wait when the shared_tg exceptions propagate
                    try {
                        nested_tg.wait();
                        shared_tg.wait(); // This will throw due to the exceptions above
                    } catch (const std::runtime_error&) {
                        // Expected exception from shared_tg
                        expected_exceptions_caught.fetch_add(1);
                    }
                } catch (...) {
                    unexpected_exceptions_caught.fetch_add(1);
                }
            });
        }
        
        // Wait for all threads to complete
        for (auto& thread : threads) {
            thread.join();
        }
        
        // Verify all waits completed successfully without crashes
        CHECK_MESSAGE(exceptions_thrown.load(), "Should have thrown at least one exception");
        CHECK_MESSAGE(expected_exceptions_caught.load(), "Should have caught at least one exception");
        CHECK_MESSAGE(expected_exceptions_caught.load() <= exceptions_thrown.load(), "Should not catch more exceptions than are thrown");
        CHECK_MESSAGE(unexpected_exceptions_caught.load() == 0, "Should not catch unknown exceptiom types");
    }
}

//! \brief \ref error_guessing
TEST_CASE("Mixed arena and task_group race test") {
    const int num_iterations = 50;
    
    for (int iter = 0; iter < num_iterations; ++iter) {
        utils::SpinBarrier barrier(3);
        std::atomic<int> successful_completions{0};
        std::atomic<int> total_exceptions{0};
        std::atomic<int> exceptions_caught{0};
        
        tbb::task_arena shared_arena;
        
        std::thread arena_thread([&barrier, &shared_arena, &total_exceptions,
                                 &successful_completions, &exceptions_caught]() {
            try {
                barrier.wait();
                shared_arena.execute([&total_exceptions, &successful_completions]() {
                    tbb::task_group tg; 
                    for (int i = 0; i < 15; ++i) {
                        tg.run([&total_exceptions, i]() { // this will bind to shared_arena's task_group_context
                            std::this_thread::sleep_for(std::chrono::microseconds(i % 3));
                            total_exceptions.fetch_add(1);
                            throw std::logic_error("Arena exception " + std::to_string(i));
                        });
                    }
                    tg.wait();
                    successful_completions.fetch_add(1);
                });
            } catch (const std::exception&) {
                exceptions_caught.fetch_add(1);
            }
        });
        
        // Thread 2: Exercises task dispatcher exception handling
        std::thread dispatcher_thread([&barrier, &total_exceptions, 
                                       &successful_completions, &exceptions_caught]() {
            try {
                barrier.wait();
                tbb::task_group tg; 
                for (int i = 0; i < 12; ++i) {
                    tg.run([&total_exceptions, i]() { // this will bind to dispatcher_thread arena
                        std::this_thread::sleep_for(std::chrono::microseconds((i + 1) % 4));
                        total_exceptions.fetch_add(1);
                        throw std::invalid_argument("Dispatcher exception " + std::to_string(i));
                    });
                }
                tg.wait();
                successful_completions.fetch_add(1);
            } catch (const std::exception&) {
                exceptions_caught.fetch_add(1);
            }
        });
        
        // Thread 3: Mixed workload
        std::thread mixed_thread([&barrier, &shared_arena, &total_exceptions,
                                  &successful_completions, &exceptions_caught]() {
            try {
                barrier.wait();
                tbb::task_group outer_tg; // binds to mixed_thread's arena's context
                
                // Task that uses arena
                outer_tg.run([&shared_arena, &total_exceptions]() {
                    try {
                        shared_arena.execute([&total_exceptions]() { // this will bind to shared_arena's context
                            total_exceptions.fetch_add(1);
                            throw std::out_of_range("Mixed arena exception");
                        });
					} catch (const std::exception&) {
                        total_exceptions.fetch_add(1);
                        throw; // back to mixed_thread's task_group_context
                    }
                });
                
                // Task that doesn't use arena
                outer_tg.run([&total_exceptions]() { // binds to mixed_thread's arena's context
                    std::this_thread::sleep_for(std::chrono::microseconds(2));
                    total_exceptions.fetch_add(1);
                    throw std::overflow_error("Mixed direct exception");
                });
                
                outer_tg.wait();
                successful_completions.fetch_add(1);
            } catch (const std::exception&) {
                exceptions_caught.fetch_add(1);
            }
        });
        
        arena_thread.join();
        dispatcher_thread.join();
        mixed_thread.join();
        
        // All threads should have completed successfully despite exceptions
        // Verify all waits completed successfully without crashes
        CHECK_MESSAGE(total_exceptions.load(), "Should have thrown at least one exception");
        CHECK_MESSAGE(successful_completions.load() + exceptions_caught.load() == 3, "Should have failed or succeeded three times total");
    }
}

#if __TBB_PREVIEW_TASK_GROUP_EXTENSIONS && __TBB_GCC_VERSION && !__clang__ && !__INTEL_COMPILER
#pragma GCC diagnostic pop
#endif

//! \brief \ref requirement
TEST_CASE("Test safe task submit from external thread") {
    tbb::task_arena ta{};
    tbb::task_group tg{};

    bool run = false;
    auto body = [&] { run = true; };
    std::thread([&] {
        tbb::task_handle task = tg.defer(body);
        ta.execute([&] {
            tg.run(std::move(task));
        });
    }).join();

    ta.execute([&] {
        tg.wait();
    });

    run = false;
    tbb::task_handle task;
    std::thread([&] { task = tg.defer(body); }).join();
    tg.run(std::move(task));
    tg.wait();
}

#endif // TBB_USE_EXCEPTIONS

#if __TBB_PREVIEW_TASK_GROUP_EXTENSIONS
//! \brief \ref interface \ref requirement \ref error_guessing
TEST_CASE("test task_completion_handle") {
    tbb::task_group tg;

    std::atomic<std::size_t> task_placeholder{0};
    auto task_body = [&] { ++task_placeholder; };

    {
        // Test empty completion_handle
        tbb::task_completion_handle completion_handle;

        CHECK_MESSAGE(!completion_handle, "Non-empty completion_handle was default constructed");
        CHECK_MESSAGE(completion_handle == nullptr, "Unexpected result for comparison of empty completion_handle with nullptr");
        CHECK_MESSAGE(nullptr == completion_handle, "Unexpected result for comparison of nullptr with empty completion_handle");
        CHECK_MESSAGE(!(completion_handle != nullptr), "Unexpected result for comparison of empty completion_handle with nullptr");
        CHECK_MESSAGE(!(nullptr != completion_handle), "Unexpected result for comparison of nullptr with empty completion_handle");

        tbb::task_completion_handle handle_copy(completion_handle);
        CHECK_MESSAGE(!handle_copy, "Non-empty completion_handle was copied from empty completion_handle");
        CHECK_MESSAGE(handle_copy == completion_handle, "Unexpected result for comparison of empty completion_handle");
        CHECK_MESSAGE(!(handle_copy != completion_handle), "Unexpected result for comparison of empty completion_handle");

        tbb::task_completion_handle handle_move(std::move(completion_handle));
        CHECK_MESSAGE(!handle_move, "Non-empty completion_handle was moved from empty completion_handle");
        CHECK_MESSAGE(!completion_handle, "Moved-from completion_handle is not empty");

        handle_copy = completion_handle;
        CHECK_MESSAGE(!handle_copy, "Non-empty completion_handle after copy assignment from empty completion_handle");
        
        handle_move = std::move(completion_handle);
        CHECK_MESSAGE(!handle_move, "Non-empty completion_handle after move assignment from empty completion_handle");
    }
    {
        // Test non-empty completion_handle
        tbb::task_handle task1_handle = tg.defer(task_body);
        tbb::task_completion_handle task1_completion_handle = task1_handle;

        CHECK(task1_handle);
        CHECK_MESSAGE(task1_completion_handle, "Empty completion_handle created from non-empty handle");
        CHECK_MESSAGE(!(task1_completion_handle == nullptr), "Unexpected result for comparison of non-empty completion_handle with nullptr");
        CHECK_MESSAGE(!(nullptr == task1_completion_handle), "Unexpected result for comparison of nullptr with non-empty completion_handle");
        CHECK_MESSAGE(task1_completion_handle != nullptr, "Unexpected result for comparison of non-empty completion_handle with nullptr");
        CHECK_MESSAGE(nullptr != task1_completion_handle, "Unexpected result for comparison of nullptr with non-empty completion_handle");

        tg.run(std::move(task1_handle));
        tg.wait();
        CHECK_MESSAGE(task_placeholder == 1, "Task body was not executed");

        tbb::task_handle task2_handle = tg.defer(task_body);
        tbb::task_completion_handle task2_completion_handle = task2_handle;

        tg.run_and_wait(std::move(task2_handle));
        CHECK_MESSAGE(task_placeholder == 2, "Task body was not executed");
        CHECK_MESSAGE(task2_completion_handle, "Completed task completion_handle is empty");

        tbb::task_handle task3_handle = tg.defer(task_body);
        tbb::task_completion_handle task3_completion_handle1 = task3_handle;
        tbb::task_completion_handle task3_completion_handle2 = task3_handle;

        CHECK_MESSAGE(task3_completion_handle1 == task3_completion_handle2, "Unexpected result for comparison of single task completion handles");
        CHECK_MESSAGE(!(task3_completion_handle1 != task3_completion_handle2), "Unexpected result for comparison of single task completion handles");

        tg.run_and_wait(std::move(task3_handle));
        CHECK_MESSAGE(task_placeholder == 3, "Task body was not executed");

        task3_completion_handle2 = std::move(task3_completion_handle1);
        CHECK_MESSAGE(task3_completion_handle2, "Empty task completion_handle after move assignment from non-empty handle");
        CHECK_MESSAGE(!task3_completion_handle1, "Moved-from task_completion_handle is non-empty");

        tbb::task_handle task4_handle = tg.defer(task_body);        
        tg.run_and_wait(std::move(task4_handle));
        CHECK_MESSAGE(task_placeholder == 4, "Task body was not executed");
    }
    {
        // Test submission through enqueue
        tbb::task_arena arena;

        {
            tbb::task_handle enqueue_task_handle = tg.defer(task_body);
            tbb::task_completion_handle enqueue_task_completion_handle = enqueue_task_handle;

            arena.enqueue(std::move(enqueue_task_handle));
            CHECK_MESSAGE(enqueue_task_completion_handle, "task_completion_handle should not be empty after enqueue");
            tg.wait();
            CHECK_MESSAGE(task_placeholder == 5, "Task body was not executed");
            CHECK_MESSAGE(enqueue_task_completion_handle, "task_completion_handle should not be empty after task completion");
        }
        {
            tbb::task_completion_handle enqueue_task_completion_handle;
            arena.execute([&] {
                tbb::task_handle enqueue_task_handle = tg.defer(task_body);
                enqueue_task_completion_handle = enqueue_task_handle;

                tbb::this_task_arena::enqueue(std::move(enqueue_task_handle));
                CHECK_MESSAGE(enqueue_task_completion_handle, "task_completion_handle should not be empty after enqueue");
            });
            tg.wait();
            CHECK_MESSAGE(task_placeholder == 6, "Task body was not executed");
            CHECK_MESSAGE(enqueue_task_completion_handle, "task_completion_handle should not be empty after task completion");
        }
    }
}

enum class submit_function {
    run = 0,
    run_and_wait = 1,
    arena_enqueue = 2,
    this_arena_enqueue = 3
};

void submit(submit_function func, tbb::task_handle&& handle, tbb::task_group& group, tbb::task_arena& arena) {
    if (func == submit_function::run || func == submit_function::run_and_wait) {
        group.run(std::move(handle));
    } else if (func == submit_function::arena_enqueue) {
        arena.enqueue(std::move(handle));
    } else {
        CHECK_MESSAGE(func == submit_function::this_arena_enqueue, "new submit function added but not handled");
        tbb::this_task_arena::enqueue(std::move(handle));
    }
}

void submit_and_wait(submit_function func, tbb::task_handle&& handle, tbb::task_group& group, tbb::task_arena& arena) {
    if (func != submit_function::run_and_wait) {
        submit(func, std::move(handle), group, arena);
        group.wait();
    } else {
        group.run_and_wait(std::move(handle));
    }
}

template <typename Function>
void submit_and_wait(submit_function func, const Function& function, tbb::task_group& group, tbb::task_arena& arena) {
    if (func == submit_function::run_and_wait ) {
        group.run_and_wait(function);
    } else if (func == submit_function::run) {
        group.run(function);
        group.wait();
    } else {
        submit_and_wait(func, group.defer(function), group, arena);
    }
}

struct leaf_task {
    void operator()() const {
        *placeholder = value;
    }
    std::shared_ptr<std::size_t> placeholder;
    std::size_t value;
};

struct combine_task {
    void operator()() const {
        *placeholder = *left_leaf + *right_leaf;
    }

    std::shared_ptr<std::size_t> placeholder;
    std::shared_ptr<std::size_t> left_leaf;
    std::shared_ptr<std::size_t> right_leaf;
};

void test_not_submitted_predecessors(submit_function submit_function_tag) {
    tbb::task_arena arena;
    constexpr std::size_t depth = 100; 

    tbb::task_group tg;
    std::size_t task_initializer = 0;

    std::shared_ptr<std::size_t> left_leaf_placeholder = std::make_shared<std::size_t>(0);
    tbb::task_completion_handle left_leaf_completion_handle;
    tbb::task_handle deepest_left_leaf_task;

    for (std::size_t i = 0; i < depth; ++i) {
        if (i == 0) {
            deepest_left_leaf_task = tg.defer(leaf_task{left_leaf_placeholder, task_initializer++});
            left_leaf_completion_handle = deepest_left_leaf_task;
        }

        std::shared_ptr<std::size_t> right_leaf_placeholder = std::make_shared<std::size_t>(0);
        tbb::task_handle right_leaf_task = tg.defer(leaf_task{right_leaf_placeholder, task_initializer++});

        std::shared_ptr<std::size_t> combine_placeholder = std::make_shared<std::size_t>(0);
        tbb::task_handle combine = tg.defer(combine_task{combine_placeholder, left_leaf_placeholder, right_leaf_placeholder});
        tbb::task_completion_handle combine_completion_handle = combine;

        tbb::task_group::set_task_order(left_leaf_completion_handle, combine);
        tbb::task_group::set_task_order(right_leaf_task, combine);

        submit(submit_function_tag, std::move(combine), tg, arena);
        submit(submit_function_tag, std::move(right_leaf_task), tg, arena);

        left_leaf_completion_handle = combine_completion_handle;
        left_leaf_placeholder = combine_placeholder;
    }
    
    CHECK(deepest_left_leaf_task);
    CHECK_MESSAGE(*left_leaf_placeholder == 0, "Receiving results from incomplete task graph");

    // "Run" the graph
    submit_and_wait(submit_function_tag, std::move(deepest_left_leaf_task), tg, arena);

    std::size_t expected_result = 0;
    std::size_t counter = 0;

    // 2 tasks on the first layer + one task for each next layer
    for (std::size_t i = 0; i < depth + 1; ++i) {
        expected_result += counter++;
    }

    CHECK_MESSAGE(expected_result == *left_leaf_placeholder,
                  "Unexpected result for task graph execution");
}

// all_predecessors_completed flag means creating a set of predecessors that a guaranteed to be completed
// before setting dependencies
void test_submitted_predecessors(submit_function submit_function_tag, bool all_predecessors_completed) {
    tbb::task_arena arena;
    const std::size_t num_predecessors = 500;
    tbb::task_group tg;
    std::atomic<std::size_t> task_placeholder{0};

    std::vector<tbb::task_completion_handle> predecessors(num_predecessors);

    for (std::size_t i = 0; i < num_predecessors; ++i) {
        tbb::task_handle h = tg.defer([&] { ++task_placeholder; });
        predecessors[i] = h;
        submit(submit_function_tag, std::move(h), tg, arena);
    }

    if (all_predecessors_completed) {
        tg.wait();
        CHECK_MESSAGE(task_placeholder == num_predecessors, "Not all tasks were executed");
    }

    tbb::task_handle successor_task = tg.defer([&] {
        CHECK_MESSAGE(task_placeholder == num_predecessors, "Not all predecessors completed");
        ++task_placeholder;
    });

    for (std::size_t i = 0; i < num_predecessors; ++i) {
        tbb::task_group::set_task_order(predecessors[i], successor_task);
    }

    if (all_predecessors_completed) {
        CHECK_MESSAGE(task_placeholder == num_predecessors, "successor task completed before being submitted");
    }
    submit_and_wait(submit_function_tag, std::move(successor_task), tg, arena);

    CHECK_MESSAGE(task_placeholder == num_predecessors + 1, "successor task was not completed");
}

void test_predecessors(submit_function submit_function_tag) {
    test_not_submitted_predecessors(submit_function_tag);
    test_submitted_predecessors(submit_function_tag, /*all_predecessors_completed = */true);
    test_submitted_predecessors(submit_function_tag, /*all_predecessors_completed = */false);
}

int serial_reduction(int begin, int end) {
    int result = 0;
    for (int i = begin; i < end; ++i) {
        result += i;
    }
    return result;
}

void recursive_reduction(submit_function fn, tbb::task_group& tg, tbb::task_arena& arena, int begin, int end, int* result_placeholder, int cutoff) {
    int size = end - begin;
    if (size <= cutoff) {
        // Do serial reduction
        *result_placeholder = serial_reduction(begin, end);
    } else {
        // Do parallel reduction
        int* left_placeholder = new int(0);
        int* right_placeholder = new int(0);

        int middle = begin + size / 2;

        tbb::task_handle left_subtask = tg.defer([=, &tg, &arena] {
            recursive_reduction(fn, tg, arena, begin, middle, left_placeholder, cutoff);
        });

        tbb::task_handle right_subtask = tg.defer([=, &tg, &arena]() {
            recursive_reduction(fn, tg, arena, middle, end, right_placeholder, cutoff);
        });

        tbb::task_handle reduction = tg.defer([=] {
            CHECK_MESSAGE(*left_placeholder == serial_reduction(begin, middle), "Incorrect result for left sub-reduction");
            CHECK_MESSAGE(*right_placeholder == serial_reduction(middle, end), "Incorrect result for right sub-reduction");
            *result_placeholder = *left_placeholder + *right_placeholder;
            delete left_placeholder;
            delete right_placeholder;
        });

        tbb::task_group::set_task_order(left_subtask, reduction);
        tbb::task_group::set_task_order(right_subtask, reduction);
        tbb::task_group::transfer_this_task_completion_to(reduction);

        submit(fn, std::move(left_subtask), tg, arena);
        submit(fn, std::move(right_subtask), tg, arena);
        submit(fn, std::move(reduction), tg, arena);
    }
}

void test_recursive_reduction(submit_function func) {
    tbb::task_group tg;
    tbb::task_arena arena;

    int* result_placeholder = new int(0);

    for (auto n : {2, 5, 10, 100, 1000, 5000, 10000}) {
        auto start_reduction = [=, &tg, &arena] {
            recursive_reduction(func, tg, arena, 0, n, result_placeholder, /*cutoff = */n < 100 ? 1 : 50);
        };

        submit_and_wait(func, start_reduction, tg, arena);
        CHECK_MESSAGE(*result_placeholder == serial_reduction(0, n), "Incorrect final result for reduction");
        *result_placeholder = 0;
    }
    delete result_placeholder;
}

void test_adding_successors_after_transfer(submit_function func) {
    tbb::task_group tg;
    tbb::task_arena arena;

    const int not_finished_task = 0;
    const int finished_task = 1;

    std::atomic<int> receiver_task_placeholder{not_finished_task};
    std::atomic<int> successor_task_placeholder{not_finished_task};
    std::atomic<int> transferring_task_placeholder{not_finished_task};
    std::atomic<int> new_successor_task_placeholder{not_finished_task};

    auto receiver_task_body = [&] {
        CHECK(receiver_task_placeholder == not_finished_task);
        CHECK(successor_task_placeholder == not_finished_task);
        CHECK(transferring_task_placeholder == finished_task);
        CHECK(new_successor_task_placeholder == not_finished_task);

        receiver_task_placeholder = finished_task;
    };

    tbb::task_handle successor_task = tg.defer([&] {
        CHECK(receiver_task_placeholder == finished_task);
        CHECK(successor_task_placeholder == not_finished_task);
        CHECK(transferring_task_placeholder == finished_task);
        
        successor_task_placeholder = finished_task;
    });

    tbb::task_handle receiver_task = tg.defer(receiver_task_body);

    tbb::task_handle transferring_task = tg.defer([&] {
        CHECK(receiver_task_placeholder == not_finished_task);
        CHECK(successor_task_placeholder == not_finished_task);
        CHECK(transferring_task_placeholder == not_finished_task);
        CHECK(new_successor_task_placeholder == not_finished_task);

        tbb::task_group::transfer_this_task_completion_to(receiver_task);
        transferring_task_placeholder = finished_task;
    });

    tbb::task_handle new_successor_task = tg.defer([&] {
        CHECK(receiver_task_placeholder == finished_task);
        CHECK(transferring_task_placeholder == finished_task);
        CHECK(new_successor_task_placeholder == not_finished_task);

        new_successor_task_placeholder = finished_task;
    });

    tbb::task_completion_handle transferring_task_completion_handle = transferring_task;
    
    tbb::task_group::set_task_order(transferring_task, successor_task);

    submit(func, std::move(successor_task), tg, arena);
    submit(func, std::move(transferring_task), tg, arena);

    // Wait for the transferring task to complete before adding new successor
    utils::SpinWaitUntilEq(transferring_task_placeholder, finished_task);
    CHECK(transferring_task_placeholder == finished_task);

    CHECK_MESSAGE(successor_task_placeholder == not_finished_task, "successor task ran before the receiving task");
    submit(func, std::move(receiver_task), tg, arena);

    tbb::task_group::set_task_order(transferring_task_completion_handle, new_successor_task);
    submit_and_wait(func, std::move(new_successor_task), tg, arena);

    CHECK_MESSAGE(receiver_task_placeholder == finished_task, "receiver task was not finished");
    CHECK_MESSAGE(successor_task_placeholder == finished_task, "successor task was not finished");
    CHECK_MESSAGE(transferring_task_placeholder == finished_task, "transferring task was not finished");
    CHECK_MESSAGE(new_successor_task_placeholder == finished_task, "new successor task was not finished");
}

void test_transferring_completion(unsigned num_threads, submit_function func) {
    test_recursive_reduction(func);
    if (num_threads != 1) test_adding_successors_after_transfer(func);
}

void test_return_task_with_dependencies(submit_function submit_function_tag) {
    tbb::task_group tg;
    tbb::task_arena arena;

    std::atomic<std::size_t> predecessor_placeholder(0);
    std::atomic<std::size_t> successor_placeholder(0);

    tbb::task_handle predecessor = tg.defer([&] {
        predecessor_placeholder = 1;
    });
    tbb::task_completion_handle predecessor_completion_handle = predecessor;

    tbb::task_handle task = tg.defer([&] {
        tbb::task_handle successor = tg.defer([&, predecessor_completion_handle] {
            CHECK_MESSAGE(predecessor_placeholder == 1, "Predecessor task was not completed");
            successor_placeholder = 1;
        });

        tbb::task_group::set_task_order(predecessor_completion_handle, successor);
        return successor;
    });

    submit(submit_function_tag, std::move(task), tg, arena);
    submit_and_wait(submit_function_tag, std::move(predecessor), tg, arena);
    CHECK_MESSAGE(successor_placeholder == 1, "Successor task was not completed");
}

//! \brief \ref interface \ref requirement
TEST_CASE("test task_group dynamic dependencies feature test macro") {
    CHECK_MESSAGE(TBB_HAS_TASK_GROUP_DEPENDENCIES == 202603,
                  "Incorrect feature test macro for dependencies");
}

//! \brief \ref interface \ref requirement \ref error_guessing
TEST_CASE("test task_group dynamic dependencies") {
    for (unsigned p = MinThread; p <= MaxThread; ++p) {
        tbb::global_control limit(tbb::global_control::max_allowed_parallelism, p);

        test_predecessors(submit_function::run);
        test_predecessors(submit_function::run_and_wait);
        test_predecessors(submit_function::arena_enqueue);
        test_predecessors(submit_function::this_arena_enqueue);

        test_transferring_completion(p, submit_function::run);
        test_transferring_completion(p, submit_function::run_and_wait);
        test_transferring_completion(p, submit_function::arena_enqueue);
        test_transferring_completion(p, submit_function::this_arena_enqueue);
        
        test_return_task_with_dependencies(submit_function::run);
        test_return_task_with_dependencies(submit_function::run_and_wait);
        test_return_task_with_dependencies(submit_function::arena_enqueue);
        test_return_task_with_dependencies(submit_function::this_arena_enqueue);
    }
}

//! \brief \ref error_guessing
TEST_CASE("test task_completion_handle in concurrent environment") {
    tbb::task_group tg;
    std::size_t task_placeholder = 0;
    const int n = 100;

    tbb::task_handle task = tg.defer([&] {
        ++task_placeholder;
    });

    std::atomic<std::size_t> succ_counter(0);

    tbb::parallel_for(0, n, [&](int) {
        tbb::task_completion_handle completion_handle(task);
        CHECK_MESSAGE(completion_handle, "task_completion_handle should not be empty");
        tbb::task_handle succ = tg.defer([&] {
            CHECK_MESSAGE(task_placeholder == 1, "Predecessor task was not executed");
            ++succ_counter;
        });
        tbb::task_group::set_task_order(completion_handle, succ);
        tg.run(std::move(succ));
    });

    tg.run_and_wait(std::move(task));
    CHECK_MESSAGE(task_placeholder == 1, "task body was not executed");
    CHECK_MESSAGE(succ_counter == n, "Not all of the successors were executed");
}

//! \brief \ref error_guessing
TEST_CASE("test dependencies and cancellation") {
    tbb::task_group tg;

    auto body = [&] { CHECK_MESSAGE(false, "Some tasks were executed in the cancelled task_group"); };
    std::size_t n_layer_tasks = 10;
    std::size_t n_layers = 4;

    std::unordered_map<std::size_t, std::vector<tbb::task_handle>> tasks;

    for (std::size_t layer_index = 0; layer_index < n_layers; ++layer_index) {
        auto& layer_tasks = tasks[layer_index];
        layer_tasks.reserve(n_layer_tasks);

        for (std::size_t task_index = 0; task_index < n_layer_tasks; ++task_index) {
            layer_tasks.emplace_back(tg.defer(body));

            if (layer_index != 0) {
                auto& pred_layer = tasks.at(layer_index - 1);
                for (std::size_t pred_task_index = 0; pred_task_index < n_layer_tasks; ++pred_task_index) {
                    tbb::task_group::set_task_order(pred_layer[pred_task_index], layer_tasks.back());
                }
            }
        }
    }

    tg.cancel();

    for (auto& layer_pair : tasks) {
        for (auto& task : layer_pair.second) {
            tg.run(std::move(task));
        }
    }
    tbb::task_group_status status = tg.wait();
    CHECK_MESSAGE(status == tbb::task_group_status::canceled, "Incorrect status of cancelled task_group");
}
#endif

struct stateful_task_body {
    std::size_t& placeholder;
    char state[1024]{0};

    stateful_task_body(std::size_t& p)
        : placeholder(p) {}

    void operator()() const {
        ++placeholder;
    }
};

//! \brief \ref regression
TEST_CASE("Test stateful task body") {
    tbb::task_group tg;
    std::size_t placeholder = 0;

    stateful_task_body body{placeholder};

    // Test run + wait
    tg.run(body);
    tg.wait();
    CHECK_MESSAGE(placeholder == 1, "Task was not executed");

    // Test cancellation
    tg.cancel();
    tg.run(body);
    tg.wait();
    CHECK_MESSAGE(placeholder == 1, "Task was executed in the cancelled task_group");

    // Test task destruction
    {
        tbb::task_handle task = tg.defer(body);
    }
    CHECK_MESSAGE(placeholder == 1, "Not submitted task was executed");
}

#if TBB_PREVIEW_TASK_GROUP_EXTENSIONS

enum class wait_for_function_tag {
    group_wait_task,
    group_run_and_wait_task,
    arena_wait_for,
};

tbb::task_group_status wait_for(tbb::task_completion_handle& task, tbb::task_group& tg, tbb::task_arena& arena, wait_for_function_tag tag) {
    tbb::task_group_status status = tbb::task_group_status::not_complete;
    if (tag == wait_for_function_tag::group_wait_task || tag == wait_for_function_tag::group_run_and_wait_task) {
        status = tg.wait_for_task(task);
    } else {
        if (tag == wait_for_function_tag::arena_wait_for) {
            status = arena.wait_for(task);
        }
    }
    return status;
}

tbb::task_group_status run_and_wait_for(tbb::task_handle&& task, tbb::task_group& tg, tbb::task_arena& arena, wait_for_function_tag tag) {
    auto body = [&] {
        if (tag == wait_for_function_tag::group_run_and_wait_task) {
            return tg.run_and_wait_for_task(std::move(task));
        } else {
            tbb::task_completion_handle comp_handle = task;
            arena.enqueue(std::move(task));
            return wait_for(comp_handle, tg, arena, tag);
        }
    };

    if (tag == wait_for_function_tag::arena_wait_for) {
        return body();
    } else {
        return arena.execute(body);
    }
}

void test_single_task_wait_with_no_dependencies(bool do_cancellation, wait_for_function_tag tag) {
    tbb::task_group tg;
    tbb::task_arena arena;

    std::size_t block_size = 5;
    std::size_t num_blocks = 100;
    std::vector<int> items(num_blocks * block_size, 0);

    tbb::task_handle first_block_task;

    arena.execute([&] {
        auto process_block = [&](std::size_t block_index) {
            for (std::size_t i = 0; i < block_size; ++i) {
                ++items[block_index * block_size + i];
            }
        };

        first_block_task = tg.defer([=] { process_block(0); });

        for (std::size_t i = 1; i < num_blocks; ++i) {
            tg.run([=] { process_block(i); });
        }

        if (do_cancellation) tg.cancel();
    });

    tbb::task_completion_handle first_block_comp_handle = first_block_task;

    tbb::task_group_status task_status = run_and_wait_for(std::move(first_block_task), tg, arena, tag);
    
    tbb::task_group_status expected_task_status = tbb::task_group_status::not_complete;
    tbb::task_group_status expected_group_status = tbb::task_group_status::not_complete;
    std::size_t expected_item = 0;

    if (do_cancellation) {
        expected_task_status = expected_group_status = tbb::task_group_status::canceled;
    } else {
        expected_task_status = tbb::task_group_status::task_complete;
        expected_group_status = tbb::task_group_status::complete;
        expected_item = 1;
    }
    
    CHECK_MESSAGE(task_status == expected_task_status, "Incorrect task status returned");
    for (std::size_t i = 0; i < block_size; ++i) {
        CHECK_MESSAGE(items[i] == expected_item, "Incorrect item processing result");
    }

    // Test that waiting on the completed task returns the same status
    for (std::size_t i = 0; i < 10; ++i) {
        CHECK_MESSAGE(expected_task_status == wait_for(first_block_comp_handle, tg, arena, tag),
                      "Incorrect task status returned");
    }


    tbb::task_group_status group_status = arena.wait_for(tg);
    CHECK_MESSAGE(group_status == expected_group_status, "Incorrect group status returned");
    
    if (!do_cancellation) {
        for (auto& item : items) {
            CHECK_MESSAGE(item == 1, "Some task was not executed");
        }
    }
}

class test_single_task_wait_graph {
    struct placeholder_task {
        void operator()() const {
            CHECK(placeholder == 0);
            placeholder = 1;
        }

        std::size_t& placeholder;
    };

    tbb::task_group& m_tg;
    tbb::task_arena& m_arena;
    bool             do_cancellation;

    std::size_t predecessor1_placeholder = 0;
    std::size_t predecessor2_placeholder = 0;
    std::size_t wait_task_placeholder = 0;
    std::size_t controlled_task_placeholder = 0;
    std::size_t upper_placeholder = 0;

    tbb::task_handle controlled_task;
    tbb::task_handle predecessor1;
    tbb::task_handle predecessor2;
    tbb::task_handle upper_task;
    tbb::task_handle cancel_task;
public:
    tbb::task_handle wait_task;
    tbb::task_completion_handle wait_comp_handle;
    std::atomic<std::size_t> execution_allowed{0};

    test_single_task_wait_graph(tbb::task_group& tg, tbb::task_arena& arena, bool do_cancel)
        : m_tg(tg), m_arena(arena), do_cancellation(do_cancel)
    {
        arena.execute([&] {
            controlled_task = tg.defer([&] {
                utils::SpinWaitWhileEq(execution_allowed, 0ul);
                controlled_task_placeholder = 1;
            });

            predecessor1 = tg.defer(placeholder_task{predecessor1_placeholder});
            predecessor2 = tg.defer(placeholder_task{predecessor2_placeholder});
            wait_task = tg.defer(placeholder_task{wait_task_placeholder});
            upper_task = tg.defer(placeholder_task{upper_placeholder});

            if (do_cancellation) {
                cancel_task = tg.defer([&tg] { tg.cancel(); });
            }

            tbb::task_group::set_task_order(predecessor1, wait_task);
            tbb::task_group::set_task_order(predecessor2, wait_task);
            tbb::task_group::set_task_order(wait_task, controlled_task);
            tbb::task_group::set_task_order(controlled_task, upper_task);

            if (do_cancellation) {
                tbb::task_group::set_task_order(cancel_task, wait_task);
            }

            wait_comp_handle = wait_task;
        });
    }

    void run() {
        m_arena.execute([&] {
            m_tg.run(std::move(upper_task));
            m_tg.run(std::move(controlled_task));
            m_tg.run(std::move(predecessor2));
            m_tg.run(std::move(predecessor1));
            if (do_cancellation) {
                m_tg.run(std::move(cancel_task));
            }
        });
    }

    bool wait_task_executed() {
        return predecessor1_placeholder == 1 && predecessor2_placeholder == 1 && wait_task_placeholder == 1;
    }

    bool all_tasks_executed() {
        return wait_task_executed() && controlled_task_placeholder == 1 && upper_placeholder == 1;
    }
};

void test_single_task_wait_with_dependencies(bool do_cancellation, wait_for_function_tag tag) {
    tbb::task_group tg;
    tbb::task_arena arena;
    
    test_single_task_wait_graph graph(tg, arena, do_cancellation);
    graph.run();

    tbb::task_group_status task_status = run_and_wait_for(std::move(graph.wait_task), tg, arena, tag);

    if (do_cancellation) {
        CHECK_MESSAGE(task_status == tbb::task_group_status::canceled, "Incorrect task_group_status returned");
        CHECK_MESSAGE(!graph.wait_task_executed(), "successor task should not be executed");
    } else {
        CHECK_MESSAGE(task_status == tbb::task_group_status::task_complete, "Incorrect task_group_status returned");
        CHECK_MESSAGE((graph.wait_task_executed() && !graph.all_tasks_executed()), "successor task should be executed");
    }
    
    // Allow execution of controlled task
    graph.execution_allowed = 1;

    tbb::task_group_status group_status = arena.wait_for(tg);

    if (do_cancellation) {
        CHECK_MESSAGE(group_status == tbb::task_group_status::canceled, "Incorrect task_group_status returned");
        CHECK_MESSAGE(!graph.all_tasks_executed(), "tasks should not be executed after cancellation");
    } else {
        CHECK_MESSAGE(group_status == tbb::task_group_status::complete, "Incorrect task_group_status returned");
        CHECK_MESSAGE(graph.all_tasks_executed(), "task graph not executed");
    }
}

void test_single_task_wait_transferring(bool do_cancellation, wait_for_function_tag tag) {
    tbb::task_group tg;
    tbb::task_arena arena;

    test_single_task_wait_graph graph(tg, arena, do_cancellation);

    tbb::task_handle transferring_task;
    std::size_t transfer_task_placeholder = 0;

    arena.execute([&] {
        transferring_task = tg.defer([&] {
            transfer_task_placeholder = 1;
            tbb::task_group::transfer_this_task_completion_to(graph.wait_task);
            graph.run();
            tg.run(std::move(graph.wait_task));
        });
    });

    tbb::task_completion_handle transfer_comp_handle = transferring_task;
    tbb::task_group_status task_status = run_and_wait_for(std::move(transferring_task), tg, arena, tag);
    CHECK_MESSAGE(transfer_task_placeholder == 1, "Transferring task should be executed");

    if (do_cancellation) {
        CHECK_MESSAGE(task_status == tbb::task_group_status::canceled, "Incorrect task_group_status returned");
        CHECK_MESSAGE(!graph.wait_task_executed(), "successor task should not be executed");
    } else {
        CHECK_MESSAGE(task_status == tbb::task_group_status::task_complete, "Incorrect task_group_status returned");
        CHECK_MESSAGE((graph.wait_task_executed() && !graph.all_tasks_executed()), "successor task should be executed");
    }
    
    // Allow execution of controlled task
    graph.execution_allowed = 1;

    tbb::task_group_status group_status = arena.wait_for(tg);

    if (do_cancellation) {
        CHECK_MESSAGE(group_status == tbb::task_group_status::canceled, "Incorrect task_group_status returned");
        CHECK_MESSAGE(!graph.all_tasks_executed(), "tasks should not be executed after cancellation");
    } else {
        CHECK_MESSAGE(group_status == tbb::task_group_status::complete, "Incorrect task_group_status returned");
        CHECK_MESSAGE(graph.all_tasks_executed(), "task graph not executed");
    }
}

void test_concurrent_single_task_wait(bool do_cancellation, wait_for_function_tag tag) {
    tbb::task_group tg;
    tbb::task_arena arena;

    tbb::task_handle task;
    tbb::task_completion_handle comp_handle;
    
    arena.execute([&] {
        task = tg.defer([&] {
            CHECK_MESSAGE(!do_cancellation, "Task should not be called in case of group cancellation");
        });
        comp_handle = task;
    });

    tbb::task_group_status expected_status = do_cancellation ? tbb::task_group_status::canceled
                                                             : tbb::task_group_status::task_complete;

    int num_threads = tbb::this_task_arena::max_concurrency();
    utils::SpinBarrier barrier(num_threads);

    utils::NativeParallelFor(num_threads,
        [&](std::size_t index) {
            tbb::task_group_status status;
            barrier.wait();
            if (index == 0) {
                // Short sleep to increase the number of threads entered the wait_for function
                utils::Sleep(10);
                
                if (do_cancellation) {
                    tg.cancel();
                }

                status = run_and_wait_for(std::move(task), tg, arena, tag);
            } else {
                status = wait_for(comp_handle, tg, arena, tag);
            }
            CHECK_MESSAGE(status == expected_status, "Incorrect status returned");
        });
}

void test_single_task_wait_base(bool cancellation, wait_for_function_tag tag) {
    for (unsigned num_threads = MinThread; num_threads <= MaxThread; ++num_threads) {
        tbb::global_control ctl(tbb::global_control::max_allowed_parallelism, num_threads);
        test_single_task_wait_with_no_dependencies(cancellation, tag);
        test_single_task_wait_with_dependencies(cancellation, tag);
        test_single_task_wait_transferring(cancellation, tag);
        test_concurrent_single_task_wait(cancellation, tag);
    }
}

void test_single_task_wait(bool cancellation) {
    test_single_task_wait_base(cancellation, wait_for_function_tag::group_wait_task);
    test_single_task_wait_base(cancellation, wait_for_function_tag::group_run_and_wait_task);
    test_single_task_wait_base(cancellation, wait_for_function_tag::arena_wait_for);
}

void test_get_status_of_with_transferring(bool cancellation) {
    tbb::task_group tg;

    std::size_t placeholder = 0;

    tbb::task_completion_handle sender_comp_handle;
    tbb::task_completion_handle receiver_comp_handle;

    tbb::task_handle receiver = tg.defer([&] {
        CHECK_MESSAGE(!cancellation, "Receiver task should not be called when task_group is cancelled");
        ++placeholder;
        CHECK_MESSAGE(tg.get_status_of(sender_comp_handle) == tbb::task_group_status::not_complete,
                      "Incorrect status of sender completion handle");
        CHECK_MESSAGE(tg.get_status_of(receiver_comp_handle) == tbb::task_group_status::not_complete,
                      "Incorrect status of receiver completion handle");
    });
    receiver_comp_handle = receiver;

    tbb::task_handle sender = tg.defer([&] {
        ++placeholder;
        CHECK_MESSAGE(tg.get_status_of(sender_comp_handle) == tbb::task_group_status::not_complete,
                      "Incorrect status of sender completion handle");
        CHECK_MESSAGE(tg.get_status_of(receiver_comp_handle) == tbb::task_group_status::not_complete,
                      "Incorrect status of receiver completion handle");

        tbb::task_group::transfer_this_task_completion_to(receiver);

        CHECK_MESSAGE(tg.get_status_of(sender_comp_handle) == tbb::task_group_status::not_complete,
                      "Incorrect status of sender completion handle");
        CHECK_MESSAGE(tg.get_status_of(receiver_comp_handle) == tbb::task_group_status::not_complete,
                      "Incorrect status of receiver completion handle");

        if (cancellation) tg.cancel();
        tg.run(std::move(receiver));
    });
    
    sender_comp_handle = sender;

    CHECK_MESSAGE(tg.get_status_of(sender_comp_handle) == tbb::task_group_status::not_complete,
                      "Incorrect status of sender completion handle");
    CHECK_MESSAGE(tg.get_status_of(receiver_comp_handle) == tbb::task_group_status::not_complete,
                    "Incorrect status of receiver completion handle");

    tg.run_and_wait(std::move(sender));

    tbb::task_group_status expected_status = cancellation ? tbb::task_group_status::canceled
                                                          : tbb::task_group_status::task_complete;
    std::size_t expected_executed_tasks_count = cancellation ? 1 : 2;

    CHECK_MESSAGE(tg.get_status_of(sender_comp_handle) == expected_status,
                      "Incorrect status of sender completion handle");
    CHECK_MESSAGE(tg.get_status_of(receiver_comp_handle) == expected_status,
                    "Incorrect status of receiver completion handle");
    CHECK_MESSAGE(placeholder == expected_executed_tasks_count, "Required task bodies did not run");
}

void test_get_status_of() {
    std::size_t placeholder = 0;
    tbb::task_group* tg_ptr = nullptr;

    tbb::task_completion_handle comp_handle;
    auto task_body = [&] {
        CHECK(tg_ptr != nullptr);
        CHECK_MESSAGE(tg_ptr->get_status_of(comp_handle) == tbb::task_group_status::not_complete,
                    "Incorrect task_group_status returned");
        ++placeholder;
        };

    {
        tbb::task_group tg;
        tg_ptr = &tg;

        tbb::task_handle task = tg.defer(task_body);

        comp_handle = task;
        
        CHECK_MESSAGE(tg.get_status_of(comp_handle) == tbb::task_group_status::not_complete,
                    "Incorrect task_group_status returned");
        tg.run_and_wait(std::move(task));

        CHECK_MESSAGE(tg.get_status_of(comp_handle) == tbb::task_group_status::task_complete,
                    "Incorrect task_group_status returned");
        CHECK_MESSAGE(placeholder == 1, "Task body was not executed");
    }
    {
        tbb::task_group tg;
        tg_ptr = &tg;
        
        tbb::task_handle task = tg.defer(task_body);
        comp_handle = task;
        tg.cancel();

        CHECK_MESSAGE(tg.get_status_of(comp_handle) == tbb::task_group_status::not_complete,
                      "Incorrect task_group_status returned");
        tg.run_and_wait(std::move(task));

        CHECK_MESSAGE(tg.get_status_of(comp_handle) == tbb::task_group_status::canceled,
                     "Incorrect task_group_status returned");
        CHECK_MESSAGE(placeholder == 1, "Canceled task body was executed");
    }
    test_get_status_of_with_transferring(/*cancellation = */false);
    test_get_status_of_with_transferring(/*cancellation = */true);
}

//! \brief \ref error_guessing
TEST_CASE("test single task wait") {
    CHECK_MESSAGE(TBB_HAS_TASK_GROUP_WAIT_FOR_SINGLE_TASK == 202603,
                  "Incorrect feature test macro for waiting a single task");

    test_single_task_wait(/*cancel = */false);
    test_single_task_wait(/*cancel = */true);
}

//! \brief \ref interface \ref requirement
TEST_CASE("test task_group::get_status_of") {
    test_get_status_of();
}
#endif

#if _MSC_VER
#pragma warning (pop)
#endif
