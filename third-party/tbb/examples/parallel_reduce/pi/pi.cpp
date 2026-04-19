/*
    Copyright (c) 2023 Intel Corporation

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

#include "common.h"
#include "oneapi/tbb/blocked_range.h"
#include "oneapi/tbb/global_control.h"
#include "oneapi/tbb/parallel_reduce.h"

struct reduce_body {
    double my_pi;
    reduce_body() : my_pi(0) {}
    reduce_body(reduce_body& x, tbb::split) : my_pi(0) {}
    void operator()(const tbb::blocked_range<number_t>& r) {
        my_pi += pi_slice_kernel(r.begin(), r.size());
    }
    void join(const reduce_body& y) {
        my_pi += y.my_pi;
    }
};

double compute_pi_parallel() {
    step = pi_t(1.0) / num_intervals;

    double ret = 0.0;

    reduce_body body;
    tbb::parallel_reduce(tbb::blocked_range<number_t>(0, num_intervals), body);

    ret = body.my_pi * step;

    return ret;
}

static std::unique_ptr<tbb::global_control> gc;

threading::threading(int p) {
    gc.reset(new tbb::global_control(tbb::global_control::max_allowed_parallelism, p));
}

threading::~threading() {
    gc.reset();
}
