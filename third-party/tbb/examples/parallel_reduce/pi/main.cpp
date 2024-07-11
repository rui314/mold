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

#include "oneapi/tbb/tick_count.h"

#include "common/utility/get_default_num_threads.hpp"
#include "common/utility/utility.hpp"

#include "common.h"

const number_t chunk_size = 4096; // Multiple of 16, to fit float datatype to a vector register.

// number of intervals
number_t num_intervals = 1000000000;
pi_t step = pi_t(0.0);

bool silent = false;

double compute_pi_serial() {
    double ret = 0;

    step = pi_t(1.0) / num_intervals;

    number_t tail = num_intervals % chunk_size;
    number_t last = num_intervals - tail;

    for (number_t slice = 0; slice < last; slice += chunk_size) {
        ret += pi_slice_kernel(slice);
    }
    ret += pi_slice_kernel(last, tail);
    ret *= step;

    return ret;
}

int main(int argc, char* argv[]) {
    try {
        tbb::tick_count main_start_time = tbb::tick_count::now();
        // zero number of threads means to run serial version
        utility::thread_number_range threads(utility::get_default_num_threads, 0);

        utility::parse_cli_arguments(
            argc,
            argv,
            utility::cli_argument_pack()
                //"-h" option for for displaying help is present implicitly
                .positional_arg(threads, "n-of-threads", utility::thread_number_range_desc)
                .positional_arg(num_intervals, "n-of-intervals", "number of intervals")
                .arg(silent, "silent", "no output except time elapsed"));

        for (int p = threads.first; p <= threads.last; p = threads.step(p)) {
            pi_t pi;
            double compute_time;
            if (p == 0) {
                //run a serial version
                tbb::tick_count compute_start_time = tbb::tick_count::now();
                pi = compute_pi_serial();
                compute_time = (tbb::tick_count::now() - compute_start_time).seconds();
            }
            else {
                //run a parallel version
                threading tp(p);
                tbb::tick_count compute_start_time = tbb::tick_count::now();
                pi = compute_pi_parallel();
                compute_time = (tbb::tick_count::now() - compute_start_time).seconds();
            }

            if (!silent) {
                if (p == 0) {
                    std::cout << "Serial run:\tpi = " << pi << "\tcompute time = " << compute_time
                              << " sec\n";
                }
                else {
                    std::cout << "Parallel run:\tpi = " << pi << "\tcompute time = " << compute_time
                              << " sec\t on " << p << " threads\n";
                }
            }
        }

        utility::report_elapsed_time((tbb::tick_count::now() - main_start_time).seconds());
        return 0;
    }
    catch (std::exception& e) {
        std::cerr << "error occurred. error text is :\"" << e.what() << "\"\n";
        return 1;
    }
}
