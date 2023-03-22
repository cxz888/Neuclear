//! 用于向操作系统镜像里面加入一些应用

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

    let initproc = std::fs::read(format!("{PREFIX}/initproc")).unwrap();
    let mut file = root_dir.create_file("initproc").unwrap();
    file.truncate().unwrap();
    file.write_all(&initproc).unwrap();

    let shell = std::fs::read(format!("{PREFIX}/shell")).unwrap();
    file = root_dir.create_file("shell").unwrap();
    file.truncate().unwrap();
    file.write_all(&shell).unwrap();

    let shell = std::fs::read(format!("{PREFIX}/echo")).unwrap();
    file = root_dir.create_file("echo").unwrap();
    file.truncate().unwrap();
    file.write_all(&shell).unwrap();
}
