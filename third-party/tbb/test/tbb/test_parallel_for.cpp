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

#include "common/test.h"

#include "tbb/parallel_for.h"

#include "common/config.h"
#include "common/utils.h"
#include "common/utils_concurrency_limit.h"
#include "common/utils_report.h"
#include "common/vector_types.h"
#include "common/cpu_usertime.h"
#include "common/spin_barrier.h"
#include "common/exception_handling.h"
#include "common/concepts_common.h"
#include "test_partitioner.h"

#include <cstddef>
#include <vector>

//! \file test_parallel_for.cpp
//! \brief Test for [algorithms.parallel_for] specification

#if _MSC_VER
#pragma warning (push)
// Suppress conditional expression is constant
#pragma warning (disable: 4127)
#if __TBB_MSVC_UNREACHABLE_CODE_IGNORED
    // Suppress pointless "unreachable code" warning.
    #pragma warning (disable: 4702)
#endif
#if defined(_Wp64)
    // Workaround for overzealous compiler warnings in /Wp64 mode
    #pragma warning (disable: 4267)
#endif
#define _SCL_SECURE_NO_WARNINGS
#endif //#if _MSC_VER


#if (HAVE_m128 || HAVE_m256)
template<typename ClassWithVectorType>
struct SSE_Functor {
    ClassWithVectorType* Src, * Dst;
    SSE_Functor( ClassWithVectorType* src, ClassWithVectorType* dst ) : Src(src), Dst(dst) {}

    void operator()( tbb::blocked_range<int>& r ) const {
        for( int i=r.begin(); i!=r.end(); ++i )
            Dst[i] = Src[i];
    }
};

//! Test that parallel_for works with stack-allocated __m128
template<typename ClassWithVectorType>
void TestVectorTypes() {
    const int aSize = 300;
    ClassWithVectorType Array1[aSize], Array2[aSize];
    for( int i=0; i<aSize; ++i ) {
        // VC8 does not properly align a temporary value; to work around, use explicit variable
        ClassWithVectorType foo(i);
        Array1[i] = foo;
    }
    tbb::parallel_for( tbb::blocked_range<int>(0,aSize), SSE_Functor<ClassWithVectorType>(Array1, Array2) );
    for( int i=0; i<aSize; ++i ) {
        ClassWithVectorType foo(i);
        CHECK( Array2[i]==foo ) ;
    }
}
#endif /* HAVE_m128 || HAVE_m256 */

struct TestSimplePartitionerStabilityFunctor {
  std::vector<int> & ranges;
  TestSimplePartitionerStabilityFunctor(std::vector<int> & theRanges):ranges(theRanges){}
  void operator()(tbb::blocked_range<size_t>& r)const{
      ranges.at(r.begin()) = 1;
  }
};
void TestSimplePartitionerStability(){
    const std::size_t repeat_count= 10;
    const std::size_t rangeToSplitSize=1000000;
    const std::size_t grainsizeStep=rangeToSplitSize/repeat_count;
    typedef TestSimplePartitionerStabilityFunctor FunctorType;

    for (std::size_t i=0 , grainsize=grainsizeStep; i<repeat_count;i++, grainsize+=grainsizeStep){
        std::vector<int> firstSeries(rangeToSplitSize,0);
        std::vector<int> secondSeries(rangeToSplitSize,0);

        tbb::parallel_for(tbb::blocked_range<size_t>(0,rangeToSplitSize,grainsize),FunctorType(firstSeries),tbb::simple_partitioner());
        tbb::parallel_for(tbb::blocked_range<size_t>(0,rangeToSplitSize,grainsize),FunctorType(secondSeries),tbb::simple_partitioner());

        CHECK_MESSAGE(
            firstSeries == secondSeries,
            "Splitting range with tbb::simple_partitioner must be reproducible; i = " << i
        );
    }
}

namespace various_range_implementations {

using namespace test_partitioner_utils;
using namespace test_partitioner_utils::TestRanges;

// Body ensures that initial work distribution is done uniformly through affinity mechanism and not through work stealing
class Body {
    utils::SpinBarrier &m_sb;
public:
    Body(utils::SpinBarrier& sb) : m_sb(sb) { }
    Body(Body& b, tbb::split) : m_sb(b.m_sb) { }

    template <typename Range>
    void operator()(Range& r) const {
        INFO("Executing range [" << r.begin() << ", " << r.end() << "]");
        m_sb.wait(); // waiting for all threads
    }
};

namespace correctness {

/* Testing only correctness (that is parallel_for does not hang) */
template <typename RangeType, bool /* feedback */, bool ensure_non_emptiness>
void test() {
    RangeType range( 0, utils::get_platform_max_threads(), nullptr, false, ensure_non_emptiness );
    tbb::affinity_partitioner ap;
    tbb::parallel_for( range, SimpleBody(), ap );
}

} // namespace correctness

namespace uniform_distribution {

/* Body of parallel_for algorithm would hang if non-uniform work distribution happened  */
template <typename RangeType, bool feedback, bool ensure_non_emptiness>
void test() {
    static const std::size_t thread_num = utils::get_platform_max_threads();
    utils::SpinBarrier sb( thread_num );
    RangeType range(0, thread_num, nullptr, feedback, ensure_non_emptiness);
    const Body sync_body( sb );
    tbb::affinity_partitioner ap;
    tbb::parallel_for( range, sync_body, ap );
    tbb::parallel_for( range, sync_body, tbb::static_partitioner() );
}

} // namespace uniform_distribution

void test() {
    const bool provide_feedback = false;
    const bool ensure_non_empty_range = true;

    // BlockedRange does not take into account feedback and non-emptiness settings but uses the
    // tbb::blocked_range implementation
    uniform_distribution::test<BlockedRange, !provide_feedback, !ensure_non_empty_range>();
    using correctness::test;

    {
        test<RoundedDownRange, provide_feedback, ensure_non_empty_range>();
        test<RoundedDownRange, provide_feedback, !ensure_non_empty_range>();
    }

    {
        test<RoundedUpRange, provide_feedback, ensure_non_empty_range>();
        test<RoundedUpRange, provide_feedback, !ensure_non_empty_range>();
    }

    // Testing that parallel_for algorithm works with such weird ranges
    correctness::test<Range1_2, /* provide_feedback= */ false, !ensure_non_empty_range>();
    correctness::test<Range1_999, /* provide_feedback= */ false, !ensure_non_empty_range>();
    correctness::test<Range999_1, /* provide_feedback= */ false, !ensure_non_empty_range>();

    // The following ranges do not comply with the proportion suggested by partitioner. Therefore
    // they have to provide the proportion in which they were actually split back to partitioner and
    // ensure theirs non-emptiness
    test<Range1_2, provide_feedback, ensure_non_empty_range>();
    test<Range1_999, provide_feedback, ensure_non_empty_range>();
    test<Range999_1, provide_feedback, ensure_non_empty_range>();
}

} // namespace various_range_implementations

namespace test_cancellation {

struct FunctorToCancel {
    static std::atomic<bool> need_to_wait;

    void operator()( std::size_t ) const {
        ++g_CurExecuted;
        if (need_to_wait) {
            need_to_wait = Cancellator::WaitUntilReady();
        }
    }

    void operator()( const tbb::blocked_range<std::size_t>& ) const {
        ++g_CurExecuted;
        Cancellator::WaitUntilReady();
    }

    static void reset() { need_to_wait = true; }
}; // struct FunctorToCancel

std::atomic<bool> FunctorToCancel::need_to_wait(true);

static constexpr std::size_t buffer_test_size = 1024;
static constexpr std::size_t maxParallelForRunnerMode = 14;

template <std::size_t Mode>
class ParallelForRunner {
    tbb::task_group_context& my_ctx;
    const std::size_t worker_task_step = 1;

    static_assert(Mode >= 0 && Mode <= maxParallelForRunnerMode, "Incorrect mode for ParallelForRunner");

    template <typename Partitioner, typename... Args>
    void run_parallel_for( Args&&... args ) const {
        Partitioner part;
        tbb::parallel_for(std::forward<Args>(args)..., part, my_ctx);
    }

    template <typename... Args>
    void run_overload( Args&&... args ) const {

        switch(Mode % 5) {
            case 0 : {
                tbb::parallel_for(std::forward<Args>(args)..., my_ctx);
                break;
            }
            case 1 : {
                run_parallel_for<tbb::simple_partitioner>(std::forward<Args>(args)...);
                break;
            }
            case 2 : {
                run_parallel_for<tbb::auto_partitioner>(std::forward<Args>(args)...);
                break;
            }
            case 3 : {
                run_parallel_for<tbb::static_partitioner>(std::forward<Args>(args)...);
                break;
            }
            case 4 : {
                run_parallel_for<tbb::affinity_partitioner>(std::forward<Args>(args)...);
                break;
            }
        }
    }

public:
    ParallelForRunner( tbb::task_group_context& ctx )
        : my_ctx(ctx) {}

    ~ParallelForRunner() { FunctorToCancel::reset(); }

    void operator()() const {
        if (Mode < 5) {
            // Overload with blocked range
            tbb::blocked_range<std::size_t> br(0, buffer_test_size);
            run_overload(br, FunctorToCancel{});
        } else if (Mode < 10) {
            // Overload with two indexes
            run_overload(std::size_t(0), buffer_test_size, FunctorToCancel{});
        } else {
            // Overload with two indexes and step
            run_overload(std::size_t(0), buffer_test_size, worker_task_step, FunctorToCancel{});
        }
    }
}; // class ParallelForRunner

template <std::size_t Mode>
void run_parallel_for_cancellation_test() {
    // TODO: enable concurrency_range
    if (utils::get_platform_max_threads() < 2) {
        // The test requires at least one worker thread to request cancellation
        return;
    }
    ResetEhGlobals();
    RunCancellationTest<ParallelForRunner<Mode>, Cancellator>();
}

template <std::size_t Mode>
struct ParallelForTestRunner {
    static void run() {
        run_parallel_for_cancellation_test<Mode>();
        ParallelForTestRunner<Mode + 1>::run();
    }
}; // struct ParallelForTestRunner

template <>
struct ParallelForTestRunner<maxParallelForRunnerMode> {
    static void run() {
        run_parallel_for_cancellation_test<maxParallelForRunnerMode>();
    }
}; // struct ParallelForTestRunner<maxParallelForRunnerMode>

} // namespace test_cancellation

#if __TBB_CPP20_CONCEPTS_PRESENT
template <typename... Args>
concept can_call_parallel_for_basic = requires( Args&&... args ) {
    tbb::parallel_for(std::forward<Args>(args)...);
};

template <typename... Args>
concept can_call_parallel_for_helper = can_call_parallel_for_basic<Args...> &&
                                       can_call_parallel_for_basic<Args..., tbb::task_group_context&>;

template <typename... Args>
concept can_call_parallel_for_with_partitioner = can_call_parallel_for_helper<Args...> &&
                                                 can_call_parallel_for_helper<Args..., const tbb::simple_partitioner&> &&
                                                 can_call_parallel_for_helper<Args..., const tbb::auto_partitioner&> &&
                                                 can_call_parallel_for_helper<Args..., const tbb::static_partitioner> &&
                                                 can_call_parallel_for_helper<Args..., tbb::affinity_partitioner&>;

template <typename Range, typename Body>
concept can_call_range_pfor = can_call_parallel_for_with_partitioner<const Range&, const Body&>;

template <typename Index, typename Function>
concept can_call_index_pfor = can_call_parallel_for_with_partitioner<Index, Index, const Function&> &&
                              can_call_parallel_for_with_partitioner<Index, Index, Index, const Function&>;


template <typename Range>
using CorrectBody = test_concepts::parallel_for_body::Correct<Range>;
template <typename Index>
using CorrectFunc = test_concepts::parallel_for_function::Correct<Index>;

void test_pfor_range_constraints() {
    using namespace test_concepts::range;

    static_assert(can_call_range_pfor<Correct, CorrectBody<Correct>>);
    static_assert(!can_call_range_pfor<NonCopyable, CorrectBody<NonCopyable>>);
    static_assert(!can_call_range_pfor<NonSplittable, CorrectBody<NonSplittable>>);
    static_assert(!can_call_range_pfor<NonDestructible, CorrectBody<NonDestructible>>);
    static_assert(!can_call_range_pfor<NoEmpty, CorrectBody<NoEmpty>>);
    static_assert(!can_call_range_pfor<EmptyNonConst, CorrectBody<EmptyNonConst>>);
    static_assert(!can_call_range_pfor<WrongReturnEmpty, CorrectBody<WrongReturnEmpty>>);
    static_assert(!can_call_range_pfor<NoIsDivisible, CorrectBody<NoIsDivisible>>);
    static_assert(!can_call_range_pfor<IsDivisibleNonConst, CorrectBody<IsDivisibleNonConst>>);
    static_assert(!can_call_range_pfor<WrongReturnIsDivisible, CorrectBody<WrongReturnIsDivisible>>);
}

void test_pfor_body_constraints() {
    using namespace test_concepts::parallel_for_body;
    using CorrectRange = test_concepts::range::Correct;

    static_assert(can_call_range_pfor<CorrectRange, Correct<CorrectRange>>);
    static_assert(!can_call_range_pfor<CorrectRange, NonCopyable<CorrectRange>>);
    static_assert(!can_call_range_pfor<CorrectRange, NonDestructible<CorrectRange>>);
    static_assert(!can_call_range_pfor<CorrectRange, NoOperatorRoundBrackets<CorrectRange>>);
    static_assert(!can_call_range_pfor<CorrectRange, OperatorRoundBracketsNonConst<CorrectRange>>);
    static_assert(!can_call_range_pfor<CorrectRange, WrongInputOperatorRoundBrackets<CorrectRange>>);
}

void test_pfor_func_constraints() {
    using namespace test_concepts::parallel_for_function;
    using CorrectIndex = test_concepts::parallel_for_index::Correct;

    static_assert(can_call_index_pfor<CorrectIndex, Correct<CorrectIndex>>);
    static_assert(!can_call_index_pfor<CorrectIndex, NoOperatorRoundBrackets<CorrectIndex>>);
    static_assert(!can_call_index_pfor<CorrectIndex, OperatorRoundBracketsNonConst<CorrectIndex>>);
    static_assert(!can_call_index_pfor<CorrectIndex, WrongInputOperatorRoundBrackets<CorrectIndex>>);
}

void test_pfor_index_constraints() {
    using namespace test_concepts::parallel_for_index;
    static_assert(can_call_index_pfor<Correct, CorrectFunc<Correct>>);
    static_assert(!can_call_index_pfor<NoIntCtor, CorrectFunc<NoIntCtor>>);
    static_assert(!can_call_index_pfor<NonCopyable, CorrectFunc<NonCopyable>>);
    static_assert(!can_call_index_pfor<NonCopyAssignable, CorrectFunc<NonCopyAssignable>>);
    static_assert(!can_call_index_pfor<NonDestructible, CorrectFunc<NonDestructible>>);
    static_assert(!can_call_index_pfor<NoOperatorLess, CorrectFunc<NoOperatorLess>>);
    static_assert(!can_call_index_pfor<OperatorLessNonConst, CorrectFunc<OperatorLessNonConst>>);
    static_assert(!can_call_index_pfor<WrongInputOperatorLess, CorrectFunc<WrongInputOperatorLess>>);
    static_assert(!can_call_index_pfor<WrongReturnOperatorLess, CorrectFunc<WrongReturnOperatorLess>>);
    static_assert(!can_call_index_pfor<NoOperatorMinus, CorrectFunc<NoOperatorMinus>>);
    static_assert(!can_call_index_pfor<OperatorMinusNonConst, CorrectFunc<OperatorMinusNonConst>>);
    static_assert(!can_call_index_pfor<WrongInputOperatorMinus, CorrectFunc<WrongInputOperatorMinus>>);
    static_assert(!can_call_index_pfor<WrongReturnOperatorMinus, CorrectFunc<WrongReturnOperatorMinus>>);
    static_assert(!can_call_index_pfor<NoOperatorPlus, CorrectFunc<NoOperatorPlus>>);
    static_assert(!can_call_index_pfor<OperatorPlusNonConst, CorrectFunc<OperatorPlusNonConst>>);
    static_assert(!can_call_index_pfor<WrongInputOperatorPlus, CorrectFunc<WrongInputOperatorPlus>>);
    static_assert(!can_call_index_pfor<WrongReturnOperatorPlus, CorrectFunc<WrongReturnOperatorPlus>>);
}
#endif // __TBB_CPP20_CONCEPTS_PRESENT

#if TBB_USE_EXCEPTIONS && !__TBB_THROW_ACROSS_MODULE_BOUNDARY_BROKEN && TBB_REVAMP_TODO
#include "tbb/global_control.h"
//! Testing exceptions
//! \brief \ref requirement
TEST_CASE("Exceptions support") {
    for ( int p = MinThread; p <= MaxThread; ++p ) {
        if ( p > 0 ) {
            tbb::global_control control(tbb::global_control::max_allowed_parallelism, p);
            TestExceptionsSupport();
        }
    }
}
#endif /* TBB_USE_EXCEPTIONS && !__TBB_THROW_ACROSS_MODULE_BOUNDARY_BROKEN */

//! Testing cancellation
//! \brief \ref error_guessing
TEST_CASE("Vector types") {
#if HAVE_m128
    TestVectorTypes<ClassWithSSE>();
#endif
#if HAVE_m256
    if (have_AVX()) TestVectorTypes<ClassWithAVX>();
#endif
}

//! Testing workers going to sleep
//! \brief \ref resource_usage
TEST_CASE("That all workers sleep when no work") {
    const std::size_t N = 100000;
    std::atomic<int> counter{};

    tbb::parallel_for(std::size_t(0), N, [&](std::size_t) {
        for (int i = 0; i < 1000; ++i) {
            ++counter;
        }
    }, tbb::simple_partitioner());
    TestCPUUserTime(utils::get_platform_max_threads());
}

//! Testing simple partitioner stability
//! \brief \ref error_guessing
TEST_CASE("Simple partitioner stability") {
    TestSimplePartitionerStability();
}

//! Testing various range implementations
//! \brief \ref requirement
TEST_CASE("Various range implementations") {
    various_range_implementations::test();
}

//! Testing parallel_for with explicit task_group_context
//! \brief \ref interface \ref error_guessing
TEST_CASE("Сancellation test for tbb::parallel_for") {
    test_cancellation::ParallelForTestRunner</*FirstMode = */0>::run();
}

#if __TBB_CPP20_CONCEPTS_PRESENT
//! \brief \ref error_guessing
TEST_CASE("parallel_for constraints") {
    test_pfor_range_constraints();
    test_pfor_body_constraints();
    test_pfor_func_constraints();
    test_pfor_index_constraints();
}
#endif // __TBB_CPP20_CONCEPTS_PRESENT

#if _MSC_VER
#pragma warning (pop)
#endif
