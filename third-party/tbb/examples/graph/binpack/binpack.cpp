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

/* Bin-packing algorithm that attempts to use minimal number of bins B of
   size desired_bin_capacity to contain elements_num items of varying sizes. */

#include <cmath>

#include <string>
#include <iostream>
#include <tuple>
#include <vector>
#include <atomic>
#include <algorithm>

#include "oneapi/tbb/tick_count.h"
#include "oneapi/tbb/flow_graph.h"
#include "oneapi/tbb/global_control.h"

#include "common/utility/utility.hpp"
#include "common/utility/get_default_num_threads.hpp"

typedef std::size_t size_type; // to represent non-zero indices, capacities, etc.
typedef std::size_t value_type; // the type of items we are attempting to pack into bins
typedef std::vector<value_type> bin; // we use a simple vector to represent a bin
// Our bin packers will be function nodes in the graph that take value_type items and
// return a dummy value.  They will also implicitly send packed bins to the bin_buffer
// node, and unused items back to the value_pool node:
typedef oneapi::tbb::flow::
    multifunction_node<value_type, std::tuple<value_type, bin>, oneapi::tbb::flow::rejecting>
        bin_packer;
// Items are placed into a pool that all bin packers grab from, represent by a queue_node:
typedef oneapi::tbb::flow::queue_node<value_type> value_pool;
// Packed bins are placed in this buffer waiting to be serially printed and/or accounted for:
typedef oneapi::tbb::flow::buffer_node<bin> bin_buffer;
// Packed bins are taken from the_bin_buffer and processed by the_writer:
typedef oneapi::tbb::flow::
    function_node<bin, oneapi::tbb::flow::continue_msg, oneapi::tbb::flow::rejecting>
        bin_writer;
// Items are injected into the graph when this node sends them to the_value_pool:
typedef oneapi::tbb::flow::input_node<value_type> value_source;

// User-specified globals with default values
size_type desired_bin_capacity = 42;
size_type elements_num = 1000; // number of elements to generate
bool verbose = false; // prints bin details and other diagnostics to screen
bool silent = false; // suppress all output except for time
int num_bin_packers = -1; // number of concurrent bin packers in operation; default is #threads;
    // larger values can result in more bins at less than full capacity
size_type optimality =
    1; // 1 (default) is highest the algorithm can obtain; larger numbers run faster

// Calculated globals
size_type bins_num_min; // lower bound on the optimal number of bins
size_type bins_num; // the answer, i.e. number of bins used by the algorithm
std::vector<size_type> input_array; // stores randomly generated input values
value_type item_sum; // sum of all randomly generated input values
std::atomic<value_type> packed_sum; // sum of all values currently packed into all bins
std::atomic<size_type> packed_items; // number of values currently packed into all bins
std::atomic<size_type> active_bins; // number of active bin_packers
std::vector<bin_packer*> bins; // the array of bin packers

// This class is the Body type for bin_packer
class bin_filler {
    typedef bin_packer::output_ports_type ports_type;
    bin my_bin; // the current bin that this bin_filler is packing
    size_type
        my_used; // capacity of bin used by current contents (not to be confused with my_bin.size())
    size_type relax,
        relax_val; // relaxation counter for determining when to settle for a non-full bin
    bin_packer* my_bin_packer; // ptr to the bin packer that this body object is associated with
    size_type bin_index; // index of the encapsulating bin packer in the global bins array
    value_type looking_for; // the minimum size of item this bin_packer will accept
    value_pool* the_value_pool; // the queue of incoming values
    bool done; // flag to indicate that this binpacker has been deactivated
public:
    bin_filler(std::size_t bidx, value_pool* _q)
            : my_used(0),
              relax(0),
              relax_val(0),
              my_bin_packer(nullptr),
              bin_index(bidx),
              looking_for(desired_bin_capacity),
              the_value_pool(_q),
              done(false) {}
    void operator()(const value_type& item, ports_type& p) {
        if (!my_bin_packer)
            my_bin_packer = bins[bin_index];
        if (done)
            // this bin_packer is done packing items; put item back to pool
            std::get<0>(p).try_put(item);
        else if (
            item >
            desired_bin_capacity) { // signal that packed_sum has reached item_sum at some point
            size_type remaining = active_bins--;
            if (remaining == 1 &&
                packed_sum == item_sum) { // this is the last bin and it has seen everything
                // this bin_packer may not have seen everything, so stay active
                if (my_used > 0)
                    std::get<1>(p).try_put(my_bin);
                my_bin.clear();
                my_used = 0;
                looking_for = desired_bin_capacity;
                ++active_bins;
            }
            else if (remaining == 1) { // this is the last bin, but there are remaining items
                std::get<0>(p).try_put(desired_bin_capacity + 1); // send out signal
                ++active_bins;
            }
            else if (remaining > 1) { // this is not the last bin; deactivate
                // this bin is ill-utilized; throw back items and deactivate
                if (my_used < desired_bin_capacity / (1 + optimality * .1)) {
                    packed_sum -= my_used;
                    packed_items -= my_bin.size();
                    for (size_type i = 0; i < my_bin.size(); ++i)
                        std::get<0>(p).try_put(my_bin[i]);
                    oneapi::tbb::flow::remove_edge(*the_value_pool, *my_bin_packer); // deactivate
                    done = true;
                    std::get<0>(p).try_put(desired_bin_capacity + 1); // send out signal
                }
                else { // this bin is well-utilized; send out bin and deactivate
                    oneapi::tbb::flow::remove_edge(*the_value_pool,
                                                   *my_bin_packer); // build no more bins
                    done = true;
                    if (my_used > 0)
                        std::get<1>(p).try_put(my_bin);
                    std::get<0>(p).try_put(desired_bin_capacity + 1); // send out signal
                }
            }
        }
        else if (item <= desired_bin_capacity - my_used &&
                 item >= looking_for) { // this item can be packed
            my_bin.push_back(item);
            my_used += item;
            packed_sum += item;
            ++packed_items;
            looking_for = desired_bin_capacity - my_used;
            relax = 0;
            if (packed_sum == item_sum) {
                std::get<0>(p).try_put(desired_bin_capacity + 1); // send out signal
            }
            if (my_used == desired_bin_capacity) {
                std::get<1>(p).try_put(my_bin);
                my_bin.clear();
                my_used = 0;
                looking_for = desired_bin_capacity;
            }
        }
        else { // this item can't be packed; relax constraints
            ++relax;
            // this bin_packer has looked through enough items
            if (relax >= (elements_num - packed_items) / optimality) {
                relax = 0;
                --looking_for; // accept a wider range of items
                if (looking_for == 0 && my_used < desired_bin_capacity / (1 + optimality * .1) &&
                    my_used > 0 && active_bins > 1) {
                    // this bin_packer is ill-utilized and can't find items; deactivate and throw back items
                    size_type remaining = active_bins--;
                    if (remaining > 1) { // not the last bin_packer
                        oneapi::tbb::flow::remove_edge(*the_value_pool,
                                                       *my_bin_packer); // deactivate
                        done = true;
                    }
                    else
                        active_bins++; // can't deactivate last bin_packer
                    packed_sum -= my_used;
                    packed_items -= my_bin.size();
                    for (size_type i = 0; i < my_bin.size(); ++i)
                        std::get<0>(p).try_put(my_bin[i]);
                    my_bin.clear();
                    my_used = 0;
                }
                else if (looking_for == 0 &&
                         (my_used >= desired_bin_capacity / (1 + optimality * .1) ||
                          active_bins == 1)) {
                    // this bin_packer can't find items but is well-utilized, so send it out and reset
                    std::get<1>(p).try_put(my_bin);
                    my_bin.clear();
                    my_used = 0;
                    looking_for = desired_bin_capacity;
                }
            }
            std::get<0>(p).try_put(item); // put unused item back to pool
        }
    }
};

// input node uses this to send the values to the value_pool
class item_generator {
    size_type counter;

public:
    item_generator() : counter(0) {}
    value_type operator()(oneapi::tbb::flow_control& fc) {
        if (counter < elements_num) {
            value_type result = input_array[counter];
            ++counter;
            return result;
        }

        fc.stop();
        return value_type{};
    }
};

// the terminal function_node uses this to gather stats and print bin information
class bin_printer {
    value_type running_count;
    size_type item_count;
    value_type my_min, my_max;
    double avg;

public:
    bin_printer()
            : running_count(0),
              item_count(0),
              my_min(desired_bin_capacity),
              my_max(0),
              avg(0) {}
    oneapi::tbb::flow::continue_msg operator()(bin b) {
        value_type sum = 0;
        ++bins_num;
        if (verbose)
            std::cout << "[ ";
        for (size_type i = 0; i < b.size(); ++i) {
            if (verbose)
                std::cout << b[i] << " ";
            sum += b[i];
            ++item_count;
        }
        my_min = std::min(sum, my_min);
        my_max = std::max(sum, my_max);
        avg += sum;
        running_count += sum;
        if (verbose) {
            std::cout << "]=" << sum << "; Done/Packed/Total cap: " << running_count << "/"
                      << packed_sum << "/" << item_sum << " items:" << item_count << "/"
                      << packed_items << "/" << elements_num << " bins_num=" << bins_num << "\n";
        }
        if (item_count == elements_num) { // should be the last; print stats
            avg = avg / (double)bins_num;
            if (!silent)
                std::cout << "SUMMARY: #Bins used: " << bins_num << "; Avg size: " << avg
                          << "; Max size: " << my_max << "; Min size: " << my_min << "\n"
                          << "         Lower bound on optimal #bins: " << bins_num_min
                          << "; Start #bins: " << num_bin_packers << "\n";
        }
        return oneapi::tbb::flow::continue_msg(); // need to return something
    }
};

int main(int argc, char* argv[]) {
    utility::thread_number_range threads(utility::get_default_num_threads);
    utility::parse_cli_arguments(
        argc,
        argv,
        utility::cli_argument_pack()
            //"-h" option for displaying help is present implicitly
            .positional_arg(threads, "#threads", utility::thread_number_range_desc)
            .arg(verbose, "verbose", "   print diagnostic output to screen")
            .arg(silent, "silent", "    limits output to timing info; overrides verbose")
            .arg(elements_num, "elements_num", "         number of values to pack")
            .arg(desired_bin_capacity, "bin_capacity", "         capacity of each bin")
            .arg(num_bin_packers,
                 "#packers",
                 "  number of concurrent bin packers to use "
                 "(default=#threads)")
            .arg(optimality,
                 "optimality",
                 "controls optimality of solution; 1 is highest, use\n"
                 "              larger numbers for less optimal but faster solution"));

    if (silent)
        verbose = false; // make silent override verbose
    // Generate random input data
    srand(42);
    input_array.resize(elements_num);
    item_sum = 0;
    for (auto& item : input_array) {
        item = rand() % desired_bin_capacity + 1; // generate items that fit in a bin
        item_sum += item;
    }
    bins_num_min = (item_sum % desired_bin_capacity) ? item_sum / desired_bin_capacity + 1
                                                     : item_sum / desired_bin_capacity;

    oneapi::tbb::tick_count start = oneapi::tbb::tick_count::now();
    for (int p = threads.first; p <= threads.last; p = threads.step(p)) {
        oneapi::tbb::global_control c(oneapi::tbb::global_control::max_allowed_parallelism, p);
        packed_sum = 0;
        packed_items = 0;
        bins_num = 0;
        if (num_bin_packers == -1)
            num_bin_packers = p;
        active_bins = num_bin_packers;
        if (!silent)
            std::cout << "binpack running with " << item_sum << " capacity over " << elements_num
                      << " items, optimality=" << optimality << ", " << num_bin_packers
                      << " bins of capacity=" << desired_bin_capacity << " on " << p << " threads."
                      << "\n";
        oneapi::tbb::flow::graph g;
        value_source the_source(g, item_generator());
        value_pool the_value_pool(g);
        oneapi::tbb::flow::make_edge(the_source, the_value_pool);
        bin_buffer the_bin_buffer(g);
        bins.resize(num_bin_packers);
        for (int i = 0; i < num_bin_packers; ++i) {
            bins[i] = new bin_packer(g, 1, bin_filler(i, &the_value_pool));
            oneapi::tbb::flow::make_edge(the_value_pool, *(bins[i]));
            oneapi::tbb::flow::make_edge(oneapi::tbb::flow::output_port<0>(*(bins[i])),
                                         the_value_pool);
            oneapi::tbb::flow::make_edge(oneapi::tbb::flow::output_port<1>(*(bins[i])),
                                         the_bin_buffer);
        }
        bin_writer the_writer(g, 1, bin_printer());
        make_edge(the_bin_buffer, the_writer);
        the_source.activate();
        g.wait_for_all();
        for (int i = 0; i < num_bin_packers; ++i) {
            delete bins[i];
        }
    }
    utility::report_elapsed_time((oneapi::tbb::tick_count::now() - start).seconds());
    return 0;
}
