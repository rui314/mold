/*
    Copyright (c) 2025 Intel Corporation

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

/*begin_blocked_nd_range_example*/
#include "oneapi/tbb/blocked_nd_range.h"
#include "oneapi/tbb/parallel_for.h"

template<typename Features>
float kernel3d(const Features& feature_maps, int i, int j, int k,
               int kernel_length, int kernel_width, int kernel_height) {
    float result = 0.f;

    for (int feature_i = i; feature_i < i + kernel_length; ++feature_i)
        for (int feature_j = j; feature_j < j + kernel_width; ++feature_j)
            for (int feature_k = k; feature_k < k + kernel_width; ++feature_k)
                result += feature_maps[feature_i][feature_j][feature_k];

    return result;
}

template<typename Features, typename Output>
void convolution3d(const Features& feature_maps, Output& out,
                   int out_length, int out_width, int out_heigth,
                   int kernel_length, int kernel_width, int kernel_height) {
    using range_t = oneapi::tbb::blocked_nd_range<int, 3>;

    oneapi::tbb::parallel_for(
        range_t({0, out_length}, {0, out_width}, {0, out_heigth}),
        [&](const range_t& out_range) {
            auto out_x = out_range.dim(0);
            auto out_y = out_range.dim(1);
            auto out_z = out_range.dim(2);

            for (int i = out_x.begin(); i < out_x.end(); ++i)
                for (int j = out_y.begin(); j < out_y.end(); ++j)
                    for (int k = out_z.begin(); k < out_z.end(); ++k)
                        out[i][j][k] = kernel3d(feature_maps, i, j, k,
                                                kernel_length, kernel_width, kernel_height);
        }
    );
}
/*end_blocked_nd_range_example*/

#include <vector>
#include <cassert>

int main() {
    const int kernel_length = 9;
    const int kernel_width = 5;
    const int kernel_height = 5;

    const int feature_maps_length = 128;
    const int feature_maps_width = 16;
    const int feature_maps_heigth = 16;

    const int out_length = feature_maps_length - kernel_length + 1;
    const int out_width = feature_maps_width - kernel_width + 1;
    const int out_heigth = feature_maps_heigth - kernel_height + 1;

    // Initializes feature maps with 1 in each cell and out with zeros.
    std::vector<std::vector<std::vector<float>>> feature_maps(feature_maps_length, std::vector<std::vector<float>>(feature_maps_width, std::vector<float>(feature_maps_heigth, 1.0f)));
    std::vector<std::vector<std::vector<float>>> out(out_length, std::vector<std::vector<float>>(out_width, std::vector<float>(out_heigth, 0.f)));

    // 3D convolution calculates the sum of all elements in the kernel
    convolution3d(feature_maps, out,
                  out_length, out_width, out_heigth,
                  kernel_length, kernel_width, kernel_height);

    // Checks correctness of convolution by equality to the expected sum of elements
    float expected = float(kernel_length * kernel_height * kernel_width);
    for (auto i : out) {
        for (auto j : i) {
            for (auto k : j) {
                assert(k == expected && "convolution failed to calculate correctly");
            }
        }
    }
    return 0;
}
