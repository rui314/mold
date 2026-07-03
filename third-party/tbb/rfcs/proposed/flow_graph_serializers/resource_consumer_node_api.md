# Resource-Limited Node API for the Flow Graph

Note: This document is a sub-RFC for the [Resource-limited Nodes RFC](./README.md).

## Table of Contents

* 1 [Feature Design](#feature-design)
  * 1.1 [Terminology](#terminology)
  * 1.2 [Proposed Design](#proposed-design)
  * 1.3 [API Details](#api-details)
  * 1.4 [API Specification](#api-specification)
    * 1.4.1 [`oneapi::tbb::flow::resource_limiter` Class](#oneapitbbflowresource_limiter-class)
    * 1.4.2 [`ResourceLimitedBody` Named Requirements](#resourcelimitedbody-named-requirements)
    * 1.4.3 [`oneapi::tbb::flow::resource_limited_node` Class](#oneapitbbflowresource_limited_node-class)
* 2 [API to Support Polymorphic Providers](#api-to-support-polymorphic-providers)
* 3 [Exit Criteria and Open Questions](#exit-criteria-and-open-questions)
* 4 [Usage Examples](#usage-examples)
  * 4.1 [ROOT, GENIE and DB Example](#root-genie-and-db-example)
  * 4.2 [Dining Philosophers](#dining-philosophers)

## Feature Design

As described in the [parent RFC](./README.md),
the current Flow Graph functional nodes API provides only a mechanism for limiting the number of bodies executed in parallel.
However, since limiting only the concurrency of a single node does not satisfy
some use cases, the API should be extended to support limiting access to shared resources across several nodes in the graph.

### Terminology

*Resource* - an entity that represents the possibility of accessing something.

*Provider* - an entity that provides one or several resources of a certain kind.

*Consumer* - a user of one or several resources.

*Protocol* - a set of rules and actions that define the relationship between a consumer and a provider.

### Proposed Design

It is proposed to extend the current Flow Graph API with two entities representing a *Provider* and a *Consumer* of one or several
*Resources* and to implement a *Protocol* with unspecified details.

```cpp
namespace oneapi {
namespace tbb {
namespace flow {

template <typename ResourceHandle>
class resource_limiter {
    using resource_handle_type = ResourceHandle;

    template <typename Handle, typename... Handles>
    resource_limiter(Handle&& handle, Handles&&... handles);
};

template <typename Input, typename OutputTuple>
class resource_limited_node : public graph_node, public receiver<Input>
{
public:
    using output_ports_type = /*undefined tuple of output ports*/

    template <typename Body, typename ResourceProvider, typename... ResourceProviders>
    resource_limited_node(graph& g, std::size_t concurrency,
                          std::tuple<ResourceProvider&, ResourceProviders&...> resource_providers,
                          Body body);

    resource_limited_node(const resource_limited_node& other);
    ~resource_limited_node();

    bool try_put(const Input& input);
}; // class resource_limited_node

} // namespace flow
} // namespace tbb
} // namespace oneapi
```

### API Details

The class `oneapi::tbb::flow::resource_limiter<ResourceHandle>` is a *Provider* of the *Resource* represented by
the `ResourceHandle` template argument. It can be viewed an unspecified container type holding one
or several resource handle instances.

For some resource types, the `ResourceHandle` is the resource itself, e.g. for the resource and a handle of type `int` or `float`:

```cpp
oneapi::tbb::flow::resource_limiter<int> int_holder{1, 2, 3, 4, 5, 6, 7, 8, 9, 10};
oneapi::tbb::flow::resource_limiter<float> float_holder{11.f};
```

The `int_holder` object contains 10 resources of type `int`, and `float_holder` contains a single resource of type `float`.

For other resource types, the `ResourceHandle` may represent a lightweight entity used to access the resource. For example:

```cpp
HeavyResourceOutsideGraphScope resource;

using handle_type = HeavyResourceOutsideGraphScope*;
oneapi::tbb::flow::resource_limiter<handle_type> provider{&resource};
```

All the resource handles managed by the provider are considered equivalent, and the order in which the access to resources
is granted is unspecified.

`oneapi::tbb::flow::resource_limited_node<Input, OutputTuple>` is similar to `multifunction_node<Input, Output>`, but 
additionally represents a *Consumer* of resources, provided by one or several providers. The node takes a tuple of references to the providers
of the required resources as an additional constructor argument.

When the input message arrives at the node, it acquires part of the node's concurrency limit. If the concurrency limit is exceeded,
the input is buffered in the internal queue, similar to how a `multifunction_node` with the `oneapi::tbb::flow::queueing` policy behaves.

The necessity of providing a `rejecting` `resource_limited_node` is an open question.

If the concurrency limit is not exceeded, the node spawns a task that requests access to each resource needed to execute the body.
Once all the accesses are granted, the user body is executed, and a reference to the handles representing the resources is passed to it:

```cpp
oneapi::tbb::flow::graph g;

using input = int;
using output = std::tuple<double>;

auto node_body = [](int input,         // input message
                    auto& ports,       // output ports tuple, similar to multifunction_node
                    int& i_resource,   // reference to integral resource from int_holder
                    float& f_resource // reference to float resource from float_holder
                    )
    {
        std::get<0>(ports).try_put(output);
    };

oneapi::tbb::flow::resource_limited_node<input, output> node(g, oneapi::tbb::flow::unlimited,
    std::tie(int_holder, float_holder), // tuple of references to two resources needed by the node
    node_body);
```

When the resource is used by one of the nodes in the Flow Graph, the `resource_limiter` would not grant access to it to any other node.

If access to one or several resources cannot be granted immediately, the node and the provider utilize the unspecified *Protocol*, which defines
how and when access will be granted to each node that requests it. The concurrency held by the currently processed input message is not 
released until all necessary resource accesses are granted and the body is executed.

### API Specification

#### `oneapi::tbb::flow::resource_limiter` Class

```cpp
template <typename ResourceHandle>
class resource_limiter;
```

A provider of one or several resources represented by the `ResourceHandle` type.

`ResourceHandle` must meet the `MoveConstructible` requirements from [moveconstructible] and the `MoveAssignable` requirements from [moveassignable]
sections of the ISO C++ Standard.

```cpp
using resource_handle_type = ResourceHandle;
```

An alias to the resource handle type used by the provider.

```cpp
template <typename Handle, typename... Handles>
resource_limiter(Handle&& handle, Handles&&... handles);
```

Constructs a resource provider containing resources represented by the ``handle`` and the ``handles``.

``ResourceHandle`` must be constructible from ``std::forward<Handle>(handle)``,
and from ``std::forward<H>(h)`` for each ``H`` in ``Handles`` and for each ``h`` in ``handles``.

#### `oneapi::tbb::flow::resource_limited_node` Class

```cpp
template <typename Input, typename OutputTuple>
class resource_limited_node;
```

A node that receives messages at a single input port and requires access to resources provided by one or several resource providers.
A node may generate one or more output messages that are broadcast to successors using `N` output ports, where `N` is `std::tuple_size<OutputTuple>::value`.

Type requirements:

* The `Input` type must meet the `DefaultConstructible` requirements from [defaultconstructible] and the `CopyConstructible` requirements from [copyconstructible]
sections of the ISO C++ Standard.
* The `OutputTuple` type must be a specialization of `std::tuple`.

`resource_limited_node` is a `graph_node` and a `receiver<Input>`, and it has a tuple of `sender<Output>` outputs, where `Output` is a type of element in `OutputTuple`.

`resource_limited_node` has the *discarding* and *broadcast-push* [properties](https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/flow_graph/forwarding_and_buffering).

```cpp
using output_ports_type = ...;
```

An alias for a `std::tuple` of output ports.

```cpp
template <typename Body, typename ResourceProvider, typename... ResourceProviders>
resource_limited_node(graph& g, std::size_t concurrency,
                      std::tuple<ResourceProvider&, ResourceProviders&...> resource_providers,
                      Body body);
```

Constructs a `resource_limited_node` that belongs to the graph `g`. 

The concurrency limit of the node is set to `concurrency`. It can be one of the
[predefined values](https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/flow_graph/predefined_concurrency_limits)
or any value of type `std::size_t`.

When the concurrency limit allows, and access to all required resources is granted by each element in `resource_providers`,
the node executes the user-provided body on the input messages. The body can create one or more output messages and broadcast them to successors.

After execution of the user-provided body, the node returns all granted resources back to their respective providers.

If the concurrency limit is exceeded, the input message is queued in the internal buffer and is processed once the concurrency becomes available.

The body object passed to a `resource_limited_node` is copied. Updates to member variables do not affect the original object used to construct the node.
If the state held within a body object must be inspected from outside the node, the
[`copy_body` function](https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/flow_graph/copy_body_func) can be used to obtain an
updated body.

The type `ResourceProvider` and each type `RP` in `ResourceProviders` must be specializations of `oneapi::tbb::flow::resource_limiter`.

The type `Body` must meet the requirements of [`ResourceLimitedNodeBody`](#resourcelimitedbody-named-requirements).

```cpp
resource_limited_node(const resource_limited_node& other);
```

Constructs a `resource_limited_node` with the same initial state that `other` had when it was constructed:

* It belongs to the same `graph` object as `other`
* It has the same concurrency threshold as `other`
* It requests access to the same set of resources as `other`
* It has a copy of the initial `body` used by `other`.

The predecessors and successors of `other` are not copied.

The new body object is copy-constructed from a copy of the original body provided to `other` at its construction. Changes made to member variables in `other`'s body
after the construction of `other` do not affect the body of the new `resource_limited_node`.

```cpp
~resource_limited_node();
```

Destroys the `resource_limited_node` object.

```cpp
bool try_put(const Input& v);
```

Passes the incoming message `v` to the node. Once the concurrency limit allows and the access to all required resources is granted,
the node executes the user-provided body on `v`.

*Returns*: `true`.

#### `ResourceLimitedBody` Named Requirements

The type `Body` satisfies `ResourceLimitedBody` if it

* is `CopyConstructible`,
* is `Destructible`
* provides the invoke function with the pseudo-signature shown below.

```cpp
void Body::operator()(const Input& v, OutputPortsType& p,
                      ResourceHandle1& resource_handle1, ..., ResourceHandleN& resource_handleN)
```

Requirements:

* The `Input` type must be the same as the `Input` template type argument of the `resource_limited_node` instance into which the `Body` object is passed during construction.
* The `OutputPortsType` must be the same as the `output_ports_type` member type of the `resource_limited_node` instance into which the `Body` object is passed during construction.
* `ResourceHandle1`, ..., `ResourceHandleN` must be the same as `resource_limiter::resource_handle_type` member type for each `ResourceProvider` used by the `resource_limited_node`
instance into which the `Body` object is passed during construction.

Performs an operation on `v`. It may call `try_put` on zero or more of the output ports and may call `try_put` on any output port multiple times.

## API to Support Polymorphic Providers

The API proposed in this document assumes that the protocol between the *Provider* and the *Consumer* (the node) is undefined and that only the `oneapi::tbb::flow::resource_limiter`
object may be used to construct the node. Hence, the types of the resource handles needed to execute the body are known at the time of the node's construction.

Therefore, the references to the exact resource handles are provided as arguments to the node body.

However, if different protocols, different *Providers*, or user-defined providers were allowed, the proposed design would break.

Consider defining a provider as an interface that requires two functions, `get` and `release`, to be implemented:

```cpp
class provider_base {
    virtual get_return_type     get() = 0;
    virtual release_return_type release() = 0;
};
```

In this case, the `resource_limited_node` would be allowed to take objects of types derived from `provider_base` as arguments. Something like:

```cpp
template <typename Body>
resource_limited_node(graph& g, std::size_t concurrency, std::vector<provider_base&> resource_providers, Body body);
```

In this case, the base class does not define the type of the resource handle used by the actual implementation, and therefore an object of the correct type cannot be passed to the node's body.
The exact type may be unknown even to the implementers of the Flow Graph.

```cpp
void construct_graph(provider_base& resource_provider1, provider_base& resource_provider2) {
    using namespace oneapi::tbb::flow;

    graph g;

    // Some nodes
    resource_limited_node<input, output> node(g, unlimited,
        {resource_provider1, resource_provider2}, // Types of resource handles are unknown
        [](input i, auto& ports, /*???? arguments of which types to accept ????*/) {});
}
```

If such an extension to allowed resource providers is considered, the named requirements of the body should be changed to address this concern.
For example:

```cpp
auto node_body = [](input i, auto& ports, void* resource_handle_ptr1, void* resource_handle_ptr2) {
    // At the point where the type is known
    ActualResourceHandle1& resource_handle1 = *static_cast<ActualResourceHandle1*>(resource_handle_ptr1);
    ActualResourceHandle2& resource_handle2 = *static_cast<ActualResourceHandle2*>(resource_handle_ptr2);
};
```

## Exit Criteria and Open Questions

1. Is `resource_limited_node` a suitable name for a Resource-Limited Flow Graph node? Alternative names are:
    * `resource_consumer_node`
    * `rl_multifunction_node`
    * `rc_multifunction_node`
    * `resource_limited_multifunction_node`
    * `resource_consumer_multifunction_node`
2. Should a Resource-Limited alternative for `function_node` be provided?
3. Should the experimental version of `resource_limited_node` support node priorities?
4. Should the `output_ports()` member function be provided by `resource_limited_node`?
5. Should the `rejecting` alternative be provided for `resource_limited_node`?
6. Instead of a separate node type, should `resource_limited` be considered as a policy for existing functional nodes?
7. A possibility to extend the API to support polymorphic resource providers should be considered. Refer to the [separate section](#api-to-support-polymorphic-providers) for more details.
8. The protocol used by consumers and providers must be defined and part of the public interface before moving to production. This protocol should support consumers beyond flow graph nodes.
9. There should be at least one concrete provider type that provides some level of starvation avoidance before moving to production.

## Usage Examples

### ROOT, GENIE and DB Example

Consider implementing the flow graph described in the [parent RFC](./README.md#implementation-experience).
Below is an example of how to implement it using proposed API.

```cpp
using namespace tbb::flow;

using input_type = ...;

auto generate_input_body = [](tbb::flow_control& fc) -> input_type {
    // generate input data
};

using histogramming_output = std::tuple<...>;   // Output types for histogramming node
using generating_output = std::tuple<...>;      // Output types for generating node
using histogenerating_output = std::tuple<...>; // Output types for histogenerating node
using calibration_output = std::tuple<...>;     // Output types for calibration nodes

graph g;

resource_limiter<ROOT_handle_type> root_provider{ROOT_handle};
resource_limiter<GENIE_handle_type> genie_provider{GENIE_handle};
resource_limiter<DB_handle_type> db_provider{DB_handle1, DB_handle2};

input_node<input_type> source(g, generate_input_body);

resource_limited_node<input_type, histogramming_output>
    histogramming_node(g, unlimited,
                       std::tie(root_provider),
                       [](input_type input, auto& output_ports,
                          ROOT_handle_type& root_resource_handle) {
                           // Using root_resource_handle for histogramming
                           // Using output_ports to broadcast outputs
                       });

resource_limited_node<input_type, generating_output>
    generating_node(g, unlimited,
                    std::tie(genie_provider),
                    [](input_type input, auto& output_ports,
                       GENIE_handle_type& genie_resource_handle) {
                        // ...
                    });

resource_limited_node<input_type, histogenerating_output>
    histogenerating_node(g, unlimited,
                         std::tie(root_provider, genie_provider),
                         [](input_type input, auto& output_ports,
                            ROOT_handle_type& root_resource_handle, GENIE_handle_type& genie_resource_handle) {
                             // ...
                         });

function_node<input_type, input_type>
    propagating_node(g, unlimited,
                     [](input_type input) {
                        return input;
                     });

resource_limited_node<input_type, calibration_output>
    calibration_node_A(g, unlimited,
                       std::tie(db_provider),
                       [](input_type input, auto& output_ports,
                          DB_handle_type& db_resource_handle) {
                           // ...
                       });

resource_limited_node<input_type, calibration_output>
    calibration_node_B(/*same as calibration node A, except the body*/);

resource_limited_node<input_type, calibration_output>
    calibration_node_C(g, serial,
                       std::tie(db_provider),
                       [](input_type input, auto& output_ports,
                          DB_handle_type& db_resource_handle) {
                           // ...
                       });

make_edge(source, histogramming_node);
make_edge(source, generating_node);
make_edge(source, histogenerating_node);
make_edge(source, propagating_node);
make_edge(source, calibration_node_A);
make_edge(source, calibration_node_B);
make_edge(source, calibration_node_C);

source.activate();
g.wait_for_all();
```

### Dining Philosophers

Below is how the oneTBB Dining Philosophers example can be implemented using the proposed API:

```cpp
struct chopstick {};

std::size_t num_philosophers = 4;

using namespace tbb::flow;

using think_node_type = function_node<continue_msg, continue_msg>;
using eat_node_type = resource_limited_node<continue_msg, std::tuple<continue_msg>>;

auto think_body = [](continue_msg) { think(); };
auto eat_body = [](continue_msg, auto& output_ports, chopstick, chopstick) {
    eat();
    if (keep_thinking) {
        std::get<0>(output_ports).try_put(continue_msg{});
    }
};

std::vector<resource_limiter<chopstick>> chopsticks;
std::vector<think_node_type*> think_nodes;
std::vector<eat_node_type*> eat_nodes;

chopsticks.reserve(num_philosophers);
think_nodes.reserve(num_philosophers);
eat_nodes.reserve(num_philosophers);

for (std::size_t i = 0; i < num_philosophers; ++i) {
    auto init = {chopstick{}};
    chopsticks.emplace_back(init);
}

for (std::size_t i = 0; i < num_philosophers; ++i) {
    think_nodes.emplace_back(new think_node_type(g, unlimited, think_body));
    eat_nodes.emplace_back(
        new eat_node_type(g, unlimited,
                          std::tie(chopsticks[i], chopsticks[(i + 1) % num_philosophers]),
                          eat_body));

    make_edge(eat_nodes[i], think_nodes[i]);
}

// Start thinking
for (std::size_t i = 0; i < num_philosophers; ++i) {
    think_nodes[i].try_put(continue_msg{});
}
g.wait_for_all();
```
