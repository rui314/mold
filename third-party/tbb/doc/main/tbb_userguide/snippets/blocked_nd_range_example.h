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
