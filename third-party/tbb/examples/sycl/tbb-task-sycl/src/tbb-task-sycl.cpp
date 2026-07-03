/*
    Copyright (c) 2005-2025 Intel Corporation

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

#include <array>
#include <iostream>
#include <numeric>

#include <sycl/sycl.hpp>

#include <tbb/blocked_range.h>
#include <tbb/global_control.h>
#include <tbb/parallel_for.h>
#include "tbb/task_group.h"
// dpc_common.hpp can be found in the dev-utilities include folder.
// e.g., $ONEAPI_ROOT/dev-utilities//include/dpc_common.hpp
#include "dpc_common.hpp"

#define VERBOSE

const float alpha = 0.5; // coeff for triad calculation

const size_t array_size = 16;
std::array<float, array_size> a_array; // input
std::array<float, array_size> b_array; // input
std::array<float, array_size> c_array; // output
std::array<float, array_size> c_array_tbb; // output

class ExecuteOnGpu {
    const char* message;

public:
    ExecuteOnGpu(const char* str) : message(str) {}
    void operator()() const {
        std::cout << message << "\n";

        // By including all the SYCL work in a {} block, we ensure
        // all SYCL tasks must complete before exiting the block
        { // starting SYCL code

            const float coeff = alpha; // coeff is a local variable
            sycl::range<1> n_items{ array_size };
            sycl::buffer a_buffer(a_array);
            sycl::buffer b_buffer(b_array);
            sycl::buffer c_buffer(c_array);

            sycl::queue q(sycl::default_selector_v, dpc_common::exception_handler);
            q.submit([&](sycl::handler& h) {
                 sycl::accessor a_accessor(a_buffer, h, sycl::read_only);
                 sycl::accessor b_accessor(b_buffer, h, sycl::read_only);
                 sycl::accessor c_accessor(c_buffer, h, sycl::write_only);

                 h.parallel_for(n_items, [=](sycl::id<1> index) {
                     c_accessor[index] = a_accessor[index] + b_accessor[index] * coeff;
                 }); // end of the kernel -- parallel for
             }).wait_and_throw(); // end of the commands for the SYCL queue

        } // end of the scope for SYCL code; wait unti queued work completes
    } // operator
};

class ExecuteOnCpu {
    const char* message;

public:
    ExecuteOnCpu(const char* str) : message(str) {}
    void operator()() const {
        std::cout << message << "\n";

        tbb::parallel_for(tbb::blocked_range<int>(0, a_array.size()),
                          [&](tbb::blocked_range<int> r) {
                              for (int index = r.begin(); index < r.end(); ++index) {
                                  c_array_tbb[index] = a_array[index] + b_array[index] * alpha;
                              }
                          });
    } // operator()
};

void PrintArr(const char* text, const std::array<float, array_size>& array) {
    std::cout << text;
    for (const auto& s : array)
        std::cout << s << ' ';
    std::cout << "\n";
}

int main() {
    // init input arrays
    std::iota(a_array.begin(), a_array.end(), 0);
    std::iota(b_array.begin(), b_array.end(), 0);

    // start tbb task group
    tbb::task_group tg;
    //  tbb::task_scheduler_init init(2);
    int nth = 4; // number of threads
    auto mp = tbb::global_control::max_allowed_parallelism;
    tbb::global_control gc(mp, nth);

    tg.run(ExecuteOnGpu("executing on GPU")); // spawn task and return
    tg.run(ExecuteOnCpu("executing on CPU")); // spawn task and return

    tg.wait(); // wait for tasks to complete

    // Serial execution
    std::array<float, array_size> c_gold;

    for (size_t i = 0; i < array_size; ++i)
        c_gold[i] = a_array[i] + alpha * b_array[i];

    // Compare golden triad with heterogeneous triad
    if (!std::equal(std::begin(c_array), std::end(c_array), std::begin(c_gold)))
        std::cout << "Heterogeneous triad error.\n";
    else
        std::cout << "Heterogeneous triad correct.\n";

    // Compare golden triad with TBB triad
    if (!std::equal(std::begin(c_array_tbb), end(c_array_tbb), std::begin(c_gold)))
        std::cout << "TBB triad error.\n";
    else
        std::cout << "TBB triad correct.\n";

#ifdef VERBOSE
    PrintArr("input array a_array: ", a_array);
    PrintArr("input array b_array: ", b_array);
    PrintArr("output array c_array on GPU: ", c_array);
    PrintArr("output array c_array_tbb on CPU: ", c_array_tbb);
#endif

} // main
