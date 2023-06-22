# Building
TARGET := riscv64imac-unknown-none-elf
KERNEL_ELF := kernel/target/$(TARGET)/release/kernel
KERNEL_BIN := $(KERNEL_ELF).bin
MODE = test

ifeq ($(MODE), test)
	FS_IMG := res/test_suits.img
else
	FS_IMG := res/fat32.img
endif

# BOARD
BOARD ?= qemu
SBI ?= rustsbi
BOOTLOADER := res/$(SBI)-$(BOARD).bin

# Binutils
OBJDUMP := rust-objdump --arch-name=riscv64
OBJCOPY := rust-objcopy --binary-architecture=riscv64
GDB ?= riscv64-unknown-elf-gdb

.PHONY: env kernel build clean asm run dbg dbg-listener all

env:
	(rustup target list | grep "riscv64imac-unknown-none-elf (installed)") || rustup target add $(TARGET)
	cargo install cargo-binutils

kernel:
ifeq ($(MODE), test)
	@rm -rf kernel/.cargo
	@cp -r kernel/cargo-config kernel/.cargo
	@cd kernel && cargo build --package major --release --features test --offline
else
	@cd kernel && cargo build --package major --release
endif

build: $(KERNEL_BIN)
ifeq ($(MODE), test)
	# @cd bintool && cargo run --bin fs_init
else
	@make -C user build
endif

$(KERNEL_BIN): kernel
	@$(OBJCOPY) $(KERNEL_ELF) --strip-all -O binary $@
	@cp $(KERNEL_BIN) kernel-qemu
	@cp $(BOOTLOADER) sbi-qemu

clean:
	@cd kernel && cargo clean
	@cd user && make clean

asm:
	@cp $(KERNEL_ELF) bintool/res/kernel
	@cp $(KERNEL_ELF).d bintool/res/kernel.d
	cd bintool/res && rust-objdump --arch-name=riscv64 kernel -S --section=.data --section=.bss --section=.text > kernel.asm

run: build
	@qemu-system-riscv64 \
		-machine virt \
		-kernel kernel-qemu \
		-m 128M \
		-nographic \
		-smp 2 \
		-bios sbi-qemu \
		-drive file=$(FS_IMG),if=none,format=raw,id=x0 \
		-device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0

dbg: build
	@qemu-system-riscv64 \
		-machine virt \
		-kernel kernel-qemu \
		-m 128M \
		-nographic \
		-smp 2 \
		-bios sbi-qemu \
		-drive file=$(FS_IMG),if=none,format=raw,id=x0 \
		-device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 \
		-s -S

dbg-listener:
	$(GDB) -ex 'file $(KERNEL_ELF)'  -ex 'set arch riscv:rv64' -ex 'target remote localhost:1234'

all: build