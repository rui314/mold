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

#if __INTEL_COMPILER && _MSC_VER
#pragma warning(disable : 2586) // decorated name length exceeded, name was truncated
#endif

#define DOCTEST_CONFIG_SUPER_FAST_ASSERTS
#include "common/test.h"

#include "tbb/spin_mutex.h"
#include "tbb/spin_rw_mutex.h"
#include "tbb/queuing_mutex.h"
#include "tbb/queuing_rw_mutex.h"

//! \file test_profiling.cpp
//! \brief Test for [internal] functionality

//! Test for mutexes set_name
//! \brief \ref interface
TEST_CASE_TEMPLATE("Mutexes set_name", Mutex, tbb::spin_mutex,
                                              tbb::spin_rw_mutex,
                                              tbb::speculative_spin_mutex,
                                              tbb::speculative_spin_rw_mutex,
                                              tbb::queuing_mutex,
                                              tbb::queuing_rw_mutex) {
    Mutex mutex;
    tbb::profiling::set_name(mutex, "mutex");
}


#if (_WIN32||_WIN64)
//! Test for mutexes set_name with wchar
//! \brief \ref interface
TEST_CASE_TEMPLATE("Mutexes set_name wchar", Mutex, tbb::spin_mutex,
                                              tbb::spin_rw_mutex,
                                              tbb::speculative_spin_mutex,
                                              tbb::speculative_spin_rw_mutex,
                                              tbb::queuing_mutex,
                                              tbb::queuing_rw_mutex) {
    Mutex mutex;
    tbb::profiling::set_name(mutex, L"mutex");
}
#endif //WIN

#include "tbb/flow_graph.h"

struct Body {
    template<typename... Args>
    int operator()(Args...) { return 0; }
};

//! Test for flow graph set_name
//! \brief \ref interface
TEST_CASE("Flow Graph set_name") {
    tbb::flow::graph g;
    tbb::profiling::set_name(g, "graph");
}

struct async_body
{
    template<typename T>
    void operator()(int, T&) {};
};

//! Test for async_node set_name
//! \brief \ref interface
TEST_CASE("async_node set_name") {
    tbb::flow::graph g;
    tbb::flow::async_node<int, int> node(g, 1, async_body{});
    tbb::profiling::set_name(node, "async_node");
}

//! Test for broadcast_node set_name
//! \brief \ref interface
TEST_CASE("broadcast_node set_name") {
    tbb::flow::graph g;
    tbb::flow::broadcast_node<int> node(g);
    tbb::profiling::set_name(node, "broadcast_node");
}

//! Test for buffer_node set_name
//! \brief \ref interface
TEST_CASE("buffer_node set_name") {
    tbb::flow::graph g;
    tbb::flow::buffer_node<int> node(g);
    tbb::profiling::set_name(node, "buffer_node");
}

//! Test for composite_node set_name
//! \brief \ref interface
TEST_CASE("composite_node set_name") {
    tbb::flow::graph g;
    tbb::flow::composite_node<std::tuple<int>, std::tuple<int>> node(g);
    tbb::profiling::set_name(node, "composite_node");
}

//! Test for continue_node set_name
//! \brief \ref interface
TEST_CASE("continue_node set_name") {
    tbb::flow::graph g;
    tbb::flow::continue_node<tbb::flow::continue_msg> node(g,
        [](const tbb::flow::continue_msg& val) -> const tbb::flow::continue_msg&
        {
            return val;
        });
    tbb::profiling::set_name(node, "continue_node");
}

//! Test for function_node set_name
//! \brief \ref interface
TEST_CASE("function_node set_name") {
    tbb::flow::graph g;
    tbb::flow::function_node<int, int> node(g, 1, [](const int& val) -> int {return val; });
    tbb::profiling::set_name(node, "function_node");
}

//! Test for indexer_node set_name
//! \brief \ref interface
TEST_CASE("indexer_node set_name") {
    tbb::flow::graph g;
    tbb::flow::indexer_node<int> node1(g);
    tbb::profiling::set_name(node1, "indexer_node");
    tbb::flow::indexer_node<int, int> node2(g);
    tbb::profiling::set_name(node2, "indexer_node");
    tbb::flow::indexer_node<int, int, int> node3(g);
    tbb::profiling::set_name(node3, "indexer_node");
    tbb::flow::indexer_node<int, int, int, int> node4(g);
    tbb::profiling::set_name(node4, "indexer_node");
    tbb::flow::indexer_node<int, int, int, int, int> node5(g);
    tbb::profiling::set_name(node5, "indexer_node");
    tbb::flow::indexer_node<int, int, int, int, int, int> node6(g);
    tbb::profiling::set_name(node6, "indexer_node");
    tbb::flow::indexer_node<int, int, int, int, int, int, int> node7(g);
    tbb::profiling::set_name(node7, "indexer_node");
    tbb::flow::indexer_node<int, int, int, int, int, int, int, int> node8(g);
    tbb::profiling::set_name(node8, "indexer_node");
    tbb::flow::indexer_node<int, int, int, int, int, int, int, int, int> node9(g);
    tbb::profiling::set_name(node9, "indexer_node");
    tbb::flow::indexer_node<int, int, int, int, int, int, int, int, int, int> node10(g);
    tbb::profiling::set_name(node10, "indexer_node");
}

//! Test for input_node set_name
//! \brief \ref interface
TEST_CASE("input_node set_name") {
    tbb::flow::graph g;
    tbb::flow::input_node<int> node(g, [](tbb::flow_control& ) -> int { return 0;});
    tbb::profiling::set_name(node, "input_node");
}

//! Test for join_node set_name
//! \brief \ref interface
TEST_CASE("join_node set_name") {
    tbb::flow::graph g;
    tbb::flow::join_node<std::tuple<int>> node(g);
    tbb::profiling::set_name(node, "join_node");
}

//! Test for limiter_node set_name
//! \brief \ref interface
TEST_CASE("limiter_node set_name") {
    tbb::flow::graph g;
    tbb::flow::limiter_node<int> node(g, 1);
    tbb::profiling::set_name(node, "limiter_node");
}

struct mf_body
{
    template<typename T>
    void operator()(int, T&) {};
};

//! Test for multifunction_node set_name
//! \brief \ref interface
TEST_CASE("multifunction_node set_name") {
    tbb::flow::graph g;
    tbb::flow::multifunction_node<int, std::tuple<int>> node(g, 1, mf_body{});
    tbb::profiling::set_name(node, "multifunction_node");
}

//! Test for overwrite_node set_name
//! \brief \ref interface
TEST_CASE("overwrite_node set_name") {
    tbb::flow::graph g;
    tbb::flow::overwrite_node<int> node(g);
    tbb::profiling::set_name(node, "overwrite_node");
}

//! Test for priority_queue_node set_name
//! \brief \ref interface
TEST_CASE("priority_queue_node set_name") {
    tbb::flow::graph g;
    tbb::flow::priority_queue_node<int> node(g);
    tbb::profiling::set_name(node, "priority_queue_node");
}

//! Test for queue_node set_name
//! \brief \ref interface
TEST_CASE("queue_node set_name") {
    tbb::flow::graph g;
    tbb::flow::queue_node<int> node(g);
    tbb::profiling::set_name(node, "queue_node");
}

struct seq_inspector {
    std::size_t operator()(const int& v) const { return v; }
};

//! Test for sequencer_node set_name
//! \brief \ref interface
TEST_CASE("sequencer_node set_name") {
    tbb::flow::graph g;
    tbb::flow::sequencer_node<int> node(g, seq_inspector{});
    tbb::profiling::set_name(node, "sequencer_node");
}

//! Test for split_node set_name
//! \brief \ref interface
TEST_CASE("split_node set_name") {
    tbb::flow::graph g;
    tbb::flow::split_node<std::tuple<int>> node(g);
    tbb::profiling::set_name(node, "split_node");
}

//! Test for write_once_node set_name
//! \brief \ref interface
TEST_CASE("write_once_node set_name") {
    tbb::flow::graph g;
    tbb::flow::write_once_node<int> node(g);
    tbb::profiling::set_name(node, "write_once_node");
}

#include "tbb/profiling.h"

//! Test for event
//! \brief \ref interface
TEST_CASE("event class") {
    tbb::profiling::event e("event");
    e.emit();
    tbb::profiling::event::emit("emit");
}
