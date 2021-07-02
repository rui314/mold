/*
    Copyright (c) 2005-2021 Intel Corporation

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

//
// Example program that reads a file of decimal integers in text format
// and changes each to its square.
//

#include <cstring>
#include <cstdlib>
#include <cstdio>
#include <cctype>

#include "oneapi/tbb/parallel_pipeline.h"
#include "oneapi/tbb/tick_count.h"
#include "oneapi/tbb/tbb_allocator.h"
#include "oneapi/tbb/global_control.h"

#include "common/utility/utility.hpp"
#include "common/utility/get_default_num_threads.hpp"

extern void generate_if_needed(const char*);

//! Holds a slice of text.
/** Instances *must* be allocated/freed using methods herein, because the C++ declaration
    represents only the header of a much larger object in memory. */
class TextSlice {
    //! Pointer to one past last character in sequence
    char* logical_end;
    //! Pointer to one past last available byte in sequence.
    char* physical_end;

public:
    //! Allocate a TextSlice object that can hold up to max_size characters.
    static TextSlice* allocate(std::size_t max_size) {
        // +1 leaves room for a terminating null character.
        TextSlice* t = (TextSlice*)oneapi::tbb::tbb_allocator<char>().allocate(sizeof(TextSlice) +
                                                                               max_size + 1);
        t->logical_end = t->begin();
        t->physical_end = t->begin() + max_size;
        return t;
    }
    //! Free a TextSlice object
    void free() {
        oneapi::tbb::tbb_allocator<char>().deallocate(
            (char*)this, sizeof(TextSlice) + (physical_end - begin()) + 1);
    }
    //! Pointer to beginning of sequence
    char* begin() {
        return (char*)(this + 1);
    }
    //! Pointer to one past last character in sequence
    char* end() {
        return logical_end;
    }
    //! Length of sequence
    std::size_t size() const {
        return logical_end - (char*)(this + 1);
    }
    //! Maximum number of characters that can be appended to sequence
    std::size_t avail() const {
        return physical_end - logical_end;
    }
    //! Append sequence [first,last) to this sequence.
    void append(char* first, char* last) {
        memcpy(logical_end, first, last - first);
        logical_end += last - first;
    }
    //! Set end() to given value.
    void set_end(char* p) {
        logical_end = p;
    }
};

std::size_t MAX_CHAR_PER_INPUT_SLICE = 4000;
std::string InputFileName = "input.txt";
std::string OutputFileName = "output.txt";

TextSlice* next_slice = nullptr;

class MyInputFunc {
public:
    MyInputFunc(FILE* input_file_);
    MyInputFunc(const MyInputFunc& f) : input_file(f.input_file) {}
    ~MyInputFunc();
    TextSlice* operator()(oneapi::tbb::flow_control& fc) const;

private:
    FILE* input_file;
};

MyInputFunc::MyInputFunc(FILE* input_file_) : input_file(input_file_) {}

MyInputFunc::~MyInputFunc() {}

TextSlice* MyInputFunc::operator()(oneapi::tbb::flow_control& fc) const {
    // Read characters into space that is available in the next slice.
    if (!next_slice)
        next_slice = TextSlice::allocate(MAX_CHAR_PER_INPUT_SLICE);
    std::size_t m = next_slice->avail();
    std::size_t n = fread(next_slice->end(), 1, m, input_file);
    if (!n && next_slice->size() == 0) {
        // No more characters to process
        fc.stop();
        return nullptr;
    }
    else {
        // Have more characters to process.
        TextSlice* t = next_slice;
        next_slice = TextSlice::allocate(MAX_CHAR_PER_INPUT_SLICE);
        char* p = t->end() + n;
        if (n == m) {
            // Might have read partial number.
            // If so, transfer characters of partial number to next slice.
            while (p > t->begin() && isdigit(p[-1]))
                --p;
            assert(p > t->begin()); // Number too large to fit in buffer
            next_slice->append(p, t->end() + n);
        }
        t->set_end(p);
        return t;
    }
}

// Functor that changes each decimal number to its square.
class MyTransformFunc {
public:
    TextSlice* operator()(TextSlice* input) const;
};

TextSlice* MyTransformFunc::operator()(TextSlice* input) const {
    // Add terminating null so that strtol works right even if number is at end of the input.
    *input->end() = '\0';
    char* p = input->begin();
    TextSlice* out = TextSlice::allocate(2 * MAX_CHAR_PER_INPUT_SLICE);
    char* q = out->begin();
    for (;;) {
        while (p < input->end() && !isdigit(*p))
            *q++ = *p++;
        if (p == input->end())
            break;
        long x = strtol(p, &p, 10);
        // Note: no overflow checking is needed here, as we have twice the
        // input string length, but the square of a non-negative integer n
        // cannot have more than twice as many digits as n.
        long y = x * x;
        sprintf(q, "%ld", y);
        q = strchr(q, 0);
    }
    out->set_end(q);
    input->free();
    return out;
}

// Functor that writes a TextSlice to a file.
class MyOutputFunc {
    FILE* my_output_file;

public:
    MyOutputFunc(FILE* output_file);
    void operator()(TextSlice* item) const;
};

MyOutputFunc::MyOutputFunc(FILE* output_file) : my_output_file(output_file) {}

void MyOutputFunc::operator()(TextSlice* out) const {
    std::size_t n = fwrite(out->begin(), 1, out->size(), my_output_file);
    if (n != out->size()) {
        fprintf(stderr, "Can't write into file '%s'\n", OutputFileName.c_str());
        std::exit(-1);
    }
    out->free();
}

bool silent = false;

int run_pipeline(int nthreads) {
    FILE* input_file = fopen(InputFileName.c_str(), "r");
    if (!input_file) {
        throw std::invalid_argument(("Invalid input file name: " + InputFileName).c_str());
        return 0;
    }
    FILE* output_file = fopen(OutputFileName.c_str(), "w");
    if (!output_file) {
        throw std::invalid_argument(("Invalid output file name: " + OutputFileName).c_str());
        return 0;
    }

    oneapi::tbb::tick_count t0 = oneapi::tbb::tick_count::now();

    // Need more than one token in flight per thread to keep all threads
    // busy; 2-4 works
    oneapi::tbb::parallel_pipeline(
        nthreads * 4,
        oneapi::tbb::make_filter<void, TextSlice*>(oneapi::tbb::filter_mode::serial_in_order,
                                                   MyInputFunc(input_file)) &
            oneapi::tbb::make_filter<TextSlice*, TextSlice*>(oneapi::tbb::filter_mode::parallel,
                                                             MyTransformFunc()) &
            oneapi::tbb::make_filter<TextSlice*, void>(oneapi::tbb::filter_mode::serial_in_order,
                                                       MyOutputFunc(output_file)));

    oneapi::tbb::tick_count t1 = oneapi::tbb::tick_count::now();

    fclose(output_file);
    fclose(input_file);

    if (!silent)
        printf("time = %g\n", (t1 - t0).seconds());

    return 1;
}

int main(int argc, char* argv[]) {
    oneapi::tbb::tick_count mainStartTime = oneapi::tbb::tick_count::now();

    // The 1st argument is the function to obtain 'auto' value; the 2nd is the default value
    // The example interprets 0 threads as "run serially, then fully subscribed"
    utility::thread_number_range threads(utility::get_default_num_threads, 0);

    utility::parse_cli_arguments(
        argc,
        argv,
        utility::cli_argument_pack()
            //"-h" option for displaying help is present implicitly
            .positional_arg(threads, "n-of-threads", utility::thread_number_range_desc)
            .positional_arg(InputFileName, "input-file", "input file name")
            .positional_arg(OutputFileName, "output-file", "output file name")
            .positional_arg(MAX_CHAR_PER_INPUT_SLICE,
                            "max-slice-size",
                            "the maximum number of characters in one slice")
            .arg(silent, "silent", "no output except elapsed time"));
    generate_if_needed(InputFileName.c_str());

    if (threads.first) {
        for (int p = threads.first; p <= threads.last; p = threads.step(p)) {
            if (!silent)
                printf("threads = %d ", p);
            oneapi::tbb::global_control c(oneapi::tbb::global_control::max_allowed_parallelism, p);
            if (!run_pipeline(p))
                return -1;
        }
    }
    else { // Number of threads wasn't set explicitly. Run serial and parallel version
        { // serial run
            if (!silent)
                printf("serial run   ");
            oneapi::tbb::global_control c(oneapi::tbb::global_control::max_allowed_parallelism, 1);
            if (!run_pipeline(1))
                return -1;
        }
        { // parallel run (number of threads is selected automatically)
            if (!silent)
                printf("parallel run ");
            oneapi::tbb::global_control c(oneapi::tbb::global_control::max_allowed_parallelism,
                                          utility::get_default_num_threads());
            if (!run_pipeline(utility::get_default_num_threads()))
                return -1;
        }
    }

    utility::report_elapsed_time((oneapi::tbb::tick_count::now() - mainStartTime).seconds());

    return 0;
}
