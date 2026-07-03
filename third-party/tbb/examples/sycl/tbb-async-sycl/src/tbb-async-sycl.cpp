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

#include <cmath> //for std::ceil
#include <array>
#include <atomic>
#include <iostream>
#include <thread>
#include <numeric>

#include <sycl/sycl.hpp>

#include <tbb/blocked_range.h>
#include <tbb/flow_graph.h>
#include <tbb/global_control.h>
#include <tbb/parallel_for.h>
// dpc_common.hpp can be found in the dev-utilities include folder.
// e.g., $ONEAPI_ROOT/dev-utilities//include/dpc_common.hpp
#include "dpc_common.hpp"

struct done_tag {};

const float ratio = 0.5; // CPU to GPU offload ratio
const float alpha = 0.5; // coeff for triad calculation

const size_t array_size = 16;
std::array<float, array_size> a_array; // input
std::array<float, array_size> b_array; // input
std::array<float, array_size> c_array; // output

void PrintArr(const char* text, const std::array<float, array_size>& array) {
    std::cout << text;
    for (const auto& s : array)
        std::cout << s << ' ';
    std::cout << "\n";
}

using async_node_type = tbb::flow::async_node<float, done_tag>;
using gateway_type = async_node_type::gateway_type;

class AsyncActivity {
    gateway_type* gateway_ptr;
    float offload_ratio;
    std::atomic<bool> submit_flag;
    std::thread service_thread;

public:
    AsyncActivity()
            : gateway_ptr(nullptr),
              offload_ratio(0),
              submit_flag(false),
              service_thread([this] {
                  while (!submit_flag) {
                      std::this_thread::yield();
                  }
                  // Execute the kernel over a portion of the array range
                  size_t array_size_sycl = std::ceil(array_size * offload_ratio);
                  std::cout << "start index for GPU = 0; end index for GPU = " << array_size_sycl
                            << "\n";
                  const float coeff = alpha; // coeff is a local variable

                  // By including all the SYCL work in a {} block, we ensure
                  // all SYCL tasks must complete before exiting the block
                  { // starting SYCL code
                      sycl::range<1> n_items{ array_size_sycl };
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
                       }).wait();
                  } // end of the scope for SYCL code; wait unti queued work completes;

                  gateway_ptr->try_put(done_tag{});
                  gateway_ptr->release_wait();
              }) {}

    ~AsyncActivity() {
        service_thread.join();
    }

    void submit(float ratio, gateway_type& gateway) {
        gateway.reserve_wait();
        offload_ratio = ratio;
        gateway_ptr = &gateway;
        submit_flag = true;
    }
};

int main() {
    // init input arrays
    std::iota(a_array.begin(), a_array.end(), 0);
    std::iota(b_array.begin(), b_array.end(), 0);

    int nth = 4; // number of threads

    auto mp = tbb::global_control::max_allowed_parallelism;
    tbb::global_control gc(mp, nth + 1); // One more thread, but sleeping
    tbb::flow::graph g;

    // Input node:
    tbb::flow::input_node<float> in_node{ g, [&](tbb::flow_control& fc) -> float {
                                             static bool has_run = false;
                                             if (has_run)
                                                 fc.stop();
                                             has_run = true;
                                             return ratio;
                                         } };

    // CPU node
    tbb::flow::function_node<float, done_tag> cpu_node{
        g,
        tbb::flow::unlimited,
        [&](float offload_ratio) {
            size_t i_start = static_cast<size_t>(std::ceil(array_size * offload_ratio));
            size_t i_end = static_cast<size_t>(array_size);
            std::cout << "start index for CPU = " << i_start << "; end index for CPU = " << i_end
                      << "\n";

            tbb::parallel_for(tbb::blocked_range<size_t>{ i_start, i_end },
                              [&](const tbb::blocked_range<size_t>& r) {
                                  for (size_t i = r.begin(); i < r.end(); ++i)
                                      c_array[i] = a_array[i] + alpha * b_array[i];
                              });
            return done_tag{};
        }
    };

    // async node -- GPU
    AsyncActivity async_act;
    async_node_type a_node{ g,
                            tbb::flow::unlimited,
                            [&async_act](const float& offload_ratio, gateway_type& gateway) {
                                async_act.submit(offload_ratio, gateway);
                            } };

    // join node
    using join_t = tbb::flow::join_node<std::tuple<done_tag, done_tag>, tbb::flow::queueing>;
    join_t node_join{ g };

    // out node
    tbb::flow::function_node<join_t::output_type> out_node{
        g,
        tbb::flow::unlimited,
        [&](const join_t::output_type&) {
            // Serial execution
            std::array<float, array_size> c_gold;
            for (size_t i = 0; i < array_size; ++i)
                c_gold[i] = a_array[i] + alpha * b_array[i];

            // Compare golden triad with heterogeneous triad
            if (!std::equal(std::begin(c_array), std::end(c_array), std::begin(c_gold)))
                std::cout << "Heterogeneous triad error.\n";
            else
                std::cout << "Heterogeneous triad correct.\n";

            PrintArr("c_array: ", c_array);
            PrintArr("c_gold : ", c_gold);
        }
    }; // end of out node

    // construct graph
    tbb::flow::make_edge(in_node, a_node);
    tbb::flow::make_edge(in_node, cpu_node);
    tbb::flow::make_edge(a_node, tbb::flow::input_port<0>(node_join));
    tbb::flow::make_edge(cpu_node, tbb::flow::input_port<1>(node_join));
    tbb::flow::make_edge(node_join, out_node);

    in_node.activate();
    g.wait_for_all();

    return 0;
}
