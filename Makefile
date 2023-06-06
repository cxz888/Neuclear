# Building
TARGET := riscv64imac-unknown-none-elf
KERNEL_ELF := kernel/target/$(TARGET)/release/kernel
KERNEL_BIN := $(KERNEL_ELF).bin
FS_IMG := res/fat32.img
TEST_SUITS := res/test_suits.img
MODE = test

# BOARD
BOARD ?= qemu
SBI ?= rustsbi
BOOTLOADER := res/$(SBI)-$(BOARD).bin

# KERNEL ENTRY
KERNEL_ENTRY_PA := 0x80200000

# Binutils
OBJDUMP := rust-objdump --arch-name=riscv64
OBJCOPY := rust-objcopy --binary-architecture=riscv64
GDB ?= riscv64-unknown-elf-gdb

.PHONY: env kernel build clean asm all

env:
	(rustup target list | grep "riscv64imac-unknown-none-elf (installed)") || rustup target add $(TARGET)
	cargo install cargo-binutils

kernel:
ifeq ($(MODE), test)
	@cp -r kernel/cargo-config kernel/.cargo
	@cd kernel && cargo build --package major --release --features test
else
	@cd kernel && cargo build --package major --release
endif

build: env $(KERNEL_BIN)
ifeq ($(MODE), test)
	@cd bintool && cargo run --bin fs_init
else
	@make -C user build
endif

$(KERNEL_BIN): kernel
	@$(OBJCOPY) $(KERNEL_ELF) --strip-all -O binary $@
ifeq ($(MODE), test)
	@cp $(KERNEL_BIN) kernel-qemu
	@cp $(BOOTLOADER) sbi-qemu
endif

clean:
	@cd kernel && cargo clean
	@cd user && make clean

asm:
	@cp $(KERNEL_ELF) bintool/res/kernel
	@cp $(KERNEL_ELF).d bintool/res/kernel.d
	cd bintool/res && rust-objdump --arch-name=riscv64 kernel -S --section=.data --section=.bss --section=.text > kernel.asm

run: build
ifeq ($(MODE), test)
	@qemu-system-riscv64 \
		-machine virt \
		-kernel kernel-qemu \
		-m 128M \
		-nographic \
		-smp 2 \
		-bios sbi-qemu \
		-drive file=$(TEST_SUITS),if=none,format=raw,id=x0 \
		-device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0
else
	@qemu-system-riscv64 \
		-machine virt \
		-nographic \
		-bios $(BOOTLOADER) \
		-device loader,file=$(KERNEL_BIN),addr=$(KERNEL_ENTRY_PA) \
		-drive file=$(FS_IMG),if=none,format=raw,id=x0 \
		-device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0
endif

dbg: build
ifeq ($(MODE), test)
	@qemu-system-riscv64 \
		-machine virt \
		-kernel kernel-qemu \
		-m 128M \
		-nographic \
		-smp 2 \
		-bios sbi-qemu \
		-drive file=$(TEST_SUITS),if=none,format=raw,id=x0 \
		-device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 \
		-s -S
else
	@qemu-system-riscv64 \
		-machine virt \
		-nographic \
		-bios $(BOOTLOADER) \
		-device loader,file=$(KERNEL_BIN),addr=$(KERNEL_ENTRY_PA) \
		-drive file=$(FS_IMG),if=none,format=raw,id=x0 \
		-device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 \
		-s -S
endif

dbg-listener:
	$(GDB) -ex 'file $(KERNEL_ELF)'  -ex 'set arch riscv:rv64' -ex 'target remote localhost:1234'

all: build