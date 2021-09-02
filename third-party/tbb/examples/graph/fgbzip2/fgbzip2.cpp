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

#include <iostream>
#include <fstream>
#include <string>
#include <memory>
#include <queue>
#include <thread>

#include "bzlib.hpp"

#include "common/utility/utility.hpp"

#include "oneapi/tbb/flow_graph.h"
#include "oneapi/tbb/tick_count.h"
#include "oneapi/tbb/concurrent_queue.h"

// TODO: change memory allocation/deallocation to be managed in constructor/destructor
struct Buffer {
    std::size_t len;
    char* b;
};

struct BufferMsg {
    BufferMsg() {}
    BufferMsg(Buffer& inputBuffer, Buffer& outputBuffer, std::size_t seqId, bool isLast = false)
            : inputBuffer(inputBuffer),
              outputBuffer(outputBuffer),
              seqId(seqId),
              isLast(isLast) {}

    static BufferMsg createBufferMsg(std::size_t seqId, std::size_t chunkSize) {
        Buffer inputBuffer;
        inputBuffer.b = new char[chunkSize];
        inputBuffer.len = chunkSize;

        Buffer outputBuffer;
        std::size_t compressedChunkSize = chunkSize * 1.01 + 600; // compression overhead
        outputBuffer.b = new char[compressedChunkSize];
        outputBuffer.len = compressedChunkSize;

        return BufferMsg(inputBuffer, outputBuffer, seqId);
    }

    static void destroyBufferMsg(const BufferMsg& destroyMsg) {
        delete[] destroyMsg.inputBuffer.b;
        delete[] destroyMsg.outputBuffer.b;
    }

    void markLast(std::size_t lastId) {
        isLast = true;
        seqId = lastId;
    }

    std::size_t seqId;
    Buffer inputBuffer;
    Buffer outputBuffer;
    bool isLast;
};

class BufferCompressor {
public:
    BufferCompressor(int blockSizeIn100KB) : m_blockSize(blockSizeIn100KB) {}

    BufferMsg operator()(BufferMsg buffer) const {
        if (!buffer.isLast) {
            unsigned int outSize = buffer.outputBuffer.len;
            BZ2_bzBuffToBuffCompress(buffer.outputBuffer.b,
                                     &outSize,
                                     buffer.inputBuffer.b,
                                     buffer.inputBuffer.len,
                                     m_blockSize,
                                     0,
                                     30);
            buffer.outputBuffer.len = outSize;
        }
        return buffer;
    }

private:
    int m_blockSize;
};

class IOOperations {
public:
    IOOperations(std::ifstream& inputStream, std::ofstream& outputStream, std::size_t chunkSize)
            : m_inputStream(inputStream),
              m_outputStream(outputStream),
              m_chunkSize(chunkSize),
              m_chunksRead(0) {}

    void readChunk(Buffer& buffer) {
        m_inputStream.read(buffer.b, m_chunkSize);
        buffer.len = static_cast<std::size_t>(m_inputStream.gcount());
        m_chunksRead++;
    }

    void writeChunk(const Buffer& buffer) {
        m_outputStream.write(buffer.b, buffer.len);
    }

    std::size_t chunksRead() const {
        return m_chunksRead;
    }

    std::size_t chunkSize() const {
        return m_chunkSize;
    }

    bool hasDataToRead() const {
        return m_inputStream.is_open() && !m_inputStream.eof();
    }

private:
    std::ifstream& m_inputStream;
    std::ofstream& m_outputStream;

    std::size_t m_chunkSize;
    std::size_t m_chunksRead;
};

//-----------------------------------------------------------------------------------------------------------------------
//---------------------------------------Compression example based on async_node-----------------------------------------
//-----------------------------------------------------------------------------------------------------------------------

typedef oneapi::tbb::flow::async_node<oneapi::tbb::flow::continue_msg, BufferMsg>
    async_file_reader_node;
typedef oneapi::tbb::flow::async_node<BufferMsg, oneapi::tbb::flow::continue_msg>
    async_file_writer_node;

class AsyncNodeActivity {
public:
    AsyncNodeActivity(IOOperations& io)
            : m_io(io),
              m_fileWriterThread(&AsyncNodeActivity::writingLoop, this) {}

    ~AsyncNodeActivity() {
        m_fileReaderThread.join();
        m_fileWriterThread.join();
    }

    void submitRead(async_file_reader_node::gateway_type& gateway) {
        gateway.reserve_wait();
        std::thread(&AsyncNodeActivity::readingLoop, this, std::ref(gateway))
            .swap(m_fileReaderThread);
    }

    void submitWrite(const BufferMsg& bufferMsg) {
        m_writeQueue.push(bufferMsg);
    }

private:
    void readingLoop(async_file_reader_node::gateway_type& gateway) {
        while (m_io.hasDataToRead()) {
            BufferMsg bufferMsg = BufferMsg::createBufferMsg(m_io.chunksRead(), m_io.chunkSize());
            m_io.readChunk(bufferMsg.inputBuffer);
            gateway.try_put(bufferMsg);
        }
        sendLastMessage(gateway);
        gateway.release_wait();
    }

    void writingLoop() {
        BufferMsg buffer;
        m_writeQueue.pop(buffer);
        while (!buffer.isLast) {
            m_io.writeChunk(buffer.outputBuffer);
            m_writeQueue.pop(buffer);
        }
    }

    void sendLastMessage(async_file_reader_node::gateway_type& gateway) {
        BufferMsg lastMsg;
        lastMsg.markLast(m_io.chunksRead());
        gateway.try_put(lastMsg);
    }

    IOOperations& m_io;

    oneapi::tbb::concurrent_bounded_queue<BufferMsg> m_writeQueue;

    std::thread m_fileReaderThread;
    std::thread m_fileWriterThread;
};

void fgCompressionAsyncNode(IOOperations& io, int blockSizeIn100KB) {
    oneapi::tbb::flow::graph g;

    AsyncNodeActivity asyncNodeActivity(io);

    async_file_reader_node file_reader(
        g,
        oneapi::tbb::flow::unlimited,
        [&asyncNodeActivity](const oneapi::tbb::flow::continue_msg& msg,
                             async_file_reader_node::gateway_type& gateway) {
            asyncNodeActivity.submitRead(gateway);
        });

    oneapi::tbb::flow::function_node<BufferMsg, BufferMsg> compressor(
        g, oneapi::tbb::flow::unlimited, BufferCompressor(blockSizeIn100KB));

    oneapi::tbb::flow::sequencer_node<BufferMsg> ordering(g,
                                                          [](const BufferMsg& bufferMsg) -> size_t {
                                                              return bufferMsg.seqId;
                                                          });

    // The node is serial to preserve the right order of buffers set by the preceding sequencer_node
    async_file_writer_node output_writer(
        g,
        oneapi::tbb::flow::serial,
        [&asyncNodeActivity](const BufferMsg& bufferMsg,
                             async_file_writer_node::gateway_type& gateway) {
            asyncNodeActivity.submitWrite(bufferMsg);
        });

    make_edge(file_reader, compressor);
    make_edge(compressor, ordering);
    make_edge(ordering, output_writer);

    file_reader.try_put(oneapi::tbb::flow::continue_msg());

    g.wait_for_all();
}

//-----------------------------------------------------------------------------------------------------------------------
//---------------------------------------------Simple compression example------------------------------------------------
//-----------------------------------------------------------------------------------------------------------------------

void fgCompression(IOOperations& io, int blockSizeIn100KB) {
    oneapi::tbb::flow::graph g;

    oneapi::tbb::flow::input_node<BufferMsg> file_reader(
        g, [&io](oneapi::tbb::flow_control& fc) -> BufferMsg {
            if (io.hasDataToRead()) {
                BufferMsg bufferMsg = BufferMsg::createBufferMsg(io.chunksRead(), io.chunkSize());
                io.readChunk(bufferMsg.inputBuffer);
                return bufferMsg;
            }
            fc.stop();
            return BufferMsg{};
        });
    file_reader.activate();

    oneapi::tbb::flow::function_node<BufferMsg, BufferMsg> compressor(
        g, oneapi::tbb::flow::unlimited, BufferCompressor(blockSizeIn100KB));

    oneapi::tbb::flow::sequencer_node<BufferMsg> ordering(g, [](const BufferMsg& buffer) -> size_t {
        return buffer.seqId;
    });

    oneapi::tbb::flow::function_node<BufferMsg> output_writer(
        g, oneapi::tbb::flow::serial, [&io](const BufferMsg& bufferMsg) {
            io.writeChunk(bufferMsg.outputBuffer);
            BufferMsg::destroyBufferMsg(bufferMsg);
        });

    make_edge(file_reader, compressor);
    make_edge(compressor, ordering);
    make_edge(ordering, output_writer);

    g.wait_for_all();
}

//-----------------------------------------------------------------------------------------------------------------------

bool endsWith(const std::string& str, const std::string& suffix) {
    return str.find(suffix, str.length() - suffix.length()) != std::string::npos;
}

//-----------------------------------------------------------------------------------------------------------------------

int main(int argc, char* argv[]) {
    oneapi::tbb::tick_count mainStartTime = oneapi::tbb::tick_count::now();

    const std::string archiveExtension = ".bz2";
    bool verbose = false;
    bool asyncType;
    std::string inputFileName;
    int blockSizeIn100KB = 1; // block size in 100KB chunks
    std::size_t memoryLimitIn1MB = 1; // memory limit for compression in megabytes granularity

    utility::parse_cli_arguments(
        argc,
        argv,
        utility::cli_argument_pack()
            //"-h" option for displaying help is present implicitly
            .arg(blockSizeIn100KB, "-b", "\t block size in 100KB chunks, [1 .. 9]")
            .arg(verbose, "-v", "verbose mode")
            .arg(memoryLimitIn1MB,
                 "-l",
                 "used memory limit for compression algorithm in 1MB (minimum) granularity")
            .arg(asyncType, "-async", "use graph async_node-based implementation")
            .positional_arg(inputFileName, "filename", "input file name"));

    if (inputFileName.empty()) {
        throw std::invalid_argument(
            "Input file name is not specified. Try 'fgbzip2 -h' for more information.");
    }

    if (blockSizeIn100KB < 1 || blockSizeIn100KB > 9) {
        throw std::invalid_argument("Incorrect block size. Try 'fgbzip2 -h' for more information.");
    }

    if (memoryLimitIn1MB < 1) {
        throw std::invalid_argument(
            "Incorrect memory limit size. Try 'fgbzip2 -h' for more information.");
    }

    if (verbose)
        std::cout << "Input file name: " << inputFileName << "\n";
    if (endsWith(inputFileName, archiveExtension)) {
        throw std::invalid_argument("Input file already have " + archiveExtension + " extension.");
    }

    std::ifstream inputStream(inputFileName.c_str(), std::ios::in | std::ios::binary);
    if (!inputStream.is_open()) {
        throw std::invalid_argument("Cannot open " + inputFileName + " file.");
    }

    std::string outputFileName(inputFileName + archiveExtension);

    std::ofstream outputStream(outputFileName.c_str(),
                               std::ios::out | std::ios::binary | std::ios::trunc);
    if (!outputStream.is_open()) {
        throw std::invalid_argument("Cannot open " + outputFileName + " file.");
    }

    // General interface to work with I/O buffers operations
    std::size_t chunkSize = blockSizeIn100KB * 100 * 1024;
    IOOperations io(inputStream, outputStream, chunkSize);

    if (asyncType) {
        if (verbose)
            std::cout
                << "Running flow graph based compression algorithm with async_node based asynchronous IO operations."
                << "\n";
        fgCompressionAsyncNode(io, blockSizeIn100KB);
    }
    else {
        if (verbose)
            std::cout << "Running flow graph based compression algorithm."
                      << "\n";
        fgCompression(io, blockSizeIn100KB);
    }

    inputStream.close();
    outputStream.close();

    utility::report_elapsed_time((oneapi::tbb::tick_count::now() - mainStartTime).seconds());

    return 0;
}
