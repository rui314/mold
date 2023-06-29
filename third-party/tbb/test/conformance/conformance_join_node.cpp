/*
    Copyright (c) 2020-2022 Intel Corporation

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

#include "conformance_flowgraph.h"

//! \file conformance_join_node.cpp
//! \brief Test for [flow_graph.join_node] specification

using input_msg = conformance::message</*default_ctor*/true, /*copy_ctor*/true, /*copy_assign*/true>;
using my_input_tuple = std::tuple<int, float, input_msg>;

std::vector<my_input_tuple> get_values( conformance::test_push_receiver<my_input_tuple>& rr ) {
    std::vector<my_input_tuple> messages;
    my_input_tuple tmp(0, 0.f, input_msg(0));
    while(rr.try_get(tmp)) {
        messages.push_back(tmp);
    }
    return messages;
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
void test_deduction_guides() {
    using namespace tbb::flow;

    graph g;
    using tuple_type = std::tuple<int, int, int>;
    broadcast_node<int> b1(g), b2(g), b3(g);
    broadcast_node<tuple_type> b4(g);
    join_node<tuple_type> j0(g);

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
    join_node j1(follows(b1, b2, b3));
    static_assert(std::is_same_v<decltype(j1), join_node<tuple_type>>);

    join_node j2(follows(b1, b2, b3), reserving());
    static_assert(std::is_same_v<decltype(j2), join_node<tuple_type, reserving>>);

    join_node j3(precedes(b4));
    static_assert(std::is_same_v<decltype(j3), join_node<tuple_type>>);

    join_node j4(precedes(b4), reserving());
    static_assert(std::is_same_v<decltype(j4), join_node<tuple_type, reserving>>);
#endif

    join_node j5(j0);
    static_assert(std::is_same_v<decltype(j5), join_node<tuple_type>>);
}

#endif

//! The node that is constructed has a reference to the same graph object as src.
//! The list of predecessors, messages in the input ports, and successors are not copied.
//! \brief \ref interface
TEST_CASE("join_node copy constructor"){
    oneapi::tbb::flow::graph g;
    oneapi::tbb::flow::continue_node<int> node0( g,
                                [](oneapi::tbb::flow::continue_msg) { return 1; } );

    oneapi::tbb::flow::join_node<std::tuple<int>> node1(g);
    conformance::test_push_receiver<std::tuple<int>> node2(g);
    conformance::test_push_receiver<std::tuple<int>> node3(g);

    oneapi::tbb::flow::make_edge(node0, oneapi::tbb::flow::input_port<0>(node1));
    oneapi::tbb::flow::make_edge(node1, node2);
    oneapi::tbb::flow::join_node<std::tuple<int>> node_copy(node1);

    oneapi::tbb::flow::make_edge(node_copy, node3);

    oneapi::tbb::flow::input_port<0>(node_copy).try_put(1);
    g.wait_for_all();

    auto values = conformance::get_values(node3);
    CHECK_MESSAGE((conformance::get_values(node2).size() == 0 && values.size() == 1), "Copied node doesn`t copy successor");

    node0.try_put(oneapi::tbb::flow::continue_msg());
    g.wait_for_all();

    CHECK_MESSAGE((conformance::get_values(node2).size() == 1 && conformance::get_values(node3).size() == 0), "Copied node doesn`t copy predecessor");

    oneapi::tbb::flow::remove_edge(node1, node2);
    oneapi::tbb::flow::input_port<0>(node1).try_put(1);
    g.wait_for_all();
    oneapi::tbb::flow::join_node<std::tuple<int>> node_copy2(node1);
    oneapi::tbb::flow::make_edge(node_copy2, node3);
    oneapi::tbb::flow::input_port<0>(node_copy2).try_put(2);
    g.wait_for_all();
    CHECK_MESSAGE((std::get<0>(conformance::get_values(node3)[0]) == 2), "Copied node doesn`t copy messages in the input ports");
}

//! Test inheritance relations
//! \brief \ref interface
TEST_CASE("join_node inheritance"){
    CHECK_MESSAGE((std::is_base_of<oneapi::tbb::flow::graph_node,
                   oneapi::tbb::flow::join_node<my_input_tuple>>::value),
                   "join_node should be derived from graph_node");
    CHECK_MESSAGE((std::is_base_of<oneapi::tbb::flow::sender<my_input_tuple>,
                   oneapi::tbb::flow::join_node<my_input_tuple>>::value),
                   "join_node should be derived from sender<input_tuple>");
}

//! Test join_node<queueing> behavior and broadcast property
//! \brief \ref requirement
TEST_CASE("join_node queueing policy and broadcast property") {
    oneapi::tbb::flow::graph g;
    oneapi::tbb::flow::function_node<int, int>
        f1( g, oneapi::tbb::flow::unlimited, [](const int &i) { return i; } );
    oneapi::tbb::flow::function_node<float, float>
        f2( g, oneapi::tbb::flow::unlimited, [](const float &f) { return f; } );
    oneapi::tbb::flow::continue_node<input_msg> c1( g,
                            [](oneapi::tbb::flow::continue_msg) { return input_msg(1); } );

    oneapi::tbb::flow::join_node<my_input_tuple, oneapi::tbb::flow::queueing> testing_node(g);

    conformance::test_push_receiver<my_input_tuple> q_node(g);

    std::atomic<int> number{1};
    oneapi::tbb::flow::function_node<my_input_tuple, my_input_tuple>
        f3( g, oneapi::tbb::flow::unlimited,
            [&]( const my_input_tuple &t ) {
                CHECK_MESSAGE((std::get<0>(t) == number), "Messages must be in first-in first-out order" );
                CHECK_MESSAGE((std::get<1>(t) == static_cast<float>(number) + 0.5f), "Messages must be in first-in first-out order" );
                CHECK_MESSAGE((std::get<2>(t) == 1), "Messages must be in first-in first-out order" );
                ++number;
                return t;
            } );

    oneapi::tbb::flow::make_edge(f1, oneapi::tbb::flow::input_port<0>(testing_node));
    oneapi::tbb::flow::make_edge(f2, oneapi::tbb::flow::input_port<1>(testing_node));
    oneapi::tbb::flow::make_edge(c1, oneapi::tbb::flow::input_port<2>(testing_node));
    make_edge(testing_node, f3);
    make_edge(f3, q_node);

    f1.try_put(1);
    g.wait_for_all();
    CHECK_MESSAGE((get_values(q_node).size() == 0),
        "join_node must broadcast when there is at least one message at each input port");
    f1.try_put(2);
    f2.try_put(1.5f);
    g.wait_for_all();
    CHECK_MESSAGE((get_values(q_node).size() == 0),
        "join_node must broadcast when there is at least one message at each input port");
    f1.try_put(3);
    f2.try_put(2.5f);
    c1.try_put(oneapi::tbb::flow::continue_msg());
    g.wait_for_all();
    CHECK_MESSAGE((get_values(q_node).size() == 1),
        "join_node must broadcast when there is at least one message at each input port");
    f2.try_put(3.5f);
    c1.try_put(oneapi::tbb::flow::continue_msg());
    g.wait_for_all();
    CHECK_MESSAGE((get_values(q_node).size() == 1),
        "If at least one successor accepts the tuple, the head of each input port’s queue is removed");
    c1.try_put(oneapi::tbb::flow::continue_msg());
    g.wait_for_all();
    CHECK_MESSAGE((get_values(q_node).size() == 1),
        "If at least one successor accepts the tuple, the head of each input port’s queue is removed");
    c1.try_put(oneapi::tbb::flow::continue_msg());
    g.wait_for_all();
    CHECK_MESSAGE((get_values(q_node).size() == 0),
        "join_node must broadcast when there is at least one message at each input port");

    oneapi::tbb::flow::remove_edge(testing_node, f3);

    f1.try_put(1);
    f2.try_put(1);
    c1.try_put(oneapi::tbb::flow::continue_msg());
    g.wait_for_all();

    my_input_tuple tmp(0, 0.f, input_msg(0));
    CHECK_MESSAGE((testing_node.try_get(tmp)), "If no one successor accepts the tuple the messages\
        must remain in their respective input port queues");
    CHECK_MESSAGE((tmp == my_input_tuple(1, 1.f, input_msg(1))), "If no one successor accepts the tuple\
        the messages must remain in their respective input port queues");
}

//! Test join_node<reserving> behavior
//! \brief \ref requirement
TEST_CASE("join_node reserving policy") {
    conformance::test_with_reserving_join_node_class<oneapi::tbb::flow::write_once_node<int>>();
}

template<typename KeyType>
struct MyHash{
    std::size_t hash(const KeyType &k) const {
        return k * 2000 + 3;
    }

    bool equal(const KeyType &k1, const KeyType &k2) const{
        return hash(k1) == hash(k2);
    }
};

//! Test join_node<key_matching> behavior
//! \brief \ref requirement
TEST_CASE("join_node key_matching policy"){
    oneapi::tbb::flow::graph g;
    auto body1 = [](const oneapi::tbb::flow::continue_msg &) -> int { return 1; };
    auto body2 = [](const float &val) -> int { return static_cast<int>(val); };

    oneapi::tbb::flow::join_node<std::tuple<oneapi::tbb::flow::continue_msg, float>,
        oneapi::tbb::flow::key_matching<int, MyHash<int>>> testing_node(g, body1, body2);

    oneapi::tbb::flow::input_port<0>(testing_node).try_put(oneapi::tbb::flow::continue_msg());
    oneapi::tbb::flow::input_port<1>(testing_node).try_put(1.3f);

    g.wait_for_all();

    std::tuple<oneapi::tbb::flow::continue_msg, float> tmp;
    CHECK_MESSAGE((testing_node.try_get(tmp)), "Mapped keys should match.\
        If no successor accepts the tuple, it is must been saved and will be forwarded on a subsequent try_get");
    CHECK_MESSAGE((!testing_node.try_get(tmp)), "Message should not exist after item is consumed");
}

//! Test join_node<tag_matching> behavior
//! \brief \ref requirement
TEST_CASE("join_node tag_matching policy"){
    oneapi::tbb::flow::graph g;
    auto body1 = [](const oneapi::tbb::flow::continue_msg &) -> oneapi::tbb::flow::tag_value { return 1; };
    auto body2 = [](const float &val) -> oneapi::tbb::flow::tag_value { return static_cast<oneapi::tbb::flow::tag_value>(val); };

    oneapi::tbb::flow::join_node<std::tuple<oneapi::tbb::flow::continue_msg, float>,
        oneapi::tbb::flow::tag_matching> testing_node(g, body1, body2);

    oneapi::tbb::flow::input_port<0>(testing_node).try_put(oneapi::tbb::flow::continue_msg());
    oneapi::tbb::flow::input_port<1>(testing_node).try_put(1.3f);

    g.wait_for_all();

    std::tuple<oneapi::tbb::flow::continue_msg, float> tmp;
    CHECK_MESSAGE((testing_node.try_get(tmp) == true), "Mapped keys should match");
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
//! Test deduction guides
//! \brief \ref requirement
TEST_CASE("Deduction guides test"){
    test_deduction_guides();
}
#endif

//! Test join_node input_ports() returns a tuple of input ports.
//! \brief \ref interface \ref requirement
TEST_CASE("join_node output_ports") {
    oneapi::tbb::flow::graph g;
    oneapi::tbb::flow::join_node<std::tuple<int>> node(g);

    CHECK_MESSAGE((std::is_same<oneapi::tbb::flow::join_node<std::tuple<int>>::input_ports_type&,
        decltype(node.input_ports())>::value), "join_node input_ports should returns a tuple of input ports");
}
