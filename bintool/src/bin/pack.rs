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
    let initproc = std::fs::read(format!("{PREFIX}/initproc.elf")).unwrap();
    let mut file = root_dir.create_file("initproc").unwrap();
    file.truncate().unwrap();
    file.write_all(&initproc).unwrap();

    let shell = std::fs::read(format!("{PREFIX}/shell.elf")).unwrap();
    file = root_dir.create_file("shell").unwrap();
    file.truncate().unwrap();
    file.write_all(&shell).unwrap();

    let shell = std::fs::read(format!("{PREFIX}/echo.elf")).unwrap();
    file = root_dir.create_file("echo").unwrap();
    file.truncate().unwrap();
    file.write_all(&shell).unwrap();

    let shell = std::fs::read(format!("{PREFIX}/exec_test.elf")).unwrap();
    file = root_dir.create_file("exec_test").unwrap();
    file.truncate().unwrap();
    file.write_all(&shell).unwrap();

    let dir = std::fs::read_dir("../res/test_bin").unwrap();
    for entry in dir {
        let entry = entry.unwrap();
        assert!(entry.file_type().unwrap().is_file());
        let app = std::fs::read(entry.path()).unwrap();
        let mut file = root_dir
            .create_file(entry.file_name().to_str().unwrap())
            .unwrap();
        file.truncate().unwrap();
        file.write_all(&app).unwrap();
    }
}
