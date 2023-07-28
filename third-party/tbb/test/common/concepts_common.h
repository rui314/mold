/*
    Copyright (c) 2021 Intel Corporation

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

#ifndef __TBB_test_common_concepts_common_H
#define __TBB_test_common_concepts_common_H
#include "tbb/parallel_pipeline.h"
#include "tbb/parallel_for_each.h"
#include "tbb/flow_graph.h"
#include "tbb/parallel_scan.h"
#include "iterator.h"
#include <vector>

#if __TBB_CPP20_CONCEPTS_PRESENT

namespace test_concepts {

struct Dummy {};

enum class State {
    correct,
    incorrect_first_input,
    incorrect_second_input,
    incorrect_third_input,
    incorrect_return_type,
    incorrect_constness,
    not_defined,
    incorrect,
    non_constant_expression
};

struct Copyable { Copyable( const Copyable& ) = default; };
struct NonCopyable { NonCopyable( const NonCopyable& ) = delete; };
struct CopyAssignable { CopyAssignable& operator=( const CopyAssignable& ) = default; };
struct NonCopyAssignable { NonCopyAssignable& operator=( const NonCopyAssignable& ) = delete; };

struct DefaultInitializable { DefaultInitializable() = default; };
struct NonDefaultInitializable { NonDefaultInitializable() = delete; };

namespace blocked_range_value {

template <bool EnableCopyCtor, bool EnableCopyAssignment, bool EnableDtor,
          State EnableOperatorLess, State EnableOperatorMinus, State EnableOperatorPlusSizeT>
struct BlockedRangeValue {
    BlockedRangeValue( const BlockedRangeValue& ) requires EnableCopyCtor = default;

    BlockedRangeValue& operator=( const BlockedRangeValue& ) requires EnableCopyAssignment = default;

    // Prospective destructors
    ~BlockedRangeValue() requires EnableDtor = default;
    ~BlockedRangeValue() = delete;

    bool operator<( const BlockedRangeValue& ) const requires (EnableOperatorLess == State::correct) { return true; }
    bool operator<( Dummy ) const requires (EnableOperatorLess == State::incorrect_first_input) { return true; }
    Dummy operator<( const BlockedRangeValue& ) const requires (EnableOperatorLess == State::incorrect_return_type) { return Dummy{}; }
    bool operator<( const BlockedRangeValue& ) requires (EnableOperatorLess == State::incorrect_constness) { return true; }

    std::size_t operator-( const BlockedRangeValue& ) const requires (EnableOperatorMinus == State::correct) { return 0; }
    std::size_t operator-( Dummy ) const requires (EnableOperatorMinus == State::incorrect_first_input) { return 0; }
    Dummy operator-( const BlockedRangeValue& ) const requires (EnableOperatorMinus == State::incorrect_return_type) { return Dummy{}; }
    std::size_t operator-( const BlockedRangeValue& ) requires (EnableOperatorMinus == State::incorrect_constness) { return 0; }

    BlockedRangeValue operator+( std::size_t ) const requires (EnableOperatorPlusSizeT == State::correct) { return *this; }
    BlockedRangeValue operator+( Dummy ) const requires (EnableOperatorPlusSizeT == State::incorrect_first_input) { return *this; }
    Dummy operator+( std::size_t ) const requires (EnableOperatorPlusSizeT == State::incorrect_return_type) { return Dummy{}; }
    BlockedRangeValue operator+( std::size_t ) requires (EnableOperatorPlusSizeT == State::incorrect_constness) { return *this; }
};

using Correct = BlockedRangeValue</*CopyCtor = */true, /*CopyAssignment = */true, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::correct, /*PlusSizeT = */State::correct>;
using NonCopyable = BlockedRangeValue</*CopyCtor = */false, /*CopyAssignment = */true, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::correct, /*PlusSizeT = */State::correct>;
using NonCopyAssignable = BlockedRangeValue</*CopyCtor = */true, /*CopyAssignment = */false, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::correct, /*PlusSizeT = */State::correct>;
using NonDestructible = BlockedRangeValue</*CopyCtor = */true, /*CopyAssignment = */true, /*Dtor = */false, /*Less = */State::correct, /*Minus = */State::correct, /*PlusSizeT = */State::correct>;
using NoOperatorLess = BlockedRangeValue</*CopyCtor = */true, /*CopyAssignment = */true, /*Dtor = */true, /*Less = */State::not_defined, /*Minus = */State::correct, /*PlusSizeT = */State::correct>;
using OperatorLessNonConst = BlockedRangeValue</*CopyCtor = */true, /*CopyAssignment = */true, /*Dtor = */true, /*Less = */State::incorrect_constness, /*Minus = */State::correct, /*PlusSizeT = */State::correct>;
using WrongInputOperatorLess = BlockedRangeValue</*CopyCtor = */true, /*CopyAssignment = */true, /*Dtor = */true, /*Less = */State::incorrect_first_input, /*Minus = */State::correct, /*PlusSizeT = */State::correct>;
using WrongReturnOperatorLess = BlockedRangeValue</*CopyCtor = */true, /*CopyAssignment = */true, /*Dtor = */true, /*Less = */State::incorrect_return_type, /*Minus = */State::correct, /*PlusSizeT = */State::correct>;
using NoOperatorMinus = BlockedRangeValue</*CopyCtor = */true, /*CopyAssignment = */true, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::not_defined, /*PlusSizeT = */State::correct>;
using OperatorMinusNonConst = BlockedRangeValue</*CopyCtor = */true, /*CopyAssignment = */true, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::incorrect_constness, /*PlusSizeT = */State::correct>;
using WrongInputOperatorMinus = BlockedRangeValue</*CopyCtor = */true, /*CopyAssignment = */true, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::incorrect_first_input, /*PlusSizeT = */State::correct>;
using WrongReturnOperatorMinus = BlockedRangeValue</*CopyCtor = */true, /*CopyAssignment = */true, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::incorrect_return_type, /*PlusSizeT = */State::correct>;
using NoOperatorPlus = BlockedRangeValue</*CopyCtor = */true, /*CopyAssignment = */true, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::correct, /*PlusSizeT = */State::not_defined>;
using OperatorPlusNonConst = BlockedRangeValue</*CopyCtor = */true, /*CopyAssignment = */true, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::correct, /*PlusSizeT = */State::incorrect_constness>;
using WrongInputOperatorPlus = BlockedRangeValue</*CopyCtor = */true, /*CopyAssignment = */true, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::correct, /*PlusSizeT = */State::incorrect_first_input>;
using WrongReturnOperatorPlus = BlockedRangeValue</*CopyCtor = */true, /*CopyAssignment = */true, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::correct, /*PlusSizeT = */State::incorrect_return_type>;

} // namespace blocked_range_value
namespace range {

template <bool EnableCopyCtor, bool EnableSplitCtor, bool EnableDtor, State EnableEmpty, State EnableIsDivisible>
struct Range {
    Range( Range&, tbb::split ) requires EnableSplitCtor {}
    Range( const Range& ) requires EnableCopyCtor = default;
    // Prospective destructors
    ~Range() requires EnableDtor = default;
    ~Range() = delete;

    bool empty() const requires (EnableEmpty == State::correct) { return true; }
    bool empty() requires (EnableEmpty == State::incorrect_constness) { return true; }
    Dummy empty() const requires (EnableEmpty == State::incorrect_return_type) { return Dummy{}; }

    bool is_divisible() const requires (EnableIsDivisible == State::correct) { return true; }
    bool is_divisible() requires (EnableIsDivisible == State::incorrect_constness) { return true; }
    Dummy is_divisible() const requires (EnableIsDivisible == State::incorrect_return_type) { return Dummy{}; }
};

using Correct = Range</*CopyCtor = */true, /*SplitCtor = */true, /*Dtor = */true, /*Empty = */State::correct, /*IsDivisible = */State::correct>;
using NonCopyable = Range</*CopyCtor = */false, /*SplitCtor = */true, /*Dtor = */true, /*Empty = */State::correct, /*IsDivisible = */State::correct>;
using NonSplittable = Range</*CopyCtor = */true, /*SplitCtor = */false, /*Dtor = */true, /*Empty = */State::correct, /*IsDivisible = */State::correct>;
using NonDestructible = Range</*CopyCtor = */true, /*SplitCtor = */true, /*Dtor = */false, /*Empty = */State::correct, /*IsDivisible = */State::correct>;
using NoEmpty = Range</*CopyCtor = */true, /*SplitCtor = */true, /*Dtor = */true, /*Empty = */State::not_defined, /*IsDivisible = */State::correct>;
using EmptyNonConst = Range</*CopyCtor = */true, /*SplitCtor = */true, /*Dtor = */true, /*Empty = */State::incorrect_constness, /*IsDivisible = */State::correct>;
using WrongReturnEmpty = Range</*CopyCtor = */true, /*SplitCtor = */true, /*Dtor = */true, /*Empty = */State::incorrect_return_type, /*IsDivisible = */State::correct>;
using NoIsDivisible = Range</*CopyCtor = */true, /*SplitCtor = */true, /*Dtor = */true, /*Empty = */State::correct, /*IsDivisible = */State::not_defined>;
using IsDivisibleNonConst = Range</*CopyCtor = */true, /*SplitCtor = */true, /*Dtor = */true, /*Empty = */State::correct, /*IsDivisible = */State::incorrect_constness>;
using WrongReturnIsDivisible = Range</*CopyCtor = */true, /*SplitCtor = */true, /*Dtor = */true, /*Empty = */State::correct, /*IsDivisible = */State::incorrect_return_type>;

} // namespace range
namespace parallel_for_body {

template <typename Range, bool EnableCopyCtor, bool EnableDtor, State EnableFunctionCallOperator>
struct ParallelForBody {
    ParallelForBody( const ParallelForBody& ) requires EnableCopyCtor = default;
    // Prospective destructors
    ~ParallelForBody() requires EnableDtor = default;
    ~ParallelForBody() = delete;

    void operator()( Range& ) const requires (EnableFunctionCallOperator == State::correct) {}
    void operator()( Range& ) requires (EnableFunctionCallOperator == State::incorrect_constness) {}
    void operator()( Dummy ) const requires (EnableFunctionCallOperator == State::incorrect_first_input) {}
};

template <typename R> using Correct = ParallelForBody<R, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::correct>;
template <typename R> using NonCopyable = ParallelForBody<R, /*CopyCtor = */false, /*Dtor = */true, /*() = */State::correct>;
template <typename R> using NonDestructible = ParallelForBody<R, /*CopyCtor = */true, /*Dtor = */false, /*() = */State::correct>;
template <typename R> using NoOperatorRoundBrackets = ParallelForBody<R, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::not_defined>;
template <typename R> using OperatorRoundBracketsNonConst = ParallelForBody<R, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::incorrect_constness>;
template <typename R> using WrongInputOperatorRoundBrackets = ParallelForBody<R, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::incorrect_first_input>;
} // namespace parallel_for_body
namespace parallel_for_function {

template <typename Index, State EnableFunctionCallOperator>
struct ParallelForFunc {
    void operator()( Index ) const requires (EnableFunctionCallOperator == State::correct) {}
    void operator()( Index ) requires (EnableFunctionCallOperator == State::incorrect_constness) {}
    void operator()( Dummy ) const requires (EnableFunctionCallOperator == State::incorrect_first_input) {}
};

template <typename I> using Correct = ParallelForFunc<I, /*() = */State::correct>;
template <typename I> using NoOperatorRoundBrackets = ParallelForFunc<I, /*() = */State::not_defined>;
template <typename I> using OperatorRoundBracketsNonConst = ParallelForFunc<I, /*() = */State::incorrect_constness>;
template <typename I> using WrongInputOperatorRoundBrackets = ParallelForFunc<I, /*() = */State::incorrect_first_input>;
} // namespace parallel_for_function
namespace parallel_for_index {
template <bool EnableIntCtor, bool EnableCopyCtor, bool EnableCopyAssign, bool EnableDtor,
          State EnableLess, State EnableMinus, State EnablePlus>
struct ParallelForIndex {
    ParallelForIndex(int) requires EnableIntCtor {}
    ParallelForIndex( const ParallelForIndex& ) requires EnableCopyCtor = default;
    ParallelForIndex& operator=( const ParallelForIndex& ) requires EnableCopyAssign = default;
    // Prospective destructors
    ~ParallelForIndex() requires EnableDtor = default;
    ~ParallelForIndex() = delete;

    bool operator<( const ParallelForIndex& ) const requires (EnableLess == State::correct) { return true; }
    bool operator<( const ParallelForIndex& ) requires (EnableLess == State::incorrect_constness) { return true; }
    bool operator<( Dummy ) const requires (EnableLess == State::incorrect_first_input) { return true; }
    Dummy operator<( const ParallelForIndex& ) const requires (EnableLess == State::incorrect_return_type) { return Dummy{}; }

    std::size_t operator-( const ParallelForIndex& ) const requires (EnableMinus == State::correct) { return 0; }
    std::size_t operator-( const ParallelForIndex& ) requires (EnableMinus == State::incorrect_constness) { return 0; }
    std::size_t operator-( Dummy ) const requires (EnableMinus == State::incorrect_first_input) { return 0; }
    Dummy operator-( const ParallelForIndex& ) const requires (EnableMinus == State::incorrect_return_type) { return Dummy{}; }

    ParallelForIndex operator+( std::size_t ) const requires (EnablePlus == State::correct) { return *this; }
    ParallelForIndex operator+( std::size_t ) requires (EnablePlus == State::incorrect_constness) { return *this; }
    ParallelForIndex operator+( Dummy ) const requires (EnablePlus == State::incorrect_first_input) { return *this; }
    Dummy operator+( std::size_t ) const requires (EnablePlus == State::incorrect_return_type) { return Dummy{}; }
};

using Correct = ParallelForIndex</*IntCtor = */true, /*CopyCtor = */true, /*CopyAssign = */true, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::correct, /*Plus = */State::correct>;
using NoIntCtor = ParallelForIndex</*IntCtor = */false, /*CopyCtor = */true, /*CopyAssign = */true, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::correct, /*Plus = */State::correct>;
using NonCopyable = ParallelForIndex</*IntCtor = */true, /*CopyCtor = */false, /*CopyAssign = */true, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::correct, /*Plus = */State::correct>;
using NonCopyAssignable = ParallelForIndex</*IntCtor = */true, /*CopyCtor = */true, /*CopyAssign = */false, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::correct, /*Plus = */State::correct>;
using NonDestructible = ParallelForIndex</*IntCtor = */true, /*CopyCtor = */true, /*CopyAssign = */true, /*Dtor = */false, /*Less = */State::correct, /*Minus = */State::correct, /*Plus = */State::correct>;
using NoOperatorLess = ParallelForIndex</*IntCtor = */true, /*CopyCtor = */true, /*CopyAssign = */true, /*Dtor = */true, /*Less = */State::not_defined, /*Minus = */State::correct, /*Plus = */State::correct>;
using OperatorLessNonConst = ParallelForIndex</*IntCtor = */true, /*CopyCtor = */true, /*CopyAssign = */true, /*Dtor = */true, /*Less = */State::incorrect_constness, /*Minus = */State::correct, /*Plus = */State::correct>;
using WrongInputOperatorLess = ParallelForIndex</*IntCtor = */true, /*CopyCtor = */true, /*CopyAssign = */true, /*Dtor = */true, /*Less = */State::incorrect_first_input, /*Minus = */State::correct, /*Plus = */State::correct>;
using WrongReturnOperatorLess = ParallelForIndex</*IntCtor = */true, /*CopyCtor = */true, /*CopyAssign = */true, /*Dtor = */true, /*Less = */State::incorrect_return_type, /*Minus = */State::correct, /*Plus = */State::correct>;
using NoOperatorMinus = ParallelForIndex</*IntCtor = */true, /*CopyCtor = */true, /*CopyAssign = */true, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::not_defined, /*Plus = */State::correct>;
using OperatorMinusNonConst = ParallelForIndex</*IntCtor = */true, /*CopyCtor = */true, /*CopyAssign = */true, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::incorrect_constness, /*Plus = */State::correct>;
using WrongInputOperatorMinus = ParallelForIndex</*IntCtor = */true, /*CopyCtor = */true, /*CopyAssign = */true, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::incorrect_first_input, /*Plus = */State::correct>;
using WrongReturnOperatorMinus = ParallelForIndex</*IntCtor = */true, /*CopyCtor = */true, /*CopyAssign = */true, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::incorrect_return_type, /*Plus = */State::correct>;
using NoOperatorPlus = ParallelForIndex</*IntCtor = */true, /*CopyCtor = */true, /*CopyAssign = */true, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::correct, /*Plus = */State::not_defined>;
using OperatorPlusNonConst = ParallelForIndex</*IntCtor = */true, /*CopyCtor = */true, /*CopyAssign = */true, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::correct, /*Plus = */State::incorrect_constness>;
using WrongInputOperatorPlus = ParallelForIndex</*IntCtor = */true, /*CopyCtor = */true, /*CopyAssign = */true, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::correct, /*Plus = */State::incorrect_first_input>;
using WrongReturnOperatorPlus = ParallelForIndex</*IntCtor = */true, /*CopyCtor = */true, /*CopyAssign = */true, /*Dtor = */true, /*Less = */State::correct, /*Minus = */State::correct, /*Plus = */State::incorrect_return_type>;
} // namespace parallel_for_index
namespace parallel_for_each_body {

template <typename T, State EnableFunctionCallOperator>
struct ParallelForEachBody {
    void operator()( const T& ) const requires (EnableFunctionCallOperator == State::correct) {}
    void operator()( const T& ) requires (EnableFunctionCallOperator == State::incorrect_constness) {}
    void operator()( Dummy ) const requires (EnableFunctionCallOperator == State::incorrect_first_input) {}
};

template <typename T, typename FeederT, State EnableFunctionCallOperator>
struct ParallelForEachFeederBody {
    void operator()( const T&, tbb::feeder<FeederT>& ) const requires (EnableFunctionCallOperator == State::correct) {}
    void operator()( const T&, tbb::feeder<FeederT>& ) requires (EnableFunctionCallOperator == State::incorrect_constness) {}
    void operator()( Dummy, tbb::feeder<FeederT>& ) const requires (EnableFunctionCallOperator == State::incorrect_first_input) {}
    void operator()( const T&, Dummy ) const requires (EnableFunctionCallOperator == State::incorrect_second_input) {}
};

template <typename T> using Correct = ParallelForEachBody<T, /*() = */State::correct>;
template <typename T> using NoOperatorRoundBrackets = ParallelForEachBody<T, /*() = */State::not_defined>;
template <typename T> using OperatorRoundBracketsNonConst = ParallelForEachBody<T, /*() = */State::incorrect_constness>;
template <typename T> using WrongInputOperatorRoundBrackets = ParallelForEachBody<T, /*() = */State::incorrect_first_input>;

template <typename T, typename F = T> using WithFeeder = ParallelForEachFeederBody<T, F, /*() = */State::correct>;
template <typename T, typename F = T> using WithFeederNoOperatorRoundBrackets = ParallelForEachFeederBody<T, F, /*() = */State::not_defined>;
template <typename T, typename F = T> using WithFeederOperatorRoundBracketsNonConst = ParallelForEachFeederBody<T, F, /*() = */State::incorrect_constness>;
template <typename T, typename F = T> using WithFeederWrongFirstInputOperatorRoundBrackets = ParallelForEachFeederBody<T, F, /*() = */State::incorrect_first_input>;
template <typename T, typename F = T> using WithFeederWrongSecondInputOperatorRoundBrackets = ParallelForEachFeederBody<T, F, /*() = */State::incorrect_second_input>;
} // namespace parallel_for_each_body
namespace parallel_sort_value {
template<bool MovableV, bool MoveAssignableV, bool ComparableV>
struct ParallelSortValue
{
    ParallelSortValue(ParallelSortValue&&) requires MovableV = default;
    ParallelSortValue& operator=(ParallelSortValue&&) requires MoveAssignableV = default;

    friend bool operator<(const ParallelSortValue&, const ParallelSortValue&) requires ComparableV { return true; }
};

using CorrectValue = ParallelSortValue</*MovableV = */true, /*MoveAssignableV = */true, /*ComparableV = */true>;
using NonMovableValue = ParallelSortValue</*MovableV = */false, /*MoveAssignableV = */true, /*ComparableV = */true>;
using NonMoveAssignableValue = ParallelSortValue</*MovableV = */true, /*MoveAssignableV = */false, /*ComparableV = */true>;
using NonComparableValue = ParallelSortValue</*MovableV = */true, /*MoveAssignableV = */true, /*ComparableV = */false>;
} // namespace parallel_sort_value
template <typename T>
class ConstantIT {
    T data{};
    const T& operator* () const { return data; }
};
namespace container_based_sequence {

template <bool EnableBegin, bool EnableEnd, typename T = int>
struct ContainerBasedSequence {
    using iterator = T*;
    T* begin() requires EnableBegin { return nullptr; }
    T* end() requires EnableEnd { return nullptr; }
};

using Correct = ContainerBasedSequence</*Begin = */true, /*End = */true>;
using NoBegin = ContainerBasedSequence</*Begin = */false, /*End = */true>;
using NoEnd = ContainerBasedSequence</*Begin = */true, /*End = */false>;

template <typename T>
using CustomValueCBS = ContainerBasedSequence</*Begin = */true, /*End = */true, T>;

struct ConstantCBS {
    ConstantIT<int> begin() const { return ConstantIT<int>{}; }
    ConstantIT<int> end() const { return ConstantIT<int>{}; }
};

struct ForwardIteratorCBS {
    utils::ForwardIterator<int> begin() { return utils::ForwardIterator<int>{}; }
    utils::ForwardIterator<int> end() { return begin(); }
};

} // namespace container_based_sequence
namespace parallel_reduce_body {

template <typename Range, bool EnableSplitCtor, bool EnableDtor, State EnableFunctionCallOperator, State EnableJoin>
struct ParallelReduceBody {
    ParallelReduceBody( ParallelReduceBody&, tbb::split ) requires EnableSplitCtor {}
    // Prospective destructors
    ~ParallelReduceBody() requires EnableDtor = default;
    ~ParallelReduceBody() = delete;

    void operator()( const Range& ) requires (EnableFunctionCallOperator == State::correct) {}
    void operator()( Dummy ) requires (EnableFunctionCallOperator == State::incorrect_first_input) {}

    void join( ParallelReduceBody& ) requires (EnableJoin == State::correct) {}
    void join( Dummy ) requires (EnableJoin == State::incorrect_first_input) {}
};

template <typename R> using Correct = ParallelReduceBody<R, /*SplitCtor = */true, /*Dtor = */true, /*() = */State::correct, /*Join = */State::correct>;
template <typename R> using NonSplittable = ParallelReduceBody<R, /*SplitCtor = */false, /*Dtor = */true, /*() = */State::correct, /*Join = */State::correct>;
template <typename R> using NonDestructible = ParallelReduceBody<R, /*SplitCtor = */true, /*Dtor = */false, /*() = */State::correct, /*Join = */State::correct>;
template <typename R> using NoOperatorRoundBrackets = ParallelReduceBody<R, /*SplitCtor = */true, /*Dtor = */true, /*() = */State::not_defined, /*Join = */State::correct>;
template <typename R> using WrongInputOperatorRoundBrackets = ParallelReduceBody<R, /*SplitCtor = */true, /*Dtor = */true, /*() = */State::incorrect_first_input, /*Join = */State::correct>;
template <typename R> using NoJoin = ParallelReduceBody<R, /*SplitCtor = */true, /*Dtor = */true, /*() = */State::correct, /*Join = */State::not_defined>;
template <typename R> using WrongInputJoin = ParallelReduceBody<R, /*SplitCtor = */true, /*Dtor = */true, /*() = */State::correct, /*Join = */State::incorrect_first_input>;
} // namespace parallel_reduce_body
namespace parallel_reduce_function {

template <typename Range, State EnableFunctionCallOperator>
struct ParallelReduceFunction {
    int operator()( const Range&, const int& ) const requires (EnableFunctionCallOperator == State::correct) { return 0; }
    int operator()( const Range&, const int& ) requires (EnableFunctionCallOperator == State::incorrect_constness) { return 0; }
    int operator()( Dummy, const int& ) const requires (EnableFunctionCallOperator == State::incorrect_first_input) { return 0; }
    int operator()( const Range&, Dummy ) const requires (EnableFunctionCallOperator == State::incorrect_second_input) { return 0; }
    Dummy operator()( const Range&, const int& ) const requires (EnableFunctionCallOperator == State::incorrect_return_type) { return Dummy{}; }
};

template <typename R> using Correct = ParallelReduceFunction<R, /*() = */State::correct>;
template <typename R> using NoOperatorRoundBrackets = ParallelReduceFunction<R, /*() = */State::not_defined>;
template <typename R> using OperatorRoundBracketsNonConst = ParallelReduceFunction<R, /*() = */State::incorrect_constness>;
template <typename R> using WrongFirstInputOperatorRoundBrackets = ParallelReduceFunction<R, /*() = */State::incorrect_first_input>;
template <typename R> using WrongSecondInputOperatorRoundBrackets = ParallelReduceFunction<R, /*() = */State::incorrect_second_input>;
template <typename R> using WrongReturnOperatorRoundBrackets = ParallelReduceFunction<R, /*() = */State::incorrect_return_type>;
} // namespace parallel_reduce_function
namespace parallel_reduce_combine {

template <typename T, State EnableFunctionCallOperator>
struct ParallelReduceCombine {
    T operator()( const T& a, const T& ) const requires (EnableFunctionCallOperator == State::correct) { return a; }
    T operator()( const T& a, const T& ) requires (EnableFunctionCallOperator == State::incorrect_constness) { return a; }
    T operator()( Dummy, const T& a ) const requires (EnableFunctionCallOperator == State::incorrect_first_input) { return a; }
    T operator()( const T& a, Dummy ) const requires (EnableFunctionCallOperator == State::incorrect_second_input) { return a; }
    Dummy operator()( const T&, const T& ) const requires (EnableFunctionCallOperator == State::incorrect_return_type) { return Dummy{}; }
};

template <typename T> using Correct = ParallelReduceCombine<T, /*() = */State::correct>;
template <typename T> using NoOperatorRoundBrackets = ParallelReduceCombine<T, /*() = */State::not_defined>;
template <typename T> using OperatorRoundBracketsNonConst = ParallelReduceCombine<T, /*() = */State::incorrect_constness>;
template <typename T> using WrongFirstInputOperatorRoundBrackets = ParallelReduceCombine<T, /*() = */State::incorrect_first_input>;
template <typename T> using WrongSecondInputOperatorRoundBrackets = ParallelReduceCombine<T, /*() = */State::incorrect_second_input>;
template <typename T> using WrongReturnOperatorRoundBrackets = ParallelReduceCombine<T, /*() = */State::incorrect_return_type>;
} // namespace parallel_reduce_reduction
namespace parallel_scan_body {

template <typename Range, bool EnableSplitCtor, State EnableReverseJoin, State EnableAssign, State EnablePreScanRoundBrackets, State EnableFinalScanRoundBrackets>
struct ParallelScanBody {
    ParallelScanBody( ParallelScanBody&, tbb::split ) requires EnableSplitCtor {}

    void reverse_join( ParallelScanBody& ) requires (EnableReverseJoin == State::correct) {}
    void reverse_join( Dummy ) requires (EnableReverseJoin == State::incorrect_first_input) {}

    void assign( ParallelScanBody& ) requires (EnableAssign == State::correct) {}
    void assign( Dummy ) requires (EnableAssign == State::incorrect_first_input) {}

    void operator()( const Range&, tbb::pre_scan_tag ) requires (EnablePreScanRoundBrackets == State::correct) {}
    void operator()( Dummy, tbb::pre_scan_tag ) requires (EnablePreScanRoundBrackets == State::incorrect_first_input) {}
    void operator()( const Range&, Dummy ) requires (EnablePreScanRoundBrackets == State::incorrect_second_input) {}

    void operator()( const Range&, tbb::final_scan_tag ) requires (EnableFinalScanRoundBrackets == State::correct) {}
    void operator()( Dummy, tbb::final_scan_tag ) requires (EnableFinalScanRoundBrackets == State::incorrect_first_input) {}
    void operator()( const Range&, Dummy ) requires (EnableFinalScanRoundBrackets == State::incorrect_second_input) {}
};

template <typename R> using Correct = ParallelScanBody<R, /*SplitCtor = */true, /*ReverseJoin = */State::correct, /*Assign = */State::correct, /*PreScan = */State::correct, /*FinalScan = */State::correct>;
template <typename R> using NonSplittable = ParallelScanBody<R, /*SplitCtor = */false, /*ReverseJoin = */State::correct, /*Assign = */State::correct, /*PreScan = */State::correct, /*FinalScan = */State::correct>;
template <typename R> using NoReverseJoin = ParallelScanBody<R, /*SplitCtor = */true, /*ReverseJoin = */State::not_defined, /*Assign = */State::correct, /*PreScan = */State::correct, /*FinalScan = */State::correct>;
template <typename R> using WrongInputReverseJoin = ParallelScanBody<R, /*SplitCtor = */true, /*ReverseJoin = */State::incorrect_first_input, /*Assign = */State::correct, /*PreScan = */State::correct, /*FinalScan = */State::correct>;
template <typename R> using NoAssign = ParallelScanBody<R, /*SplitCtor = */true, /*ReverseJoin = */State::correct, /*Assign = */State::not_defined, /*PreScan = */State::correct, /*FinalScan = */State::correct>;
template <typename R> using WrongInputAssign = ParallelScanBody<R, /*SplitCtor = */true, /*ReverseJoin = */State::correct, /*Assign = */State::incorrect_first_input, /*PreScan = */State::correct, /*FinalScan = */State::correct>;
template <typename R> using NoPreScanOperatorRoundBrackets = ParallelScanBody<R, /*SplitCtor = */true, /*ReverseJoin = */State::correct, /*Assign = */State::correct, /*PreScan = */State::not_defined, /*FinalScan = */State::correct>;
template <typename R> using WrongFirstInputPreScanOperatorRoundBrackets = ParallelScanBody<R, /*SplitCtor = */true, /*ReverseJoin = */State::correct, /*Assign = */State::correct, /*PreScan = */State::incorrect_first_input, /*FinalScan = */State::correct>;
template <typename R> using WrongSecondInputPreScanOperatorRoundBrackets = ParallelScanBody<R, /*SplitCtor = */true, /*ReverseJoin = */State::correct, /*Assign = */State::correct, /*PreScan = */State::incorrect_second_input, /*FinalScan = */State::correct>;
template <typename R> using NoFinalScanOperatorRoundBrackets = ParallelScanBody<R, /*SplitCtor = */true, /*ReverseJoin = */State::correct, /*Assign = */State::correct, /*PreScan = */State::correct, /*FinalScan = */State::not_defined>;
template <typename R> using WrongFirstInputFinalScanOperatorRoundBrackets = ParallelScanBody<R, /*SplitCtor = */true, /*ReverseJoin = */State::correct, /*Assign = */State::correct, /*PreScan = */State::correct, /*FinalScan = */State::incorrect_first_input>;
template <typename R> using WrongSecondInputFinalScanOperatorRoundBrackets = ParallelScanBody<R, /*SplitCtor = */true, /*ReverseJoin = */State::correct, /*Assign = */State::correct, /*PreScan = */State::correct, /*FinalScan = */State::incorrect_second_input>;
} // namespace parallel_scan_body
namespace parallel_scan_function {

template <typename Range, typename T, State EnableFunctionCallOperator>
struct ParallelScanFunction {
    T operator()( const Range&, const T& a, bool ) const requires (EnableFunctionCallOperator == State::correct) { return a; }
    T operator()( const Range&, const T& a, bool ) requires (EnableFunctionCallOperator == State::incorrect_constness) { return a; }
    T operator()( Dummy, const T& a, bool ) const requires (EnableFunctionCallOperator == State::incorrect_first_input) { return a; }
    T operator()( const Range&, Dummy, bool ) const requires (EnableFunctionCallOperator == State::incorrect_second_input) { return T{}; }
    T operator()( const Range&, const T& a, Dummy ) const requires (EnableFunctionCallOperator == State::incorrect_third_input) { return a; }
    Dummy operator()( const Range&, const T& a, bool ) const requires (EnableFunctionCallOperator == State::incorrect_return_type) { return Dummy{}; }
};

template <typename R, typename T> using Correct = ParallelScanFunction<R, T, /*() = */State::correct>;
template <typename R, typename T> using NoOperatorRoundBrackets = ParallelScanFunction<R, T, /*() = */State::not_defined>;
template <typename R, typename T> using OperatorRoundBracketsNonConst = ParallelScanFunction<R, T, /*() = */State::incorrect_constness>;
template <typename R, typename T> using WrongFirstInputOperatorRoundBrackets = ParallelScanFunction<R, T, /*() = */State::incorrect_first_input>;
template <typename R, typename T> using WrongSecondInputOperatorRoundBrackets = ParallelScanFunction<R, T, /*() = */State::incorrect_second_input>;
template <typename R, typename T> using WrongThirdInputOperatorRoundBrackets = ParallelScanFunction<R, T, /*() = */State::incorrect_third_input>;
template <typename R, typename T> using WrongReturnOperatorRoundBrackets = ParallelScanFunction<R, T, /*() = */State::incorrect_return_type>;
} // namespace parallel_scan_function
namespace parallel_scan_combine {
using namespace parallel_reduce_combine;
} // namespace parallel_scan_combine
namespace compare {

template <typename T, State EnableFunctionCallOperator>
struct Compare {
    bool operator()( const T&, const T& ) const requires (EnableFunctionCallOperator == State::correct) { return true; }
    bool operator()( Dummy, const T& ) const requires (EnableFunctionCallOperator == State::incorrect_first_input) { return true; }
    bool operator()( const T&, Dummy ) const requires (EnableFunctionCallOperator == State::incorrect_second_input) { return true; }
    Dummy operator()( const T&, const T& ) const requires (EnableFunctionCallOperator == State::incorrect_return_type) { return Dummy{}; }
};

template <typename T> using Correct = Compare<T, /*() = */State::correct>;
template <typename T> using NoOperatorRoundBrackets = Compare<T, /*() = */State::not_defined>;
template <typename T> using WrongFirstInputOperatorRoundBrackets = Compare<T, /*() = */State::incorrect_first_input>;
template <typename T> using WrongSecondInputOperatorRoundBrackets = Compare<T, /*() = */State::incorrect_second_input>;
template <typename T> using WrongReturnOperatorRoundBrackets = Compare<T, /*() = */State::incorrect_return_type>;
} // namespace compare

namespace hash_compare {

template <typename Key, bool EnableCopyCtor, bool EnableDtor, State EnableHash, State EnableEqual>
struct HashCompare {
    HashCompare( const HashCompare& ) requires EnableCopyCtor = default;
    // Prospective destructors
    ~HashCompare() requires EnableDtor = default;
    ~HashCompare() = delete;

    std::size_t hash( const Key& ) const requires (EnableHash == State::correct) { return 0; }
    std::size_t hash( const Key& ) requires (EnableHash == State::incorrect_constness) { return 0; }
    std::size_t hash( Dummy ) const requires (EnableHash == State::incorrect_first_input) { return 0; }
    Dummy hash( const Key& ) const requires (EnableHash == State::incorrect_return_type) { return Dummy{}; }

    bool equal( const Key&, const Key& ) const requires (EnableEqual == State::correct) { return true; }
    bool equal( const Key&, const Key& ) requires (EnableEqual == State::incorrect_constness) { return true; }
    bool equal( Dummy, const Key& ) const requires (EnableEqual == State::incorrect_first_input) { return true; }
    bool equal( const Key&, Dummy ) const requires (EnableEqual == State::incorrect_second_input) { return true; }
    Dummy equal( const Key&, const Key& ) const requires (EnableEqual == State::incorrect_return_type) { return Dummy{}; }
};

template <typename K> using Correct = HashCompare<K, /*CopyCtor = */true, /*Dtor = */true, /*Hash = */State::correct, /*Equal = */State::correct>;
template <typename K> using NonCopyable = HashCompare<K, /*CopyCtor = */false, /*Dtor = */true, /*Hash = */State::correct, /*Equal = */State::correct>;
template <typename K> using NonDestructible = HashCompare<K, /*CopyCtor = */true, /*Dtor = */false, /*Hash = */State::correct, /*Equal = */State::correct>;
template <typename K> using NoHash = HashCompare<K, /*CopyCtor = */true, /*Dtor = */true, /*Hash = */State::not_defined, /*Equal = */State::correct>;
template <typename K> using HashNonConst = HashCompare<K, /*CopyCtor = */true, /*Dtor = */true, /*Hash = */State::incorrect_constness, /*Equal = */State::correct>;
template <typename K> using WrongInputHash = HashCompare<K, /*CopyCtor = */true, /*Dtor = */true, /*Hash = */State::incorrect_first_input, /*Equal = */State::correct>;
template <typename K> using WrongReturnHash = HashCompare<K, /*CopyCtor = */true, /*Dtor = */true, /*Hash = */State::incorrect_return_type, /*Equal = */State::correct>;
template <typename K> using NoEqual = HashCompare<K, /*CopyCtor = */true, /*Dtor = */true, /*Hash = */State::correct, /*Equal = */State::not_defined>;
template <typename K> using EqualNonConst = HashCompare<K, /*CopyCtor = */true, /*Dtor = */true, /*Hash = */State::correct, /*Equal = */State::incorrect_constness>;
template <typename K> using WrongFirstInputEqual = HashCompare<K, /*CopyCtor = */true, /*Dtor = */true, /*Hash = */State::correct, /*Equal = */State::incorrect_first_input>;
template <typename K> using WrongSecondInputEqual = HashCompare<K, /*CopyCtor = */true, /*Dtor = */true, /*Hash = */State::correct, /*Equal = */State::incorrect_second_input>;
template <typename K> using WrongReturnEqual = HashCompare<K, /*CopyCtor = */true, /*Dtor = */true, /*Hash = */State::correct, /*Equal = */State::incorrect_return_type>;

} // namespace hash_compare
namespace rw_mutex {

template <typename RwMutex, bool EnableSLDefaultCtor, bool EnableSLMutexCtor, bool EnableSLDtor, State EnableSLAcquire, State EnableSLTryAcquire, bool EnableSLRelease, State EnableSLUpgrade, State EnableSLDowngrade, State EnableIsWriter>
struct DefineRWScopedLock {
    struct scoped_lock {
        scoped_lock() requires EnableSLDefaultCtor = default;
        scoped_lock( RwMutex&, bool = true ) requires EnableSLMutexCtor {}
        // Prospective destructors
        ~scoped_lock() requires EnableSLDtor = default;
        ~scoped_lock() = delete;

        void acquire( RwMutex&, bool = true ) requires (EnableSLAcquire == State::correct) {}
        void acquire( Dummy, bool = true ) requires (EnableSLAcquire == State::incorrect_first_input) {}
        void acquire( RwMutex&, Dummy = Dummy{} ) requires (EnableSLAcquire == State::incorrect_second_input) {}

        bool try_acquire( RwMutex&, bool = true ) requires (EnableSLTryAcquire == State::correct) { return true; }
        bool try_acquire( Dummy, bool = true ) requires (EnableSLTryAcquire == State::incorrect_first_input) { return true; }
        bool try_acquire( RwMutex&, Dummy = Dummy{} ) requires (EnableSLTryAcquire == State::incorrect_second_input) { return true; }
        Dummy try_acquire( RwMutex&, bool = true ) requires (EnableSLTryAcquire == State::incorrect_return_type) { return Dummy{}; }

        void release() requires (EnableSLRelease) {}

        bool upgrade_to_writer() requires (EnableSLUpgrade == State::correct) { return true; }
        Dummy upgrade_to_writer() requires (EnableSLUpgrade == State::incorrect_return_type) { return Dummy{}; }

        bool downgrade_to_reader() requires (EnableSLDowngrade == State::correct) { return true; }
        Dummy downgrade_to_reader() requires (EnableSLDowngrade == State::incorrect_return_type) { return Dummy{}; }

        bool is_writer() const requires (EnableIsWriter == State::correct) { return true; }
        Dummy is_writer() const requires (EnableIsWriter == State::incorrect_return_type) { return Dummy{}; }
        bool is_writer() requires (EnableIsWriter == State::incorrect_constness) { return true; }
    };
};

template <State S>
inline const bool mutex_trait_impl = true;

template <>
inline const int mutex_trait_impl<State::incorrect> = 0;

template <>
inline bool mutex_trait_impl<State::non_constant_expression> = true;

template <bool EnableScopedLock, bool EnableSLDefaultCtor, bool EnableSLMutexCtor, bool EnableSLDtor, State EnableSLAcquire, State EnableSLTryAcquire,
          bool EnableSLRelease, State EnableSLUpgrade, State EnableSLDowngrade, State EnableSLIsWriter>
struct RWMutex
    : std::conditional_t<EnableScopedLock, DefineRWScopedLock<RWMutex<EnableScopedLock, EnableSLDefaultCtor, EnableSLMutexCtor, EnableSLDtor, EnableSLAcquire, EnableSLTryAcquire, EnableSLRelease, EnableSLUpgrade, EnableSLDowngrade, EnableSLIsWriter>,
                                                                      EnableSLDefaultCtor, EnableSLMutexCtor, EnableSLDtor, EnableSLAcquire, EnableSLTryAcquire, EnableSLRelease, EnableSLUpgrade, EnableSLDowngrade, EnableSLIsWriter>,
                                           Dummy>
{};

using Correct = RWMutex</*ScopedLock = */true, /*DefaultCtor = */true, /*MutexCtor = */true, /*Dtor = */true, /*Acquire = */State::correct,
                        /*try_acquire = */State::correct, /*release = */true, /*upgrade = */State::correct, /*downgrade = */State::correct, /*is_writer = */State::correct>;
using NoScopedLock = RWMutex</*ScopedLock = */false, /*DefaultCtor = */true, /*MutexCtor = */true, /*Dtor = */true, /*Acquire = */State::correct,
                             /*try_acquire = */State::correct, /*release = */true, /*upgrade = */State::correct, /*downgrade = */State::correct, /*is_writer = */State::correct>;
using ScopedLockNoDefaultCtor = RWMutex</*ScopedLock = */true, /*DefaultCtor = */false, /*MutexCtor = */true, /*Dtor = */true, /*Acquire = */State::correct,
                                        /*try_acquire = */State::correct, /*release = */true, /*upgrade = */State::correct, /*downgrade = */State::correct, /*is_writer = */State::correct>;
using ScopedLockNoMutexCtor = RWMutex</*ScopedLock = */true, /*DefaultCtor = */true, /*MutexCtor = */false, /*Dtor = */true, /*Acquire = */State::correct,
                                      /*try_acquire = */State::correct, /*release = */true, /*upgrade = */State::correct, /*downgrade = */State::correct, /*is_writer = */State::correct>;
using ScopedLockNoDtor = RWMutex</*ScopedLock = */true, /*DefaultCtor = */true, /*MutexCtor = */true, /*Dtor = */false, /*Acquire = */State::correct,
                                 /*try_acquire = */State::correct, /*release = */true, /*upgrade = */State::correct, /*downgrade = */State::correct, /*is_writer = */State::correct>;
using ScopedLockNoAcquire = RWMutex</*ScopedLock = */true, /*DefaultCtor = */true, /*MutexCtor = */true, /*Dtor = */true, /*Acquire = */State::not_defined,
                                    /*try_acquire = */State::correct, /*release = */true, /*upgrade = */State::correct, /*downgrade = */State::correct, /*is_writer = */State::correct>;
using ScopedLockWrongFirstInputAcquire = RWMutex</*ScopedLock = */true, /*DefaultCtor = */true, /*MutexCtor = */true, /*Dtor = */true, /*Acquire = */State::incorrect_first_input,
                                                 /*try_acquire = */State::correct, /*release = */true, /*upgrade = */State::correct, /*downgrade = */State::correct, /*is_writer = */State::correct>;
using ScopedLockWrongSecondInputAcquire = RWMutex</*ScopedLock = */true, /*DefaultCtor = */true, /*MutexCtor = */true, /*Dtor = */true, /*Acquire = */State::incorrect_second_input,
                                                  /*try_acquire = */State::correct, /*release = */true, /*upgrade = */State::correct, /*downgrade = */State::correct, /*is_writer = */State::correct>;
using ScopedLockNoTryAcquire = RWMutex</*ScopedLock = */true, /*DefaultCtor = */true, /*MutexCtor = */true, /*Dtor = */true, /*Acquire = */State::correct,
                                       /*try_acquire = */State::not_defined, /*release = */true, /*upgrade = */State::correct, /*downgrade = */State::correct, /*is_writer = */State::correct>;
using ScopedLockWrongFirstInputTryAcquire = RWMutex</*ScopedLock = */true, /*DefaultCtor = */true, /*MutexCtor = */true, /*Dtor = */true, /*Acquire = */State::correct,
                                                    /*try_acquire = */State::incorrect_first_input, /*release = */true, /*upgrade = */State::correct, /*downgrade = */State::correct, /*is_writer = */State::correct>;
using ScopedLockWrongSecondInputTryAcquire = RWMutex</*ScopedLock = */true, /*DefaultCtor = */true, /*MutexCtor = */true, /*Dtor = */true, /*Acquire = */State::correct,
                                                     /*try_acquire = */State::incorrect_second_input, /*release = */true, /*upgrade = */State::correct, /*downgrade = */State::correct, /*is_writer = */State::correct>;
using ScopedLockWrongReturnTryAcquire = RWMutex</*ScopedLock = */true, /*DefaultCtor = */true, /*MutexCtor = */true, /*Dtor = */true, /*Acquire = */State::correct,
                                                /*try_acquire = */State::incorrect_return_type, /*release = */true, /*upgrade = */State::correct, /*downgrade = */State::correct, /*is_writer = */State::correct>;
using ScopedLockNoRelease = RWMutex</*ScopedLock = */true, /*DefaultCtor = */true, /*MutexCtor = */true, /*Dtor = */true, /*Acquire = */State::correct,
                                    /*try_acquire = */State::correct, /*release = */false, /*upgrade = */State::correct, /*downgrade = */State::correct, /*is_writer = */State::correct>;
using ScopedLockNoUpgrade = RWMutex</*ScopedLock = */true, /*DefaultCtor = */true, /*MutexCtor = */true, /*Dtor = */true, /*Acquire = */State::correct,
                                    /*try_acquire = */State::correct, /*release = */true, /*upgrade = */State::not_defined, /*downgrade = */State::correct, /*is_writer = */State::correct>;
using ScopedLockWrongReturnUpgrade = RWMutex</*ScopedLock = */true, /*DefaultCtor = */true, /*MutexCtor = */true, /*Dtor = */true, /*Acquire = */State::correct,
                                             /*try_acquire = */State::correct, /*release = */true, /*upgrade = */State::incorrect_return_type, /*downgrade = */State::correct, /*is_writer = */State::correct>;
using ScopedLockNoDowngrade = RWMutex</*ScopedLock = */true, /*DefaultCtor = */true, /*MutexCtor = */true, /*Dtor = */true, /*Acquire = */State::correct,
                                      /*try_acquire = */State::correct, /*release = */true, /*upgrade = */State::correct, /*downgrade = */State::not_defined, /*is_writer = */State::correct>;
using ScopedLockWrongReturnDowngrade = RWMutex</*ScopedLock = */true, /*DefaultCtor = */true, /*MutexCtor = */true, /*Dtor = */true, /*Acquire = */State::correct,
                                               /*try_acquire = */State::correct, /*release = */true, /*upgrade = */State::correct, /*downgrade = */State::incorrect_return_type, /*is_writer = */State::correct>;
using ScopedLockNoIsWriter = RWMutex</*ScopedLock = */true, /*DefaultCtor = */true, /*MutexCtor = */true, /*Dtor = */true, /*Acquire = */State::correct,
                                     /*try_acquire = */State::correct, /*release = */true, /*upgrade = */State::correct, /*downgrade = */State::correct, /*is_writer = */State::not_defined>;
using ScopedLockIsWriterNonConst = RWMutex</*ScopedLock = */true, /*DefaultCtor = */true, /*MutexCtor = */true, /*Dtor = */true, /*Acquire = */State::correct,
                                           /*try_acquire = */State::correct, /*release = */true, /*upgrade = */State::correct, /*downgrade = */State::correct, /*is_writer = */State::incorrect_constness>;
using ScopedLockWrongReturnIsWriter = RWMutex</*ScopedLock = */true, /*DefaultCtor = */true, /*MutexCtor = */true, /*Dtor = */true, /*Acquire = */State::correct,
                                              /*try_acquire = */State::correct, /*release = */true, /*upgrade = */State::correct, /*downgrade = */State::correct, /*is_writer = */State::incorrect_return_type>;
} // namespace rw_mutex

// Flow Graph testing infrastructure below
namespace input_node_body {

template <typename Output, bool EnableCopyCtor, bool EnableDtor, State EnableFunctionCallOperator>
struct InputNodeBody {
    InputNodeBody( const InputNodeBody& ) requires EnableCopyCtor = default;
    // Prospective destructors
    ~InputNodeBody() requires EnableDtor = default;
    ~InputNodeBody() = delete;

    Output operator()( tbb::flow_control& ) requires (EnableFunctionCallOperator == State::correct) { return Output{}; }
    Output operator()( Dummy ) requires (EnableFunctionCallOperator == State::incorrect_first_input) { return Output{}; }
    Dummy operator()( tbb::flow_control& ) requires (EnableFunctionCallOperator == State::incorrect_return_type) { return Dummy{}; }
};

template <typename O> using Correct = InputNodeBody<O, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::correct>;
template <typename O> using NonCopyable = InputNodeBody<O, /*CopyCtor = */false, /*Dtor = */true, /*() = */State::correct>;
template <typename O> using NonDestructible = InputNodeBody<O, /*CopyCtor = */true, /*Dtor = */false, /*() = */State::correct>;
template <typename O> using NoOperatorRoundBrackets = InputNodeBody<O, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::not_defined>;
template <typename O> using WrongInputOperatorRoundBrackets = InputNodeBody<O, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::incorrect_first_input>;
template <typename O> using WrongReturnOperatorRoundBrackets = InputNodeBody<O, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::incorrect_return_type>;
} // namespace input_node_body
namespace function_node_body {

template <typename Input, typename Output, bool EnableCopyCtor, bool EnableDtor, State EnableFunctionCallOperator>
struct FunctionNodeBody {
    FunctionNodeBody( const FunctionNodeBody& ) requires EnableCopyCtor = default;
    // Prospective destructors
    ~FunctionNodeBody() requires EnableDtor = default;
    ~FunctionNodeBody() = delete;

    Output operator()( const Input& ) requires (EnableFunctionCallOperator == State::correct) { return Output{}; }
    Output operator()( Dummy ) requires (EnableFunctionCallOperator == State::incorrect_first_input) { return Dummy{}; }
    Dummy operator()( const Input& ) requires (EnableFunctionCallOperator == State::incorrect_return_type) { return Output{}; }
};

template <typename I, typename O> using Correct = FunctionNodeBody<I, O, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::correct>;
template <typename I, typename O> using NonCopyable = FunctionNodeBody<I, O, /*CopyCtor = */false, /*Dtor = */true, /*() = */State::correct>;
template <typename I, typename O> using NonDestructible = FunctionNodeBody<I, O, /*CopyCtor = */true, /*Dtor = */false, /*() = */State::correct>;
template <typename I, typename O> using NoOperatorRoundBrackets = FunctionNodeBody<I, O, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::not_defined>;
template <typename I, typename O> using WrongInputRoundBrackets = FunctionNodeBody<I, O, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::incorrect_first_input>;
template <typename I, typename O> using WrongReturnRoundBrackets = FunctionNodeBody<I, O, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::incorrect_return_type>;
} // namespace function_node_body
namespace mf_async_node_body {

template <typename Input, typename Output, typename PortsType, bool EnableCopyCtor, bool EnableDtor, State EnableFunctionCallOperator>
struct PortsNodeBody {
    PortsNodeBody( const PortsNodeBody& ) requires EnableCopyCtor = default;
    // Prospective destructors
    ~PortsNodeBody() requires EnableDtor = default;
    ~PortsNodeBody() = delete;

    void operator()( const Input&, PortsType& ) requires (EnableFunctionCallOperator == State::correct) {}
    void operator()( Dummy, PortsType& ) requires (EnableFunctionCallOperator == State::incorrect_first_input) {}
    void operator()( const Input&, Dummy ) requires (EnableFunctionCallOperator == State::incorrect_second_input) {}
};

} // namespace mf_async_node_body
namespace multifunction_node_body {

template <typename Input, typename Output>
using output_ports_type = typename tbb::flow::multifunction_node<Input, Output>::output_ports_type;

using mf_async_node_body::PortsNodeBody;

template <typename I, typename O> using Correct = PortsNodeBody<I, O, output_ports_type<I, O>, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::correct>;
template <typename I, typename O> using NonCopyable = PortsNodeBody<I, O, output_ports_type<I, O>, /*CopyCtor = */false, /*Dtor = */true, /*() = */State::correct>;
template <typename I, typename O> using NonDestructible = PortsNodeBody<I, O, output_ports_type<I, O>, /*CopyCtor = */true, /*Dtor = */false, /*() = */State::correct>;
template <typename I, typename O> using NoOperatorRoundBrackets = PortsNodeBody<I, O, output_ports_type<I, O>, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::not_defined>;
template <typename I, typename O> using WrongFirstInputOperatorRoundBrackets = PortsNodeBody<I, O, output_ports_type<I, O>, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::incorrect_first_input>;
template <typename I, typename O> using WrongSecondInputOperatorRoundBrackets = PortsNodeBody<I, O, output_ports_type<I, O>, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::incorrect_second_input>;
} // namespace multifunction_node_body
namespace async_node_body {

template <typename Input, typename Output>
using gateway_type = typename tbb::flow::async_node<Input, Output>::gateway_type;

using mf_async_node_body::PortsNodeBody;

template <typename I, typename O> using Correct = PortsNodeBody<I, O, gateway_type<I, O>, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::correct>;
template <typename I, typename O> using NonCopyable = PortsNodeBody<I, O, gateway_type<I, O>, /*CopyCtor = */false, /*Dtor = */true, /*() = */State::correct>;
template <typename I, typename O> using NonDestructible = PortsNodeBody<I, O, gateway_type<I, O>, /*CopyCtor = */true, /*Dtor = */false, /*() = */State::correct>;
template <typename I, typename O> using NoOperatorRoundBrackets = PortsNodeBody<I, O, gateway_type<I, O>, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::not_defined>;
template <typename I, typename O> using WrongFirstInputOperatorRoundBrackets = PortsNodeBody<I, O, gateway_type<I, O>, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::incorrect_first_input>;
template <typename I, typename O> using WrongSecondInputOperatorRoundBrackets = PortsNodeBody<I, O, gateway_type<I, O>, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::incorrect_second_input>;
} // namespace async_node_body
namespace continue_node_body {

template <typename Output, bool EnableCopyCtor, bool EnableDtor, State EnableFunctionCallOperator>
struct ContinueNodeBody {
    ContinueNodeBody( const ContinueNodeBody& ) requires EnableCopyCtor = default;
    // Prospective destructors
    ~ContinueNodeBody() requires EnableDtor = default;
    ~ContinueNodeBody() = delete;

    Output operator()( tbb::flow::continue_msg ) requires (EnableFunctionCallOperator == State::correct) { return Output{}; }
    Output operator()( Dummy ) requires (EnableFunctionCallOperator == State::incorrect_first_input) { return Output{}; }
    Dummy operator()( tbb::flow::continue_msg ) requires (EnableFunctionCallOperator == State::incorrect_return_type) { return Dummy{}; }
};

template <typename O> using Correct = ContinueNodeBody<O, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::correct>;
template <typename O> using NonCopyable = ContinueNodeBody<O, /*CopyCtor = */false, /*Dtor = */true, /*() = */State::correct>;
template <typename O> using NonDestructible = ContinueNodeBody<O, /*CopyCtor = */true, /*Dtor = */false, /*() = */State::correct>;
template <typename O> using NoOperatorRoundBrackets = ContinueNodeBody<O, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::not_defined>;
template <typename O> using WrongInputOperatorRoundBrackets = ContinueNodeBody<O, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::incorrect_first_input>;
template <typename O> using WrongReturnOperatorRoundBrackets = ContinueNodeBody<O, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::incorrect_return_type>;
} // namespace continue_node_body
namespace sequencer {

template <typename T, bool EnableCopyCtor, bool EnableDtor, State EnableFunctionCallOperator>
struct Sequencer {
    Sequencer( const Sequencer& ) requires EnableCopyCtor = default;
    // Prospective destructors
    ~Sequencer() requires EnableDtor = default;
    ~Sequencer() = delete;

    std::size_t operator()( const T& ) requires (EnableFunctionCallOperator == State::correct) { return 0; }
    std::size_t operator()( Dummy ) requires (EnableFunctionCallOperator == State::incorrect_first_input) { return 0; }
    Dummy operator()( const T& ) requires (EnableFunctionCallOperator == State::incorrect_return_type) { return Dummy{}; }
};

template <typename T> using Correct = Sequencer<T, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::correct>;
template <typename T> using NonCopyable = Sequencer<T, /*CopyCtor = */false, /*Dtor = */true, /*() = */State::correct>;
template <typename T> using NonDestructible = Sequencer<T, /*CopyCtor = */true, /*Dtor = */false, /*() = */State::correct>;
template <typename T> using NoOperatorRoundBrackets = Sequencer<T, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::not_defined>;
template <typename T> using WrongInputOperatorRoundBrackets = Sequencer<T, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::incorrect_first_input>;
template <typename T> using WrongReturnOperatorRoundBrackets = Sequencer<T, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::incorrect_return_type>;
} // namespace sequencer
namespace join_node_function_object {

template <typename Input, typename Key, bool EnableCopyCtor, bool EnableDtor, State EnableFunctionCallOperator>
struct JoinNodeFunctionObject {
    JoinNodeFunctionObject( const JoinNodeFunctionObject& ) requires EnableCopyCtor = default;
    // Prospective destructors
    ~JoinNodeFunctionObject() requires EnableDtor = default;
    ~JoinNodeFunctionObject() = delete;

    Key operator()( const Input& ) requires (EnableFunctionCallOperator == State::correct) { return Key{}; }
    Key operator()( Dummy ) requires (EnableFunctionCallOperator == State::incorrect_first_input) { return Key{}; }
    Dummy operator()( const Input& ) requires (EnableFunctionCallOperator == State::incorrect_return_type) { return Dummy{}; }
};

template <typename I, typename K> using Correct = JoinNodeFunctionObject<I, K, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::correct>;
template <typename I, typename K> using NonCopyable = JoinNodeFunctionObject<I, K, /*CopyCtor = */false, /*Dtor = */true, /*() = */State::correct>;
template <typename I, typename K> using NonDestructible = JoinNodeFunctionObject<I, K, /*CopyCtor = */true, /*Dtor = */false, /*() = */State::correct>;
template <typename I, typename K> using NoOperatorRoundBrackets = JoinNodeFunctionObject<I, K, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::not_defined>;
template <typename I, typename K> using WrongInputOperatorRoundBrackets = JoinNodeFunctionObject<I, K, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::incorrect_first_input>;
template <typename I, typename K> using WrongReturnOperatorRoundBrackets = JoinNodeFunctionObject<I, K, /*CopyCtor = */true, /*Dtor = */true, /*() = */State::incorrect_return_type>;

} // namespace join_node_function_object

template <typename T>
concept container_range = tbb::detail::tbb_range<T> &&
                          std::input_iterator<typename T::iterator> &&
                          requires(T& range) {
                              typename T::value_type;
                              typename T::reference;
                              typename T::size_type;
                              typename T::difference_type;
                              { range.begin() } -> std::same_as<typename T::iterator>;
                              { range.end() } -> std::same_as<typename T::iterator>;
                              { std::as_const(range).grainsize() } -> std::same_as<typename T::size_type>;
                          };
} // namespace test_concepts

#endif // __TBB_CPP20_CONCEPTS_PRESENT
#endif // __TBB_test_common_concepts_common_H
