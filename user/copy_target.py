import sys
import os
from shutil import copyfile

elfs = sys.argv[1].split()

for elf in elfs:
    bin = elf + ".bin"
    cmd = (
        f"rust-objcopy --binary-architecture=riscv64 {elf} --strip-all -O binary {bin}"
    )
    os.system(cmd)
    copyfile(elf, elf + ".elf")
