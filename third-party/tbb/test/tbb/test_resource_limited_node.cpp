/*
    Copyright (c) 2026 UXL Foundation Contributors

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

#define TBB_PREVIEW_FLOW_GRAPH_RESOURCE_LIMITING 1
#include "common/config.h"
#include "common/test.h"
#include "common/utils.h"
#include "common/graph_utils.h"
#include "common/test_invoke.h"

#include "conformance/conformance_flowgraph.h"

#include "tbb/flow_graph.h"

//! \file test_resource_limited_node.cpp
//! \brief Test for [preview] functionality

using input_msg = conformance::message</*default_ctor = */true, /*copy_ctor = */true, /*copy_assign = */false>;
using output_msg = conformance::message</*default_ctor = */false, /*copy_ctor = */false, /*copy_assign = */false>;

template <typename Input, typename OutputTuple>
void test_inheritance() {
    using namespace oneapi::tbb::flow;

    using node_type = resource_limited_node<Input, OutputTuple>;
    CHECK_MESSAGE((std::is_base_of<graph_node, node_type>::value), "graph_node is not base of resource_limited_node");
    CHECK_MESSAGE((std::is_base_of<receiver<Input>, node_type>::value), "receiver is not base of resource_limited_node");
}

void test_single_resource() {
    using namespace oneapi::tbb::flow;

    using node_type = resource_limited_node<int, std::tuple<>>;
    using ports_type = typename node_type::output_ports_type;

    int resource_value = 100;
    int resource = resource_value;
    resource_limiter<int*> limiter{&resource};
    
    graph g;
    broadcast_node<int> start(g);

    const std::size_t num_nodes = 10;
    std::vector<node_type> nodes;
    nodes.reserve(num_nodes);

    int input_message = 0;
    std::atomic<std::size_t> counter(0);
    std::size_t num_body_runs = 0;
    auto node_body = [&](int input, ports_type&, int* resource_handle) {
        CHECK_MESSAGE(input == input_message, "Incorrect input");
        CHECK_MESSAGE(*resource_handle == resource_value, "Incorrect resource value");
        ++counter;
        for (std::size_t i = 0; i < 1000; ++i) {
            CHECK_MESSAGE(counter == 1, "Single resource was given to someone else");
        }
        ++num_body_runs;
        --counter;
    };

    for (std::size_t i = 0; i < num_nodes; ++i) {
        nodes.emplace_back(g, unlimited, std::tie(limiter), node_body);
        make_edge(start, nodes.back());
    }

    start.try_put(input_message);
    g.wait_for_all();

    CHECK_MESSAGE(counter == 0, "Incorrect counter value");
    CHECK_MESSAGE(num_body_runs == num_nodes, "Incorrect number of bodies executed");
    CHECK_MESSAGE(resource == resource_value, "Incorrect resource value");
}

#if __TBB_RESUMABLE_TASKS
// Test that ten resources are granted simultaneously to ten different nodes
void test_several_resources() {
    using namespace oneapi::tbb::flow;

    resource_limiter<int> limiter{0, 1, 2, 3, 4, 5, 6, 7, 8, 9};

    using node_type = resource_limited_node<int, std::tuple<int>>;
    using ports_type = typename node_type::output_ports_type;

    graph g;
    broadcast_node<int> start(g);

    const std::size_t num_nodes = 10;
    std::vector<node_type> nodes;
    nodes.reserve(num_nodes);

    buffer_node<int> output(g);

    std::size_t counter = 0;
    std::mutex body_mutex;
    std::vector<oneapi::tbb::task::suspend_point> suspend_points;
    suspend_points.reserve(num_nodes);

    auto node_body = [&](int input, ports_type& ports, int resource_copy) {
        std::unique_lock<std::mutex> lock(body_mutex);

        if (++counter == num_nodes) {
            auto points = std::move(suspend_points);
            lock.unlock();
            for (auto sp : points) {
                oneapi::tbb::task::resume(sp);
            }
        } else {
            oneapi::tbb::task::suspend([&](oneapi::tbb::task::suspend_point sp) {
                suspend_points.emplace_back(sp);
                lock.unlock();
            });
        }
        std::get<0>(ports).try_put(input + resource_copy);
    };

    for (std::size_t i = 0; i < num_nodes; ++i) {
        nodes.emplace_back(g, unlimited, std::tie(limiter), node_body);
        make_edge(start, nodes.back());
        make_edge(output_port<0>(nodes.back()), output);
    }

    start.try_put(100);
    g.wait_for_all();

    std::unordered_set<int> validation_set;
    for (int i = 0; i < 10; ++i) {
        validation_set.emplace(100 + i);
    }

    for (std::size_t i = 0; i < 10; ++i) {
        int buffered_output = -1;
        CHECK_MESSAGE(output.try_get(buffered_output), "Desired output not received");
        CHECK_MESSAGE(validation_set.erase(buffered_output) == 1, "Incorrect output");
    }
    CHECK(validation_set.empty());
}
#endif // __TBB_RESUMABLE_TASKS

struct strict_resource_handle {
    int underlying_resource;

    strict_resource_handle(int value) : underlying_resource(value) {}
    friend struct strict_resource_handle_provider;
public:
    strict_resource_handle(strict_resource_handle&&) = default;
    strict_resource_handle(const strict_resource_handle&) = default;

    strict_resource_handle& operator=(strict_resource_handle&&) = default;

    strict_resource_handle() = delete;
    strict_resource_handle& operator=(const strict_resource_handle&) = delete;

    int& get_underlying_resource() { return underlying_resource; }
};

struct strict_resource_handle_provider {
    static strict_resource_handle construct(int value) {
        return strict_resource_handle(value);
    }
};

void test_strict_resource_handle() {
    using namespace tbb::flow;

    int handle_value = 42;
    resource_limiter<strict_resource_handle> limiter{strict_resource_handle_provider::construct(handle_value)};

    using node_type = resource_limited_node<int, std::tuple<>>;
    using ports_type = typename node_type::output_ports_type;

    int input_message = 100;

    graph g;
    node_type node(g, unlimited, std::tie(limiter),
        [&](int input, ports_type&, strict_resource_handle& resource_handle) {
            CHECK_MESSAGE(input == input_message, "Incorrect input message");
            CHECK_MESSAGE(resource_handle.get_underlying_resource() == handle_value, "Incorrect resource value");
        });

    node.try_put(input_message);
    g.wait_for_all();
}

struct counting_resource {
    std::atomic<std::size_t> counter;

    counting_resource() : counter(0) {}

    void use() {
        std::size_t value = ++counter;
        CHECK_MESSAGE(value == 1, "Resource in use by someone else");

        for (std::size_t i = 0; i < 10000; ++i) {
            CHECK_MESSAGE(counter.load() == 1, "Resource in use by someone else");
        }

        --counter;
    }
};

void test_root_genie() {
    counting_resource root_resource;
    counting_resource genie_resource;

    using namespace oneapi::tbb::flow;
    using node_type = resource_limited_node<int, std::tuple<int>>;
    using ports_type = typename node_type::output_ports_type;

    resource_limiter<counting_resource*> root_limiter(&root_resource);
    resource_limiter<counting_resource*> genie_limiter(&genie_resource);

    graph g;

    broadcast_node<int> start(g);

    node_type root_node(g, unlimited, std::tie(root_limiter),
        [&](int input, ports_type& ports, counting_resource* root) {
            CHECK(root == &root_resource);
            root->use();
            std::get<0>(ports).try_put(input);
        });

    node_type genie_node(g, unlimited, std::tie(genie_limiter),
        [&](int input, ports_type& ports, counting_resource* genie) {
            CHECK(genie == &genie_resource);
            genie->use();
            std::get<0>(ports).try_put(input);
        });

    node_type root_genie_node(g, unlimited, std::tie(root_limiter, genie_limiter),
        [&](int input, ports_type& ports, counting_resource* root, counting_resource* genie) {
            CHECK(root == &root_resource);
            CHECK(genie == &genie_resource);
            root->use();
            genie->use();
            std::get<0>(ports).try_put(input);
        });

    buffer_node<int> root_inputs(g);
    buffer_node<int> genie_inputs(g);
    buffer_node<int> root_genie_inputs(g);

    make_edge(start, root_node);
    make_edge(start, root_genie_node);
    make_edge(start, genie_node);
    make_edge(output_port<0>(root_node), root_inputs);
    make_edge(output_port<0>(genie_node), genie_inputs);
    make_edge(output_port<0>(root_genie_node), root_genie_inputs);

    std::unordered_multiset<int> inputs;

    int num_inputs = 100;
    for (int i = 0; i < num_inputs; ++i) {
        for (std::size_t j = 0; j < 3; ++j) {
            inputs.emplace(i);
        }

        start.try_put(i);
    }

    g.wait_for_all();

    for (int i = 0; i < num_inputs; ++i) {
        int root_input = 0;
        int genie_input = 0;
        int root_genie_input = 0;
        CHECK_MESSAGE(root_inputs.try_get(root_input), "No input processed by root node");
        CHECK_MESSAGE(genie_inputs.try_get(genie_input), "No input processed by genie node");
        CHECK_MESSAGE(root_genie_inputs.try_get(root_genie_input), "No input processed by root-genie node");
        
        auto it = inputs.find(root_input);
        CHECK_MESSAGE(it != inputs.end(), "Root node did not process the required input");
        inputs.erase(it);

        it = inputs.find(genie_input);
        CHECK_MESSAGE(it != inputs.end(), "Genie node did not process the required input");
        inputs.erase(it);

        it = inputs.find(root_genie_input);
        CHECK_MESSAGE(it != inputs.end(), "Root-genie node did not process the required input");
        inputs.erase(it);
    }
    CHECK(inputs.empty());

    // TODO: add fairness checks
}

void test_cancellation_with_active_requests(bool same_graph, bool exception) {
    using namespace tbb::flow;

    int resource_value = 1;
    int input_value = 2;
    resource_limiter<int> limiter(resource_value);

    using node_type = resource_limited_node<int, std::tuple<>>;
    using ports_type = typename node_type::output_ports_type;
    
#if TBB_USE_EXCEPTIONS
    struct body_exception {};
#endif
    
    tbb::task_group_context g2_context(tbb::task_group_context::isolated);
    graph g1;
    graph g2(g2_context);

    graph* cancel_node_graph_ptr = same_graph ? &g2 : &g1;

    const std::size_t n_submissions = 100;
    std::atomic<std::size_t> g2_node_body_counter{0};

    node_type keep_using_node(g2, unlimited, std::tie(limiter),
        [&](int input, ports_type&, int resource) {
            CHECK_MESSAGE(input == input_value, "Incorrect input");
            CHECK_MESSAGE(resource == resource_value, "Incorrect resource");

            ++g2_node_body_counter;
        });

    node_type cancel_node(*cancel_node_graph_ptr, unlimited, std::tie(limiter),
        [&](int input, ports_type&, int resource) {
            CHECK_MESSAGE(input == input_value, "Incorrect input");
            CHECK_MESSAGE(resource == resource_value, "Incorrect resource");

            for (std::size_t i = 0; i < n_submissions; ++i) {
                keep_using_node.try_put(input);
            }

            if (exception) {
#if TBB_USE_EXCEPTIONS
                throw body_exception{};
#else
                CHECK_MESSAGE(false, "exception test was called when exceptions are not supported");
#endif
            } else {
                cancel_node_graph_ptr->cancel();
            }

            for (std::size_t i = 0; i < n_submissions; ++i) {
                keep_using_node.try_put(input);
            }
        });

    cancel_node.try_put(input_value);

#if TBB_USE_EXCEPTIONS
    bool caught_exception = false;
    try {
        cancel_node_graph_ptr->wait_for_all();
    } catch (body_exception) {
        caught_exception = true;
    }

    CHECK_MESSAGE(exception == caught_exception, "Expected exception was not caught");
#else
    cancel_node_graph_ptr->wait_for_all();
#endif

    g2.wait_for_all();

    if (same_graph) {
        CHECK_MESSAGE(g2_node_body_counter <= n_submissions, "Incorrect number of g2 node body calls");
    } else {
        std::size_t expected_g2_body_calls = exception ? n_submissions : 2 * n_submissions;
        CHECK_MESSAGE(g2_node_body_counter == expected_g2_body_calls,
                      "Incorrect number of g2 node body calls");
    }
}

//! \brief \ref interface
TEST_CASE("Feature test macro") {
    CHECK_MESSAGE(TBB_HAS_FLOW_GRAPH_RESOURCE_LIMITING == 202603, "Incorrect feature test macro");
}

//! \brief \ref interface
TEST_CASE("bases of resource_limited_node") {
    test_inheritance<int, std::tuple<>>();
    test_inheritance<int, std::tuple<int>>();
    test_inheritance<void*, std::tuple<float>>();
    test_inheritance<input_msg, std::tuple<output_msg>>();
}

//! \brief \ref requirement
TEST_CASE("test resource acquisition") {
    test_single_resource();
#if __TBB_RESUMABLE_TASKS
    test_several_resources();
#endif
}

template <typename Handle>
using limiter_unique_ptr = std::unique_ptr<oneapi::tbb::flow::resource_limiter<Handle>>;

template <std::size_t... Idx>
limiter_unique_ptr<std::size_t> get_limiter_impl(tbb::detail::index_sequence<Idx...>) {
    return limiter_unique_ptr<std::size_t>(new oneapi::tbb::flow::resource_limiter<std::size_t>(Idx...));
}

template <std::size_t NumResources>
limiter_unique_ptr<std::size_t> get_limiter() {
    return get_limiter_impl(tbb::detail::make_index_sequence<NumResources>());
}

//! \brief \ref interface \ref requirement
TEST_CASE("resource_limited_node concurrency") {
    // For correct test behavior number of resources should be greater than number of threads in arena
    constexpr int num_threads = 50;
    auto limiter_ptr = get_limiter<num_threads + 1>();
    oneapi::tbb::task_arena arena(num_threads);

    arena.execute([&] {
        conformance::test_concurrency<oneapi::tbb::flow::resource_limited_node<int, std::tuple<int>>>(std::tie(*limiter_ptr));
    });
}

//! \brief \ref interface
TEST_CASE("resource_limited_node copy_body") {
    auto limiter_ptr = get_limiter<10>();
    using node_type = oneapi::tbb::flow::resource_limited_node<int, std::tuple<int>>;
    using body_type = conformance::copy_counting_object<int>;
    conformance::test_copy_body_function<node_type, body_type>(oneapi::tbb::flow::unlimited, std::tie(*limiter_ptr));
}

//! \brief \ref requirement
TEST_CASE("resource_limiter and resource_limited_node with strict_resource_handle") {
    test_strict_resource_handle();
}

//! \brief \ref interface \ref requirement
TEST_CASE("resource_limited_node copy constructor") {
    auto limiter_ptr = get_limiter<10>();
    using node_type = oneapi::tbb::flow::resource_limited_node<int, std::tuple<int>>;
    conformance::test_copy_ctor<node_type>(std::tie(*limiter_ptr));
}

//! \brief \ref requirement
TEST_CASE("resource_limited_node broadcast") {
    conformance::counting_functor<int> fun(conformance::expected);
    auto limiter_ptr = get_limiter<10>();
    using node_type = oneapi::tbb::flow::resource_limited_node<int, std::tuple<int>>;
    conformance::test_forwarding<node_type, input_msg, int>(1, oneapi::tbb::flow::unlimited, std::tie(*limiter_ptr), fun);
}

//! \brief \ref error_guessing
TEST_CASE("root-genie test for resource_limited_node") {
    test_root_genie();
}

#if __TBB_CPP17_INVOKE_PRESENT
//! \brief \ref interface \ref requirement
TEST_CASE("resource_limited_node and std::invoke") {
    using namespace oneapi::tbb::flow;

    using output_type1 = test_invoke::SmartID<std::size_t>;
    using input_type = test_invoke::SmartID<output_type1>;

    using output_tuple1 = std::tuple<output_type1, output_type1>;
    using output_tuple2 = std::tuple<std::size_t>;

    using first_rl_node_type = resource_limited_node<input_type, output_tuple1>;
    using second_rl_node_type = resource_limited_node<output_type1, output_tuple2>;

    using first_ports_type = typename first_rl_node_type::output_ports_type;
    using second_ports_type = typename second_rl_node_type::output_ports_type;

    graph g;
    auto first_body = &input_type::template send_id<first_ports_type, std::size_t&>;
    auto second_body = &output_type1::template send_id<second_ports_type, std::size_t&>;

    auto limiter_ptr = get_limiter<10>();

    first_rl_node_type rl1(g, unlimited, std::tie(*limiter_ptr), first_body);
    second_rl_node_type rl21(g, unlimited, std::tie(*limiter_ptr), second_body);
    second_rl_node_type rl22(g, unlimited, std::tie(*limiter_ptr), second_body);

    buffer_node<std::size_t> buf(g);

    make_edge(output_port<0>(rl1), rl21);
    make_edge(output_port<1>(rl1), rl21);

    make_edge(output_port<0>(rl21), buf);
    make_edge(output_port<0>(rl22), buf);

    rl1.try_put(input_type{output_type1{1}});

    g.wait_for_all();

    std::size_t buf_size = 0;
    std::size_t tmp = 0;
    while (buf.try_get(tmp)) {
        ++buf_size;
        CHECK(tmp == 1);
    }
    CHECK(buf_size == 2);
}
#endif // __TBB_CPP17_INVOKE_PRESENT

//! \brief \ref error_guessing
TEST_CASE("resource_limited_node cancellation with active requests") {
    test_cancellation_with_active_requests(/*same_graph = */false, /*exception =*/false);
    test_cancellation_with_active_requests(/*same_graph = */true, /*exception =*/false);
#if TBB_USE_EXCEPTIONS
    test_cancellation_with_active_requests(/*same_graph = */false, /*exception =*/true);
    test_cancellation_with_active_requests(/*same_graph = */true, /*exception =*/true);
#endif
}
