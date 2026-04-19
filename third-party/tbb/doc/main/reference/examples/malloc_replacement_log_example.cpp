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

#if _WIN32

/*begin_malloc_replacement_log_example*/
#include "oneapi/tbb/tbbmalloc_proxy.h"
#include <stdio.h>

int main(){
    char **func_replacement_log;
    int func_replacement_status = TBB_malloc_replacement_log(&func_replacement_log);

    if (func_replacement_status != 0) {
        printf("tbbmalloc_proxy cannot replace memory allocation routines\n");
        for (char** log_string = func_replacement_log; *log_string != 0; log_string++) {
            printf("%s\n",*log_string);
        }
    }

    return 0;
}
/*end_malloc_replacement_log_example*/

#else
// Skip
int main() {}
#endif
