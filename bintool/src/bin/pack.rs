//! 目前用于固定向 ../res/fat32.img 镜像中加入几个 elf 文件。

use std::fs::File;

use fatfs::{FileSystem, FsOptions, Write};

const PREFIX: &str = "../user/target/riscv64imac-unknown-none-elf/release";

fn main() {
    let fs = File::options()
        .read(true)
        .write(true)
        .open("../res/fat32.img")
        .unwrap();
    let fs = FileSystem::new(fs, FsOptions::new()).unwrap();
    let root_dir = fs.root_dir();

    let elf_to_pack = ["initproc", "shell", "echo"];

    for elf_name in elf_to_pack {
        let initproc = std::fs::read(format!("{PREFIX}/{elf_name}.elf")).unwrap();
        let mut file = root_dir.create_file(elf_name).unwrap();
        file.truncate().unwrap();
        file.write_all(&initproc).unwrap();
    }
}
