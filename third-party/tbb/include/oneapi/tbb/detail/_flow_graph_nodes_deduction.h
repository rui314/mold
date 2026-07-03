/*
    Copyright (c) 2005-2024 Intel Corporation
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

#ifndef __TBB_flow_graph_nodes_deduction_H
#define __TBB_flow_graph_nodes_deduction_H

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT

namespace tbb {
namespace detail {
namespace d2 {

template <typename Input, typename Output>
struct body_types {
    using input_type = std::decay_t<Input>;
    using output_type = std::decay_t<Output>;
};

template <typename Input, typename Output>
struct checked_body_types : body_types<Input, Output> {
private:
    using base_type = body_types<Input, Output>;

    static_assert(std::is_same_v<Input, typename base_type::input_type> ||
                  std::is_same_v<Input, const typename base_type::input_type&>,
                  "The node body can only accept input by value or by const lvalue reference");
};

template <typename Body, typename = void>
struct body_traits {
    // !std::is_same_v<Body, Body> is needed to create a dependent context for static_assert(false)
    static_assert(!std::is_same_v<Body, Body>, "Body signature does not match the named requirements");
};

// Body is a pointer to function
template <typename Input, typename Output>
struct body_traits<Output (*)(Input)> : body_types<Input, Output> {};

// Body is a pointer to noexcept function 
template <typename Input, typename Output>
struct body_traits<Output (*)(Input) noexcept> : body_types<Input, Output> {};

// Body is a pointer to non-static data member
template <typename Input, typename Output>
struct body_traits<Output Input::*> : body_types<Input, Output> {};

// Body is a pointer to non-static member function
template <typename Input, typename Output>
struct body_traits<Output (Input::*)() const> : body_types<Input, Output> {};

template <typename Input, typename Output>
struct body_traits<Output (Input::*)() const noexcept> : body_types<Input, Output> {};

// Body is a callable/lambda
// Helper to read input/output types of operator() with single argument
// Supports Body defining multiple operator() overloads, but should have single unary overload
template <typename Body>
struct unary_operator_types_extractor {
    // Overloads for const, noexcept and & qualified operator() with all possible combinations
    template <typename B, typename Input, typename Output>
    static auto check_args(Output (B::*)(Input)) -> checked_body_types<Input, Output>;

    template <typename B, typename Input, typename Output>
    static auto check_args(Output (B::*)(Input) const) -> checked_body_types<Input, Output>;

    template <typename B, typename Input, typename Output>
    static auto check_args(Output (B::*)(Input) noexcept) -> checked_body_types<Input, Output>;

    template <typename B, typename Input, typename Output>
    static auto check_args(Output (B::*)(Input) const noexcept) -> checked_body_types<Input, Output>;

    template <typename B, typename Input, typename Output>
    static auto check_args(Output (B::*)(Input) &) -> checked_body_types<Input, Output>;

    template <typename B, typename Input, typename Output>
    static auto check_args(Output (B::*)(Input) const &) -> checked_body_types<Input, Output>;

    template <typename B, typename Input, typename Output>
    static auto check_args(Output (B::*)(Input) & noexcept) -> checked_body_types<Input, Output>;

    template <typename B, typename Input, typename Output>
    static auto check_args(Output (B::*)(Input) const & noexcept) -> checked_body_types<Input, Output>;

    // Checking layer to force SFINAE in case of operator() absence
    template <typename B>
    static auto check_call_operator_presence(int) -> decltype(check_args(&B::operator()));

    template <typename B>
    static void check_call_operator_presence(...);

    using operator_types = decltype(check_call_operator_presence<Body>(0));
};

template <typename Body>
struct body_traits<Body, std::void_t<typename unary_operator_types_extractor<Body>::operator_types::input_type>>
    : unary_operator_types_extractor<Body>::operator_types
{};

template <typename Body>
using input_type_of = typename body_traits<Body>::input_type;

template <typename Body>
using output_type_of = typename body_traits<Body>::output_type;

// Deduction guides for Flow Graph nodes

template <typename GraphOrSet, typename Body>
input_node(GraphOrSet&&, Body)
->input_node<std::invoke_result_t<Body, d1::flow_control&>>;

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET

template <typename NodeSet>
struct decide_on_set;

template <typename Node, typename... Nodes>
struct decide_on_set<node_set<order::following, Node, Nodes...>> {
    using type = typename Node::output_type;
};

template <typename Node, typename... Nodes>
struct decide_on_set<node_set<order::preceding, Node, Nodes...>> {
    using type = typename Node::input_type;
};

template <typename NodeSet>
using decide_on_set_t = typename decide_on_set<std::decay_t<NodeSet>>::type;

template <typename NodeSet>
broadcast_node(const NodeSet&)
->broadcast_node<decide_on_set_t<NodeSet>>;

template <typename NodeSet>
buffer_node(const NodeSet&)
->buffer_node<decide_on_set_t<NodeSet>>;

template <typename NodeSet>
queue_node(const NodeSet&)
->queue_node<decide_on_set_t<NodeSet>>;
#endif // __TBB_PREVIEW_FLOW_GRAPH_NODE_SET

template <typename GraphOrProxy, typename Sequencer>
sequencer_node(GraphOrProxy&&, Sequencer)
->sequencer_node<input_type_of<Sequencer>>;

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
template <typename NodeSet, typename Compare>
priority_queue_node(const NodeSet&, const Compare&)
->priority_queue_node<decide_on_set_t<NodeSet>, Compare>;

template <typename NodeSet>
priority_queue_node(const NodeSet&)
->priority_queue_node<decide_on_set_t<NodeSet>, std::less<decide_on_set_t<NodeSet>>>;
#endif // __TBB_PREVIEW_FLOW_GRAPH_NODE_SET

template <typename T>
using is_queueing_or_reserving_tag = std::disjunction<std::is_same<T, queueing>, std::is_same<T, reserving>>;

template <typename T, typename... Args>
using are_queueing_or_reserving_tags = std::conjunction<is_queueing_or_reserving_tag<T>,
                                                        is_queueing_or_reserving_tag<Args>...>;

template <typename Key>
struct join_key {
    using type = Key;
};

template <typename T>
struct join_key<const T&> {
    using type = T&;
};

template <typename Key>
using join_key_t = typename join_key<Key>::type;

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET

template <typename... Predecessors, typename Policy,
          std::enable_if_t<are_queueing_or_reserving_tags<Policy>::value, int> = 0>
join_node(const node_set<order::following, Predecessors...>&, Policy)
->join_node<std::tuple<typename Predecessors::output_type...>, Policy>;

template <typename Successor, typename... Successors, typename Policy,
          std::enable_if_t<are_queueing_or_reserving_tags<Policy>::value, int> = 0>
join_node(const node_set<order::preceding, Successor, Successors...>&, Policy)
->join_node<typename Successor::input_type, Policy>;

template <typename... Predecessors>
join_node(const node_set<order::following, Predecessors...>)
->join_node<std::tuple<typename Predecessors::output_type...>,
            queueing>;

template <typename Successor, typename... Successors>
join_node(const node_set<order::preceding, Successor, Successors...>)
->join_node<typename Successor::input_type, queueing>;
#endif

template <typename GraphOrProxy, typename Body, typename... Bodies,
          std::enable_if_t<!are_queueing_or_reserving_tags<Body, Bodies...>::value, int> = 0> 
join_node(GraphOrProxy&&, Body, Bodies...)
->join_node<std::tuple<input_type_of<Body>, input_type_of<Bodies>...>,
            key_matching<join_key_t<output_type_of<Body>>>>;

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
template <typename... Predecessors>
indexer_node(const node_set<order::following, Predecessors...>&)
->indexer_node<typename Predecessors::output_type...>;

template <typename NodeSet>
limiter_node(const NodeSet&, size_t)
->limiter_node<decide_on_set_t<NodeSet>>;

template <typename Predecessor, typename... Predecessors>
split_node(const node_set<order::following, Predecessor, Predecessors...>&)
->split_node<typename Predecessor::output_type>;

template <typename... Successors>
split_node(const node_set<order::preceding, Successors...>&)
->split_node<std::tuple<typename Successors::input_type...>>;
#endif

template <typename GraphOrSet, typename Body, typename Policy>
function_node(GraphOrSet&&,
              size_t, Body,
              Policy, node_priority_t = no_priority)
->function_node<input_type_of<Body>, output_type_of<Body>, Policy>;

template <typename GraphOrSet, typename Body>
function_node(GraphOrSet&&, size_t,
              Body, node_priority_t = no_priority)
->function_node<input_type_of<Body>, output_type_of<Body>, queueing>;

template <typename Output>
struct continue_output {
    using type = Output;
};

template <>
struct continue_output<void> {
    using type = continue_msg;
};

template <typename T>
using continue_output_t = typename continue_output<T>::type;

template <typename GraphOrSet, typename Body, typename Policy>
continue_node(GraphOrSet&&, Body,
              Policy, node_priority_t = no_priority)
->continue_node<continue_output_t<std::invoke_result_t<Body, continue_msg>>,
                Policy>;

template <typename GraphOrSet, typename Body, typename Policy>
continue_node(GraphOrSet&&,
              int, Body,
              Policy, node_priority_t = no_priority)
->continue_node<continue_output_t<std::invoke_result_t<Body, continue_msg>>,
                Policy>;

template <typename GraphOrSet, typename Body>
continue_node(GraphOrSet&&,
              Body, node_priority_t = no_priority)
->continue_node<continue_output_t<std::invoke_result_t<Body, continue_msg>>, Policy<void>>;

template <typename GraphOrSet, typename Body>
continue_node(GraphOrSet&&, int,
              Body, node_priority_t = no_priority)
->continue_node<continue_output_t<std::invoke_result_t<Body, continue_msg>>,
                Policy<void>>;

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET

template <typename NodeSet>
overwrite_node(const NodeSet&)
->overwrite_node<decide_on_set_t<NodeSet>>;

template <typename NodeSet>
write_once_node(const NodeSet&)
->write_once_node<decide_on_set_t<NodeSet>>;
#endif // __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
} // namespace d2
} // namespace detail
} // namespace tbb

#endif // __TBB_CPP17_DEDUCTION_GUIDES_PRESENT

#endif // __TBB_flow_graph_nodes_deduction_H
