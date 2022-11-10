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

#include <atomic>

#include "common/parallel_reduce_common.h"
#include "common/cpu_usertime.h"
#include "common/exception_handling.h"
#include "common/concepts_common.h"

//! \file test_parallel_reduce.cpp
//! \brief Test for [algorithms.parallel_reduce algorithms.parallel_deterministic_reduce] specification

#if _MSC_VER
#pragma warning (push)
// Suppress conditional expression is constant
#pragma warning (disable: 4127)
#endif //#if _MSC_VER

using ValueType = uint64_t;

struct Sum {
    template<typename T>
    T operator() ( const T& v1, const T& v2 ) const {
        return v1 + v2;
    }
};

struct Accumulator {
    ValueType operator() ( const tbb::blocked_range<ValueType*>& r, ValueType value ) const {
        for ( ValueType* pv = r.begin(); pv != r.end(); ++pv )
            value += *pv;
        return value;
    }
};

class ParallelSumTester {
public:
    ParallelSumTester( const ParallelSumTester& ) = default;
    void operator=( const ParallelSumTester& ) = delete;

    ParallelSumTester() : m_range(nullptr, nullptr) {
        m_array = new ValueType[unsigned(count)];
        for ( ValueType i = 0; i < count; ++i )
            m_array[i] = i + 1;
        m_range = tbb::blocked_range<ValueType*>( m_array, m_array + count );
    }
    ~ParallelSumTester() { delete[] m_array; }

    template<typename Partitioner>
    void CheckParallelReduce() {
        Partitioner partitioner;
        ValueType result1 = reduce_invoker<ValueType>( m_range, Accumulator(), Sum(), partitioner );
        REQUIRE_MESSAGE( result1 == expected, "Wrong parallel summation result" );
        ValueType result2 = reduce_invoker<ValueType>( m_range,
            [](const tbb::blocked_range<ValueType*>& r, ValueType value) -> ValueType {
                for ( const ValueType* pv = r.begin(); pv != r.end(); ++pv )
                    value += *pv;
                return value;
            },
            Sum(),
            partitioner
        );
        REQUIRE_MESSAGE( result2 == expected, "Wrong parallel summation result" );
    }
private:
    ValueType* m_array;
    tbb::blocked_range<ValueType*> m_range;
    static const ValueType count, expected;
};

const ValueType ParallelSumTester::count = 1000000;
const ValueType ParallelSumTester::expected = count * (count + 1) / 2;

namespace test_cancellation {

struct ReduceToCancel {
    std::size_t operator()( const tbb::blocked_range<std::size_t>&, std::size_t ) const {
        ++g_CurExecuted;
        Cancellator::WaitUntilReady();
        return 1;
    }
}; // struct ReduceToCancel

struct JoinToCancel {
    std::size_t operator()( std::size_t, std::size_t ) const {
        ++g_CurExecuted;
        Cancellator::WaitUntilReady();
        return 1;
    }
}; // struct Join

struct ReduceFunctorToCancel {
    std::size_t result;

    ReduceFunctorToCancel() : result(0) {}
    ReduceFunctorToCancel( ReduceFunctorToCancel&, tbb::split ) : result(0) {}

    void operator()( const tbb::blocked_range<std::size_t>& br ) {
        result = ReduceToCancel{}(br, result);
    }

    void join( ReduceFunctorToCancel& rhs ) {
        result = JoinToCancel{}(result, rhs.result);
    }
}; // struct ReduceFunctorToCancel

static constexpr std::size_t buffer_test_size = 1024;
static constexpr std::size_t maxParallelReduceRunnerMode = 9;

template <std::size_t Mode>
class ParallelReduceRunner {
    tbb::task_group_context& my_ctx;

    static_assert(Mode >= 0 && Mode <= maxParallelReduceRunnerMode, "Incorrect mode for ParallelReduceTask");

    template <typename... Args>
    void run_parallel_reduce( Args&&... args ) const {
        switch(Mode % 5) {
            case 0 : {
                tbb::parallel_reduce(std::forward<Args>(args)..., my_ctx);
                break;
            }
            case 1 : {
                tbb::parallel_reduce(std::forward<Args>(args)..., tbb::simple_partitioner{}, my_ctx);
                break;
            }
            case 2 : {
                tbb::parallel_reduce(std::forward<Args>(args)..., tbb::auto_partitioner{}, my_ctx);
                break;
            }
            case 3 : {
                tbb::parallel_reduce(std::forward<Args>(args)..., tbb::static_partitioner{}, my_ctx);
                break;
            }
            case 4 : {
                tbb::affinity_partitioner aff;
                tbb::parallel_reduce(std::forward<Args>(args)..., aff, my_ctx);
                break;
            }
        }
    }

public:
    ParallelReduceRunner( tbb::task_group_context& ctx )
        : my_ctx(ctx) {}

    void operator()() const {
        tbb::blocked_range<std::size_t> br(0, buffer_test_size);
        if (Mode < 5) {
            ReduceFunctorToCancel functor;
            run_parallel_reduce(br, functor);
        } else {
            run_parallel_reduce(br, std::size_t(0), ReduceToCancel{}, JoinToCancel{});
        }
    }
}; // class ParallelReduceRunner

static constexpr std::size_t maxParallelDeterministicReduceRunnerMode = 5;

// TODO: unify with ParallelReduceRunner
template <std::size_t Mode>
class ParallelDeterministicReduceRunner {
    tbb::task_group_context& my_ctx;

    static_assert(Mode >= 0 && Mode <= maxParallelDeterministicReduceRunnerMode, "Incorrect Mode for deterministic_reduce task");

    template <typename... Args>
    void run_parallel_deterministic_reduce( Args&&... args ) const {
        switch(Mode % 3) {
            case 0 : {
                tbb::parallel_deterministic_reduce(std::forward<Args>(args)..., my_ctx);
                break;
            }
            case 1 : {
                tbb::parallel_deterministic_reduce(std::forward<Args>(args)..., tbb::simple_partitioner{}, my_ctx);
                break;
            }
            case 2 : {
                tbb::parallel_deterministic_reduce(std::forward<Args>(args)..., tbb::static_partitioner{}, my_ctx);
                break;
            }
        }
    }

public:
    ParallelDeterministicReduceRunner( tbb::task_group_context& ctx )
        : my_ctx(ctx) {}

    void operator()() const {
        tbb::blocked_range<std::size_t> br(0, buffer_test_size);
        if (Mode < 3) {
            ReduceFunctorToCancel functor;
            run_parallel_deterministic_reduce(br, functor);
        } else {
            run_parallel_deterministic_reduce(br, std::size_t(0), ReduceToCancel{}, JoinToCancel{});
        }
    }
}; // class ParallelDeterministicReduceRunner

template <std::size_t Mode>
void run_parallel_reduce_cancellation_test() {
    for ( auto concurrency_level : utils::concurrency_range() ) {
        if (concurrency_level < 2) continue;

        tbb::global_control gc(tbb::global_control::max_allowed_parallelism, concurrency_level);
        ResetEhGlobals();
        RunCancellationTest<ParallelReduceRunner<Mode>, Cancellator>();
    }
}

template <std::size_t Mode>
void run_parallel_deterministic_reduce_cancellation_test() {
    for ( auto concurrency_level : utils::concurrency_range() ) {
        if (concurrency_level < 2) continue;

        tbb::global_control gc(tbb::global_control::max_allowed_parallelism, concurrency_level);
        ResetEhGlobals();
        RunCancellationTest<ParallelDeterministicReduceRunner<Mode>, Cancellator>();
    }
}

template <std::size_t Mode>
struct ParallelReduceTestRunner {
    static void run() {
        run_parallel_reduce_cancellation_test<Mode>();
        ParallelReduceTestRunner<Mode + 1>::run();
    }
}; // struct ParallelReduceTestRunner

template <>
struct ParallelReduceTestRunner<maxParallelReduceRunnerMode> {
    static void run() {
        run_parallel_reduce_cancellation_test<maxParallelReduceRunnerMode>();
    }
}; // struct ParallelReduceTestRunner<maxParallelReduceRunnerMode>

template <std::size_t Mode>
struct ParallelDeterministicReduceTestRunner {
    static void run() {
        run_parallel_deterministic_reduce_cancellation_test<Mode>();
        ParallelDeterministicReduceTestRunner<Mode + 1>::run();
    }
}; // struct ParallelDeterministicReduceTestRunner

template <>
struct ParallelDeterministicReduceTestRunner<maxParallelDeterministicReduceRunnerMode> {
    static void run() {
        run_parallel_deterministic_reduce_cancellation_test<maxParallelDeterministicReduceRunnerMode>();
    }
}; // struct ParallelDeterministicReduceTestRunner<maxParallelDeterministicReduceRunnerMode>

} // namespace test_cancellation

#if __TBB_CPP20_CONCEPTS_PRESENT
template <typename... Args>
concept can_call_parallel_reduce_basic = requires( Args&&... args ) {
    tbb::parallel_reduce(std::forward<Args>(args)...);
};

template <typename... Args>
concept can_call_parallel_deterministic_reduce_basic = requires ( Args&&... args ) {
    tbb::parallel_deterministic_reduce(std::forward<Args>(args)...);
};

template <typename... Args>
concept can_call_preduce_helper = can_call_parallel_reduce_basic<Args...> &&
                                  can_call_parallel_reduce_basic<Args..., tbb::task_group_context&>;

template <typename... Args>
concept can_call_pdet_reduce_helper = can_call_parallel_deterministic_reduce_basic<Args...> &&
                                      can_call_parallel_deterministic_reduce_basic<Args..., tbb::task_group_context&>;

template <typename... Args>
concept can_call_preduce_with_partitioner = can_call_preduce_helper<Args...> &&
                                            can_call_preduce_helper<Args..., const tbb::simple_partitioner&> &&
                                            can_call_preduce_helper<Args..., const tbb::auto_partitioner&> &&
                                            can_call_preduce_helper<Args..., const tbb::static_partitioner&> &&
                                            can_call_preduce_helper<Args..., tbb::affinity_partitioner&>;

template <typename... Args>
concept can_call_pdet_reduce_with_partitioner = can_call_pdet_reduce_helper<Args...> &&
                                                can_call_pdet_reduce_helper<Args..., const tbb::simple_partitioner&> &&
                                                can_call_pdet_reduce_helper<Args..., const tbb::static_partitioner&>;

template <typename Range, typename Body>
concept can_call_imperative_preduce = can_call_preduce_with_partitioner<const Range&, Body&>;

template <typename Range, typename Body>
concept can_call_imperative_pdet_reduce = can_call_pdet_reduce_with_partitioner<const Range&, Body&>;

template <typename Range, typename Value, typename RealBody, typename Reduction>
concept can_call_functional_preduce = can_call_preduce_with_partitioner<const Range&, const Value&,
                                                                        const RealBody&, const Reduction&>;

template <typename Range, typename Value, typename RealBody, typename Reduction>
concept can_call_functional_pdet_reduce = can_call_pdet_reduce_with_partitioner<const Range&, const Value&,
                                                                                const RealBody&, const Reduction&>;

template <typename Range>
using CorrectBody = test_concepts::parallel_reduce_body::Correct<Range>;

template <typename Range>
using CorrectFunc = test_concepts::parallel_reduce_function::Correct<Range>;

using CorrectReduction = test_concepts::parallel_reduce_combine::Correct<int>;
using CorrectRange = test_concepts::range::Correct;

void test_preduce_range_constraints() {
    using namespace test_concepts::range;
    static_assert(can_call_imperative_preduce<Correct, CorrectBody<Correct>>);
    static_assert(!can_call_imperative_preduce<NonCopyable, CorrectBody<NonCopyable>>);
    static_assert(!can_call_imperative_preduce<NonDestructible, CorrectBody<NonDestructible>>);
    static_assert(!can_call_imperative_preduce<NonSplittable, CorrectBody<NonSplittable>>);
    static_assert(!can_call_imperative_preduce<NoEmpty, CorrectBody<NoEmpty>>);
    static_assert(!can_call_imperative_preduce<EmptyNonConst, CorrectBody<EmptyNonConst>>);
    static_assert(!can_call_imperative_preduce<WrongReturnEmpty, CorrectBody<WrongReturnEmpty>>);
    static_assert(!can_call_imperative_preduce<NoIsDivisible, CorrectBody<NoIsDivisible>>);
    static_assert(!can_call_imperative_preduce<IsDivisibleNonConst, CorrectBody<NoIsDivisible>>);
    static_assert(!can_call_imperative_preduce<WrongReturnIsDivisible, CorrectBody<WrongReturnIsDivisible>>);

    static_assert(can_call_functional_preduce<Correct, int, CorrectFunc<Correct>, CorrectReduction>);
    static_assert(!can_call_functional_preduce<NonCopyable, int, CorrectFunc<NonCopyable>, CorrectReduction>);
    static_assert(!can_call_functional_preduce<NonDestructible, int, CorrectFunc<NonDestructible>, CorrectReduction>);
    static_assert(!can_call_functional_preduce<NonSplittable, int, CorrectFunc<NonSplittable>, CorrectReduction>);
    static_assert(!can_call_functional_preduce<NoEmpty, int, CorrectFunc<NoEmpty>, CorrectReduction>);
    static_assert(!can_call_functional_preduce<EmptyNonConst, int, CorrectFunc<EmptyNonConst>, CorrectReduction>);
    static_assert(!can_call_functional_preduce<WrongReturnEmpty, int, CorrectFunc<WrongReturnEmpty>, CorrectReduction>);
    static_assert(!can_call_functional_preduce<NoIsDivisible, int, CorrectFunc<NoIsDivisible>, CorrectReduction>);
    static_assert(!can_call_functional_preduce<IsDivisibleNonConst, int, CorrectFunc<IsDivisibleNonConst>, CorrectReduction>);
    static_assert(!can_call_functional_preduce<WrongReturnIsDivisible, int, CorrectFunc<WrongReturnIsDivisible>, CorrectReduction>);
}

void test_preduce_body_constraints() {
    using namespace test_concepts::parallel_reduce_body;
    static_assert(can_call_imperative_preduce<CorrectRange, Correct<CorrectRange>>);
    static_assert(!can_call_imperative_preduce<CorrectRange, NonSplittable<CorrectRange>>);
    static_assert(!can_call_imperative_preduce<CorrectRange, NonDestructible<CorrectRange>>);
    static_assert(!can_call_imperative_preduce<CorrectRange, NoOperatorRoundBrackets<CorrectRange>>);
    static_assert(!can_call_imperative_preduce<CorrectRange, WrongInputOperatorRoundBrackets<CorrectRange>>);
    static_assert(!can_call_imperative_preduce<CorrectRange, NoJoin<CorrectRange>>);
    static_assert(!can_call_imperative_preduce<CorrectRange, WrongInputJoin<CorrectRange>>);
}

void test_preduce_func_constraints() {
    using namespace test_concepts::parallel_reduce_function;
    static_assert(can_call_functional_preduce<CorrectRange, int, Correct<CorrectRange>, CorrectReduction>);
    static_assert(!can_call_functional_preduce<CorrectRange, int, NoOperatorRoundBrackets<CorrectRange>, CorrectReduction>);
    static_assert(!can_call_functional_preduce<CorrectRange, int, OperatorRoundBracketsNonConst<CorrectRange>, CorrectReduction>);
    static_assert(!can_call_functional_preduce<CorrectRange, int, WrongFirstInputOperatorRoundBrackets<CorrectRange>, CorrectReduction>);
    static_assert(!can_call_functional_preduce<CorrectRange, int, WrongSecondInputOperatorRoundBrackets<CorrectRange>, CorrectReduction>);
    static_assert(!can_call_functional_preduce<CorrectRange, int, WrongReturnOperatorRoundBrackets<CorrectRange>, CorrectReduction>);
}

void test_preduce_combine_constraints() {
    using namespace test_concepts::parallel_reduce_combine;
    static_assert(can_call_functional_preduce<CorrectRange, int, CorrectFunc<CorrectRange>, Correct<int>>);
    static_assert(!can_call_functional_preduce<CorrectRange, int, CorrectFunc<CorrectRange>, NoOperatorRoundBrackets<int>>);
    static_assert(!can_call_functional_preduce<CorrectRange, int, CorrectFunc<CorrectRange>, OperatorRoundBracketsNonConst<int>>);
    static_assert(!can_call_functional_preduce<CorrectRange, int, CorrectFunc<CorrectRange>, WrongFirstInputOperatorRoundBrackets<int>>);
    static_assert(!can_call_functional_preduce<CorrectRange, int, CorrectFunc<CorrectRange>, WrongSecondInputOperatorRoundBrackets<int>>);
    static_assert(!can_call_functional_preduce<CorrectRange, int, CorrectFunc<CorrectRange>, WrongReturnOperatorRoundBrackets<int>>);
}

void test_pdet_reduce_range_constraints() {
    using namespace test_concepts::range;
    static_assert(can_call_imperative_pdet_reduce<Correct, CorrectBody<Correct>>);
    static_assert(!can_call_imperative_pdet_reduce<NonCopyable, CorrectBody<NonCopyable>>);
    static_assert(!can_call_imperative_pdet_reduce<NonDestructible, CorrectBody<NonDestructible>>);
    static_assert(!can_call_imperative_pdet_reduce<NonSplittable, CorrectBody<NonSplittable>>);
    static_assert(!can_call_imperative_pdet_reduce<NoEmpty, CorrectBody<NoEmpty>>);
    static_assert(!can_call_imperative_pdet_reduce<EmptyNonConst, CorrectBody<EmptyNonConst>>);
    static_assert(!can_call_imperative_pdet_reduce<WrongReturnEmpty, CorrectBody<WrongReturnEmpty>>);
    static_assert(!can_call_imperative_pdet_reduce<NoIsDivisible, CorrectBody<NoIsDivisible>>);
    static_assert(!can_call_imperative_pdet_reduce<IsDivisibleNonConst, CorrectBody<NoIsDivisible>>);
    static_assert(!can_call_imperative_pdet_reduce<WrongReturnIsDivisible, CorrectBody<WrongReturnIsDivisible>>);

    static_assert(can_call_functional_pdet_reduce<Correct, int, CorrectFunc<Correct>, CorrectReduction>);
    static_assert(!can_call_functional_pdet_reduce<NonCopyable, int, CorrectFunc<NonCopyable>, CorrectReduction>);
    static_assert(!can_call_functional_pdet_reduce<NonDestructible, int, CorrectFunc<NonDestructible>, CorrectReduction>);
    static_assert(!can_call_functional_pdet_reduce<NonSplittable, int, CorrectFunc<NonSplittable>, CorrectReduction>);
    static_assert(!can_call_functional_pdet_reduce<NoEmpty, int, CorrectFunc<NoEmpty>, CorrectReduction>);
    static_assert(!can_call_functional_pdet_reduce<EmptyNonConst, int, CorrectFunc<EmptyNonConst>, CorrectReduction>);
    static_assert(!can_call_functional_pdet_reduce<WrongReturnEmpty, int, CorrectFunc<WrongReturnEmpty>, CorrectReduction>);
    static_assert(!can_call_functional_pdet_reduce<NoIsDivisible, int, CorrectFunc<NoIsDivisible>, CorrectReduction>);
    static_assert(!can_call_functional_pdet_reduce<IsDivisibleNonConst, int, CorrectFunc<IsDivisibleNonConst>, CorrectReduction>);
    static_assert(!can_call_functional_pdet_reduce<WrongReturnIsDivisible, int, CorrectFunc<WrongReturnIsDivisible>, CorrectReduction>);
}

void test_pdet_reduce_body_constraints() {
    using namespace test_concepts::parallel_reduce_body;
    static_assert(can_call_imperative_pdet_reduce<CorrectRange, Correct<CorrectRange>>);
    static_assert(!can_call_imperative_pdet_reduce<CorrectRange, NonSplittable<CorrectRange>>);
    static_assert(!can_call_imperative_pdet_reduce<CorrectRange, NonDestructible<CorrectRange>>);
    static_assert(!can_call_imperative_pdet_reduce<CorrectRange, NoOperatorRoundBrackets<CorrectRange>>);
    static_assert(!can_call_imperative_pdet_reduce<CorrectRange, WrongInputOperatorRoundBrackets<CorrectRange>>);
    static_assert(!can_call_imperative_pdet_reduce<CorrectRange, NoJoin<CorrectRange>>);
    static_assert(!can_call_imperative_pdet_reduce<CorrectRange, WrongInputJoin<CorrectRange>>);
}

void test_pdet_reduce_func_constraints() {
    using namespace test_concepts::parallel_reduce_function;
    static_assert(can_call_functional_pdet_reduce<CorrectRange, int, Correct<CorrectRange>, CorrectReduction>);
    static_assert(!can_call_functional_pdet_reduce<CorrectRange, int, NoOperatorRoundBrackets<CorrectRange>, CorrectReduction>);
    static_assert(!can_call_functional_pdet_reduce<CorrectRange, int, OperatorRoundBracketsNonConst<CorrectRange>, CorrectReduction>);
    static_assert(!can_call_functional_pdet_reduce<CorrectRange, int, WrongFirstInputOperatorRoundBrackets<CorrectRange>, CorrectReduction>);
    static_assert(!can_call_functional_pdet_reduce<CorrectRange, int, WrongSecondInputOperatorRoundBrackets<CorrectRange>, CorrectReduction>);
    static_assert(!can_call_functional_pdet_reduce<CorrectRange, int, WrongReturnOperatorRoundBrackets<CorrectRange>, CorrectReduction>);
}

void test_pdet_reduce_combine_constraints() {
    using namespace test_concepts::parallel_reduce_combine;
    static_assert(can_call_functional_pdet_reduce<CorrectRange, int, CorrectFunc<CorrectRange>, Correct<int>>);
    static_assert(!can_call_functional_pdet_reduce<CorrectRange, int, CorrectFunc<CorrectRange>, NoOperatorRoundBrackets<int>>);
    static_assert(!can_call_functional_pdet_reduce<CorrectRange, int, CorrectFunc<CorrectRange>, OperatorRoundBracketsNonConst<int>>);
    static_assert(!can_call_functional_pdet_reduce<CorrectRange, int, CorrectFunc<CorrectRange>, WrongFirstInputOperatorRoundBrackets<int>>);
    static_assert(!can_call_functional_pdet_reduce<CorrectRange, int, CorrectFunc<CorrectRange>, WrongSecondInputOperatorRoundBrackets<int>>);
    static_assert(!can_call_functional_pdet_reduce<CorrectRange, int, CorrectFunc<CorrectRange>, WrongReturnOperatorRoundBrackets<int>>);
}
#endif // __TBB_CPP20_CONCEPTS_PRESENT

//! Test parallel summation correctness
//! \brief \ref stress
TEST_CASE("Test parallel summation correctness") {
    ParallelSumTester pst;
    pst.CheckParallelReduce<utils_default_partitioner>();
    pst.CheckParallelReduce<tbb::simple_partitioner>();
    pst.CheckParallelReduce<tbb::auto_partitioner>();
    pst.CheckParallelReduce<tbb::affinity_partitioner>();
    pst.CheckParallelReduce<tbb::static_partitioner>();
}

static std::atomic<long> ForkCount;
static std::atomic<long> FooBodyCount;

//! Class with public interface that is exactly minimal requirements for Range concept
class MinimalRange {
    size_t begin, end;
    friend class FooBody;
    explicit MinimalRange( size_t i ) : begin(0), end(i) {}
    template <typename Partitioner_> friend void TestSplitting( std::size_t nthread );
public:
    MinimalRange( MinimalRange& r, tbb::split ) : end(r.end) {
        begin = r.end = (r.begin+r.end)/2;
    }
    bool is_divisible() const {return end-begin>=2;}
    bool empty() const {return begin==end;}
};

//! Class with public interface that is exactly minimal requirements for Body of a parallel_reduce
class FooBody {
private:
    FooBody( const FooBody& );          // Deny access
    void operator=( const FooBody& );   // Deny access
    template <typename Partitioner_> friend void TestSplitting( std::size_t nthread );
    //! Parent that created this body via split operation.  nullptr if original body.
    FooBody* parent;
    //! Total number of index values processed by body and its children.
    size_t sum;
    //! Range that has been processed so far by this body and its children.
    size_t begin, end;
    //! True if body has not yet been processed at least once by operator().
    bool is_new;
    //! 1 if body was created by split; 0 if original body.
    int forked;
    FooBody() {++FooBodyCount;}
public:
    ~FooBody() {
        forked = 0xDEADBEEF;
        sum=0xDEADBEEF;
        --FooBodyCount;
    }
    FooBody( FooBody& other, tbb::split ) {
        ++FooBodyCount;
        ++ForkCount;
        sum = 0;
        parent = &other;
        is_new = true;
        forked = 1;
    }

    void init() {
        sum = 0;
        parent = nullptr;
        is_new = true;
        forked = 0;
        begin = ~size_t(0);
        end = ~size_t(0);
    }

    void join( FooBody& s ) {
        REQUIRE( s.forked==1 );
        REQUIRE( this!=&s );
        REQUIRE( this==s.parent );
        REQUIRE( end==s.begin );
        end = s.end;
        sum += s.sum;
        s.forked = 2;
    }
    void operator()( const MinimalRange& r ) {
        for( size_t k=r.begin; k<r.end; ++k )
            ++sum;
        if( is_new ) {
            is_new = false;
            begin = r.begin;
        } else
            REQUIRE( end==r.begin );
        end = r.end;
    }
};

template<typename Partitioner>
void TestSplitting( std::size_t nthread ) {
    ForkCount = 0;
    Partitioner partitioner;
    for( size_t i=0; i<=1000; ++i ) {
        FooBody f;
        f.init();
        REQUIRE_MESSAGE( FooBodyCount==1, "Wrong initial BodyCount value" );
        reduce_invoker(MinimalRange(i), f, partitioner);

        if (nthread == 1) REQUIRE_MESSAGE(ForkCount==0, "Body was split during 1 thread execution");

        REQUIRE_MESSAGE( FooBodyCount==1, "Some copies of FooBody was not removed after reduction");
        REQUIRE_MESSAGE( f.sum==i, "Incorrect reduction" );
        REQUIRE_MESSAGE( f.begin==(i==0 ? ~size_t(0) : 0), "Incorrect range borders" );
        REQUIRE_MESSAGE( f.end==(i==0 ? ~size_t(0) : i), "Incorrect range borders" );
    }
}

//! Test splitting range and body during reduction, test that all workers sleep when no work
//! \brief \ref resource_usage \ref error_guessing
TEST_CASE("Test splitting range and body during reduction, test that all workers sleep when no work") {
    for ( auto concurrency_level : utils::concurrency_range() ) {
        tbb::global_control control(tbb::global_control::max_allowed_parallelism, concurrency_level);

        TestSplitting<tbb::simple_partitioner>(concurrency_level);
        TestSplitting<tbb::static_partitioner>(concurrency_level);
        TestSplitting<tbb::auto_partitioner>(concurrency_level);
        TestSplitting<tbb::affinity_partitioner>(concurrency_level);
        TestSplitting<utils_default_partitioner>(concurrency_level);

        // Test that all workers sleep when no work
        TestCPUUserTime(concurrency_level);
    }
}

//! Define overloads of parallel_deterministic_reduce that accept "undesired" types of partitioners
namespace unsupported {
    template<typename Range, typename Body>
    void parallel_deterministic_reduce(const Range&, Body&, const tbb::auto_partitioner&) { }
    template<typename Range, typename Body>
    void parallel_deterministic_reduce(const Range&, Body&, tbb::affinity_partitioner&) { }

    template<typename Range, typename Value, typename RealBody, typename Reduction>
    Value parallel_deterministic_reduce(const Range& , const Value& identity, const RealBody& , const Reduction& , const tbb::auto_partitioner&) {
        return identity;
    }
    template<typename Range, typename Value, typename RealBody, typename Reduction>
    Value parallel_deterministic_reduce(const Range& , const Value& identity, const RealBody& , const Reduction& , tbb::affinity_partitioner&) {
        return identity;
    }
}

struct Body {
    float value;
    Body() : value(0) {}
    Body(Body&, tbb::split) { value = 0; }
    void operator()(const tbb::blocked_range<int>&) {}
    void join(Body&) {}
};

//! Check that other types of partitioners are not supported (auto, affinity)
//! In the case of "unsupported" API unexpectedly sneaking into namespace tbb,
//! this test should result in a compilation error due to overload resolution ambiguity
//! \brief \ref negative \ref error_guessing
TEST_CASE("Test Unsupported Partitioners") {
    using namespace tbb;
    using namespace unsupported;
    Body body;
    parallel_deterministic_reduce(blocked_range<int>(0, 10), body, tbb::auto_partitioner());

    tbb::affinity_partitioner ap;
    parallel_deterministic_reduce(blocked_range<int>(0, 10), body, ap);

    parallel_deterministic_reduce(
        blocked_range<int>(0, 10),
        0,
        [](const blocked_range<int>&, int init)->int {
            return init;
        },
        [](int x, int y)->int {
            return x + y;
        },
        tbb::auto_partitioner()
    );
    parallel_deterministic_reduce(
        blocked_range<int>(0, 10),
        0,
        [](const blocked_range<int>&, int init)->int {
            return init;
        },
        [](int x, int y)->int {
            return x + y;
        },
        ap
    );
}

//! Testing tbb::parallel_reduce with tbb::task_group_context
//! \brief \ref interface \ref error_guessing
TEST_CASE("cancellation test for tbb::parallel_reduce") {
    test_cancellation::ParallelReduceTestRunner</*First mode = */0>::run();
}

//! Testing tbb::parallel_deterministic_reduce with tbb::task_group_context
//! \brief \ref interface \ref error_guessing
TEST_CASE("cancellation test for tbb::parallel_deterministic_reduce") {
    test_cancellation::ParallelDeterministicReduceTestRunner</*First mode = */0>::run();
}

#if __TBB_CPP20_CONCEPTS_PRESENT
//! \brief \ref error_guessing
TEST_CASE("parallel_reduce constraints") {
    test_preduce_range_constraints();
    test_preduce_body_constraints();
    test_preduce_func_constraints();
    test_preduce_combine_constraints();
}

//! \brief \ref error_guessing
TEST_CASE("parallel_deterministic_reduce constraints") {
    test_pdet_reduce_range_constraints();
    test_pdet_reduce_body_constraints();
    test_pdet_reduce_func_constraints();
    test_pdet_reduce_combine_constraints();
}
#endif

#if _MSC_VER
#pragma warning (pop)
#endif
