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

#ifndef __TBB_test_common_parallel_reduce_common_H
#define __TBB_test_common_parallel_reduce_common_H

#include "test.h"
#include "utils.h"
#include "utils_report.h"
#include "utils_concurrency_limit.h"

#include "oneapi/tbb/parallel_reduce.h"
#include "oneapi/tbb/blocked_range.h"
#include "oneapi/tbb/global_control.h"

//! Type-tag for testing algorithm with default partitioner
struct utils_default_partitioner {};

template <typename Range, typename Body, typename Partitioner>
void reduce_invoker(const Range& range, Body& body, Partitioner& partiotioner) {
    tbb::parallel_reduce( range, body, partiotioner );
}

template <typename Range, typename Body>
void reduce_invoker(const Range& range, Body& body, utils_default_partitioner&) {
    tbb::parallel_reduce( range, body );
}

template <typename ResultType, typename Range, typename Func, typename Reduction, typename Partitioner>
ResultType reduce_invoker(const Range& range, const Func& func, const Reduction& reduction, Partitioner& partiotioner) {
    return tbb::parallel_reduce( range, ResultType(), func, reduction, partiotioner );
}

template <typename ResultType, typename Range, typename Func, typename Reduction>
ResultType reduce_invoker(const Range& range, const Func& func, const Reduction& reduction, utils_default_partitioner&) {
    return tbb::parallel_reduce( range, ResultType(), func, reduction );
}

template <typename Range, typename Body, typename Partitioner>
void deterministic_reduce_invoker(const Range& range, Body& body, const Partitioner& partiotioner) {
    tbb::parallel_deterministic_reduce( range, body, partiotioner );
}

template <typename Range, typename Body>
void deterministic_reduce_invoker(const Range& range, Body& body, const utils_default_partitioner&) {
    tbb::parallel_deterministic_reduce( range, body );
}

template <typename ResultType, typename Range, typename Func, typename Reduction, typename Partitioner>
ResultType deterministic_reduce_invoker(const Range& range, const Func& func, 
                                        const Reduction& reduction, const Partitioner& partiotioner) {
    return tbb::parallel_deterministic_reduce( range, ResultType(), func, reduction, partiotioner );
}

template <typename ResultType, typename Range, typename Func, typename Reduction>
ResultType deterministic_reduce_invoker(const Range& range, const Func& func, 
                                        const Reduction& reduction, const utils_default_partitioner&) {
    return tbb::parallel_deterministic_reduce( range, ResultType(), func, reduction );
}

#endif // __TBB_test_common_parallel_reduce_common_H
