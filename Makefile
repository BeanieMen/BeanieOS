TARGET := x86_64.json
KERNEL := target/x86_64/release/beanieos
BOOT_OBJ := target/boot.o

LIMINE := limine
OVMF_CODE := OVMF_CODE.4m.fd
OVMF_VARS := out/OVMF_VARS.fd

OUT := out
DISK := $(OUT)/beanieos.img

ESP_OFFSET := 1048576
KERNEL_OFFSET := 33554432

CARGO := cargo +nightly
CARGO_FLAGS := --release --features kernel -Zbuild-std=core,alloc \
	-Zjson-target-spec --target $(TARGET)
RUSTFLAGS := -C link-arg=$(BOOT_OBJ) -C link-arg=-Tlinker.ld

.PHONY: all setup check kernel disk run symbols clean

all: $(DISK)

setup:
	rustup toolchain install nightly
	rustup +nightly component add llvm-tools-preview rust-src
	yay -S --needed qemu-system-x86 dosfstools mtools parted

$(BOOT_OBJ): src/arch/boot.s
	@mkdir -p $(@D)
	as --64 $< -o $@

check: $(BOOT_OBJ) linker.ld
	RUSTFLAGS="$(RUSTFLAGS)" $(CARGO) check $(CARGO_FLAGS)

kernel: $(BOOT_OBJ) linker.ld
	touch src/main.rs
	RUSTFLAGS="$(RUSTFLAGS)" $(CARGO) build $(CARGO_FLAGS)

$(DISK): kernel
	@mkdir -p $(OUT)
	rm -f $@
	truncate -s 128M $@
	parted -s $@ mklabel gpt
	parted -s $@ mkpart ESP fat32 1MiB 32MiB
	parted -s $@ set 1 esp on
	parted -s $@ mkpart BEANIEOS fat32 32MiB 100%
	mkfs.fat -F 32 --offset 2048 --invariant $@
	mkfs.fat -F 32 --offset 65536 --invariant $@
	mmd -i $@@@$(ESP_OFFSET) ::EFI
	mmd -i $@@@$(ESP_OFFSET) ::EFI/BOOT
	mcopy -i $@@@$(ESP_OFFSET) $(LIMINE)/BOOTX64.EFI ::EFI/BOOT/BOOTX64.EFI
	mcopy -i $@@@$(ESP_OFFSET) $(LIMINE)/limine.conf ::limine.conf
	mmd -i $@@@$(KERNEL_OFFSET) ::boot
	mcopy -i $@@@$(KERNEL_OFFSET) $(KERNEL) ::boot/kernel

disk: $(DISK)

$(OVMF_VARS): $(OVMF_CODE)
	@mkdir -p $(OUT)
	cp $(OVMF_CODE) $(OVMF_VARS)

run: $(DISK) $(OVMF_VARS)
	qemu-system-x86_64 \
		-machine q35 \
		-drive if=pflash,format=raw,readonly=on,file=$(OVMF_CODE) \
		-drive if=pflash,format=raw,file=$(OVMF_VARS) \
		-drive format=raw,file=$(DISK) \
		-m 512 \
		-display sdl

symbols: kernel
	nm $(KERNEL) | grep -E ' (rust_entry|_start|long_mode_start|kernel_start|kernel_end)$$'

clean:
	cargo clean
	rm -rf $(OUT)