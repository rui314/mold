/*
    Copyright (c) 2022 Intel Corporation

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

#ifndef __TBB_test_common_test_join_node_multiple_predecessors_H_
#define __TBB_test_common_test_join_node_multiple_predecessors_H_

#include "config.h"
#include "oneapi/tbb/flow_graph.h"

namespace multiple_predecessors {

using namespace tbb::flow;

using join_node_t = join_node<std::tuple<continue_msg, continue_msg, continue_msg>, reserving>;
using queue_node_t = queue_node<std::tuple<continue_msg, continue_msg, continue_msg>>;

void twist_join_connections(
    buffer_node<continue_msg>& bn1, buffer_node<continue_msg>& bn2, buffer_node<continue_msg>& bn3,
    join_node_t& jn)
{
    // order, in which edges are created/destroyed, is important
    make_edge(bn1, input_port<0>(jn));
    make_edge(bn2, input_port<0>(jn));
    make_edge(bn3, input_port<0>(jn));

    remove_edge(bn3, input_port<0>(jn));
    make_edge  (bn3, input_port<2>(jn));

    remove_edge(bn2, input_port<0>(jn));
    make_edge  (bn2, input_port<1>(jn));
}

std::unique_ptr<join_node_t> connect_join_via_make_edge(
    graph& g, buffer_node<continue_msg>& bn1, buffer_node<continue_msg>& bn2,
    buffer_node<continue_msg>& bn3, queue_node_t& qn)
{
    std::unique_ptr<join_node_t> jn( new join_node_t(g) );
    twist_join_connections( bn1, bn2, bn3, *jn );
    make_edge(*jn, qn);
    return jn;
}

#if TBB_PREVIEW_FLOW_GRAPH_FEATURES
std::unique_ptr<join_node_t> connect_join_via_follows(
    graph&, buffer_node<continue_msg>& bn1, buffer_node<continue_msg>& bn2,
    buffer_node<continue_msg>& bn3, queue_node_t& qn)
{
    auto bn_set = make_node_set(bn1, bn2, bn3);
    std::unique_ptr<join_node_t> jn( new join_node_t(follows(bn_set)) );
    make_edge(*jn, qn);
    return jn;
}

std::unique_ptr<join_node_t> connect_join_via_precedes(
    graph&, buffer_node<continue_msg>& bn1, buffer_node<continue_msg>& bn2,
    buffer_node<continue_msg>& bn3, queue_node_t& qn)
{
    auto qn_set = make_node_set(qn);
    auto qn_copy_set = qn_set;
    std::unique_ptr<join_node_t> jn( new join_node_t(precedes(qn_copy_set)) );
    twist_join_connections( bn1, bn2, bn3, *jn );
    return jn;
}
#endif // TBB_PREVIEW_FLOW_GRAPH_FEATURES

void run_and_check(
    graph& g, buffer_node<continue_msg>& bn1, buffer_node<continue_msg>& bn2,
    buffer_node<continue_msg>& bn3, queue_node_t& qn, bool expected)
{
    std::tuple<continue_msg, continue_msg, continue_msg> msg;

    bn1.try_put(continue_msg());
    bn2.try_put(continue_msg());
    bn3.try_put(continue_msg());
    g.wait_for_all();

    CHECK_MESSAGE(
        (qn.try_get(msg) == expected),
        "Unexpected message absence/existence at the end of the graph."
    );
}

template<typename ConnectJoinNodeFunc>
void test(ConnectJoinNodeFunc&& connect_join_node) {
    graph g;
    buffer_node<continue_msg> bn1(g);
    buffer_node<continue_msg> bn2(g);
    buffer_node<continue_msg> bn3(g);
    queue_node_t qn(g);

    auto jn = connect_join_node(g, bn1, bn2, bn3, qn);

    run_and_check(g, bn1, bn2, bn3, qn, /*expected=*/true);

    remove_edge(bn3, input_port<2>(*jn));
    remove_edge(bn2, input_port<1>(*jn));
    remove_edge(bn1, *jn); //Removes an edge between a sender and port 0 of a multi-input successor.
    remove_edge(*jn, qn);

    run_and_check(g, bn1, bn2, bn3, qn, /*expected=*/false);
}
} // namespace multiple_predecessors

#endif // __TBB_test_common_test_join_node_multiple_predecessors_H_
