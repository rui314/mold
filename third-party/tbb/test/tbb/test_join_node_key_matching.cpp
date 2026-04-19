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

#include "common/config.h"

#include "test_join_node.h"

#include "common/concepts_common.h"

//! \file test_join_node_key_matching.cpp
//! \brief Test for [flow_graph.join_node] specification

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
void test_deduction_guides() {
    using namespace tbb::flow;
    using tuple_type = std::tuple<int, int, double>;

    graph g;
    auto body_int = [](const int&)->int { return 1; };
    auto body_double = [](const double&)->int { return 1; };

    join_node j1(g, body_int, body_int, body_double);
    static_assert(std::is_same_v<decltype(j1), join_node<tuple_type, key_matching<int>>>);

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
    broadcast_node<int> b1(g), b2(g);
    broadcast_node<double> b3(g);
    broadcast_node<tuple_type> b4(g);

    join_node j2(follows(b1, b2, b3), body_int, body_int, body_double);
    static_assert(std::is_same_v<decltype(j2), join_node<tuple_type, key_matching<int>>>);

    join_node j3(precedes(b4), body_int, body_int, body_double);
    static_assert(std::is_same_v<decltype(j3), join_node<tuple_type, key_matching<int>>>);
#endif

    join_node j4(j1);
    static_assert(std::is_same_v<decltype(j4), join_node<tuple_type, key_matching<int>>>);
}
#endif

//! Test serial key matching on special input types
//! \brief \ref error_guessing
TEST_CASE("Serial test on tuples") {
    INFO("key_matching\n");
    generate_test<serial_test, std::tuple<MyKeyFirst<int, double>, MyKeySecond<int, float> >, tbb::flow::key_matching<int> >::do_test();
    generate_test<serial_test, std::tuple<MyKeyFirst<std::string, double>, MyKeySecond<std::string, float> >, tbb::flow::key_matching<std::string> >::do_test();
    generate_test<serial_test, std::tuple<MyKeyFirst<std::string, double>, MyKeySecond<std::string, float>, MyKeyWithBrokenMessageKey<std::string, int> >, tbb::flow::key_matching<std::string&> >::do_test();
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
//! Test deduction guides
//! \brief \ref requirement
TEST_CASE("Test deduction guides"){
    test_deduction_guides();
}
#endif

//! Test parallel key matching on special input types
//! \brief \ref error_guessing
TEST_CASE("Parallel test on tuples"){
    generate_test<parallel_test, std::tuple<MyKeyFirst<int, double>, MyKeySecond<int, float> >, tbb::flow::key_matching<int> >::do_test();
    generate_test<parallel_test, std::tuple<MyKeyFirst<int, double>, MyKeySecond<int, float> >, tbb::flow::key_matching<int&> >::do_test();
    generate_test<parallel_test, std::tuple<MyKeyFirst<std::string, double>, MyKeySecond<std::string, float> >, tbb::flow::key_matching<std::string&> >::do_test();
}

#if __TBB_CPP20_CONCEPTS_PRESENT
template <std::size_t Count>
struct tuple_helper {
    using type = decltype(std::tuple_cat(std::declval<std::tuple<int>>(), std::declval<typename tuple_helper<Count - 1>::type>()));
};

template <>
struct tuple_helper<1> {
    using type = std::tuple<int>;
};

template <typename... Args>
concept can_initialize_join_node = requires(tbb::flow::graph& g, Args... args) {
    tbb::flow::join_node<typename tuple_helper<sizeof...(Args)>::type,
                         tbb::flow::key_matching<int>>(g, args...);
};

// Helper for the concepts which checks if key_matching join_node cannot be instantiated if
// one of its constructor arguments do not satisfy join_node_function_object concept
// This structure substitutes IncorrectT to the sequence of arguments on IncorrectArgIndex position
// The remaining arguments in the sequence are CorrectT
template <std::size_t ArgCount, std::size_t IncorrectArgIndex, typename CorrectT, typename IncorrectT, typename... Args>
struct multiple_arguments_initialization_helper {
    // Current index is not equal to IncorrectArgIndex - substitute CorrectT at the end of the arguments sequence and continue
    static constexpr bool value = multiple_arguments_initialization_helper<ArgCount - 1, IncorrectArgIndex - 1, CorrectT, IncorrectT, Args..., CorrectT>::value;
};

template <std::size_t ArgCount, typename CorrectT, typename IncorrectT, typename... Args>
struct multiple_arguments_initialization_helper<ArgCount, 0, CorrectT, IncorrectT, Args...> {
    // Current index is equal to IncorrectArgIndex - substitute IncorrectT at the end of the sequence and continue
    // No more incorrect indices would be added - continue with MAX_TUPLE_TEST_SIZE variable as current incorrect index
    static constexpr bool value = multiple_arguments_initialization_helper<ArgCount - 1, MAX_TUPLE_TEST_SIZE, CorrectT, IncorrectT, Args..., IncorrectT>::value;
};

template <std::size_t IncorrectArgIndex, typename CorrectT, typename IncorrectT, typename... Args>
struct multiple_arguments_initialization_helper<0, IncorrectArgIndex, CorrectT, IncorrectT, Args...> {
    // ArgCount is equal to 0 - no more arguments should be added
    // Check if join_node can be initialized with Args
    static constexpr bool value = can_initialize_join_node<Args...>;
};

// Helper which iterates over incorrect indices. value is true if initialization is successful for at least for one IncorrectArgIndex
template <std::size_t ArgCount, std::size_t CurrentIncorrectIndex, typename CorrectT, typename IncorrectT>
struct incorrect_arg_index_iteration_helper {
    // CurrentIncorrectIndex is not equal to max - check with current and continue
    static constexpr bool value = multiple_arguments_initialization_helper<ArgCount, CurrentIncorrectIndex, CorrectT, IncorrectT>::value ||
                                  incorrect_arg_index_iteration_helper<ArgCount, CurrentIncorrectIndex + 1, CorrectT, IncorrectT>::value;
};

template <std::size_t ArgCount, std::size_t CurrentIncorrectIndex, typename CorrectT, typename IncorrectT>
requires (ArgCount == CurrentIncorrectIndex + 1)
struct incorrect_arg_index_iteration_helper<ArgCount, CurrentIncorrectIndex, CorrectT, IncorrectT> {
    // CurrentIncorrectIndex is equal to max - check and stop
    static constexpr bool value = multiple_arguments_initialization_helper<ArgCount, CurrentIncorrectIndex, CorrectT, IncorrectT>::value;
};

// Helper which iterates over argument count. value is true if initialization (with all possible incorrect indices) is successful for at least one ArgCount
template <std::size_t CurrentArgCount, typename CorrectT, typename IncorrectT>
struct arg_count_iteration_helper {
    // CurrentArgCount is not equal to max - check and continue
    static constexpr bool value = incorrect_arg_index_iteration_helper<CurrentArgCount, /*StartIncorrectIndex = */0, CorrectT, IncorrectT>::value ||
                                  arg_count_iteration_helper<CurrentArgCount + 1, CorrectT, IncorrectT>::value;
};

template <typename CorrectT, typename IncorrectT>
struct arg_count_iteration_helper<MAX_TUPLE_TEST_SIZE, CorrectT, IncorrectT> {
    // CurrentArgCount is equal to max - check and stop
    static constexpr bool value = incorrect_arg_index_iteration_helper<MAX_TUPLE_TEST_SIZE, /*StartIncorrectIndex = */0, CorrectT, IncorrectT>::value;
};

template <typename CorrectT, typename IncorrectT>
concept can_initialize_join_node_with_incorrect_argument = arg_count_iteration_helper</*StartArgCount = */2, CorrectT, IncorrectT>::value;

template <std::size_t CurrentArgCount, typename CorrectT, typename... Args>
struct join_node_correct_initialization_helper {
    static constexpr bool value = join_node_correct_initialization_helper<CurrentArgCount - 1, CorrectT, Args..., CorrectT>::value;
};

template <typename CorrectT, typename... Args>
struct join_node_correct_initialization_helper<0, CorrectT, Args...> {
    static constexpr bool value = can_initialize_join_node<Args...>;
};

template <std::size_t CurrentArgCount, typename CorrectT>
struct arg_count_correct_initialization_helper {
    static constexpr bool value = join_node_correct_initialization_helper<CurrentArgCount, CorrectT>::value &&
                                  arg_count_correct_initialization_helper<CurrentArgCount + 1, CorrectT>::value;
};

template <typename CorrectT>
struct arg_count_correct_initialization_helper<MAX_TUPLE_TEST_SIZE, CorrectT> {
    static constexpr bool value = join_node_correct_initialization_helper<MAX_TUPLE_TEST_SIZE, CorrectT>::value;
};

template <typename CorrectT>
concept can_initialize_join_node_with_correct_argument = arg_count_correct_initialization_helper</*Start = */2, CorrectT>::value;

//! \brief \ref error_guessing
TEST_CASE("join_node constraints") {
    using namespace test_concepts::join_node_function_object;
    static_assert(can_initialize_join_node_with_correct_argument<Correct<int, int>>);
    static_assert(!can_initialize_join_node_with_incorrect_argument<Correct<int, int>, NonCopyable<int, int>>);
    static_assert(!can_initialize_join_node_with_incorrect_argument<Correct<int, int>, NonDestructible<int, int>>);
    static_assert(!can_initialize_join_node_with_incorrect_argument<Correct<int, int>, NoOperatorRoundBrackets<int, int>>);
    static_assert(!can_initialize_join_node_with_incorrect_argument<Correct<int, int>, WrongInputOperatorRoundBrackets<int, int>>);
    static_assert(!can_initialize_join_node_with_incorrect_argument<Correct<int, int>, WrongReturnOperatorRoundBrackets<int, int>>);
}
#endif // __TBB_CPP20_CONCEPTS_PRESENT
