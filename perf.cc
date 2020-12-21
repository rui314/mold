#include "mold.h"

#include <iomanip>
#include <ios>
#include <sys/resource.h>
#include <sys/time.h>

std::vector<Counter *> Counter::instances;
bool Counter::enabled = false;

std::vector<Timer *> Timer::instances;

void Counter::print() {
  if (!enabled)
    return;

  std::vector<Counter *> vec = instances;
  sort(vec, [](Counter *a, Counter *b) { return a->value > b->value; });

  for (Counter *c : vec)
    std::cout << std::setw(20) << std::right << c->name << "=" << c->value << "\n";
}

static u64 now_nsec() {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return (u64)t.tv_sec * 1000000000 + t.tv_nsec;
}

static u64 to_nsec(struct timeval t) {
  return t.tv_sec * 1000000000 + t.tv_usec * 1000;
}

Timer::Timer(std::string name) : name(name) {
  instances.push_back(this);

  struct rusage usage;
  getrusage(RUSAGE_SELF, &usage);

  start = now_nsec();
  user = to_nsec(usage.ru_utime);
  sys = to_nsec(usage.ru_stime);
}

void Timer::stop() {
  if (stopped)
    return;
  stopped = true;

  struct rusage usage;
  getrusage(RUSAGE_SELF, &usage);

  end = now_nsec();
  user = to_nsec(usage.ru_utime) - user;
  sys = to_nsec(usage.ru_stime) - sys;
}

void Timer::print() {
  for (int i = instances.size() - 1; i >= 0; i--)
    instances[i]->stop();

  std::vector<int> depth(instances.size());

  for (int i = 0; i < instances.size(); i++)
    for (int j = 0; j < i; j++)
      if (instances[i]->end < instances[j]->end)
        depth[i]++;

  std::cout << "     User   System     Real  Name\n";

  for (int i = 0; i < instances.size(); i++) {
    Timer &t = *instances[i];
    printf(" % 8.3f % 8.3f % 8.3f  %s%s\n",
           ((double)t.user / 1000000000),
           ((double)t.sys / 1000000000),
           (((double)t.end - t.start) / 1000000000),
           std::string(depth[i] * 2, ' ').c_str(),
           t.name.c_str());
  }

  std::cout << std::flush;
}
