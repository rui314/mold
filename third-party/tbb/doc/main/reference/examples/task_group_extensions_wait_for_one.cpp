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

#include <unordered_map>
#include <mutex>
#include <vector>

class Cache {
public:
    Cache() = default;

    bool lookup(int key, int& result) const {
        std::unique_lock<std::mutex> lock(m_mutex);
        auto it = m_key_result_map.find(key);
        if (it == m_key_result_map.end()) {
            return false;
        } else {
            result = it->second;
            return true;
        }
    }

    void add(int key, int result) {
        std::unique_lock<std::mutex> lock(m_mutex);
        m_key_result_map.emplace(key, result);
    }

    void clear() {
        std::unique_lock<std::mutex> lock(m_mutex);
        m_key_result_map.clear();
    }
private:
    std::unordered_map<int, int> m_key_result_map;
    mutable std::mutex m_mutex; 
};

Cache global_cache;

bool cache_lookup(int key, int& result) {
    return global_cache.lookup(key, result);
}

void add_to_cache(int key, int result) {
    global_cache.add(key, result);
}

void clear_cache() {
    global_cache.clear();
}

struct calculate_result {
    int input;
    int& result;

    calculate_result(int i, int& r)
        : input(i), result(r) {}

    void operator()() const {
        result = input * input;
    }
};

struct cache_result {
    int input;
    int result;

    cache_result(int i, int r)
        : input(i), result(r) {}

    void operator()() const {
        add_to_cache(input, result);
    }
};

/*begin_task_group_extensions_wait_for_one_example*/
#define TBB_PREVIEW_TASK_GROUP_EXTENSIONS 1
#include <oneapi/tbb/task_group.h>

int calculate_one_result(tbb::task_group& tg, int input) {
    int result = 0;

    // Check cache first
    if (cache_lookup(input, result)) {
        return result;
    }

    // No cached item - run result calculation
    // May build task graph with dependencies
    tbb::task_handle calculate_task = tg.defer(calculate_result(input, result));
    tg.run_and_wait_for_task(std::move(calculate_task));

    // Result is calculated - run asynchronous caching
    tg.run(cache_result(input, result));

    // Return the result to caller
    return result;
}

void calculate_all_results(const std::vector<int>& inputs, std::vector<int>& results) {
    tbb::task_group tg;

    for (std::size_t i = 0; i < inputs.size(); ++i) {
        results[i] = calculate_one_result(tg, inputs[i]);
    }

    // Wait for all incomplete caching tasks before clear
    tg.wait();
    clear_cache();
}

/*end_task_group_extensions_wait_for_one_example*/

int main() {
    std::vector<int> inputs = {1, 2, 3, 4, 5};
    std::vector<int> results = {0, 0, 0, 0, 0};

    calculate_all_results(inputs, results);
}
