//! 用于初始化一个 fat32 文件系统镜像
//! 目前固定用 ../res/test_bin 里的东西
//! 并将其生成到 ../res/test_suits.img

use std::fs::File;

use fatfs::{FileSystem, FormatVolumeOptions, FsOptions, StdIoWrapper, Write};

fn main() {
    {
        let img_file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .open("../res/test_suits.img")
            .unwrap();
        img_file.set_len(16 * 2048 * 512).unwrap();
        fatfs::format_volume(&mut StdIoWrapper::new(img_file), FormatVolumeOptions::new()).unwrap();
    }

    let fs = File::options()
        .read(true)
        .write(true)
        .open("../res/test_suits.img")
        .unwrap();
    let fs = FileSystem::new(fs, FsOptions::new()).unwrap();
    let root_dir = fs.root_dir();

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
