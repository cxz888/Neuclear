//! fat-fs 探针。单纯是用于加载和查看 fat 文件系统中的东西

use std::{env::args, fs::File};

use fatfs::{FileSystem, FsOptions};

fn main() {
    let img_path = args().nth(1).unwrap();
    let fs = File::options()
        .read(true)
        .write(true)
        .open(img_path)
        .unwrap();
    let fs = FileSystem::new(fs, FsOptions::new()).unwrap();
    let root_dir = fs.root_dir();
    for entry in root_dir.iter() {
        let entry = entry.unwrap();
        println!("{}", entry.file_name());
    }
}
