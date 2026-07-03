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
        print "infile,repetitions,avg_duration,rel_error"
        summ=0;
        cnt=0;
}

match($0, /tachyon.*n-of-repeats=([0-9]+) (.+): ([.0-9]+) seconds/, arr)  {
  curr_rep = arr[1];
  curr_file = arr[2];
  summ += arr[3];
  cnt++
}

/Relative_Err/ {
  print curr_file "," curr_rep "," summ/cnt "," $3
}
