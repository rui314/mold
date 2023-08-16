#! /usr/bin/env python3

from pathlib import Path
import platform
import shutil
import subprocess
import sys

ROOT = Path(__file__).parent.parent.parent
RUST_TARGET = sys.argv[1]

subprocess.run(
    ["cargo", "build", "--target", sys.argv[1], "--release"], cwd=ROOT / "b3sum"
)

if platform.system() == "Windows":
    original_exe_name = "b3sum.exe"
else:
    original_exe_name = "b3sum"

if platform.system() == "Windows":
    new_exe_name = "b3sum_windows_x64_bin.exe"
elif platform.system() == "Darwin":
    new_exe_name = "b3sum_macos_x64_bin"
elif platform.system() == "Linux":
    new_exe_name = "b3sum_linux_x64_bin"
else:
    raise RuntimeError("Unexpected platform: " + platform.system())

# Copy the built binary so that it has the upload name we want.
out_dir = ROOT / "b3sum/target" / RUST_TARGET / "release"
original_exe_path = str(out_dir / original_exe_name)
new_exe_path = str(out_dir / new_exe_name)
print("copying", repr(original_exe_path), "to", repr(new_exe_path))
shutil.copyfile(original_exe_path, new_exe_path)

# This lets the subsequent upload step get the filepath.
print("::set-output name=bin_path::" + new_exe_path)
