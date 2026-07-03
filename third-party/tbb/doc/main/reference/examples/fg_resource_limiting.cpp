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

#include <initializer_list>

struct DB_handle {
    void read() {}
    void write() {}
};

DB_handle* open_database() {
    static DB_handle global_db_handle;
    return &global_db_handle;
}

struct processor_body {
    int operator()(int id) const { return id; }
};

auto input_ids = {1, 2, 3};

/*begin_fg_resource_limiting_example*/
#define TBB_PREVIEW_FLOW_GRAPH_RESOURCE_LIMITING 1
#include <oneapi/tbb/flow_graph.h>

int main() {
    using namespace tbb::flow;

    resource_limiter<DB_handle*> db_limiter{open_database()};

    graph g;

    using resource_limited_node_type = resource_limited_node<int, std::tuple<int>>;

    // Concurrency is unlimited, but db_limiter ensures exclusive DB access
    resource_limited_node_type db_reader(g, unlimited,
        std::tie(db_limiter),
        [](int id, auto& ports, DB_handle* db) {
            db->read();
            // other actions with the data read from db
            std::get<0>(ports).try_put(id);
        });

    function_node<int, int> processor(g, unlimited, processor_body{});

    resource_limited_node_type db_writer(g, unlimited,
        std::tie(db_limiter),
        [](int id, auto& ports, DB_handle* db) {
            // other actions with the database
            db->write();
            std::get<0>(ports).try_put(id);
        }
    );

    make_edge(output_port<0>(db_reader), processor);
    make_edge(processor, db_writer);
    // Other graph nodes and edges

    for (int id : input_ids) {
        db_reader.try_put(id);
    }

    g.wait_for_all();
}
/*end_fg_resource_limiting_example*/
