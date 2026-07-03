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
#include <atomic>
#include <cmath>
#include <iostream>
#include <sstream>
#include <thread>

#include <sycl/sycl.hpp>

#include <tbb/blocked_range.h>
#include <tbb/global_control.h>
#include <tbb/task.h>
#include <tbb/task_group.h>
#include <tbb/parallel_for.h>

constexpr float ratio = 0.5; // CPU or GPU offload ratio
constexpr float alpha = 0.5; // coeff for triad calculation

constexpr std::size_t array_size = 16;
std::array<float, array_size> a_array; // input array
std::array<float, array_size> b_array; // input array
std::array<float, array_size> c_array; // output array

void print_array(const char* text, const std::array<float, array_size>& array) {
    std::cout << text;
    for (const auto& s : array)
        std::cout << s << ' ';
    std::cout << std::endl;
}

class AsyncActivity {
    float offload_ratio;
    std::atomic<bool> submit_flag;
    tbb::task::suspend_point suspend_point;
    std::thread service_thread;

public:
    AsyncActivity()
            : offload_ratio(0),
              submit_flag(false),
              service_thread([this] {
                  // Wait until the job will be submitted into the async activity
                  while (!submit_flag)
                      std::this_thread::yield();

                  std::size_t array_size_sycl = std::ceil(array_size * offload_ratio);

                  // Note that this lambda will be executed concurrently with the task
                  // passed into tbb::task_group
                  std::stringstream sstream;
                  sstream << "start index for GPU = 0; end index for GPU = " << array_size_sycl
                          << std::endl;
                  std::cout << sstream.str();
                  const float coeff = alpha; // coeff is a local variable

                  { // starting SYCL code
                      sycl::range<1> n_items{ array_size_sycl };
                      sycl::buffer<cl_float, 1> a_buffer(a_array.data(), n_items);
                      sycl::buffer<cl_float, 1> b_buffer(b_array.data(), n_items);
                      sycl::buffer<cl_float, 1> c_buffer(c_array.data(), n_items);

                      sycl::queue q;
                      q.submit([&](sycl::handler& h) {
                           auto a_accessor = a_buffer.get_access<sycl::access::mode::read>(h);
                           auto b_accessor = b_buffer.get_access<sycl::access::mode::read>(h);
                           auto c_accessor = c_buffer.get_access<sycl::access::mode::write>(h);

                           h.parallel_for(n_items, [=](sycl::id<1> index) {
                               c_accessor[index] = a_accessor[index] + coeff * b_accessor[index];
                           }); // end of the kernel
                       }).wait();
                  }

                  // Pass a signal into the main thread that the GPU work is completed
                  tbb::task::resume(suspend_point);
              }) {}

    ~AsyncActivity() {
        service_thread.join();
    }

    void submit(float ratio, tbb::task::suspend_point sus_point) {
        offload_ratio = ratio;
        suspend_point = sus_point;
        submit_flag = true;
    }
}; // class AsyncActivity

int main() {
    // init input arrays
    for (std::size_t i = 0; i < array_size; ++i) {
        a_array[i] = i;
        b_array[i] = i;
    }

    std::size_t n_threads = 4;
    tbb::global_control gc(tbb::global_control::max_allowed_parallelism,
                           n_threads + 1); // One more thread, but sleeping
    tbb::task_group tg;
    AsyncActivity activity;

    // Run CPU part
    // tg.run is a non- blocking call
    tg.run([&] {
        std::size_t i_start = static_cast<std::size_t>(std::ceil(array_size * ratio));
        std::size_t i_end = array_size;
        std::stringstream sstream;
        sstream << "start index for CPU = " << i_start << "; end index for CPU = " << i_end
                << std::endl;
        std::cout << sstream.str();
        tbb::parallel_for(i_start, i_end, [](std::size_t index) {
            c_array[index] = a_array[index] + alpha * b_array[index];
        });
    });

    // Run GPU part
    tbb::task::suspend([&](tbb::task::suspend_point suspend_point) {
        activity.submit(ratio, suspend_point);
    });

    tg.wait();

    // Serial execution
    std::array<float, array_size> c_gold;
    for (std::size_t i = 0; i < array_size; ++i) {
        c_gold[i] = a_array[i] + alpha * b_array[i];
    }

    // Compare golden triad with heterogeneous triad
    if (c_array != c_gold) {
        std::cout << "Heterogeneous triad error." << std::endl;
    }
    else {
        std::cout << "Heterogeneous triad correct." << std::endl;
    }

    print_array("c_array: ", c_array);
    print_array("c_gold: ", c_gold);
}
