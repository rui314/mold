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

#if __cplusplus >= 201703L

#define TBB_PREVIEW_FLOW_GRAPH_FEATURES 1
#include <oneapi/tbb/flow_graph.h>

void no_ctad_example() {
/*begin_fg_ctad_no_ctad_example*/
    using namespace oneapi::tbb::flow;

    graph g;

    // Template parameters must be specified explicitly
    function_node<int, double, queueing> fn(g, unlimited,
        [](int v) -> double { return v * 1.5; });

    continue_node<int> cn1(g,
        [](continue_msg) { return 42; });

    continue_node<continue_msg> cn2(g,
        [](continue_msg) {});

    input_node<int> src(g,
        [](oneapi::tbb::flow_control& fc) -> int { fc.stop(); return 0; });
/*end_fg_ctad_no_ctad_example*/
}

void with_ctad_example() {
/*begin_fg_ctad_with_ctad_example*/
    using namespace oneapi::tbb::flow;

    graph g;

    // The compiler deduces function_node<int, double, queueing>
    function_node fn(g, unlimited,
        [](int v) -> double { return v * 1.5; });

    // The compiler deduces continue_node<int>
    continue_node cn1(g,
        [](continue_msg) { return 42; });

    // The compiler deduces continue_node<continue_msg>
    continue_node cn2(g,
        [](continue_msg) {});

    // The compiler deduces input_node<int>
    input_node src(g,
        [](oneapi::tbb::flow_control& fc) -> int { fc.stop(); return 0; });
/*end_fg_ctad_with_ctad_example*/
}

void preview_ctad_example() {
/*begin_fg_ctad_preview_example*/
    using namespace oneapi::tbb::flow;

    graph g;

    // Functional nodes: CTAD from body
    function_node doubler(g, unlimited, [](int v) { return 2 * v; });
    function_node squarer(g, unlimited, [](int v) { return v * v; });

    // Non-functional nodes: CTAD from predecessors/successors
    broadcast_node input(precedes(doubler, squarer));  // deduces broadcast_node<int>
    join_node join(follows(doubler, squarer));         // deduces join_node<std::tuple<int, int>, queueing>
/*end_fg_ctad_preview_example*/
}

int main() {
    no_ctad_example();
    with_ctad_example();
    preview_ctad_example();
}

#else
// Skip
int main() {}
#endif
