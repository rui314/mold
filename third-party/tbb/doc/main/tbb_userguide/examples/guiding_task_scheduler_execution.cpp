/*
    Copyright (c) 2025 Intel Corporation
    Copyright (c) 2025 UXL Foundation Contributors

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

#include <vector>
#include <oneapi/tbb/info.h>
#include <oneapi/tbb/task_arena.h>
#include <oneapi/tbb/task_group.h>

void set_numa_node_example() {
/*begin_set_numa_node_example*/
    std::vector<tbb::task_arena> numa_arenas = tbb::create_numa_task_arenas();
    std::vector<tbb::task_group> task_groups(numa_arenas.size());

    // Enqueue work to all but the first arena
    for(unsigned j = 1; j < numa_arenas.size(); j++) {
        numa_arenas[j].enqueue([](){/*some parallel work*/}, task_groups[j]);
    }

    // The main thread directly executes the work in the remaining arena
    numa_arenas[0].execute([&task_groups](){
        task_groups[0].run_and_wait([](){/*some parallel work*/});
    });

    for(unsigned j = 1; j < numa_arenas.size(); j++) {
        numa_arenas[j].wait_for(task_groups[j]);
    }
/*end_set_numa_node_example*/
}

int main() {
    set_numa_node_example();
    return 0;
}
