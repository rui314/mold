/*
    Copyright (c) 2022 Intel Corporation

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

// Message based key matching is a preview feature
#define TBB_PREVIEW_FLOW_GRAPH_FEATURES 1

#include "common/config.h"

#include "test_join_node.h"

//! \file test_join_node_msg_key_matching_n_args.cpp
//! \brief Test for [preview] functionality

template <typename T1, typename T2>
using make_tuple = decltype(std::tuple_cat(T1(), std::tuple<T2>()));
using T1 = std::tuple<MyMessageKeyWithoutKeyMethod<std::string, double>>;
using T2 = make_tuple<T1, MyMessageKeyWithBrokenKey<std::string, int>>;
using T3 = make_tuple < T2, MyMessageKeyWithoutKey<std::string, int>>;
using T4 = make_tuple < T3, MyMessageKeyWithoutKeyMethod<std::string, size_t>>;
using T5 = make_tuple < T4, MyMessageKeyWithBrokenKey<std::string, int>>;
using T6 = make_tuple < T5, MyMessageKeyWithoutKeyMethod<std::string, short>>;
using T7 = make_tuple < T6, MyMessageKeyWithoutKeyMethod<std::string, threebyte>>;
using T8 = make_tuple < T7, MyMessageKeyWithBrokenKey<std::string, int>>;
using T9 = make_tuple < T8, MyMessageKeyWithoutKeyMethod<std::string, threebyte>>;
using T10 = make_tuple < T9, MyMessageKeyWithBrokenKey<std::string, size_t>>;

#if TBB_TEST_LOW_WORKLOAD && TBB_USE_DEBUG
// the compiler might generate huge object file in debug (>64M)
#define TEST_CASE_TEMPLATE_N_ARGS(dec) TEST_CASE_TEMPLATE(dec, T, T2, T10)
#else
#define TEST_CASE_TEMPLATE_N_ARGS(dec) TEST_CASE_TEMPLATE(dec, T, T2, T3, T4, T5, T6, T7, T8, T9, T10)
#endif

//! Serial test with different tuple sizes
//! \brief \ref error_guessing
TEST_CASE_TEMPLATE_N_ARGS("Serial N tests") {
    generate_test<serial_test, T, message_based_key_matching<std::string&> >::do_test();
}

//! Parallel test with different tuple sizes
//! \brief \ref error_guessing
TEST_CASE_TEMPLATE_N_ARGS("Parallel N tests") {
    generate_test<parallel_test, T, message_based_key_matching<std::string&> >::do_test();
}
