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

/* Example program that shows how to use parallel_for_each to do parallel preorder
   traversal of a directed acyclic graph. */

#include <cstdlib>

#include <iostream>
#include <vector>

#include "oneapi/tbb/tick_count.h"
#include "oneapi/tbb/global_control.h"
#include "common/utility/utility.hpp"
#include "common/utility/get_default_num_threads.hpp"

#include "Graph.hpp"

// some forward declarations
class Cell;
void ParallelPreorderTraversal(const std::vector<Cell*>& root_set);

//------------------------------------------------------------------------
// Test driver
//------------------------------------------------------------------------
static unsigned nodes = 1000;
static unsigned traversals = 500;
static bool SilentFlag = false;

//! Parse the command line.
static void ParseCommandLine(int argc, char* argv[], utility::thread_number_range& threads) {
    utility::parse_cli_arguments(
        argc,
        argv,
        utility::cli_argument_pack()
            //"-h" option for displaying help is present implicitly
            .positional_arg(threads, "n-of-threads", utility::thread_number_range_desc)
            .positional_arg(nodes, "n-of-nodes", "number of nodes in the graph.")
            .positional_arg(
                traversals,
                "n-of-traversals",
                "number of times to evaluate the graph. Reduce it (e.g. to 100) to shorten example run time\n")
            .arg(SilentFlag, "silent", "no output except elapsed time "));
}

int main(int argc, char* argv[]) {
    utility::thread_number_range threads(utility::get_default_num_threads);
    oneapi::tbb::tick_count main_start = oneapi::tbb::tick_count::now();
    ParseCommandLine(argc, argv, threads);

    // Start scheduler with given number of threads.
    for (int p = threads.first; p <= threads.last; p = threads.step(p)) {
        oneapi::tbb::tick_count t0 = oneapi::tbb::tick_count::now();
        oneapi::tbb::global_control c(oneapi::tbb::global_control::max_allowed_parallelism, p);
        srand(2);
        std::size_t root_set_size = 0;
        {
            Graph g;
            g.create_random_dag(nodes);
            std::vector<Cell*> root_set;
            g.get_root_set(root_set);
            root_set_size = root_set.size();
            for (unsigned int trial = 0; trial < traversals; ++trial) {
                ParallelPreorderTraversal(root_set);
            }
        }
        oneapi::tbb::tick_count::interval_t interval = oneapi::tbb::tick_count::now() - t0;
        if (!SilentFlag) {
            std::cout << interval.seconds() << " seconds using " << p << " threads ("
                      << root_set_size << " nodes in root_set)\n";
        }
    }
    utility::report_elapsed_time((oneapi::tbb::tick_count::now() - main_start).seconds());

    return 0;
}
