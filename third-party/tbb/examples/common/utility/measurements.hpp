/*
    Copyright (c) 2005-2025 Intel Corporation

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

#ifndef TBB_examples_measurements_H
#define TBB_examples_measurements_H

#include <algorithm>
#include <cassert>
#include <chrono>
#include <numeric>
#include <vector>

namespace utility {

// utility class to aid relative error measurement of samples
class measurements {
public:
    measurements() = default;

    measurements(unsigned iterations) {
        _iterations = iterations;
        _time_intervals.reserve(_iterations);
    }

    inline unsigned iterations() {
        return _iterations;
    }

    inline void start() {
        _startTime = std::chrono::steady_clock::now();
    }

    inline std::chrono::microseconds stop() {
        auto _endTime = std::chrono::steady_clock::now();
        // store the end time and start time
        _time_intervals.push_back(std::make_pair(_startTime, _endTime));
        return std::chrono::duration_cast<std::chrono::microseconds>(_endTime - _startTime);
    }

    double computeRelError() {
        // Accumulate the total duration in microseconds using std::accumulate with a lambda function
        assert(0 != _time_intervals.size());
        unsigned long long total_duration = std::accumulate(
            _time_intervals.begin(),
            _time_intervals.end(),
            0ull, // Start with 0 count
            [](long long total, const std::pair<time_point, time_point>& interval) {
                // Compute the difference and add it to the total
                return total + std::chrono::duration_cast<std::chrono::microseconds>(
                                   interval.second - interval.first)
                                   .count();
            });
        unsigned long long averageTimePerFrame = total_duration / _time_intervals.size();
        unsigned long long sumOfSquareDiff = 0;
        std::for_each(_time_intervals.begin(),
                      _time_intervals.end(),
                      [&](const std::pair<time_point, time_point>& interval) {
                          unsigned long long duration =
                              std::chrono::duration_cast<std::chrono::microseconds>(
                                  interval.second - interval.first)
                                  .count();
                          long long diff = duration - averageTimePerFrame;
                          sumOfSquareDiff += diff * diff;
                      });
        double stdDev = std::sqrt(sumOfSquareDiff / _time_intervals.size());
        double relError = 100 * (stdDev / averageTimePerFrame);
        return relError;
    }

private:
    using time_point = std::chrono::steady_clock::time_point;
    time_point _startTime;
    std::vector<std::pair<time_point, time_point>> _time_intervals;
    unsigned _iterations;
};

} // namespace utility

#endif /* TBB_examples_measurements_H */
