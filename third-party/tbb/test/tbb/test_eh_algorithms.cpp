/*
    Copyright (c) 2005-2023 Intel Corporation

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
#include "common/concurrency_tracker.h"
#include "common/iterator.h"
#include "common/utils_concurrency_limit.h"

#include <limits.h> // for INT_MAX
#include <thread>
#include <vector>

#include "tbb/parallel_for.h"
#include "tbb/parallel_reduce.h"
#include "tbb/parallel_for_each.h"
#include "tbb/parallel_pipeline.h"
#include "tbb/blocked_range.h"
#include "tbb/task_group.h"
#include "tbb/concurrent_unordered_map.h"
#include "tbb/task.h"
#include "tbb/global_control.h"

//! \file test_eh_algorithms.cpp
//! \brief Test for [algorithms.parallel_for algorithms.parallel_reduce algorithms.parallel_deterministic_reduce algorithms.parallel_for_each algorithms.parallel_pipeline algorithms.parallel_pipeline.flow_control] specifications

#define FLAT_RANGE  100000
#define FLAT_GRAIN  100
#define OUTER_RANGE  100
#define OUTER_GRAIN  10
#define INNER_RANGE  (FLAT_RANGE / OUTER_RANGE)
#define INNER_GRAIN  (FLAT_GRAIN / OUTER_GRAIN)

struct context_specific_counter {
    tbb::concurrent_unordered_map<tbb::task_group_context*, std::atomic<unsigned>> context_map{};

    void increment() {
        tbb::task_group_context* ctx = tbb::task::current_context();
        REQUIRE(ctx != nullptr);
        context_map[ctx]++;
    }

    void reset() {
        context_map.clear();
    }

    void validate(unsigned expected_count, const char* msg) {
        for (auto it = context_map.begin(); it != context_map.end(); it++) {
            REQUIRE_MESSAGE( it->second <= expected_count, msg);
        }
    }
};

std::atomic<intptr_t> g_FedTasksCount{}; // number of tasks added by parallel_for_each feeder
std::atomic<intptr_t> g_OuterParCalls{};  // number of actual invocations of the outer construct executed.
context_specific_counter g_TGCCancelled{};  // Number of times a task sees its group cancelled at start

#include "common/exception_handling.h"

/********************************
      Variables in test

__ Test control variables
      g_ExceptionInMaster -- only the external thread is allowed to throw.  If false, the external cannot throw
      g_SolitaryException -- only one throw may be executed.

-- controls for ThrowTestException for pipeline tests
      g_NestedPipelines -- are inner pipelines being run?
      g_PipelinesStarted -- how many pipelines have run their first filter at least once.

-- Information variables

   g_Master -- Thread ID of the "external" thread
      In pipelines sometimes the external thread does not participate, so the tests have to be resilient to this.

-- Measurement variables

   g_OuterParCalls -- how many outer parallel ranges or filters started
   g_TGCCancelled --  how many inner parallel ranges or filters saw task::self().is_cancelled()
   g_ExceptionsThrown -- number of throws executed (counted in ThrowTestException)
   g_MasterExecutedThrow -- number of times external thread actually executed a throw
   g_NonMasterExecutedThrow -- number of times non-external thread actually executed a throw
   g_ExceptionCaught -- one of PropagatedException or unknown exception was caught.  (Other exceptions cause assertions.)

   --  Tallies for the task bodies which have executed (counted in each inner body, sampled in ThrowTestException)
      g_CurExecuted -- total number of inner ranges or filters which executed
      g_ExecutedAtLastCatch -- value of g_CurExecuted when last catch was made, 0 if none.
      g_ExecutedAtFirstCatch -- value of g_CurExecuted when first catch is made, 0 if none.
  *********************************/

inline void ResetGlobals (  bool throwException = true, bool flog = false ) {
    ResetEhGlobals( throwException, flog );
    g_FedTasksCount = 0;
    g_OuterParCalls = 0;
    g_NestedPipelines = false;
    g_TGCCancelled.reset();
}

////////////////////////////////////////////////////////////////////////////////
// Tests for tbb::parallel_for and tbb::parallel_reduce
////////////////////////////////////////////////////////////////////////////////

typedef size_t count_type;
typedef tbb::blocked_range<count_type> range_type;

inline intptr_t CountSubranges(range_type r) {
    if(!r.is_divisible()) return intptr_t(1);
    range_type r2(r,tbb::split());
    return CountSubranges(r) + CountSubranges(r2);
}

inline intptr_t NumSubranges ( intptr_t length, intptr_t grain ) {
    return CountSubranges(range_type(0,length,grain));
}

template<class Body>
intptr_t TestNumSubrangesCalculation ( intptr_t length, intptr_t grain, intptr_t inner_length, intptr_t inner_grain ) {
    ResetGlobals();
    g_ThrowException = false;
    intptr_t outerCalls = NumSubranges(length, grain),
             innerCalls = NumSubranges(inner_length, inner_grain),
             maxExecuted = outerCalls * (innerCalls + 1);
    tbb::parallel_for( range_type(0, length, grain), Body() );
    REQUIRE_MESSAGE (g_CurExecuted == maxExecuted, "Wrong estimation of bodies invocation count");
    return maxExecuted;
}

class NoThrowParForBody {
public:
    void operator()( const range_type& r ) const {
        if(g_Master == std::this_thread::get_id()) g_MasterExecuted = true;
        else g_NonMasterExecuted = true;
        if( tbb::is_current_task_group_canceling() ) g_TGCCancelled.increment();
        utils::doDummyWork(r.size());
    }
};

#if TBB_USE_EXCEPTIONS

void Test0 () {
    ResetGlobals();
    tbb::simple_partitioner p;
    for( size_t i=0; i<10; ++i ) {
        tbb::parallel_for( range_type(0, 0, 1), NoThrowParForBody() );
        tbb::parallel_for( range_type(0, 0, 1), NoThrowParForBody(), p );
        tbb::parallel_for( range_type(0, 128, 8), NoThrowParForBody() );
        tbb::parallel_for( range_type(0, 128, 8), NoThrowParForBody(), p );
    }
} // void Test0 ()

//! Template that creates a functor suitable for parallel_reduce from a functor for parallel_for.
template<typename ParForBody>
class SimpleParReduceBody {
    ParForBody m_Body;
public:
    void operator=(const SimpleParReduceBody&) = delete;
    SimpleParReduceBody(const SimpleParReduceBody&) = default;
    SimpleParReduceBody() = default;

    void operator()( const range_type& r ) const { m_Body(r); }
    SimpleParReduceBody( SimpleParReduceBody& left, tbb::split ) : m_Body(left.m_Body) {}
    void join( SimpleParReduceBody& /*right*/ ) {}
}; // SimpleParReduceBody

//! Test parallel_for and parallel_reduce for a given partitioner.
/** The Body need only be suitable for a parallel_for. */
template<typename ParForBody, typename Partitioner>
void TestParallelLoopAux() {
    Partitioner partitioner;
    for( int i=0; i<2; ++i ) {
        ResetGlobals();
        TRY();
            if( i==0 )
                tbb::parallel_for( range_type(0, FLAT_RANGE, FLAT_GRAIN), ParForBody(), partitioner );
            else {
                SimpleParReduceBody<ParForBody> rb;
                tbb::parallel_reduce( range_type(0, FLAT_RANGE, FLAT_GRAIN), rb, partitioner );
            }
        CATCH_AND_ASSERT();
        // two cases: g_SolitaryException and !g_SolitaryException
        //   1) g_SolitaryException: only one thread actually threw.  There is only one context, so the exception
        //      (when caught) will cause that context to be cancelled.  After this event, there may be one or
        //      more threads which are "in-flight", up to g_NumThreads, but no more will be started.  The threads,
        //      when they start, if they see they are cancelled, TGCCancelled is incremented.
        //   2) !g_SolitaryException: more than one thread can throw.  The number of threads that actually
        //      threw is g_MasterExecutedThrow if only the external thread is allowed, else g_NonMasterExecutedThrow.
        //      Only one context, so TGCCancelled should be <= g_NumThreads.
        //
        // the reasoning is similar for nested algorithms in a single context (Test2).
        //
        // If a thread throws in a context, more than one subsequent task body may see the
        // cancelled state (if they are scheduled before the state is propagated.) this is
        // infrequent, but it occurs.  So what was to be an assertion must be a remark.
        g_TGCCancelled.validate( g_NumThreads, "Too many tasks ran after exception thrown");
        REQUIRE_MESSAGE(g_CurExecuted <= g_ExecutedAtLastCatch + g_NumThreads, "Too many tasks survived exception");
        if ( g_SolitaryException ) {
            REQUIRE_MESSAGE(g_NumExceptionsCaught == 1, "No try_blocks in any body expected in this test");
            REQUIRE_MESSAGE(g_NumExceptionsCaught == (g_ExceptionInMaster ? g_MasterExecutedThrow : g_NonMasterExecutedThrow),
                "Not all throws were caught");
            REQUIRE_MESSAGE(g_ExecutedAtFirstCatch == g_ExecutedAtLastCatch, "Too many exceptions occurred");
        }
        else {
            REQUIRE_MESSAGE(g_NumExceptionsCaught >= 1, "No try blocks in any body expected in this test");
        }
    }
}  // TestParallelLoopAux

//! Test with parallel_for and parallel_reduce, over all three kinds of partitioners.
/** The Body only needs to be suitable for tbb::parallel_for. */
template<typename Body>
void TestParallelLoop() {
    // The simple and auto partitioners should be const, but not the affinity partitioner.
    TestParallelLoopAux<Body, const tbb::simple_partitioner  >();
    TestParallelLoopAux<Body, const tbb::auto_partitioner    >();
#define __TBB_TEMPORARILY_DISABLED 1
#if !__TBB_TEMPORARILY_DISABLED
    // TODO: Improve the test so that it tolerates delayed start of tasks with affinity_partitioner
    TestParallelLoopAux<Body, /***/ tbb::affinity_partitioner>();
#endif
#undef __TBB_TEMPORARILY_DISABLED
}

class SimpleParForBody {
public:
    void operator=(const SimpleParForBody&) = delete;
    SimpleParForBody(const SimpleParForBody&) = default;
    SimpleParForBody() = default;

    void operator()( const range_type& r ) const {
        utils::ConcurrencyTracker ct;
        ++g_CurExecuted;
        if(g_Master == std::this_thread::get_id()) g_MasterExecuted = true;
        else g_NonMasterExecuted = true;
        if( tbb::is_current_task_group_canceling() ) g_TGCCancelled.increment();
        utils::doDummyWork(r.size());
        WaitUntilConcurrencyPeaks();
        ThrowTestException(1);
    }
};

void Test1() {
    // non-nested parallel_for/reduce with throwing body, one context
    TestParallelLoop<SimpleParForBody>();
} // void Test1 ()

class OuterParForBody {
public:
    void operator=(const OuterParForBody&) = delete;
    OuterParForBody(const OuterParForBody&) = default;
    OuterParForBody() = default;
    void operator()( const range_type& ) const {
        utils::ConcurrencyTracker ct;
        ++g_OuterParCalls;
        tbb::parallel_for( tbb::blocked_range<size_t>(0, INNER_RANGE, INNER_GRAIN), SimpleParForBody() );
    }
};

//! Uses parallel_for body containing an inner parallel_for with the default context not wrapped by a try-block.
/** Inner algorithms are spawned inside the new bound context by default. Since
    exceptions thrown from the inner parallel_for are not handled by the caller
    (outer parallel_for body) in this test, they will cancel all the sibling inner
    algorithms. **/
void Test2 () {
    TestParallelLoop<OuterParForBody>();
} // void Test2 ()

class OuterParForBodyWithIsolatedCtx {
public:
    void operator()( const range_type& ) const {
        tbb::task_group_context ctx(tbb::task_group_context::isolated);
        ++g_OuterParCalls;
        tbb::parallel_for( tbb::blocked_range<size_t>(0, INNER_RANGE, INNER_GRAIN), SimpleParForBody(), tbb::simple_partitioner(), ctx );
    }
};

//! Uses parallel_for body invoking an inner parallel_for with an isolated context without a try-block.
/** Even though exceptions thrown from the inner parallel_for are not handled
    by the caller in this test, they will not affect sibling inner algorithms
    already running because of the isolated contexts. However because the first
    exception cancels the root parallel_for only the first g_NumThreads subranges
    will be processed (which launch inner parallel_fors) **/
void Test3 () {
    ResetGlobals();
    typedef OuterParForBodyWithIsolatedCtx body_type;
    intptr_t  innerCalls = NumSubranges(INNER_RANGE, INNER_GRAIN),
            // we expect one thread to throw without counting, the rest to run to completion
            // this formula assumes g_numThreads outer pfor ranges will be started, but that is not the
            // case; the SimpleParFor subranges are started up as part of the outer ones, and when
            // the amount of concurrency reaches g_NumThreads no more outer Pfor ranges are started.
            // so we have to count the number of outer Pfors actually started.
            minExecuted = (g_NumThreads - 1) * innerCalls;
    TRY();
        tbb::parallel_for( range_type(0, OUTER_RANGE, OUTER_GRAIN), body_type() );
    CATCH_AND_ASSERT();
    minExecuted = (g_OuterParCalls - 1) * innerCalls;  // see above

    // The first formula above assumes all ranges of the outer parallel for are executed, and one
    // cancels.  In the event, we have a smaller number of ranges that start before the exception
    // is caught.
    //
    //  g_SolitaryException:One inner range throws.  Outer parallel_For is cancelled, but sibling
    //                      parallel_fors continue to completion (unless the threads that execute
    //                      are not allowed to throw, in which case we will not see any exceptions).
    // !g_SolitaryException:multiple inner ranges may throw.  Any which throws will stop, and the
    //                      corresponding range of the outer pfor will stop also.
    //
    // In either case, once the outer pfor gets the exception it will stop executing further ranges.

    // if the only threads executing were not allowed to throw, then not seeing an exception is okay.
    bool okayNoExceptionsCaught = (g_ExceptionInMaster && !g_MasterExecuted) || (!g_ExceptionInMaster && !g_NonMasterExecuted);
    if ( g_SolitaryException ) {
        g_TGCCancelled.validate(g_NumThreads, "Too many tasks survived exception");
        REQUIRE_MESSAGE (g_CurExecuted > minExecuted, "Too few tasks survived exception");
        REQUIRE_MESSAGE ((g_CurExecuted <= minExecuted + (g_ExecutedAtLastCatch + g_NumThreads)), "Too many tasks survived exception");
        REQUIRE_MESSAGE ((g_NumExceptionsCaught == 1 || okayNoExceptionsCaught), "No try_blocks in any body expected in this test");
    }
    else {
        REQUIRE_MESSAGE ((g_CurExecuted <= g_ExecutedAtLastCatch + g_NumThreads), "Too many tasks survived exception");
        REQUIRE_MESSAGE ((g_NumExceptionsCaught >= 1 || okayNoExceptionsCaught), "No try_blocks in any body expected in this test");
    }
} // void Test3 ()

class OuterParForExceptionSafeBody {
public:
    void operator()( const range_type& ) const {
        tbb::task_group_context ctx(tbb::task_group_context::isolated);
        ++g_OuterParCalls;
        TRY();
            tbb::parallel_for( tbb::blocked_range<size_t>(0, INNER_RANGE, INNER_GRAIN), SimpleParForBody(), tbb::simple_partitioner(), ctx );
        CATCH();  // this macro sets g_ExceptionCaught
    }
};

//! Uses parallel_for body invoking an inner parallel_for (with isolated context) inside a try-block.
/** Since exception(s) thrown from the inner parallel_for are handled by the caller
    in this test, they do not affect neither other tasks of the the root parallel_for
    nor sibling inner algorithms. **/
void Test4 () {
    ResetGlobals( true, true );
    intptr_t  innerCalls = NumSubranges(INNER_RANGE, INNER_GRAIN),
              outerCalls = NumSubranges(OUTER_RANGE, OUTER_GRAIN);
    TRY();
        tbb::parallel_for( range_type(0, OUTER_RANGE, OUTER_GRAIN), OuterParForExceptionSafeBody() );
    CATCH();
    // g_SolitaryException  : one inner pfor will throw, the rest will execute to completion.
    //                        so the count should be (outerCalls -1) * innerCalls, if a throw happened.
    // !g_SolitaryException : possible multiple inner pfor throws.  Should be approximately
    //                        (outerCalls - g_NumExceptionsCaught) * innerCalls, give or take a few
    intptr_t  minExecuted = (outerCalls - g_NumExceptionsCaught) * innerCalls;
    bool okayNoExceptionsCaught = (g_ExceptionInMaster && !g_MasterExecuted) || (!g_ExceptionInMaster && !g_NonMasterExecuted);
    if ( g_SolitaryException ) {
        // only one task had exception thrown. That task had at least one execution (the one that threw).
        // There may be an arbitrary number of ranges executed after the throw but before the exception
        // is caught in the scheduler and cancellation is signaled.  (seen 9, 11 and 62 (!) for 8 threads)
        REQUIRE_MESSAGE ((g_NumExceptionsCaught == 1 || okayNoExceptionsCaught), "No exception registered");
        REQUIRE_MESSAGE (g_CurExecuted >= minExecuted, "Too few tasks executed");
        g_TGCCancelled.validate(g_NumThreads, "Too many tasks survived exception");
        // a small number of threads can execute in a throwing sub-pfor, if the task which is
        // to do the solitary throw swaps out after registering its intent to throw but before it
        // actually does so. As a result, the number of extra tasks cannot exceed the number of thread
        // for each nested pfor invication)
        REQUIRE_MESSAGE (g_CurExecuted <= minExecuted + (g_ExecutedAtLastCatch + g_NumThreads), "Too many tasks survived exception");
    }
    else {
        REQUIRE_MESSAGE (((g_NumExceptionsCaught >= 1 && g_NumExceptionsCaught <= outerCalls) || okayNoExceptionsCaught), "Unexpected actual number of exceptions");
        REQUIRE_MESSAGE (g_CurExecuted >= minExecuted, "Too few executed tasks reported");
        REQUIRE_MESSAGE ((g_CurExecuted <= g_ExecutedAtLastCatch + g_NumThreads), "Too many tasks survived multiple exceptions");
        REQUIRE_MESSAGE (g_CurExecuted <= outerCalls * (1 + g_NumThreads), "Too many tasks survived exception");
    }
} // void Test4 ()

//! Testing parallel_for and parallel_reduce exception handling
//! \brief \ref error_guessing
TEST_CASE("parallel_for and parallel_reduce exception handling test #0") {
    for (auto concurrency_level: utils::concurrency_range()) {
        g_NumThreads = static_cast<int>(concurrency_level);
        g_Master = std::this_thread::get_id();
        if (g_NumThreads > 1) {
            tbb::task_arena a(g_NumThreads);
            a.execute([] {
                // Execute in all the possible modes
                for (size_t j = 0; j < 4; ++j) {
                    g_ExceptionInMaster = (j & 1) != 0;
                    g_SolitaryException = (j & 2) != 0;

                    Test0();
                }
            });
        }
    }
}

//! Testing parallel_for and parallel_reduce exception handling
//! \brief \ref error_guessing
TEST_CASE("parallel_for and parallel_reduce exception handling test #1") {
    for (auto concurrency_level: utils::concurrency_range()) {
        g_NumThreads = static_cast<int>(concurrency_level);
        g_Master = std::this_thread::get_id();
        if (g_NumThreads > 1) {
            tbb::task_arena a(g_NumThreads);
            a.execute([] {
                // Execute in all the possible modes
                for (size_t j = 0; j < 4; ++j) {
                    g_ExceptionInMaster = (j & 1) != 0;
                    g_SolitaryException = (j & 2) != 0;

                    Test1();
                }
            });
        }
    }
}

//! Testing parallel_for and parallel_reduce exception handling
//! \brief \ref error_guessing
TEST_CASE("parallel_for and parallel_reduce exception handling test #2") {
    for (auto concurrency_level: utils::concurrency_range()) {
        g_NumThreads = static_cast<int>(concurrency_level);
        g_Master = std::this_thread::get_id();
        if (g_NumThreads > 1) {
            tbb::task_arena a(g_NumThreads);
            a.execute([] {
                // Execute in all the possible modes
                for (size_t j = 0; j < 4; ++j) {
                    g_ExceptionInMaster = (j & 1) != 0;
                    g_SolitaryException = (j & 2) != 0;

                    Test2();
                }
            });
        }
    }
}

//! Testing parallel_for and parallel_reduce exception handling
//! \brief \ref error_guessing
TEST_CASE("parallel_for and parallel_reduce exception handling test #3") {
    for (auto concurrency_level: utils::concurrency_range()) {
        g_NumThreads = static_cast<int>(concurrency_level);
        g_Master = std::this_thread::get_id();
        if (g_NumThreads > 1) {
            tbb::task_arena a(g_NumThreads);
            a.execute([] {
                // Execute in all the possible modes
                for (size_t j = 0; j < 4; ++j) {
                    g_ExceptionInMaster = (j & 1) != 0;
                    g_SolitaryException = (j & 2) != 0;

                    Test3();
                }
            });
        }
    }
}

//! Testing parallel_for and parallel_reduce exception handling
//! \brief \ref error_guessing
TEST_CASE("parallel_for and parallel_reduce exception handling test #4") {
    for (auto concurrency_level: utils::concurrency_range()) {
        g_NumThreads = static_cast<int>(concurrency_level);
        g_Master = std::this_thread::get_id();
        if (g_NumThreads > 1) {
            tbb::task_arena a(g_NumThreads);
            a.execute([] {
                // Execute in all the possible modes
                for (size_t j = 0; j < 4; ++j) {
                    g_ExceptionInMaster = (j & 1) != 0;
                    g_SolitaryException = (j & 2) != 0;

                    Test4();
                }
            });
        }
    }
}

#endif /* TBB_USE_EXCEPTIONS */

class ParForBodyToCancel {
public:
    void operator()( const range_type& ) const {
        ++g_CurExecuted;
        Cancellator::WaitUntilReady();
    }
};

template<class B>
class ParForLauncher {
    tbb::task_group_context &my_ctx;
public:
    void operator()() const {
        tbb::parallel_for( range_type(0, FLAT_RANGE, FLAT_GRAIN), B(), tbb::simple_partitioner(), my_ctx );
    }
    ParForLauncher ( tbb::task_group_context& ctx ) : my_ctx(ctx) {}
};

//! Test for cancelling an algorithm from outside (from a task running in parallel with the algorithm).
void TestCancelation1 () {
    ResetGlobals( false );
    RunCancellationTest<ParForLauncher<ParForBodyToCancel>, Cancellator>( NumSubranges(FLAT_RANGE, FLAT_GRAIN) / 4 );
}

class Cancellator2  {
    tbb::task_group_context &m_GroupToCancel;

public:
    void operator()() const {
        utils::ConcurrencyTracker ct;
        WaitUntilConcurrencyPeaks();
        m_GroupToCancel.cancel_group_execution();
        g_ExecutedAtLastCatch = g_CurExecuted.load();
    }

    Cancellator2 ( tbb::task_group_context& ctx, intptr_t ) : m_GroupToCancel(ctx) {}
};

class ParForBodyToCancel2 {
public:
    void operator()( const range_type& ) const {
        ++g_CurExecuted;
        utils::ConcurrencyTracker ct;
        // The test will hang (and be timed out by the test system) if is_cancelled() is broken
        while( !tbb::is_current_task_group_canceling() )
            utils::yield();
    }
};

//! Test for cancelling an algorithm from outside (from a task running in parallel with the algorithm).
/** This version also tests tbb::is_current_task_group_canceling() method. **/
void TestCancelation2 () {
    ResetGlobals();
    RunCancellationTest<ParForLauncher<ParForBodyToCancel2>, Cancellator2>();
    REQUIRE_MESSAGE (g_ExecutedAtLastCatch < g_NumThreads, "Somehow worker tasks started their execution before the cancellator task");
    g_TGCCancelled.validate(g_NumThreads, "Too many tasks survived cancellation");
    REQUIRE_MESSAGE (g_CurExecuted <= g_ExecutedAtLastCatch + g_NumThreads, "Some tasks were executed after cancellation");
}

////////////////////////////////////////////////////////////////////////////////
// Regression test based on the contribution by the author of the following forum post:
// http://softwarecommunity.intel.com/isn/Community/en-US/forums/thread/30254959.aspx

class Worker {
    static const int max_nesting = 3;
    static const int reduce_range = 1024;
    static const int reduce_grain = 256;
public:
    int DoWork (int level);
    int Validate (int start_level) {
        int expected = 1; // identity for multiplication
        for(int i=start_level+1; i<max_nesting; ++i)
             expected *= reduce_range;
        return expected;
    }
};

class RecursiveParReduceBodyWithSharedWorker {
    Worker * m_SharedWorker;
    int m_NestingLevel;
    int m_Result;
public:
    RecursiveParReduceBodyWithSharedWorker ( RecursiveParReduceBodyWithSharedWorker& src, tbb::split )
        : m_SharedWorker(src.m_SharedWorker)
        , m_NestingLevel(src.m_NestingLevel)
        , m_Result(0)
    {}
    RecursiveParReduceBodyWithSharedWorker ( Worker *w, int outer )
        : m_SharedWorker(w)
        , m_NestingLevel(outer)
        , m_Result(0)
    {}

    void operator() ( const tbb::blocked_range<size_t>& r ) {
        if(g_Master == std::this_thread::get_id()) g_MasterExecuted = true;
        else g_NonMasterExecuted = true;
        if( tbb::is_current_task_group_canceling() ) g_TGCCancelled.increment();
        for (size_t i = r.begin (); i != r.end (); ++i) {
            m_Result += m_SharedWorker->DoWork (m_NestingLevel);
        }
    }
    void join (const RecursiveParReduceBodyWithSharedWorker & x) {
        m_Result += x.m_Result;
    }
    int result () { return m_Result; }
};

int Worker::DoWork ( int level ) {
    ++level;
    if ( level < max_nesting ) {
        RecursiveParReduceBodyWithSharedWorker rt (this, level);
        tbb::parallel_reduce (tbb::blocked_range<size_t>(0, reduce_range, reduce_grain), rt);
        return rt.result();
    }
    else
        return 1;
}

//! Regression test for hanging that occurred with the first version of cancellation propagation
void TestCancelation3 () {
    Worker w;
    int result   = w.DoWork (0);
    int expected = w.Validate(0);
    REQUIRE_MESSAGE ( result == expected, "Wrong calculation result");
}

struct StatsCounters {
    std::atomic<size_t> my_total_created;
    std::atomic<size_t> my_total_deleted;
    StatsCounters() {
        my_total_created = 0;
        my_total_deleted = 0;
    }
};

class ParReduceBody {
    StatsCounters* my_stats;
    size_t my_id;
    bool my_exception;
    tbb::task_group_context& tgc;

public:
    ParReduceBody( StatsCounters& s_, tbb::task_group_context& context, bool e_ ) :
        my_stats(&s_), my_exception(e_), tgc(context) {
        my_id = my_stats->my_total_created++;
    }

    ParReduceBody( const ParReduceBody& lhs ) : tgc(lhs.tgc) {
        my_stats = lhs.my_stats;
        my_id = my_stats->my_total_created++;
    }

    ParReduceBody( ParReduceBody& lhs, tbb::split ) : tgc(lhs.tgc) {
        my_stats = lhs.my_stats;
        my_id = my_stats->my_total_created++;
    }

    ~ParReduceBody(){ ++my_stats->my_total_deleted; }

    void operator()( const tbb::blocked_range<std::size_t>& /*range*/ ) const {
        //Do nothing, except for one task (chosen arbitrarily)
        if( my_id >= 12 ) {
            if( my_exception )
                ThrowTestException(1);
            else
                tgc.cancel_group_execution();
        }
    }

    void join( ParReduceBody& /*rhs*/ ) {}
};

void TestCancelation4() {
    StatsCounters statsObj;
#if TBB_USE_EXCEPTIONS
    try
#endif
    {
        tbb::task_group_context tgc1, tgc2;
        ParReduceBody body_for_cancellation(statsObj, tgc1, false), body_for_exception(statsObj, tgc2, true);
        tbb::parallel_reduce( tbb::blocked_range<std::size_t>(0,100000000,100), body_for_cancellation, tbb::simple_partitioner(), tgc1 );
        tbb::parallel_reduce( tbb::blocked_range<std::size_t>(0,100000000,100), body_for_exception, tbb::simple_partitioner(), tgc2 );
    }
#if TBB_USE_EXCEPTIONS
    catch(...) {}
#endif
    REQUIRE_MESSAGE ( statsObj.my_total_created==statsObj.my_total_deleted, "Not all parallel_reduce body objects created were reclaimed");
}

//! Testing parallel_for and parallel_reduce cancellation
//! \brief \ref error_guessing
TEST_CASE("parallel_for and parallel_reduce cancellation test #1") {
    for (auto concurrency_level: utils::concurrency_range()) {
        g_NumThreads = static_cast<int>(concurrency_level);
        g_Master = std::this_thread::get_id();
        if (g_NumThreads > 1) {
            tbb::task_arena a(g_NumThreads);
            a.execute([] {
                // Execute in all the possible modes
                for (size_t j = 0; j < 4; ++j) {
                    g_ExceptionInMaster = (j & 1) != 0;
                    g_SolitaryException = (j & 2) != 0;

                    TestCancelation1();
                }
            });
        }
    }
}

//! Testing parallel_for and parallel_reduce cancellation
//! \brief \ref error_guessing
TEST_CASE("parallel_for and parallel_reduce cancellation test #2") {
    for (auto concurrency_level: utils::concurrency_range()) {
        g_NumThreads = static_cast<int>(concurrency_level);
        g_Master = std::this_thread::get_id();
        if (g_NumThreads > 1) {
            tbb::task_arena a(g_NumThreads);
            a.execute([] {
                // Execute in all the possible modes
                for (size_t j = 0; j < 4; ++j) {
                    g_ExceptionInMaster = (j & 1) != 0;
                    g_SolitaryException = (j & 2) != 0;

                    TestCancelation2();
                }
            });
        }
    }
}

//! Testing parallel_for and parallel_reduce cancellation
//! \brief \ref error_guessing
TEST_CASE("parallel_for and parallel_reduce cancellation test #3") {
    for (auto concurrency_level: utils::concurrency_range()) {
        g_NumThreads = static_cast<int>(concurrency_level);
        g_Master = std::this_thread::get_id();
        if (g_NumThreads > 1) {
            tbb::task_arena a(g_NumThreads);
            a.execute([] {
                // Execute in all the possible modes
                for (size_t j = 0; j < 4; ++j) {
                    g_ExceptionInMaster = (j & 1) != 0;
                    g_SolitaryException = (j & 2) != 0;

                    TestCancelation3();
                }
            });
        }
    }
}

//! Testing parallel_for and parallel_reduce cancellation
//! \brief \ref error_guessing
TEST_CASE("parallel_for and parallel_reduce cancellation test #4") {
    for (auto concurrency_level: utils::concurrency_range()) {
        g_NumThreads = static_cast<int>(concurrency_level);
        g_Master = std::this_thread::get_id();
        if (g_NumThreads > 1) {
            tbb::task_arena a(g_NumThreads);
            a.execute([] {
                // Execute in all the possible modes
                for (size_t j = 0; j < 4; ++j) {
                    g_ExceptionInMaster = (j & 1) != 0;
                    g_SolitaryException = (j & 2) != 0;

                    TestCancelation4();
                }
            });
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests for tbb::parallel_for_each
////////////////////////////////////////////////////////////////////////////////

std::size_t get_iter_range_size() {
    // Set the minimal iteration sequence size to 50 to improve test complexity on small machines
    return std::max(50, g_NumThreads * 2);
}

template<typename Iterator>
struct adaptive_range {
    std::vector<std::size_t> my_array;

    adaptive_range(std::size_t size) : my_array(size + 1) {}
    using iterator = Iterator;

    iterator begin() const {
        return iterator{&my_array.front()};
    }
    iterator begin() {
        return iterator{&my_array.front()};
    }
    iterator end() const {
        return iterator{&my_array.back()};
    }
    iterator end() {
        return iterator{&my_array.back()};
    }
};

void Feed ( tbb::feeder<size_t> &feeder, size_t val ) {
    if (g_FedTasksCount < 50) {
        ++g_FedTasksCount;
        feeder.add(val);
    }
}

#define RunWithSimpleBody(func, body)       \
    func<utils::ForwardIterator<size_t>, body>();         \
    func<utils::ForwardIterator<size_t>, body##WithFeeder>()

#define RunWithTemplatedBody(func, body)     \
    func<utils::ForwardIterator<size_t>, body<utils::ForwardIterator<size_t> > >();         \
    func<utils::ForwardIterator<size_t>, body##WithFeeder<utils::ForwardIterator<size_t> > >()

#if TBB_USE_EXCEPTIONS

// Simple functor object with exception
class SimpleParForEachBody {
public:
    void operator() ( size_t &value ) const {
        ++g_CurExecuted;
        if(g_Master == std::this_thread::get_id()) g_MasterExecuted = true;
        else g_NonMasterExecuted = true;
        if( tbb::is_current_task_group_canceling() ) {
            g_TGCCancelled.increment();
        }
        utils::ConcurrencyTracker ct;
        value += 1000;
        WaitUntilConcurrencyPeaks();
        ThrowTestException(1);
    }
};

// Simple functor object with exception and feeder
class SimpleParForEachBodyWithFeeder : SimpleParForEachBody {
public:
    void operator() ( size_t &value, tbb::feeder<size_t> &feeder ) const {
        Feed(feeder, 0);
        SimpleParForEachBody::operator()(value);
    }
};

// Tests exceptions without nesting
template <class Iterator, class simple_body>
void Test1_parallel_for_each () {
    ResetGlobals();
    auto range = adaptive_range<Iterator>(get_iter_range_size());
    TRY();
        tbb::parallel_for_each(std::begin(range), std::end(range), simple_body() );
    CATCH_AND_ASSERT();
    REQUIRE_MESSAGE (g_CurExecuted <= g_ExecutedAtLastCatch + g_NumThreads, "Too many tasks survived exception");
    g_TGCCancelled.validate(g_NumThreads, "Too many tasks survived cancellation");
    REQUIRE_MESSAGE (g_NumExceptionsCaught == 1, "No try_blocks in any body expected in this test");
    if ( !g_SolitaryException )
        REQUIRE_MESSAGE (g_CurExecuted <= g_ExecutedAtLastCatch + g_NumThreads, "Too many tasks survived exception");

} // void Test1_parallel_for_each ()

template <class Iterator>
class OuterParForEachBody {
public:
    void operator()( size_t& /*value*/ ) const {
        ++g_OuterParCalls;
        auto range = adaptive_range<Iterator>(get_iter_range_size());
        tbb::parallel_for_each(std::begin(range), std::end(range), SimpleParForEachBody());
    }
};

template <class Iterator>
class OuterParForEachBodyWithFeeder : OuterParForEachBody<Iterator> {
public:
    void operator()( size_t& value, tbb::feeder<size_t>& feeder ) const {
        Feed(feeder, 0);
        OuterParForEachBody<Iterator>::operator()(value);
    }
};

//! Uses parallel_for_each body containing an inner parallel_for_each with the default context not wrapped by a try-block.
/** Inner algorithms are spawned inside the new bound context by default. Since
    exceptions thrown from the inner parallel_for_each are not handled by the caller
    (outer parallel_for_each body) in this test, they will cancel all the sibling inner
    algorithms. **/
template <class Iterator, class outer_body>
void Test2_parallel_for_each () {
    ResetGlobals();
    auto range = adaptive_range<Iterator>(get_iter_range_size());
    TRY();
        tbb::parallel_for_each(std::begin(range), std::end(range), outer_body() );
    CATCH_AND_ASSERT();
    REQUIRE_MESSAGE (g_CurExecuted <= g_ExecutedAtLastCatch + g_NumThreads, "Too many tasks survived exception");
    g_TGCCancelled.validate(g_NumThreads, "Too many tasks survived cancellation");
    REQUIRE_MESSAGE (g_NumExceptionsCaught == 1, "No try_blocks in any body expected in this test");
    if ( !g_SolitaryException )
        REQUIRE_MESSAGE (g_CurExecuted <= g_ExecutedAtLastCatch + g_NumThreads, "Too many tasks survived exception");
} // void Test2_parallel_for_each ()

template <class Iterator>
class OuterParForEachBodyWithIsolatedCtx {
public:
    void operator()( size_t& /*value*/ ) const {
        tbb::task_group_context ctx(tbb::task_group_context::isolated);
        ++g_OuterParCalls;
        auto range = adaptive_range<Iterator>(get_iter_range_size());
        tbb::parallel_for_each(std::begin(range), std::end(range), SimpleParForEachBody(), ctx);
    }
};

template <class Iterator>
class OuterParForEachBodyWithIsolatedCtxWithFeeder : OuterParForEachBodyWithIsolatedCtx<Iterator> {
public:
    void operator()( size_t& value, tbb::feeder<size_t> &feeder ) const {
        Feed(feeder, 0);
        OuterParForEachBodyWithIsolatedCtx<Iterator>::operator()(value);
    }
};

//! Uses parallel_for_each body invoking an inner parallel_for_each with an isolated context without a try-block.
/** Even though exceptions thrown from the inner parallel_for_each are not handled
    by the caller in this test, they will not affect sibling inner algorithms
    already running because of the isolated contexts. However because the first
    exception cancels the root parallel_for_each, at most the first g_NumThreads subranges
    will be processed (which launch inner parallel_for_eachs) **/
template <class Iterator, class outer_body>
void Test3_parallel_for_each () {
    ResetGlobals();
    auto range = adaptive_range<Iterator>(get_iter_range_size());
    intptr_t innerCalls = get_iter_range_size(),
             // The assumption here is the same as in outer parallel fors.
             minExecuted = (g_NumThreads - 1) * innerCalls;
    g_Master = std::this_thread::get_id();
    TRY();
        tbb::parallel_for_each(std::begin(range), std::end(range), outer_body());
    CATCH_AND_ASSERT();
    // figure actual number of expected executions given the number of outer PDos started.
    minExecuted = (g_OuterParCalls - 1) * innerCalls;
    // one extra thread may run a task that sees cancellation.  Infrequent but possible
    g_TGCCancelled.validate(g_NumThreads, "Too many tasks survived exception");
    if ( g_SolitaryException ) {
        REQUIRE_MESSAGE (g_CurExecuted > minExecuted, "Too few tasks survived exception");
        REQUIRE_MESSAGE (g_CurExecuted <= minExecuted + (g_ExecutedAtLastCatch + g_NumThreads), "Too many tasks survived exception");
    }
    REQUIRE_MESSAGE (g_NumExceptionsCaught == 1, "No try_blocks in any body expected in this test");
    if ( !g_SolitaryException )
        REQUIRE_MESSAGE (g_CurExecuted <= g_ExecutedAtLastCatch + g_NumThreads, "Too many tasks survived exception");
} // void Test3_parallel_for_each ()

template <class Iterator>
class OuterParForEachWithEhBody {
public:
    void operator()( size_t& /*value*/ ) const {
        tbb::task_group_context ctx(tbb::task_group_context::isolated);
        ++g_OuterParCalls;
        auto range = adaptive_range<Iterator>(get_iter_range_size());
        TRY();
            tbb::parallel_for_each(std::begin(range), std::end(range), SimpleParForEachBody(), ctx);
        CATCH();
    }
};

template <class Iterator>
class OuterParForEachWithEhBodyWithFeeder : OuterParForEachWithEhBody<Iterator> {
public:
    void operator=(const OuterParForEachWithEhBodyWithFeeder&) = delete;
    OuterParForEachWithEhBodyWithFeeder(const OuterParForEachWithEhBodyWithFeeder&) = default;
    OuterParForEachWithEhBodyWithFeeder() = default;
    void operator()( size_t &value, tbb::feeder<size_t> &feeder ) const {
        Feed(feeder, 0);
        OuterParForEachWithEhBody<Iterator>::operator()(value);
    }
};

//! Uses parallel_for body invoking an inner parallel_for (with default bound context) inside a try-block.
/** Since exception(s) thrown from the inner parallel_for are handled by the caller
    in this test, they do not affect neither other tasks of the the root parallel_for
    nor sibling inner algorithms. **/
template <class Iterator, class outer_body_with_eh>
void Test4_parallel_for_each () {
    ResetGlobals( true, true );
    auto range = adaptive_range<Iterator>(get_iter_range_size());
    g_Master = std::this_thread::get_id();
    TRY();
        tbb::parallel_for_each(std::begin(range), std::end(range), outer_body_with_eh());
    CATCH();
    REQUIRE_MESSAGE (!l_ExceptionCaughtAtCurrentLevel, "All exceptions must have been handled in the parallel_for_each body");
    intptr_t innerCalls = get_iter_range_size(),
             outerCalls = get_iter_range_size() + g_FedTasksCount,
             maxExecuted = outerCalls * innerCalls,
             minExecuted = 0;
    g_TGCCancelled.validate(g_NumThreads, "Too many tasks survived exception");
    if ( g_SolitaryException ) {
        minExecuted = maxExecuted - innerCalls;
        REQUIRE_MESSAGE (g_NumExceptionsCaught == 1, "No exception registered");
        REQUIRE_MESSAGE (g_CurExecuted >= minExecuted, "Too few tasks executed");
        // This test has the same property as Test4 (parallel_for); the exception can be
        // thrown, but some number of tasks from the outer Pdo can execute after the throw but
        // before the cancellation is signaled (have seen 36).
        DOCTEST_WARN_MESSAGE(g_CurExecuted < maxExecuted, "All tasks survived exception. Oversubscription?");
    }
    else {
        minExecuted = g_NumExceptionsCaught;
        REQUIRE_MESSAGE ((g_NumExceptionsCaught > 1 && g_NumExceptionsCaught <= outerCalls), "Unexpected actual number of exceptions");
        REQUIRE_MESSAGE (g_CurExecuted >= minExecuted, "Too many executed tasks reported");
        REQUIRE_MESSAGE (g_CurExecuted < g_ExecutedAtLastCatch + g_NumThreads + outerCalls, "Too many tasks survived multiple exceptions");
        REQUIRE_MESSAGE (g_CurExecuted <= outerCalls * (1 + g_NumThreads), "Too many tasks survived exception");
    }
} // void Test4_parallel_for_each ()

// This body throws an exception only if the task was added by feeder
class ParForEachBodyWithThrowingFeederTasks {
public:
    //! This form of the function call operator can be used when the body needs to add more work during the processing
    void operator() (const size_t &value, tbb::feeder<size_t> &feeder ) const {
        ++g_CurExecuted;
        if(g_Master == std::this_thread::get_id()) g_MasterExecuted = true;
        else g_NonMasterExecuted = true;
        if( tbb::is_current_task_group_canceling() ) g_TGCCancelled.increment();
        Feed(feeder, 1);
        if (value == 1)
            ThrowTestException(1);
    }
}; // class ParForEachBodyWithThrowingFeederTasks

// Test exception in task, which was added by feeder.
template <class Iterator>
void Test5_parallel_for_each () {
    ResetGlobals();
    auto range = adaptive_range<Iterator>(get_iter_range_size());
    g_Master = std::this_thread::get_id();
    TRY();
        tbb::parallel_for_each(std::begin(range), std::end(range), ParForEachBodyWithThrowingFeederTasks());
    CATCH();
    if (g_SolitaryException) {
        // Failure occurs when g_ExceptionInMaster is false, but all the 1 values in the range
        // are handled by the external thread.  In this case no throw occurs.
        REQUIRE_MESSAGE ((l_ExceptionCaughtAtCurrentLevel               // we saw an exception
                || (!g_ExceptionInMaster && !g_NonMasterExecutedThrow)  // non-external trhead throws but none tried
                || (g_ExceptionInMaster && !g_MasterExecutedThrow))     // external thread throws but external thread didn't try
                , "At least one exception should occur");
    }
} // void Test5_parallel_for_each ()

//! Testing parallel_for_each exception handling
//! \brief \ref error_guessing
TEST_CASE("parallel_for_each exception handling test #1") {
    for (auto concurrency_level: utils::concurrency_range()) {
        g_NumThreads = static_cast<int>(concurrency_level);
        g_Master = std::this_thread::get_id();
        if (g_NumThreads > 1) {
            tbb::task_arena a(g_NumThreads);
            a.execute([] {
                // Execute in all the possible modes
                for (size_t j = 0; j < 4; ++j) {
                    g_ExceptionInMaster = (j & 1) != 0;
                    g_SolitaryException = (j & 2) != 0;

                    RunWithSimpleBody(Test1_parallel_for_each, SimpleParForEachBody);
                }
            });
        }
    }
}

//! Testing parallel_for_each exception handling
//! \brief \ref error_guessing
TEST_CASE("parallel_for_each exception handling test #2") {
    for (auto concurrency_level: utils::concurrency_range()) {
        g_NumThreads = static_cast<int>(concurrency_level);
        g_Master = std::this_thread::get_id();
        if (g_NumThreads > 1) {
            tbb::task_arena a(g_NumThreads);
            a.execute([] {
                // Execute in all the possible modes
                for (size_t j = 0; j < 4; ++j) {
                    g_ExceptionInMaster = (j & 1) != 0;
                    g_SolitaryException = (j & 2) != 0;

                    RunWithTemplatedBody(Test2_parallel_for_each, OuterParForEachBody);
                }
            });
        }
    }
}

//! Testing parallel_for_each exception handling
//! \brief \ref error_guessing
TEST_CASE("parallel_for_each exception handling test #3") {
    for (auto concurrency_level: utils::concurrency_range()) {
        g_NumThreads = static_cast<int>(concurrency_level);
        g_Master = std::this_thread::get_id();
        if (g_NumThreads > 1) {
            tbb::task_arena a(g_NumThreads);
            a.execute([] {
                // Execute in all the possible modes
                for (size_t j = 0; j < 4; ++j) {
                    g_ExceptionInMaster = (j & 1) != 0;
                    g_SolitaryException = (j & 2) != 0;

                    RunWithTemplatedBody(Test3_parallel_for_each, OuterParForEachBodyWithIsolatedCtx);
                }
            });
        }
    }
}

//! Testing parallel_for_each exception handling
//! \brief \ref error_guessing
TEST_CASE("parallel_for_each exception handling test #4") {
    for (auto concurrency_level: utils::concurrency_range()) {
        g_NumThreads = static_cast<int>(concurrency_level);
        g_Master = std::this_thread::get_id();
        if (g_NumThreads > 1) {
            tbb::task_arena a(g_NumThreads);
            a.execute([] {
                // Execute in all the possible modes
                for (size_t j = 0; j < 4; ++j) {
                    g_ExceptionInMaster = (j & 1) != 0;
                    g_SolitaryException = (j & 2) != 0;

                    RunWithTemplatedBody(Test4_parallel_for_each, OuterParForEachWithEhBody);
                }
            });
        }
    }
}

//! Testing parallel_for_each exception handling
//! \brief \ref error_guessing
TEST_CASE("parallel_for_each exception handling test #5") {
    for (auto concurrency_level: utils::concurrency_range()) {
        g_NumThreads = static_cast<int>(concurrency_level);
        g_Master = std::this_thread::get_id();
        if (g_NumThreads > 1) {
            tbb::task_arena a(g_NumThreads);
            a.execute([] {
                // Execute in all the possible modes
                for (size_t j = 0; j < 4; ++j) {
                    g_ExceptionInMaster = (j & 1) != 0;
                    g_SolitaryException = (j & 2) != 0;

                    Test5_parallel_for_each<utils::InputIterator<size_t> >();
                    Test5_parallel_for_each<utils::ForwardIterator<size_t> >();
                    Test5_parallel_for_each<utils::RandomIterator<size_t> >();
                }
            });
        }
    }
}

#endif /* TBB_USE_EXCEPTIONS */

class ParForEachBodyToCancel {
public:
    void operator()( size_t& /*value*/ ) const {
        ++g_CurExecuted;
        Cancellator::WaitUntilReady();
    }
};

class ParForEachBodyToCancelWithFeeder : ParForEachBodyToCancel {
public:
    void operator()( size_t& value, tbb::feeder<size_t> &feeder ) const {
        Feed(feeder, 0);
        ParForEachBodyToCancel::operator()(value);
    }
};

template<class B, class Iterator>
class ParForEachWorker {
    tbb::task_group_context &my_ctx;
public:
    void operator()() const {
        auto range = adaptive_range<Iterator>(get_iter_range_size());
        tbb::parallel_for_each( std::begin(range), std::end(range), B(), my_ctx );
    }

    ParForEachWorker ( tbb::task_group_context& ctx ) : my_ctx(ctx) {}
};

//! Test for cancelling an algorithm from outside (from a task running in parallel with the algorithm).
template <class Iterator, class body_to_cancel>
void TestCancelation1_parallel_for_each () {
    ResetGlobals( false );
    // Threshold should leave more than max_threads tasks to test the cancellation. Set the threshold to iter_range_size()/4 since iter_range_size >= max_threads*2
    intptr_t threshold = get_iter_range_size() / 4;
    REQUIRE_MESSAGE(get_iter_range_size() - threshold > g_NumThreads, "Threshold should leave more than max_threads tasks to test the cancellation.");
    tbb::task_group tg;
    tbb::task_group_context  ctx;
    Cancellator cancellator(ctx, threshold);
    ParForEachWorker<body_to_cancel, Iterator> worker(ctx);
    tg.run( cancellator );
    utils::yield();
    tg.run( worker );
    TRY();
        tg.wait();
    CATCH_AND_FAIL();
    REQUIRE_MESSAGE (g_CurExecuted < g_ExecutedAtLastCatch + g_NumThreads, "Too many tasks were executed after cancellation");
}

class ParForEachBodyToCancel2 {
public:
    void operator()( size_t& /*value*/ ) const {
        ++g_CurExecuted;
        utils::ConcurrencyTracker ct;
        // The test will hang (and be timed out by the test system) if is_cancelled() is broken
        while( !tbb::is_current_task_group_canceling() )
            utils::yield();
    }
};

class ParForEachBodyToCancel2WithFeeder : ParForEachBodyToCancel2 {
public:
    void operator()( size_t& value, tbb::feeder<size_t> &feeder ) const {
        Feed(feeder, 0);
        ParForEachBodyToCancel2::operator()(value);
    }
};

//! Test for cancelling an algorithm from outside (from a task running in parallel with the algorithm).
/** This version also tests tbb::is_current_task_group_canceling() method. **/
template <class Iterator, class body_to_cancel>
void TestCancelation2_parallel_for_each () {
    ResetGlobals();
    RunCancellationTest<ParForEachWorker<body_to_cancel, Iterator>, Cancellator2>();
}

//! Testing parallel_for_each cancellation test
//! \brief \ref error_guessing
TEST_CASE("parallel_for_each cancellation test #1") {
    for (auto concurrency_level: utils::concurrency_range()) {
        g_NumThreads = static_cast<int>(concurrency_level);
        g_Master = std::this_thread::get_id();
        if (g_NumThreads > 1) {
            tbb::task_arena a(g_NumThreads);
            a.execute([] {
                // Execute in all the possible modes
                for (size_t j = 0; j < 4; ++j) {
                    g_ExceptionInMaster = (j & 1) != 0;
                    g_SolitaryException = (j & 2) != 0;
                    RunWithSimpleBody(TestCancelation1_parallel_for_each, ParForEachBodyToCancel);
                }
            });
        }
    }
}

//! Testing parallel_for_each cancellation test
//! \brief \ref error_guessing
TEST_CASE("parallel_for_each cancellation test #2") {
    for (auto concurrency_level: utils::concurrency_range()) {
        g_NumThreads = static_cast<int>(concurrency_level);
        g_Master = std::this_thread::get_id();
        if (g_NumThreads > 1) {
            tbb::task_arena a(g_NumThreads);
            a.execute([] {
                // Execute in all the possible modes
                for (size_t j = 0; j < 4; ++j) {
                    g_ExceptionInMaster = (j & 1) != 0;
                    g_SolitaryException = (j & 2) != 0;

                    RunWithSimpleBody(TestCancelation2_parallel_for_each, ParForEachBodyToCancel2);
                }
            });
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests for tbb::parallel_pipeline
////////////////////////////////////////////////////////////////////////////////
int g_NumTokens = 0;

// Simple input filter class, it assigns 1 to all array members
// It stops when it receives item equal to -1
class InputFilter {
    mutable std::atomic<size_t> m_Item{};
    mutable std::vector<size_t> m_Buffer;
public:
    InputFilter() : m_Buffer(get_iter_range_size()) {
        m_Item = 0;
        for (size_t i = 0; i < get_iter_range_size(); ++i )
            m_Buffer[i] = 1;
    }
    InputFilter(const InputFilter& other) : m_Item(other.m_Item.load()), m_Buffer(get_iter_range_size()) {
        for (size_t i = 0; i < get_iter_range_size(); ++i )
            m_Buffer[i] = other.m_Buffer[i];
    }

    void* operator()(tbb::flow_control& control) const {
        size_t item = m_Item++;
        if(g_Master == std::this_thread::get_id()) g_MasterExecuted = true;
        else g_NonMasterExecuted = true;
        if( tbb::is_current_task_group_canceling() ) g_TGCCancelled.increment();
        if(item == 1) {
            ++g_PipelinesStarted;   // count on emitting the first item.
        }
        if ( item >= get_iter_range_size() ) {
            control.stop();
            return nullptr;
        }
        m_Buffer[item] = 1;
        return &m_Buffer[item];
    }

    size_t* buffer() { return m_Buffer.data(); }
}; // class InputFilter

#if TBB_USE_EXCEPTIONS

// Simple filter with exception throwing.  If parallel, will wait until
// as many parallel filters start as there are threads.
class SimpleFilter {
    bool m_canThrow;
    bool m_serial;
public:
    SimpleFilter (bool canThrow, bool serial ) : m_canThrow(canThrow), m_serial(serial) {}
    void* operator()(void* item) const {
        ++g_CurExecuted;
        if(g_Master == std::this_thread::get_id()) g_MasterExecuted = true;
        else g_NonMasterExecuted = true;
        if( tbb::is_current_task_group_canceling() ) g_TGCCancelled.increment();
        if ( m_canThrow ) {
            if ( !m_serial ) {
                utils::ConcurrencyTracker ct;
                WaitUntilConcurrencyPeaks( std::min(g_NumTokens, g_NumThreads) );
            }
            ThrowTestException(1);
        }
        return item;
    }
}; // class SimpleFilter

// This enumeration represents filters order in pipeline
struct FilterSet {
    tbb::filter_mode mode1, mode2;
    bool throw1, throw2;

    FilterSet( tbb::filter_mode m1, tbb::filter_mode m2, bool t1, bool t2 )
        : mode1(m1), mode2(m2), throw1(t1), throw2(t2)
    {}
}; // struct FilterSet

FilterSet serial_parallel( tbb::filter_mode::serial_in_order, tbb::filter_mode::parallel, /*throw1*/false, /*throw2*/true );

template<typename InFilter, typename Filter>
class CustomPipeline {
    InFilter inputFilter;
    Filter filter1;
    Filter filter2;
    FilterSet my_filters;
public:
    CustomPipeline( const FilterSet& filters )
        : filter1(filters.throw1, filters.mode1 != tbb::filter_mode::parallel),
          filter2(filters.throw2, filters.mode2 != tbb::filter_mode::parallel),
          my_filters(filters)
    {}

    void run () {
        tbb::parallel_pipeline(
            g_NumTokens,
            tbb::make_filter<void, void*>(tbb::filter_mode::parallel, inputFilter) &
            tbb::make_filter<void*, void*>(my_filters.mode1, filter1) &
            tbb::make_filter<void*, void>(my_filters.mode2, filter2)
        );
    }
    void run ( tbb::task_group_context& ctx ) {
        tbb::parallel_pipeline(
            g_NumTokens,
            tbb::make_filter<void, void*>(tbb::filter_mode::parallel, inputFilter) &
            tbb::make_filter<void*, void*>(my_filters.mode1, filter1) &
            tbb::make_filter<void*, void>(my_filters.mode2, filter2),
            ctx
        );
    }
};

typedef CustomPipeline<InputFilter, SimpleFilter> SimplePipeline;

// Tests exceptions without nesting
void Test1_pipeline ( const FilterSet& filters ) {
    ResetGlobals();
    SimplePipeline testPipeline(filters);
    TRY();
        testPipeline.run();
        if ( g_CurExecuted == 2 * static_cast<int>(get_iter_range_size()) ) {
            // all the items were processed, though an exception was supposed to occur.
            if(!g_ExceptionInMaster && g_NonMasterExecutedThrow > 0) {
                // if !g_ExceptionInMaster, the external thread is not allowed to throw.
                // if g_nonMasterExcutedThrow > 0 then a thread besides the external thread tried to throw.
                REQUIRE_MESSAGE((filters.mode1 != tbb::filter_mode::parallel && filters.mode2 != tbb::filter_mode::parallel),
                    "Unusual count");
            }
            // In case of all serial filters they might be all executed in the thread(s)
            // where exceptions are not allowed by the common test logic. So we just quit.
            return;
        }
    CATCH_AND_ASSERT();
    g_TGCCancelled.validate(g_NumThreads, "Too many tasks survived exception");
    REQUIRE_MESSAGE (g_CurExecuted <= g_ExecutedAtLastCatch + g_NumThreads, "Too many tasks survived exception");
    REQUIRE_MESSAGE (g_NumExceptionsCaught == 1, "No try_blocks in any body expected in this test");
    if ( !g_SolitaryException )
        REQUIRE_MESSAGE (g_CurExecuted <= g_ExecutedAtLastCatch + g_NumThreads, "Too many tasks survived exception");

} // void Test1_pipeline ()

// Filter with nesting
class OuterFilter {
public:
    OuterFilter ( bool, bool ) {}

    void* operator()(void* item) const {
        ++g_OuterParCalls;
        SimplePipeline testPipeline(serial_parallel);
        testPipeline.run();
        return item;
    }
}; // class OuterFilter

//! Uses pipeline containing an inner pipeline with the default context not wrapped by a try-block.
/** Inner algorithms are spawned inside the new bound context by default. Since
    exceptions thrown from the inner pipeline are not handled by the caller
    (outer pipeline body) in this test, they will cancel all the sibling inner
    algorithms. **/
void Test2_pipeline ( const FilterSet& filters ) {
    ResetGlobals();
    g_NestedPipelines = true;
    CustomPipeline<InputFilter, OuterFilter> testPipeline(filters);
    TRY();
        testPipeline.run();
    CATCH_AND_ASSERT();
    bool okayNoExceptionCaught = (g_ExceptionInMaster && !g_MasterExecutedThrow) || (!g_ExceptionInMaster && !g_NonMasterExecutedThrow);
    REQUIRE_MESSAGE ((g_NumExceptionsCaught == 1 || okayNoExceptionCaught), "No try_blocks in any body expected in this test");
    if ( !g_SolitaryException ) {
        REQUIRE_MESSAGE (g_CurExecuted <= g_ExecutedAtLastCatch + g_NumThreads, "Too many tasks survived exception");
    }
} // void Test2_pipeline ()

//! creates isolated inner pipeline and runs it.
class OuterFilterWithIsolatedCtx {
public:
    OuterFilterWithIsolatedCtx( bool , bool ) {}

    void* operator()(void* item) const {
        ++g_OuterParCalls;
        tbb::task_group_context ctx(tbb::task_group_context::isolated);
        // create inner pipeline with serial input, parallel output filter, second filter throws
        SimplePipeline testPipeline(serial_parallel);
        testPipeline.run(ctx);
        return item;
    }
}; // class OuterFilterWithIsolatedCtx

//! Uses pipeline invoking an inner pipeline with an isolated context without a try-block.
/** Even though exceptions thrown from the inner pipeline are not handled
    by the caller in this test, they will not affect sibling inner algorithms
    already running because of the isolated contexts. However because the first
    exception cancels the root parallel_for_each only the first g_NumThreads subranges
    will be processed (which launch inner pipelines) **/
void Test3_pipeline ( const FilterSet& filters ) {
    for( int nTries = 1; nTries <= 4; ++nTries) {
        ResetGlobals();
        g_NestedPipelines = true;
        g_Master = std::this_thread::get_id();
        intptr_t innerCalls = get_iter_range_size(),
                 minExecuted = (g_NumThreads - 1) * innerCalls;
        CustomPipeline<InputFilter, OuterFilterWithIsolatedCtx> testPipeline(filters);
        TRY();
            testPipeline.run();
        CATCH_AND_ASSERT();

        bool okayNoExceptionCaught = (g_ExceptionInMaster && !g_MasterExecuted) ||
            (!g_ExceptionInMaster && !g_NonMasterExecuted);
        // only test assertions if the test threw an exception (or we don't care)
        bool testSucceeded = okayNoExceptionCaught || g_NumExceptionsCaught > 0;
        if(testSucceeded) {
            if (g_SolitaryException) {

                // The test is one outer pipeline with two NestedFilters that each start an inner pipeline.
                // Each time the input filter of a pipeline delivers its first item, it increments
                // g_PipelinesStarted.  When g_SolitaryException, the throw will not occur until
                // g_PipelinesStarted >= 3.  (This is so at least a second pipeline in its own isolated
                // context will start; that is what we're testing.)
                //
                // There are two pipelines which will NOT run to completion when a solitary throw
                // happens in an isolated inner context: the outer pipeline and the pipeline which
                // throws.  All the other pipelines which start should run to completion.  But only
                // inner body invocations are counted.
                //
                // So g_CurExecuted should be about
                //
                //   (2*get_iter_range_size()) * (g_PipelinesStarted - 2) + 1
                //   ^ executions for each completed pipeline
                //                   ^ completing pipelines (remembering two will not complete)
                //                                              ^ one for the inner throwing pipeline

                minExecuted = (2*get_iter_range_size()) * (g_PipelinesStarted - 2) + 1;
                // each failing pipeline must execute at least two tasks
                REQUIRE_MESSAGE(g_CurExecuted >= minExecuted, "Too few tasks survived exception");
                // no more than g_NumThreads tasks will be executed in a cancelled context.  Otherwise
                // tasks not executing at throw were scheduled.
                g_TGCCancelled.validate(g_NumThreads, "Tasks not in-flight were executed");
                REQUIRE_MESSAGE(g_NumExceptionsCaught == 1, "Should have only one exception");
                // if we're only throwing from the external thread, and that thread didn't
                // participate in the pipelines, then no throw occurred.
            }
            REQUIRE_MESSAGE ((g_NumExceptionsCaught == 1 || okayNoExceptionCaught), "No try_blocks in any body expected in this test");
            REQUIRE_MESSAGE (((g_CurExecuted <= g_ExecutedAtLastCatch + g_NumThreads) || okayNoExceptionCaught), "Too many tasks survived exception");
            return;
        }
    }
}

class OuterFilterWithEhBody  {
public:
    OuterFilterWithEhBody( bool, bool ){}

    void* operator()(void* item) const {
        tbb::task_group_context ctx(tbb::task_group_context::isolated);
        ++g_OuterParCalls;
        SimplePipeline testPipeline(serial_parallel);
        TRY();
            testPipeline.run(ctx);
        CATCH();
        return item;
    }
}; // class OuterFilterWithEhBody

//! Uses pipeline body invoking an inner pipeline (with isolated context) inside a try-block.
/** Since exception(s) thrown from the inner pipeline are handled by the caller
    in this test, they do not affect other tasks of the the root pipeline
    nor sibling inner algorithms. **/
void Test4_pipeline ( const FilterSet& filters ) {
#if __GNUC__ && !__INTEL_COMPILER
    if ( strncmp(__VERSION__, "4.1.0", 5) == 0 ) {
        MESSAGE("Known issue: one of exception handling tests is skipped.");
        return;
    }
#endif
    ResetGlobals( true, true );
    // each outer pipeline stage will start get_iter_range_size() inner pipelines.
    // each inner pipeline that doesn't throw will process get_iter_range_size() items.
    // for solitary exception there will be one pipeline that only processes one stage, one item.
    // innerCalls should be 2*get_iter_range_size()
    intptr_t innerCalls = 2*get_iter_range_size(),
             outerCalls = 2*get_iter_range_size(),
             maxExecuted = outerCalls * innerCalls;  // the number of invocations of the inner pipelines
    CustomPipeline<InputFilter, OuterFilterWithEhBody> testPipeline(filters);
    TRY();
        testPipeline.run();
    CATCH_AND_ASSERT();
    intptr_t  minExecuted = 0;
    bool okayNoExceptionCaught = (g_ExceptionInMaster && !g_MasterExecuted) ||
        (!g_ExceptionInMaster && !g_NonMasterExecuted);
    if ( g_SolitaryException ) {
        minExecuted = maxExecuted - innerCalls;  // one throwing inner pipeline
        REQUIRE_MESSAGE((g_NumExceptionsCaught == 1 || okayNoExceptionCaught), "No exception registered");
        g_TGCCancelled.validate(g_NumThreads, "Too many tasks survived exception");  // probably will assert.
    }
    else {
        // we assume throwing pipelines will not count
        minExecuted = (outerCalls - g_NumExceptionsCaught) * innerCalls;
        REQUIRE_MESSAGE(((g_NumExceptionsCaught >= 1 && g_NumExceptionsCaught <= outerCalls) || okayNoExceptionCaught), "Unexpected actual number of exceptions");
        REQUIRE_MESSAGE (g_CurExecuted >= minExecuted, "Too many executed tasks reported");
        // too many already-scheduled tasks are started after the first exception is
        // thrown.  And g_ExecutedAtLastCatch is updated every time an exception is caught.
        // So with multiple exceptions there are a variable number of tasks that have been
        // discarded because of the signals.
        // each throw is caught, so we will see many cancelled tasks.  g_ExecutedAtLastCatch is
        // updated with each throw, so the value will be the number of tasks executed at the last
        REQUIRE_MESSAGE (g_CurExecuted <= g_ExecutedAtLastCatch + g_NumThreads, "Too many tasks survived multiple exceptions");
    }
} // void Test4_pipeline ()

//! Tests pipeline function passed with different combination of filters
template<void testFunc(const FilterSet&)>
void TestWithDifferentFiltersAndConcurrency() {
#if __TBB_USE_ADDRESS_SANITIZER
    // parallel_pipeline allocates tls that sporadically observed as a memory leak with
    // detached threads. So, use task_scheduler_handle to join threads with finalize
    tbb::task_scheduler_handle handle{ tbb::attach{} };
#endif
    for (auto concurrency_level: utils::concurrency_range()) {
        g_NumThreads = static_cast<int>(concurrency_level);
        g_Master = std::this_thread::get_id();
        if (g_NumThreads > 1) {

            const tbb::filter_mode modes[] = {
                tbb::filter_mode::parallel,
                tbb::filter_mode::serial_in_order,
                tbb::filter_mode::serial_out_of_order
            };

            const int NumFilterTypes = sizeof(modes)/sizeof(modes[0]);

            // Execute in all the possible modes
            for ( size_t j = 0; j < 4; ++j ) {
                tbb::task_arena a(g_NumThreads);
                a.execute([&] {
                    g_ExceptionInMaster = (j & 1) != 0;
                    g_SolitaryException = (j & 2) != 0;
                    g_NumTokens = 2 * g_NumThreads;

                    for (int i = 0; i < NumFilterTypes; ++i) {
                        for (int n = 0; n < NumFilterTypes; ++n) {
                            for (int k = 0; k < 2; ++k)
                                testFunc(FilterSet(modes[i], modes[n], k == 0, k != 0));
                        }
                    }
                });
            }
        }
    }
#if __TBB_USE_ADDRESS_SANITIZER
    tbb::finalize(handle);
#endif
}

//! Testing parallel_pipeline exception handling
//! \brief \ref error_guessing
TEST_CASE("parallel_pipeline exception handling test #1") {
    TestWithDifferentFiltersAndConcurrency<Test1_pipeline>();
}

//! Testing parallel_pipeline exception handling
//! \brief \ref error_guessing
TEST_CASE("parallel_pipeline exception handling test #2") {
    TestWithDifferentFiltersAndConcurrency<Test2_pipeline>();
}

//! Testing parallel_pipeline exception handling
//! \brief \ref error_guessing
TEST_CASE("parallel_pipeline exception handling test #3") {
    TestWithDifferentFiltersAndConcurrency<Test3_pipeline>();
}

//! Testing parallel_pipeline exception handling
//! \brief \ref error_guessing
TEST_CASE("parallel_pipeline exception handling test #4") {
    TestWithDifferentFiltersAndConcurrency<Test4_pipeline>();
}

#endif /* TBB_USE_EXCEPTIONS */

class FilterToCancel  {
public:
    FilterToCancel() {}
    void* operator()(void* item) const {
        ++g_CurExecuted;
        Cancellator::WaitUntilReady();
        return item;
    }
}; // class FilterToCancel

template <class Filter_to_cancel>
class PipelineLauncher {
    tbb::task_group_context &my_ctx;
public:
    PipelineLauncher ( tbb::task_group_context& ctx ) : my_ctx(ctx) {}

    void operator()() const {
        // Run test when serial filter is the first non-input filter
        InputFilter inputFilter;
        Filter_to_cancel filterToCancel;
        tbb::parallel_pipeline(
            g_NumTokens,
            tbb::make_filter<void, void*>(tbb::filter_mode::parallel, inputFilter) &
            tbb::make_filter<void*, void>(tbb::filter_mode::parallel, filterToCancel),
            my_ctx
        );
    }
};

//! Test for cancelling an algorithm from outside (from a task running in parallel with the algorithm).
void TestCancelation1_pipeline () {
    ResetGlobals();
    g_ThrowException = false;
    // Threshold should leave more than max_threads tasks to test the cancellation. Set the threshold to iter_range_size()/4 since iter_range_size >= max_threads*2
    intptr_t threshold = get_iter_range_size() / 4;
    REQUIRE_MESSAGE(get_iter_range_size() - threshold > g_NumThreads, "Threshold should leave more than max_threads tasks to test the cancellation.");
    RunCancellationTest<PipelineLauncher<FilterToCancel>, Cancellator>(threshold);
    g_TGCCancelled.validate(g_NumThreads, "Too many tasks survived cancellation");
    REQUIRE_MESSAGE (g_CurExecuted < g_ExecutedAtLastCatch + g_NumThreads, "Too many tasks were executed after cancellation");
}

class FilterToCancel2  {
public:
    FilterToCancel2() {}

    void* operator()(void* item) const {
        ++g_CurExecuted;
        utils::ConcurrencyTracker ct;
        // The test will hang (and be timed out by the test system) if is_cancelled() is broken
        while( !tbb::is_current_task_group_canceling() )
            utils::yield();
        return item;
    }
};

//! Test for cancelling an algorithm from outside (from a task running in parallel with the algorithm).
/** This version also tests task::is_cancelled() method. **/
void TestCancelation2_pipeline () {
    ResetGlobals();
    RunCancellationTest<PipelineLauncher<FilterToCancel2>, Cancellator2>();
    // g_CurExecuted is always >= g_ExecutedAtLastCatch, because the latter is always a snapshot of the
    // former, and g_CurExecuted is monotonic increasing.  so the comparison should be at least ==.
    // If another filter is started after cancel but before cancellation is propagated, then the
    // number will be larger.
    REQUIRE_MESSAGE (g_CurExecuted <= g_ExecutedAtLastCatch, "Some tasks were executed after cancellation");
}

/** If min and max thread numbers specified on the command line are different,
    the test is run only for 2 sizes of the thread pool (MinThread and MaxThread)
    to be able to test the high and low contention modes while keeping the test reasonably fast **/

//! Testing parallel_pipeline cancellation
//! \brief \ref error_guessing
TEST_CASE("parallel_pipeline cancellation test #1") {
    for (auto concurrency_level: utils::concurrency_range()) {
        g_NumThreads = static_cast<int>(concurrency_level);
        g_Master = std::this_thread::get_id();
        if (g_NumThreads > 1) {
            tbb::task_arena a(g_NumThreads);
            a.execute([] {
                // Execute in all the possible modes
                for (size_t j = 0; j < 4; ++j) {
                    g_ExceptionInMaster = (j & 1) != 0;
                    g_SolitaryException = (j & 2) != 0;
                    g_NumTokens = 2 * g_NumThreads;

                    TestCancelation1_pipeline();
                }
            });
        }
    }
}

//! Testing parallel_pipeline cancellation
//! \brief \ref error_guessing
TEST_CASE("parallel_pipeline cancellation test #2") {
    for (auto concurrency_level: utils::concurrency_range()) {
        g_NumThreads = static_cast<int>(concurrency_level);
        g_Master = std::this_thread::get_id();
        if (g_NumThreads > 1) {
            tbb::task_arena a(g_NumThreads);
            a.execute([] {
                // Execute in all the possible modes
                for (size_t j = 0; j < 4; ++j) {
                    g_ExceptionInMaster = (j & 1) != 0;
                    g_SolitaryException = (j & 2) != 0;
                    g_NumTokens = 2 * g_NumThreads;

                    TestCancelation2_pipeline();
                }
            });
        }
    }
}
