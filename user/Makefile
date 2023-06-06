TARGET := riscv64imac-unknown-none-elf
COMPILE_MOD := release
APP_DIR := src/bin
TARGET_DIR := target/$(TARGET)/$(COMPILE_MOD)
OBJDUMP := rust-objdump --arch-name=riscv64
OBJCOPY := rust-objcopy --binary-architecture=riscv64
PY := python3
APPS :=  $(wildcard $(APP_DIR)/*.rs)
ELFS := $(patsubst $(APP_DIR)/%.rs, $(TARGET_DIR)/%, $(APPS))

binary:
	@echo $(ELFS)
	@cargo build --release
	@$(PY) copy_target.py "$(ELFS)"

build: binary
	@cd ../bintool && cargo run --release --bin pack

clean:
	@cargo clean

all: build

.PHONY: binary build clean all