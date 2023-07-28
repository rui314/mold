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

/* Flow Graph Code Example for the Userguide.
*/

#include <oneapi/tbb/flow_graph.h>
#include <vector>

using namespace tbb::flow;

//! Example shows how to set the most performant core type as the preferred one
//! for a graph execution.
static void flow_graph_attach_to_arena_1() {
/*begin_attach_to_arena_1*/
    std::vector<tbb::core_type_id> core_types = tbb::info::core_types();
    tbb::task_arena arena(
        tbb::task_arena::constraints{}.set_core_type(core_types.back())
    );

    arena.execute( [&]() {
        graph g;
        function_node< int > f( g, unlimited, []( int ) {
             /*the most performant core type is defined as preferred.*/
        } );
        f.try_put(1);
        g.wait_for_all();
    } );
/*end_attach_to_arena_1*/
}

//! Reattach existing graph to an arena with the most performant core type as
//! the preferred one for a work execution.
static void flow_graph_attach_to_arena_2() {
/*begin_attach_to_arena_2*/
    graph g;
    function_node< int > f( g, unlimited, []( int ) {
        /*the most performant core type is defined as preferred.*/
    } );

    std::vector<tbb::core_type_id> core_types = tbb::info::core_types();
    tbb::task_arena arena(
        tbb::task_arena::constraints{}.set_core_type(core_types.back())
    );

    arena.execute( [&]() {
        g.reset();
    } );
    f.try_put(1);
    g.wait_for_all();
/*end_attach_to_arena_2*/
}

int main() {
    flow_graph_attach_to_arena_1();
    flow_graph_attach_to_arena_2();

    return 0;
}
