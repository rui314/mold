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

#include <cmath>
#include <cstdio>

#include <vector>
#include <atomic>

#include "oneapi/tbb/tick_count.h"
#include "oneapi/tbb/task_group.h"
#include "oneapi/tbb/concurrent_priority_queue.h"
#include "oneapi/tbb/spin_mutex.h"
#include "oneapi/tbb/parallel_for.h"
#include "oneapi/tbb/blocked_range.h"
#include "oneapi/tbb/global_control.h"

#include "common/utility/utility.hpp"
#include "common/utility/fast_random.hpp"
#include "common/utility/get_default_num_threads.hpp"

#if defined(_MSC_VER) && defined(_Wp64)
// Workaround for overzealous compiler warnings in /Wp64 mode
#pragma warning(disable : 4267)
#endif /* _MSC_VER && _Wp64 */

struct point {
    double x, y;
    point() {}
    point(double _x, double _y) : x(_x), y(_y) {}
    point(const point& p) : x(p.x), y(p.y) {}
};

double get_distance(const point& p1, const point& p2) {
    double xdiff = p1.x - p2.x, ydiff = p1.y - p2.y;
    return sqrt(xdiff * xdiff + ydiff * ydiff);
}

// generates random points on 2D plane within a box of maxsize width & height
point generate_random_point(utility::FastRandom& mr) {
    const std::size_t maxsize = 500;
    double x = (double)(mr.get() % maxsize);
    double y = (double)(mr.get() % maxsize);
    return point(x, y);
}

// weighted toss makes closer nodes (in the point vector) heavily connected
bool die_toss(std::size_t a, std::size_t b, utility::FastRandom& mr) {
    int node_diff = std::abs(int(a - b));
    // near nodes
    if (node_diff < 16)
        return true;
    // mid nodes
    if (node_diff < 64)
        return ((int)mr.get() % 8 == 0);
    // far nodes
    if (node_diff < 512)
        return ((int)mr.get() % 16 == 0);
    return false;
}

typedef std::vector<point> point_set;
typedef std::size_t vertex_id;
typedef std::pair<vertex_id, double> vertex_rec;
typedef std::vector<std::vector<vertex_id>> edge_set;

bool verbose = false; // prints bin details and other diagnostics to screen
bool silent = false; // suppress all output except for time
std::size_t N = 1000; // number of vertices
std::size_t src = 0; // start of path
std::size_t dst = N - 1; // end of path
double INF = 100000.0; // infinity
std::size_t grainsize = 16; // number of vertices per task on average
std::size_t max_spawn; // max tasks to spawn
std::atomic<std::size_t> num_spawn; // number of active tasks

point_set vertices; // vertices
edge_set edges; // edges
std::vector<vertex_id> predecessor; // for recreating path from src to dst

std::vector<double> f_distance; // estimated distances at particular vertex
std::vector<double> g_distance; // current shortest distances from src vertex
oneapi::tbb::spin_mutex* locks; // a lock for each vertex
oneapi::tbb::task_group* sp_group; // task group for tasks executing sub-problems

struct compare_f {
    bool operator()(const vertex_rec& u, const vertex_rec& v) const {
        return u.second > v.second;
    }
};

oneapi::tbb::concurrent_priority_queue<vertex_rec, compare_f> open_set; // tentative vertices

void shortpath_helper();

void shortpath() {
    sp_group = new oneapi::tbb::task_group;
    g_distance[src] = 0.0; // src's distance from src is zero
    f_distance[src] =
        get_distance(vertices[src], vertices[dst]); // estimate distance from src to dst
    open_set.emplace(src, f_distance[src]); // emplace src into open_set
    sp_group->run([]() {
        shortpath_helper();
    });
    sp_group->wait();
    delete sp_group;
}

void shortpath_helper() {
    vertex_rec u_rec;
    while (open_set.try_pop(u_rec)) {
        vertex_id u = u_rec.first;
        if (u == dst)
            continue;
        double f = u_rec.second;
        double old_g_u = 0.0;
        {
            oneapi::tbb::spin_mutex::scoped_lock l(locks[u]);
            if (f > f_distance[u])
                continue; // prune search space
            old_g_u = g_distance[u];
        }
        for (std::size_t i = 0; i < edges[u].size(); ++i) {
            vertex_id v = edges[u][i];
            double new_g_v = old_g_u + get_distance(vertices[u], vertices[v]);
            double new_f_v = 0.0;
            // the push flag lets us move some work out of the critical section below
            bool push = false;
            {
                oneapi::tbb::spin_mutex::scoped_lock l(locks[v]);
                if (new_g_v < g_distance[v]) {
                    predecessor[v] = u;
                    g_distance[v] = new_g_v;
                    new_f_v = f_distance[v] =
                        g_distance[v] + get_distance(vertices[v], vertices[dst]);
                    push = true;
                }
            }
            if (push) {
                open_set.push(std::make_pair(v, new_f_v));
                std::size_t n_spawn = ++num_spawn;
                if (n_spawn < max_spawn) {
                    sp_group->run([] {
                        shortpath_helper();
                    });
                }
                else
                    --num_spawn;
            }
        }
    }
    --num_spawn;
}

void make_path(vertex_id src, vertex_id dst, std::vector<vertex_id>& path) {
    vertex_id at = predecessor[dst];
    if (at == N)
        path.push_back(src);
    else if (at == src) {
        path.push_back(src);
        path.push_back(dst);
    }
    else {
        make_path(src, at, path);
        path.push_back(dst);
    }
}

void print_path() {
    std::vector<vertex_id> path;
    double path_length = 0.0;
    make_path(src, dst, path);
    if (verbose)
        printf("\n      ");
    for (std::size_t i = 0; i < path.size(); ++i) {
        if (path[i] != dst) {
            double seg_length = get_distance(vertices[path[i]], vertices[path[i + 1]]);
            if (verbose)
                printf("%6.1f       ", seg_length);
            path_length += seg_length;
        }
        else if (verbose)
            printf("\n");
    }
    if (verbose) {
        for (std::size_t i = 0; i < path.size(); ++i) {
            if (path[i] != dst)
                printf("(%4d)------>", (int)path[i]);
            else
                printf("(%4d)\n", (int)path[i]);
        }
    }
    if (verbose)
        printf("Total distance = %5.1f\n", path_length);
    else if (!silent)
        printf(" %5.1f\n", path_length);
}

void InitializeGraph() {
    oneapi::tbb::global_control c(oneapi::tbb::global_control::max_allowed_parallelism,
                                  utility::get_default_num_threads());
    vertices.resize(N);
    edges.resize(N);
    predecessor.resize(N);
    g_distance.resize(N);
    f_distance.resize(N);
    locks = new oneapi::tbb::spin_mutex[N];

    if (verbose)
        printf("Generating vertices...\n");
    oneapi::tbb::parallel_for(
        oneapi::tbb::blocked_range<std::size_t>(0, N, 64),
        [&](oneapi::tbb::blocked_range<std::size_t>& r) {
            utility::FastRandom my_random(r.begin());
            for (std::size_t i = r.begin(); i != r.end(); ++i) {
                vertices[i] = generate_random_point(my_random);
            }
        },
        oneapi::tbb::simple_partitioner());

    if (verbose)
        printf("Generating edges...\n");
    oneapi::tbb::parallel_for(
        oneapi::tbb::blocked_range<std::size_t>(0, N, 64),
        [&](oneapi::tbb::blocked_range<std::size_t>& r) {
            utility::FastRandom my_random(r.begin());
            for (std::size_t i = r.begin(); i != r.end(); ++i) {
                for (std::size_t j = 0; j < i; ++j) {
                    if (die_toss(i, j, my_random))
                        edges[i].push_back(j);
                }
            }
        },
        oneapi::tbb::simple_partitioner());

    for (std::size_t i = 0; i < N; ++i) {
        for (std::size_t j = 0; j < edges[i].size(); ++j) {
            vertex_id k = edges[i][j];
            edges[k].push_back(i);
        }
    }
    if (verbose)
        printf("Done.\n");
}

void ReleaseGraph() {
    delete[] locks;
}

void ResetGraph() {
    oneapi::tbb::global_control c(oneapi::tbb::global_control::max_allowed_parallelism,
                                  utility::get_default_num_threads());
    oneapi::tbb::parallel_for(oneapi::tbb::blocked_range<std::size_t>(0, N),
                              [&](oneapi::tbb::blocked_range<std::size_t>& r) {
                                  for (std::size_t i = r.begin(); i != r.end(); ++i) {
                                      f_distance[i] = g_distance[i] = INF;
                                      predecessor[i] = N;
                                  }
                              });
}

int main(int argc, char* argv[]) {
    utility::thread_number_range threads(utility::get_default_num_threads);
    utility::parse_cli_arguments(
        argc,
        argv,
        utility::cli_argument_pack()
            //"-h" option for displaying help is present implicitly
            .positional_arg(threads, "#threads", utility::thread_number_range_desc)
            .arg(verbose, "verbose", "   print diagnostic output to screen")
            .arg(silent, "silent", "    limits output to timing info; overrides verbose")
            .arg(N, "N", "         number of vertices")
            .arg(src, "start", "      start of path")
            .arg(dst, "end", "        end of path"));
    if (silent)
        verbose = false; // make silent override verbose
    else
        printf("shortpath will run with %d vertices to find shortest path between vertices"
               " %d and %d using %d:%d threads.\n",
               (int)N,
               (int)src,
               (int)dst,
               (int)threads.first,
               (int)threads.last);

    if (dst >= N) {
        if (verbose)
            printf("end value %d is invalid for %d vertices; correcting to %d\n",
                   (int)dst,
                   (int)N,
                   (int)N - 1);
        dst = N - 1;
    }

    num_spawn = 0;
    max_spawn = N / grainsize;
    oneapi::tbb::tick_count t0, t1;
    InitializeGraph();
    for (int n_thr = threads.first; n_thr <= threads.last; n_thr = threads.step(n_thr)) {
        ResetGraph();
        oneapi::tbb::global_control c(oneapi::tbb::global_control::max_allowed_parallelism, n_thr);
        t0 = oneapi::tbb::tick_count::now();
        shortpath();
        t1 = oneapi::tbb::tick_count::now();
        if (!silent) {
            if (predecessor[dst] != N) {
                printf("%d threads: [%6.6f] The shortest path from vertex %d to vertex %d is:",
                       (int)n_thr,
                       (t1 - t0).seconds(),
                       (int)src,
                       (int)dst);
                print_path();
            }
            else {
                printf("%d threads: [%6.6f] There is no path from vertex %d to vertex %d\n",
                       (int)n_thr,
                       (t1 - t0).seconds(),
                       (int)src,
                       (int)dst);
            }
        }
        else
            utility::report_elapsed_time((t1 - t0).seconds());
    }
    ReleaseGraph();
    return 0;
}
