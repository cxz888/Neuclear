# Building
TARGET := riscv64imac-unknown-none-elf
KERNEL_ELF := kernel/target/$(TARGET)/release/kernel
KERNEL_BIN := $(KERNEL_ELF).bin
FS_IMG := res/fat32.img

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

build: env $(KERNEL_BIN)
	@make -C user build

env:
	(rustup target list | grep "riscv64imac-unknown-none-elf (installed)") || rustup target add $(TARGET)
	cargo install cargo-binutils

$(KERNEL_BIN): kernel
	@$(OBJCOPY) $(KERNEL_ELF) --strip-all -O binary $@

kernel:
	@cd kernel && cargo build --release

clean:
	@cd kernel && cargo clean
	@cd user && make clean

run: build
	@qemu-system-riscv64 \
		-machine virt \
		-nographic \
		-bios $(BOOTLOADER) \
		-device loader,file=$(KERNEL_BIN),addr=$(KERNEL_ENTRY_PA) \
		-drive file=$(FS_IMG),if=none,format=raw,id=x0 \
		-device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0

dbg: build
	qemu-system-riscv64 \
		-machine virt \
		-nographic \
		-bios $(BOOTLOADER) \
		-device loader,file=$(KERNEL_BIN),addr=$(KERNEL_ENTRY_PA) \
		-drive file=$(FS_IMG),if=none,format=raw,id=x0 \
		-device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 \
		-s -S

dbg-listener:
	$(GDB) -ex 'file $(KERNEL_ELF)'  -ex 'set arch riscv:rv64' -ex 'target remote localhost:1234'

.PHONY: build env kernel clean
