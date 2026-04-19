#include "blocked_nd_range_example.h"
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
