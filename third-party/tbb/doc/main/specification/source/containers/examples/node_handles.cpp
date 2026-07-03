#include "oneapi/tbb/concurrent_unordered_map.h"

int main() {
    using Map = oneapi::tbb::concurrent_unordered_map<int, int>;
    Map map = {{1, 1}, {2, 2}, {3, 3}};

    // Extract an element from the container
    Map::node_type nh = map.unsafe_extract(2);

    // Change key/value of handled element
    nh.key() = 7;
    nh.mapped() = 9;

    // Insert an element to the new container
    Map map2;
    map2.insert(std::move(nh));
}
