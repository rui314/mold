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
        print "infile,repetitions,threads,avg_duration,rel_error"
        match(run_args, /filename=(.+)[ ;]/, run_args_);
        curr_file = run_args_[1]
}

match($0, /(Sudoku: Time to find .* on )([0-9]+)( threads: )([.0-9]+) seconds./, arr) {
  t_id=arr[2];
  thread_sum[t_id] += arr[4];
  thread_cnt[t_id] = thread_cnt[t_id] + 1
}

match($0, /(Sudoku: Time to find.* on )([0-9]+)( threads[:]* Relative_Err : )([.0-9]+) %/, arr) {
  t_id=arr[2];
  print curr_file "," thread_cnt[t_id] "," t_id "," thread_sum[t_id]/(thread_cnt[t_id]) "," arr[4];
}
