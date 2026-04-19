/*
    Copyright (c) 2023 Intel Corporation

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

#ifndef __TBB_test_common_test_invoke_H
#define __TBB_test_common_test_invoke_H

#include "test.h"
#include "oneapi/tbb/flow_graph.h"
#include "oneapi/tbb/blocked_range.h"

#if __TBB_CPP17_INVOKE_PRESENT
namespace test_invoke {

// Can be customized
template <typename T>
std::size_t get_real_index(const T& obj) {
    return obj;
}

template <typename Value>
class SmartRange : public oneapi::tbb::blocked_range<Value> {
    using base_range = oneapi::tbb::blocked_range<Value>;
public:
    SmartRange(const Value& first, const Value& last) : base_range(first, last), change_vector(nullptr) {}
    SmartRange(const Value& first, const Value& last, std::vector<std::size_t>& cv)
        : base_range(first, last), change_vector(&cv) {}

    SmartRange(const SmartRange&) = default;
    SmartRange(SmartRange& other, oneapi::tbb::split)
        : base_range(other, oneapi::tbb::split{}), change_vector(other.change_vector) {}

    void increase() const {
        CHECK_MESSAGE(change_vector, "Attempt to operate with no associated vector");
        for (std::size_t index = get_real_index(this->begin()); index != get_real_index(this->end()); ++index) {
            ++(*change_vector)[index];
        }
    }

    Value reduction(const Value& idx) const {
        Value result = idx;
        for (std::size_t index = get_real_index(this->begin()); index != get_real_index(this->end()); ++index) {
            result = result + Value(index);
        }
        return Value(result);
    }

    Value scan(const Value& idx, bool is_final_scan) const {
        CHECK_MESSAGE(change_vector, "Attempt to operate with no associated vector");
        Value result = idx;
        for (std::size_t index = get_real_index(this->begin()); index != get_real_index(this->end()); ++index) {
            result = result + Value(index);
            if (is_final_scan) (*change_vector)[index] = get_real_index(result);
        }
        return result;
    }
private:
    std::vector<std::size_t>* change_vector;
};

template <typename IDType>
class SmartID {
public:
    SmartID() : id(999), operate_signal_point(nullptr) {}
    SmartID(std::size_t* sp) : id(999), operate_signal_point(sp) {}

    SmartID(const IDType& n) : id(n), operate_signal_point(nullptr) {}
    SmartID(const IDType& n, std::size_t* sp) : id(n), operate_signal_point(sp) {}

    IDType get_id() const { return id; }
    const IDType& get_id_ref() const { return id; }

private:
    template <typename TupleOfPorts, std::size_t... Is>
    void send_id_impl(TupleOfPorts& ports, std::index_sequence<Is...>) const {
        (std::get<Is>(ports).try_put(id) , ...);
    }
public:
    template <typename TupleOfPorts>
    void send_id(TupleOfPorts& ports) const {
        send_id_impl(ports, std::make_index_sequence<std::tuple_size<TupleOfPorts>::value>());
    }

    template <typename GatewayType>
    void send_id_to_gateway(GatewayType& gateway) const {
        gateway.reserve_wait();
        gateway.try_put(id);
        gateway.release_wait();
    }

    void operate() const {
        CHECK_MESSAGE(operate_signal_point, "incorrect test setup");
        ++(*operate_signal_point);
    }

    IDType id;
private:
    std::size_t* operate_signal_point;
};

class SmartValue {
public:
    SmartValue(std::size_t rv) : real_value(rv) {}
    SmartValue(const SmartValue&) = default;
    SmartValue& operator=(const SmartValue&) = default;

    SmartValue operator+(const SmartValue& other) const {
        return SmartValue{real_value + other.real_value};
    }
    std::size_t operator-(const SmartValue& other) const {
        return real_value - other.real_value;
    }

    std::size_t get() const { return real_value; }

    bool operator<(const SmartValue& other) const {
        return real_value < other.real_value;
    }

    SmartValue& operator++() { ++real_value; return *this; }
private:
    std::size_t real_value;
};

std::size_t get_real_index(const SmartValue& value) {
    return value.get();
}


} // namespace test_invoke

#endif // __TBB_CPP17_INVOKE_PRESENT
#endif // __TBB_test_common_test_invoke_H
