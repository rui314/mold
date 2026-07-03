# Copyright (c) 2025 Intel Corporation
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

BEGIN {
        print "range,repetitions,parallelism,avg_duration,rel_error"
}

match($0, /^#primes from \[([.0-9]+)\] .*\(([.0-9]+) sec with (.+) /, arr) {
  t_id=arr[3];
  range[t_id] = arr[1]
  thread_sum[t_id] += arr[2];
  thread_cnt[t_id] = thread_cnt[t_id] + 1
}

match($0, /(.+) (.+) Relative_Err : ([.0-9]+) %/, arr) {
  t_id=arr[1];
  print range[t_id] "," thread_cnt[t_id] "," t_id "," thread_sum[t_id]/(thread_cnt[t_id]) "," arr[3];
}
