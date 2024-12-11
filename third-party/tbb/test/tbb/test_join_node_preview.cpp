/*
    Copyright (c) 2024 Intel Corporation

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

#define TBB_PREVIEW_FLOW_GRAPH_FEATURES 1

#include "common/config.h"

#include "test_join_node.h"
#include "common/test_join_node_multiple_predecessors.h"
#include "common/test_follows_and_precedes_api.h"

#include <array>
#include <vector>

//! \file test_join_node_preview.cpp
//! \brief Test for [preview] functionality

void jn_follows_and_precedes() {
    using msg_t = tbb::flow::continue_msg;
    using JoinOutputType = std::tuple<msg_t, msg_t, msg_t>;

    std::array<msg_t, 3> messages_for_follows = { {msg_t(), msg_t(), msg_t()} };
    std::vector<msg_t> messages_for_precedes = {msg_t(), msg_t(), msg_t()};

    follows_and_precedes_testing::test_follows
        <msg_t, tbb::flow::join_node<JoinOutputType>, tbb::flow::buffer_node<msg_t>>(messages_for_follows);
    follows_and_precedes_testing::test_follows
        <msg_t, tbb::flow::join_node<JoinOutputType, tbb::flow::queueing>>(messages_for_follows);
    follows_and_precedes_testing::test_follows
        <msg_t, tbb::flow::join_node<JoinOutputType, tbb::flow::reserving>, tbb::flow::buffer_node<msg_t>>(messages_for_follows);
    auto b = [](msg_t) { return msg_t(); };
    class hash_compare {
    public:
        std::size_t hash(msg_t) const { return 0; }
        bool equal(msg_t, msg_t) const { return true; }
    };
    follows_and_precedes_testing::test_follows
        <msg_t, tbb::flow::join_node<JoinOutputType, tbb::flow::key_matching<msg_t, hash_compare>>, tbb::flow::buffer_node<msg_t>>
        (messages_for_follows, b, b, b);

    follows_and_precedes_testing::test_precedes
        <msg_t, tbb::flow::join_node<JoinOutputType>>(messages_for_precedes);
    follows_and_precedes_testing::test_precedes
        <msg_t, tbb::flow::join_node<JoinOutputType, tbb::flow::queueing>>(messages_for_precedes);
    follows_and_precedes_testing::test_precedes
        <msg_t, tbb::flow::join_node<JoinOutputType, tbb::flow::reserving>>(messages_for_precedes);
    follows_and_precedes_testing::test_precedes
        <msg_t, tbb::flow::join_node<JoinOutputType, tbb::flow::key_matching<msg_t, hash_compare>>>
        (messages_for_precedes, b, b, b);
}

void jn_msg_key_matching_follows_and_precedes() {
    using msg_t = MyMessageKeyWithoutKey<int, int>;
    using JoinOutputType = std::tuple<msg_t, msg_t, msg_t>;

    std::array<msg_t, 3> messages_for_follows = { {msg_t(), msg_t(), msg_t()} };
    std::vector<msg_t> messages_for_precedes = { msg_t(), msg_t(), msg_t() };

    follows_and_precedes_testing::test_follows
        <msg_t, tbb::flow::join_node<JoinOutputType, tbb::flow::key_matching<std::size_t>>, tbb::flow::buffer_node<msg_t>>
        (messages_for_follows);
    follows_and_precedes_testing::test_precedes
        <msg_t, tbb::flow::join_node<JoinOutputType, tbb::flow::key_matching<std::size_t>>>
        (messages_for_precedes);
}

void test_follows_and_precedes_api() {
    jn_follows_and_precedes();
    jn_msg_key_matching_follows_and_precedes();
}

void test_try_put_and_wait_queueing() {
    tbb::task_arena arena(1);

    arena.execute([] {
        tbb::flow::graph g;

        std::vector<int> start_work_items;
        std::vector<int> processed_items;
        std::vector<int> new_work_items;
        int wait_message = 10;

        for (int i = 0; i < wait_message; ++i) {
            start_work_items.emplace_back(i);
            new_work_items.emplace_back(i + 1 + wait_message);
        }

        using tuple_type = std::tuple<int, int, int>;
        tbb::flow::join_node<tuple_type, tbb::flow::queueing> join(g);

        tbb::flow::function_node<tuple_type, int, tbb::flow::rejecting> function(g, tbb::flow::serial,
            [&](tuple_type tuple) noexcept {
                CHECK(std::get<0>(tuple) == std::get<1>(tuple));
                CHECK(std::get<1>(tuple) == std::get<2>(tuple));

                auto input = std::get<0>(tuple);

                if (input == wait_message) {
                    for (auto item : new_work_items) {
                        tbb::flow::input_port<0>(join).try_put(item);
                        tbb::flow::input_port<1>(join).try_put(item);
                        tbb::flow::input_port<2>(join).try_put(item);
                    }
                }
                processed_items.emplace_back(input);
                return 0;
            });

        tbb::flow::make_edge(join, function);

        for (auto item : start_work_items) {
            tbb::flow::input_port<0>(join).try_put(item);
            tbb::flow::input_port<1>(join).try_put(item);
            tbb::flow::input_port<2>(join).try_put(item);
        }

        tbb::flow::input_port<0>(join).try_put(wait_message);
        tbb::flow::input_port<1>(join).try_put(wait_message);
        tbb::flow::input_port<2>(join).try_put_and_wait(wait_message);

        // It is expected that the join_node would push the tuple of three copies of first element in start_work_items
        // And occupy the concurrency of function. Other tuples would be rejected and taken using push-pull protocol
        // in FIFO order
        std::size_t check_index = 0;

        for (auto item : start_work_items) {
            CHECK_MESSAGE(processed_items[check_index++] == item, "Unexpected start_work_items processing");
        }

        CHECK_MESSAGE(processed_items[check_index++] == wait_message, "Unexpected wait_message processing");
        CHECK_MESSAGE(check_index == processed_items.size(), "Unexpected number of messages");

        g.wait_for_all();

        for (auto item : new_work_items) {
            CHECK_MESSAGE(processed_items[check_index++] == item, "Unexpected start_work_items processing");
        }
    });
}

void test_try_put_and_wait_reserving() {
    tbb::task_arena arena(1);

    arena.execute([]{
        tbb::flow::graph g;

        std::vector<int> start_work_items;
        std::vector<int> processed_items;
        std::vector<int> new_work_items;
        int wait_message = 10;

        for (int i = 0; i < wait_message; ++i) {
            start_work_items.emplace_back(i);
            new_work_items.emplace_back(i + 1 + wait_message);
        }

        using tuple_type = std::tuple<int, int, int>;
        tbb::flow::queue_node<int> buffer1(g);
        tbb::flow::queue_node<int> buffer2(g);
        tbb::flow::queue_node<int> buffer3(g);

        tbb::flow::join_node<tuple_type, tbb::flow::reserving> join(g);

        tbb::flow::function_node<tuple_type, int, tbb::flow::rejecting> function(g, tbb::flow::serial,
            [&](tuple_type tuple) noexcept {
                CHECK(std::get<0>(tuple) == std::get<1>(tuple));
                CHECK(std::get<1>(tuple) == std::get<2>(tuple));

                auto input = std::get<0>(tuple);

                if (input == wait_message) {
                    for (auto item : new_work_items) {
                        buffer1.try_put(item);
                        buffer2.try_put(item);
                        buffer3.try_put(item);
                    }
                }
                processed_items.emplace_back(input);
                return 0;
            });

        tbb::flow::make_edge(buffer1, tbb::flow::input_port<0>(join));
        tbb::flow::make_edge(buffer2, tbb::flow::input_port<1>(join));
        tbb::flow::make_edge(buffer3, tbb::flow::input_port<2>(join));
        tbb::flow::make_edge(join, function);

        for (auto item : start_work_items) {
            buffer1.try_put(item);
            buffer2.try_put(item);
            buffer3.try_put(item);
        }

        buffer1.try_put(wait_message);
        buffer2.try_put(wait_message);
        buffer3.try_put_and_wait(wait_message);

        // It is expected that the join_node would push the tuple of three copies of first element in start_work_items
        // And occupy the concurrency of function. Other tuples would be rejected and taken using push-pull protocol
        // between function and join_node and between join_node and each buffer in FIFO order because queue_node is used
        std::size_t check_index = 0;

        for (auto item : start_work_items) {
            CHECK_MESSAGE(processed_items[check_index++] == item, "Unexpected start_work_items processing");
        }

        CHECK_MESSAGE(processed_items[check_index++] == wait_message, "Unexpected wait_message processing");
        CHECK_MESSAGE(check_index == processed_items.size(), "Unexpected number of messages");

        g.wait_for_all();

        for (auto item : new_work_items) {
            CHECK_MESSAGE(processed_items[check_index++] == item, "Unexpected start_work_items processing");
        }
    });
}

struct int_wrapper {
    int i = 0;
    int_wrapper() : i(0) {}
    int_wrapper(int ii) : i(ii) {}
    int_wrapper& operator=(int ii) {
        i = ii;
        return *this;
    }

    int key() const {
        return i;
    }

    friend bool operator==(const int_wrapper& lhs, const int_wrapper& rhs) {
        return lhs.i == rhs.i;
    }
};

template <typename... Body>
void test_try_put_and_wait_key_matching(Body... body) {
    // Body of one argument for testing standard key_matching
    // Body of zero arguments for testing message based key_matching
    static_assert(sizeof...(Body) == 0 || sizeof...(Body) == 1, "incorrect test setup");
    tbb::task_arena arena(1);

    arena.execute([=] {
        tbb::flow::graph g;

        std::vector<int_wrapper> start_work_items;
        std::vector<int_wrapper> processed_items;
        std::vector<int_wrapper> new_work_items;
        int_wrapper wait_message = 10;

        for (int i = 0; i < wait_message.i; ++i) {
            start_work_items.emplace_back(i);
            new_work_items.emplace_back(i + 1 + wait_message.i);
        }

        using tuple_type = std::tuple<int_wrapper, int_wrapper, int_wrapper>;
        tbb::flow::join_node<tuple_type, tbb::flow::key_matching<int>> join(g, body..., body..., body...);

        tbb::flow::function_node<tuple_type, int, tbb::flow::rejecting> function(g, tbb::flow::serial,
            [&](tuple_type tuple) noexcept {
                CHECK(std::get<0>(tuple) == std::get<1>(tuple));
                CHECK(std::get<1>(tuple) == std::get<2>(tuple));

                auto input = std::get<0>(tuple);

                if (input == wait_message) {
                    for (auto item : new_work_items) {
                        tbb::flow::input_port<0>(join).try_put(item);
                        tbb::flow::input_port<1>(join).try_put(item);
                        tbb::flow::input_port<2>(join).try_put(item);
                    }
                }
                processed_items.emplace_back(input);
                return 0;
            });

        tbb::flow::make_edge(join, function);

        tbb::flow::input_port<0>(join).try_put(wait_message);
        tbb::flow::input_port<1>(join).try_put(wait_message);

        // For the first port - submit items in reversed order
        for (std::size_t i = start_work_items.size(); i != 0; --i) {
            tbb::flow::input_port<0>(join).try_put(start_work_items[i - 1]);
        }

        // For first two ports - submit items in direct order
        for (auto item : start_work_items) {
            tbb::flow::input_port<1>(join).try_put(item);
            tbb::flow::input_port<2>(join).try_put(item);
        }

        tbb::flow::input_port<2>(join).try_put_and_wait(wait_message);

        // It is expected that the join_node would push the tuple of three copies of first element in start_work_items
        // And occupy the concurrency of function. Other tuples would be rejected and taken using push-pull protocol
        // in order of submission
        std::size_t check_index = 0;

        for (auto item : start_work_items) {
            CHECK_MESSAGE(processed_items[check_index++] == item, "Unexpected start_work_items processing");
        }

        CHECK_MESSAGE(processed_items[check_index++] == wait_message, "Unexpected wait_message processing");
        CHECK_MESSAGE(check_index == processed_items.size(), "Unexpected number of messages");

        g.wait_for_all();

        for (auto item : new_work_items) {
            CHECK_MESSAGE(processed_items[check_index++] == item, "Unexpected start_work_items processing");
        }
        CHECK_MESSAGE(check_index == processed_items.size(), "Unexpected number of messages");
    });
}

//! Test follows and precedes API
//! \brief \ref error_guessing
TEST_CASE("Test follows and precedes API"){
    test_follows_and_precedes_api();
}

// TODO: Look deeper into this test to see if it has the right name
// and if it actually tests some kind of regression. It is possible
// that `connect_join_via_follows` and `connect_join_via_precedes`
// functions are redundant.

//! Test maintaining correct count of ports without input
//! \brief \ref error_guessing
TEST_CASE("Test removal of the predecessor while having none") {
    using namespace multiple_predecessors;

    test(connect_join_via_follows);
    test(connect_join_via_precedes);
}

//! \brief \ref error_guessing
TEST_CASE("Test join_node try_put_and_wait") {
    test_try_put_and_wait_queueing();
    test_try_put_and_wait_reserving();
    // Test standard key_matching policy
    test_try_put_and_wait_key_matching([](int_wrapper w) { return w.i; });
    // Test msg based key_matching policy
    test_try_put_and_wait_key_matching();
}
