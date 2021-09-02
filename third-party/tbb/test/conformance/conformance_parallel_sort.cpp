/*
    Copyright (c) 2020-2021 Intel Corporation

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
#include "common/utils_concurrency_limit.h"

#include "oneapi/tbb/parallel_sort.h"
#include "oneapi/tbb/global_control.h"

#include <vector>

//! \file conformance_parallel_sort.cpp
//! \brief Test for [algorithms.parallel_sort]

const int vector_size = 10000;

std::vector<int> get_random_vector() {
    std::vector<int> result(vector_size);
    for (int i = 0; i < vector_size; ++i)
        result[i] = rand() % vector_size;
    return result;
}

//! Iterator based range sorting test (default comparator)
//! \brief \ref requirement \ref interface
TEST_CASE("Iterator based range sorting test (default comparator)") {
    for ( auto concurrency_level : utils::concurrency_range() ) {
        oneapi::tbb::global_control control(oneapi::tbb::global_control::max_allowed_parallelism, concurrency_level);

        auto test_vector = get_random_vector();
        oneapi::tbb::parallel_sort(test_vector.begin(), test_vector.end());

        for(auto it = test_vector.begin(); it != test_vector.end() - 1; ++it)
            REQUIRE_MESSAGE(*it <= *(it+1), "Testing data not sorted");
    }
}

//! Iterator based range sorting test (greater comparator)
//! \brief \ref requirement \ref interface
TEST_CASE ("Iterator based range sorting test (greater comparator)") {
    for ( auto concurrency_level : utils::concurrency_range() ) {
        oneapi::tbb::global_control control(oneapi::tbb::global_control::max_allowed_parallelism, concurrency_level);

        auto test_vector = get_random_vector();
        oneapi::tbb::parallel_sort(test_vector.begin(), test_vector.end(), std::greater<int>());

        for(auto it = test_vector.begin(); it != test_vector.end() - 1; ++it)
            REQUIRE_MESSAGE(*it >= *(it+1), "Testing data not sorted");
    }
}

//! Range sorting test (default comparator)
//! \brief \ref requirement \ref interface
TEST_CASE ("Range sorting test (default comparator)") {
    for ( auto concurrency_level : utils::concurrency_range() ) {
        oneapi::tbb::global_control control(oneapi::tbb::global_control::max_allowed_parallelism, concurrency_level);

        auto test_vector = get_random_vector();
        oneapi::tbb::parallel_sort(test_vector);

        for(auto it = test_vector.begin(); it != test_vector.end() - 1; ++it)
            REQUIRE_MESSAGE(*it <= *(it+1), "Testing data not sorted");
    }
}

//! Range sorting test (greater comparator)
//! \brief \ref requirement \ref interface
TEST_CASE ("Range sorting test (greater comparator)") {
    for ( auto concurrency_level : utils::concurrency_range() ) {
        oneapi::tbb::global_control control(oneapi::tbb::global_control::max_allowed_parallelism, concurrency_level);

        auto test_vector = get_random_vector();
        oneapi::tbb::parallel_sort(test_vector, std::greater<int>());

        for(auto it = test_vector.begin(); it != test_vector.end() - 1; ++it)
            REQUIRE_MESSAGE(*it >= *(it+1), "Testing data not sorted");
    }
}
