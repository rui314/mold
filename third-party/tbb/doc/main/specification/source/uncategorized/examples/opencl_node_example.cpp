#define TBB_PREVIEW_FLOW_GRAPH_NODES 1
#define TBB_PREVIEW_FLOW_GRAPH_FEATURES 1

#include "oneapi/tbb/flow_graph_opencl_node.h"

typedef oneapi::tbb::flow::opencl_buffer<cl_int> opencl_vector;

class gpu_device_selector {
public:
    template <typename DeviceFilter>
    oneapi::tbb::flow::opencl_device operator()(oneapi::tbb::flow::opencl_factory<DeviceFilter>& f) {
        // Set your GPU device if available to execute kernel on
        const oneapi::tbb::flow::opencl_device_list &devices = f.devices();
        oneapi::tbb::flow::opencl_device_list::const_iterator it = std::find_if(
            devices.cbegin(), devices.cend(),
            [](const oneapi::tbb::flow::opencl_device &d) {
            cl_device_type type;
            d.info(CL_DEVICE_TYPE, type);
            return CL_DEVICE_TYPE_GPU == type;
        });
        if (it == devices.cend()) {
            std::cout << "Info: could not find any GPU devices. Choosing the first available device (default behaviour)." << std::endl;
            return *(f.devices().begin());
        } else {
            // Return GPU device from factory
            return *it;
        }
    }
};

int main() {
    const int vector_size = 10;
    int sum = 0;

    oneapi::tbb::flow::graph g;

    oneapi::tbb::flow::broadcast_node< opencl_vector > broadcast(g);

    // GPU computation part
    gpu_device_selector gpu_selector;

    oneapi::tbb::flow::opencl_program<> program("vector_operations.cl");

    oneapi::tbb::flow::opencl_node< std::tuple< opencl_vector > >
        vector_cuber(g, program.get_kernel("cuber"), gpu_selector);

    oneapi::tbb::flow::opencl_node< std::tuple< opencl_vector > >
        vector_squarer(g, program.get_kernel("squarer"), gpu_selector);

    // Define kernel argument and problem size
    vector_cuber.set_args(oneapi::tbb::flow::port_ref<0>);
    vector_cuber.set_range({{ vector_size }});
    vector_squarer.set_args(oneapi::tbb::flow::port_ref<0>);
    vector_squarer.set_range({{ vector_size }});

    // Computation results join
    oneapi::tbb::flow::join_node< std::tuple< opencl_vector, opencl_vector > > join_data(g);

    // CPU computation part
    oneapi::tbb::flow::function_node< std::tuple< opencl_vector, opencl_vector > >
        summer( g, 1, [&](const std::tuple< opencl_vector, opencl_vector > &res ) {
            opencl_vector vect_cubed = std::get<0>(res);
            opencl_vector vect_squared = std::get<1>(res);

            for (int i = 0; i < vector_size; i++) {
                sum += vect_cubed[i] + vect_squared[i];
            }
        });

    // Graph topology
    make_edge( broadcast, vector_cuber );
    make_edge( broadcast, vector_squarer );
    make_edge( vector_cuber, oneapi::tbb::flow::input_port<0>(join_data) );
    make_edge( vector_squarer, oneapi::tbb::flow::input_port<1>(join_data) );
    make_edge( join_data, summer );

    // Data initialization
    opencl_vector vec(vector_size);
    std::fill( vec.begin(), vec.end(), 2 );

    // Run the graph
    broadcast.try_put(vec);
    g.wait_for_all();

    std::cout << "Sum is " << sum << "\n";

    return 0;
}
