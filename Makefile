.PHONY: all setup kernel boot limine disk run clean fmt fmt-check clippy lint check

KERNEL_TARGET := x86_64.json
KERNEL := target/x86_64/release/beanieos

BOOT_OBJ := target/boot.o

LIMINE_DIR := limine
LIMINE_EFI := $(LIMINE_DIR)/BOOTX64.EFI

OUT := out

DISK := $(OUT)/beanieos.img
TEST_FILE := $(OUT)/test.txt
ESP_OFFSET := 1048576

OVMF_CODE := OVMF_CODE.4m.fd
OVMF_VARS := OVMF_VARS.4m.fd

all: $(DISK)

setup:
	rustup default stable
	rustup toolchain install nightly
	rustup +nightly component add llvm-tools-preview
	yay -S --needed qemu-system-x86 qemu-desktop dosfstools mtools parted

boot: $(BOOT_OBJ)

$(BOOT_OBJ): src/arch/boot.s
	mkdir -p target
	as --64 src/arch/boot.s -o $(BOOT_OBJ)

file:
	printf "This is a test file for BeanieOS.\n" > $(TEST_FILE)

kernel: boot linker.ld
	touch src/main.rs
	RUSTFLAGS="-C link-arg=$(BOOT_OBJ) -C link-arg=-Tlinker.ld" \
	cargo +nightly build --release \
		-Zbuild-std=core,alloc \
		-Zjson-target-spec \
		--target $(KERNEL_TARGET)
	llvm-strip --strip-debug $(KERNEL) -o $(KERNEL).stripped

disk: $(DISK)

$(DISK): kernel limine file
	mkdir -p $(OUT)

	rm -f $(DISK)

	truncate -s 128M $(DISK)

	parted -s $(DISK) mklabel gpt
	parted -s $(DISK) mkpart ESP fat32 1MiB 100%
	parted -s $(DISK) set 1 esp on

	mkfs.fat -F 32 --offset 2048 $(DISK)

	mmd -i $(DISK)@@$(ESP_OFFSET) ::EFI
	mmd -i $(DISK)@@$(ESP_OFFSET) ::EFI/BOOT

	mcopy -i $(DISK)@@$(ESP_OFFSET) \
		$(LIMINE_EFI) \
		::EFI/BOOT/BOOTX64.EFI

	mcopy -i $(DISK)@@$(ESP_OFFSET) \
		$(KERNEL).stripped \
		::kernel

	mcopy -i $(DISK)@@$(ESP_OFFSET) \
		$(TEST_FILE) \
		::test.txt
		
	mcopy -i $(DISK)@@$(ESP_OFFSET) \
		${LIMINE_DIR}/limine.conf \
		::limine.conf

run: $(DISK)
	mkdir -p $(OUT)

	qemu-system-x86_64 \
		-drive if=pflash,format=raw,readonly=on,file=$(OVMF_CODE) \
		-drive if=pflash,format=raw,file=$(OVMF_VARS) \
		-drive format=raw,file=$(DISK) \
		-m 512 \
		-display sdl

clean:
	cargo clean
	rm -rf $(OUT)

fmt:
	cargo +nightly fmt --all

fmt-check:
	cargo +nightly fmt --all -- --check

clippy: boot
	RUSTFLAGS="-C link-arg=$(BOOT_OBJ) -C link-arg=-Tlinker.ld" \
	cargo +nightly clippy --release \
		-Zbuild-std=core,alloc \
		-Zjson-target-spec \
		--target $(KERNEL_TARGET) \
		--all-targets -- -D warnings

lint: fmt-check clippy

check: boot
	RUSTFLAGS="-C link-arg=$(BOOT_OBJ) -C link-arg=-Tlinker.ld" \
	cargo +nightly check --release \
		-Zbuild-std=core,alloc \
		-Zjson-target-spec \
		--target $(KERNEL_TARGET)