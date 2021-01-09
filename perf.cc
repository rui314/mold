#include "mold.h"

#include <iomanip>
#include <ios>
#include <sys/resource.h>
#include <sys/time.h>

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
  return (u64)t.tv_sec * 1000000000 + t.tv_usec * 1000;
}

TimerRecord::TimerRecord(std::string name) : name(name) {
  struct rusage usage;
  getrusage(RUSAGE_SELF, &usage);

  start = now_nsec();
  user = to_nsec(usage.ru_utime);
  sys = to_nsec(usage.ru_stime);
}

void TimerRecord::stop() {
  if (stopped)
    return;
  stopped = true;

  struct rusage usage;
  getrusage(RUSAGE_SELF, &usage);

  end = now_nsec();
  user = to_nsec(usage.ru_utime) - user;
  sys = to_nsec(usage.ru_stime) - sys;
}

Timer::Timer(std::string name) {
  record = new TimerRecord(name);
  records.push_back(record);
}

Timer::~Timer() {
  record->stop();
}

void Timer::stop() {
  record->stop();
}

void Timer::print() {
  for (int i = records.size() - 1; i >= 0; i--)
    records[i]->stop();

  std::vector<int> depth(records.size());

  for (int i = 0; i < records.size(); i++)
    for (int j = 0; j < i; j++)
      if (records[i]->end < records[j]->end)
        depth[i]++;

  std::cout << "     User   System     Real  Name\n";

  for (int i = 0; i < records.size(); i++) {
    TimerRecord &rec = *records[i];
    printf(" % 8.3f % 8.3f % 8.3f  %s%s\n",
           ((double)rec.user / 1000000000),
           ((double)rec.sys / 1000000000),
           (((double)rec.end - rec.start) / 1000000000),
           std::string(depth[i] * 2, ' ').c_str(),
           rec.name.c_str());
  }

  std::cout << std::flush;
}
