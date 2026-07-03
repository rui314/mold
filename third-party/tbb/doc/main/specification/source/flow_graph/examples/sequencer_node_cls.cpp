#include "oneapi/tbb/flow_graph.h"

struct Message {
    int id;
    int data;
};

int main() {
    oneapi::tbb::flow::graph g;

    // Due to parallelism the node can push messages to its successors in any order
    oneapi::tbb::flow::function_node< Message, Message > process(g, oneapi::tbb::flow::unlimited, [] (Message msg) -> Message {
        msg.data++;
        return msg; 
    });

    oneapi::tbb::flow::sequencer_node< Message > ordering(g, [](const Message& msg) -> int {
        return msg.id;
    });

    oneapi::tbb::flow::function_node< Message > writer(g, oneapi::tbb::flow::serial, [] (const Message& msg) {
        printf("Message received with id: %d\n", msg.id);
    });

    oneapi::tbb::flow::make_edge(process, ordering);
    oneapi::tbb::flow::make_edge(ordering, writer);

    for (int i = 0; i < 100; ++i) {
        Message msg = { i, 0 };
        process.try_put(msg);
    }

    g.wait_for_all();
}
