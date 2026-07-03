/*
    Copyright (c) 2025 UXL Foundation Contributors

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

#ifndef _TBB_cgroup_info_H
#define _TBB_cgroup_info_H

#include <cstdint>
#include <cstdio>
#include <climits>
#include <cstring>
#include <memory>
#include <cerrno>
#include <cstdlib>

#include <mntent.h>

#include "oneapi/tbb/detail/_assert.h"

namespace tbb {
namespace detail {
namespace r1 {

struct default_cgroup_settings {
    const char* proc_self_mounts_path  = "/proc/self/mounts";
    const char* proc_self_cgroup_path  = "/proc/self/cgroup";
    const char* sys_fs_cgroup_dir_path = "/sys/fs/cgroup";
};

// Linux control groups support
template <typename cgroup_settings = default_cgroup_settings>
class cgroup_info {
public:
    static bool is_cpu_constrained(int& constrained_num_cpus) {
        static const int num_cpus = parse_cpu_constraints(cgroup_settings{});
        if (num_cpus == error_value || num_cpus == unlimited_num_cpus)
            return false;

        constrained_num_cpus = num_cpus;
        return true;
    }

private:
    static void close_file(std::FILE *file) { std::fclose(file); };
    using unique_file_t = std::unique_ptr<std::FILE, decltype(&close_file)>;

    static constexpr int unlimited_num_cpus = INT_MAX;
    static constexpr int error_value = 0; // Some impossible value for the number of CPUs

    static int determine_num_cpus(long long cpu_quota, long long cpu_period) {
        if (0 == cpu_period)
            return error_value; // Avoid division by zero, use the default number of CPUs

        const long long num_cpus = (cpu_quota + cpu_period - 1) / cpu_period;
        return num_cpus > 0 ? int(num_cpus) : 1; // Ensure at least one CPU is returned
    }

    static constexpr std::size_t rel_path_size = 256; // Size of the relative path buffer
    struct process_cgroup_data {
        enum class cgroup_version : std::uint8_t {
            unknown = 0,
            v1      = 1,
            v2      = 2
        };
        cgroup_version version { cgroup_version::unknown };
        char relative_path[rel_path_size] = {0};
    };

    static const char* look_for_cpu_controller_path(const char* line, const char* last_char) {
        const char* path_start = line;
        constexpr int cpu_ctrl_str_length = 3;
        while ((path_start = std::strstr(path_start, "cpu"))) {
            // At least ":/" must be at the end of line for a valid cgroups file
            if (line - path_start == 0 || last_char - path_start <= cpu_ctrl_str_length) {
                path_start = nullptr;
                break; // Incorrect line in the cgroup file, skip it
            }

            const char prev_char = *(path_start - 1);
            if (prev_char != ':' && prev_char != ',') {
                ++path_start; // Not a valid "cpu" controller, continue searching
                continue;
            }

            const char next_char = *(path_start + cpu_ctrl_str_length);
            if (next_char != ':' && next_char != ',') {
                ++path_start; // Not a valid "cpu" controller, continue searching
                continue;
            }

            path_start = std::strchr(path_start + cpu_ctrl_str_length, ':') + 1;
            __TBB_ASSERT(path_start <= last_char, "Too long path?");
            break;
        }
        return path_start;
    }

    static void parse_proc_cgroup_file(std::FILE* cgroup_fd, process_cgroup_data& pcd,
                                       bool look_for_v2_only = false)
    {
        using cgroup_version_t = typename process_cgroup_data::cgroup_version;
        cgroup_version_t cgroup_version { cgroup_version_t::unknown };

        char line[rel_path_size] = {0};
        const char* last_char = line + rel_path_size - 1;

        const char* path_start = nullptr;
        constexpr std::size_t cgroup_v2_prefix_size = 3;
        while (std::fgets(line, rel_path_size, cgroup_fd)) {
            // Both cgroup v1 and v2 mounts may be present. However, a specific controller can only
            // be active in one of them. If the cgroup v1 CPU controller is found first, the search
            // can stop immediately, as the CPU controller will not be active in cgroup v2. But if
            // the cgroup v2 controller is found first, the search must continue, as the cgroup v1
            // CPU controller might appear later in the file.
            if (!path_start && std::strncmp(line, "0::", cgroup_v2_prefix_size) == 0) {
                path_start = line + cgroup_v2_prefix_size; // cgroup v2 unified path
                cgroup_version = cgroup_version_t::v2;
                if (look_for_v2_only) break;
            } else if (!look_for_v2_only) {
                // cgroups v1 allows comount multiple controllers against the same hierarchy
                auto v1_path_start = look_for_cpu_controller_path(line, last_char);
                if (v1_path_start) {
                    cgroup_version = cgroup_version_t::v1;
                    path_start = v1_path_start;
                    break;
                }
            }
        }
        if (path_start) {
            // Ensure no new line at the end of the path is copied
            std::size_t real_rel_path_size = std::strcspn(path_start, "\n");
            __TBB_ASSERT(real_rel_path_size < rel_path_size, nullptr);
            std::strncpy(pcd.relative_path, path_start, real_rel_path_size);
            pcd.relative_path[real_rel_path_size] = '\0';
            pcd.version = cgroup_version;
        }
    }

    static bool try_read_cgroup_v1_num_cpus_from(const char* dir, int& num_cpus) {
        char path[PATH_MAX] = {0};
        if (std::snprintf(path, PATH_MAX, "%s/cpu.cfs_quota_us", dir) < 0)
            return false;       // Failed to create path

        unique_file_t fd(std::fopen(path, "r"), &close_file);
        if (!fd)
            return false;

        long long cpu_quota = 0;
        if (std::fscanf(fd.get(), "%lld", &cpu_quota) != 1)
            return false;

        if (-1 == cpu_quota) {
            num_cpus = unlimited_num_cpus; // -1 quota means maximum available CPUs
            return true;
        }

        if (std::snprintf(path, PATH_MAX, "%s/cpu.cfs_period_us", dir) < 0)
            return false;       // Failed to create path;
        fd.reset(std::fopen(path, "r"));
        if (!fd)
            return false;

        long long cpu_period = 0;
        if (std::fscanf(fd.get(), "%lld", &cpu_period) != 1)
            return false;

        num_cpus = determine_num_cpus(cpu_quota, cpu_period);
        return num_cpus != error_value; // Return true if valid number of CPUs was determined
    }

    static bool try_read_cgroup_v2_num_cpus_from(const char* dir, int& num_cpus) {
        char path[PATH_MAX] = {0};
        if (std::snprintf(path, PATH_MAX, "%s/cpu.max", dir) < 0)
            return false;       // Failed to create path

        unique_file_t fd(std::fopen(path, "r"), &close_file);
        if (!fd)
            return false;

        long long cpu_period = 0;
        char cpu_quota_str[16] = {0};
        if (std::fscanf(fd.get(), "%15s %lld", cpu_quota_str, &cpu_period) != 2)
            return false;

        if (std::strncmp(cpu_quota_str, "max", 3) == 0) {
            num_cpus = unlimited_num_cpus;  // "max" means no CPU constraint
            return true;
        }

        errno = 0; // Reset errno before strtoll
        char* str_end = nullptr;
        long long cpu_quota = std::strtoll(cpu_quota_str, &str_end, /*base*/ 10);
        if (errno == ERANGE || str_end == cpu_quota_str)
            return false;

        num_cpus = determine_num_cpus(cpu_quota, cpu_period);
        return num_cpus != error_value; // Return true if valid number of CPUs was determined
    }

    static bool try_read_cgroup_num_cpus_from(const char* dir, int& num_cpus,
                                              typename process_cgroup_data::cgroup_version version)
    {
        // Try reading based on the provided cgroup version
        if (version == process_cgroup_data::cgroup_version::v2) {
            return try_read_cgroup_v2_num_cpus_from(dir, num_cpus);
        }
        __TBB_ASSERT(version == process_cgroup_data::cgroup_version::v1, nullptr);
        return try_read_cgroup_v1_num_cpus_from(dir, num_cpus);
    }

    static int parse_cgroup_entry(const char* mnt_dir, process_cgroup_data& pcd) {
        int num_cpus = error_value; // Initialize to an impossible value
        char dir[PATH_MAX] = {0};
        if (std::snprintf(dir, PATH_MAX, "%s/%s", mnt_dir, pcd.relative_path) >= 0) {
            if (try_read_cgroup_num_cpus_from(dir, num_cpus, pcd.version)) {
                return num_cpus;
            }
        }

        return try_read_cgroup_num_cpus_from(mnt_dir, num_cpus, pcd.version) ? num_cpus : error_value;
    }

    static bool is_cpu_restriction_possible(process_cgroup_data& pcd) {
        if (pcd.version == process_cgroup_data::cgroup_version::unknown) {
            return false;
        } else if (pcd.version == process_cgroup_data::cgroup_version::v1) {
            return true;
        } else if (pcd.relative_path[1]) {
            return true;
        }
        __TBB_ASSERT(pcd.version == process_cgroup_data::cgroup_version::v2
                     && *pcd.relative_path == '/', nullptr);

        // At this point, we have cgroup v2 with a root path, which may indicate that the process is
        // under the root cgroup. This implies that we shouldn't find any cpu.max file in the cgroup
        // mount path. However, to verify whether the process is not running within a cgroup
        // namespace, we need to inspect the cgroup information of the init process. On a host OS,
        // the process with PID 1 is the init system (e.g., systemd). In containerized environments,
        // though, PID 1 might correspond to a different process (e.g., a shell or application). In
        // such cases, the /proc/1/cgroup file shouldn't show the "0::/init.scope" entry associated
        // with systemd if non-root cgroup is used.
        unique_file_t init_process_cgroup_file(std::fopen("/proc/1/cgroup", "r"), &close_file);
        if (!init_process_cgroup_file)
            // We can't be sure whether it is root cgroup or not, so need to inspect cgroup mount
            return true;

        process_cgroup_data init_process_cgroup_data{};
        parse_proc_cgroup_file(init_process_cgroup_file.get(), init_process_cgroup_data,
                               /*look_for_v2_only*/true);
        if (init_process_cgroup_data.version != process_cgroup_data::cgroup_version::unknown) {
            __TBB_ASSERT(init_process_cgroup_data.version == process_cgroup_data::cgroup_version::v2
                         && *init_process_cgroup_data.relative_path, nullptr);

            // If the init process cgroup path is "/init.scope", it means systemd is used
            // and we are running on the host
            if (!std::strncmp(init_process_cgroup_data.relative_path, "/init.scope", 11)) {
                return false;
            }
        }

        return true;
    }

    static int try_common_cgroup_mount_path(const process_cgroup_data& pcd,
                                            const cgroup_settings& cg_cfg) {
        int num_cpus = error_value;
        char dir[PATH_MAX] = {0};
        __TBB_ASSERT(*pcd.relative_path, nullptr);
        if (0 <= std::snprintf(dir, PATH_MAX, "%s/%s", cg_cfg.sys_fs_cgroup_dir_path,
                               pcd.relative_path))
            try_read_cgroup_num_cpus_from(dir, num_cpus, pcd.version);

        if (error_value == num_cpus && pcd.version == process_cgroup_data::cgroup_version::v2) {
            if (0 <= std::snprintf(dir, PATH_MAX, "%s/%s", "/sys/fs/cgroup/unified",
                                   pcd.relative_path))
                try_read_cgroup_v2_num_cpus_from(dir, num_cpus);
        }
        return num_cpus;
    }

    static int parse_cpu_constraints(const cgroup_settings& cg_cfg) {
        // Reading /proc/self/cgroup anyway, so open it right away
        unique_file_t cgroup_file_ptr(std::fopen(cg_cfg.proc_self_cgroup_path, "r"), &close_file);
        if (!cgroup_file_ptr)
            return error_value; // Failed to open cgroup file

        process_cgroup_data pcd{};
        parse_proc_cgroup_file(cgroup_file_ptr.get(), pcd);

        if (!is_cpu_restriction_possible(pcd)) {
            return unlimited_num_cpus;
        }

        __TBB_ASSERT(pcd.version != process_cgroup_data::cgroup_version::unknown, nullptr);

        int found_num_cpus = error_value; // Initialize to an impossible value
        found_num_cpus = try_common_cgroup_mount_path(pcd, cg_cfg);
        if (found_num_cpus != error_value) {
            return found_num_cpus;
        }

        auto close_mounts_file = [](std::FILE* file) { endmntent(file); };
        using unique_mounts_file_t = std::unique_ptr<std::FILE, decltype(close_mounts_file)>;
        unique_mounts_file_t mounts_file_ptr(setmntent(cg_cfg.proc_self_mounts_path, "r"),
                                             close_mounts_file);
        if (!mounts_file_ptr)
            return error_value;

        std::size_t cgroup_mnt_strlen{};
        const char* cgroup_mnt_str;
        if (pcd.version == process_cgroup_data::cgroup_version::v2) {
            cgroup_mnt_str = "cgroup2";
            cgroup_mnt_strlen = 7;
        } else {
            cgroup_mnt_str = "cgroup";
            cgroup_mnt_strlen = 6;
        }

        struct mntent mntent;
        constexpr std::size_t buffer_size = 4096; // Allocate a buffer for reading mount entries
        char mount_entry_buffer[buffer_size];

        // Read the mounts file and cgroup file to determine the number of CPUs
        while (getmntent_r(mounts_file_ptr.get(), &mntent, mount_entry_buffer, buffer_size)) {
            if (std::strncmp(mntent.mnt_type, cgroup_mnt_str, cgroup_mnt_strlen) == 0) {
                found_num_cpus = parse_cgroup_entry(mntent.mnt_dir, pcd);
                if (found_num_cpus != error_value)
                    break;
            }
        }
        return found_num_cpus;
    }
};

} // namespace r
} // namespace detail
} // namespace tbb

#endif // _TBB_cgroup_info_H
