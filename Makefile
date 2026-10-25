TARGET := x86_64.json
KERNEL := target/x86_64/release/beanieos
BOOT_OBJ := target/boot.o
TIMER_OBJ := target/timer.o

LIMINE := limine
OUT := out
DISK := $(OUT)/beanieos.img

OVMF_CODE := OVMF_CODE.4m.fd
OVMF_VARS_TEMPLATE := OVMF_VARS.4m.fd
OVMF_VARS := $(OUT)/OVMF_VARS.fd

ESP_OFFSET := 1048576
KERNEL_OFFSET := 33554432

CARGO := cargo +nightly
CARGO_FLAGS := --release --features kernel -Zbuild-std=core,alloc \
	-Zjson-target-spec --target $(TARGET)
RUSTFLAGS := -C link-arg=$(BOOT_OBJ) -C link-arg=$(TIMER_OBJ) -C link-arg=-Tlinker.ld
USER_BIN_DIR := userspace/bin
USER_HDRS := userspace/include/syscall.h userspace/linker.ld userspace/Makefile


.PHONY: all setup check kernel disk run symbols clean userbins xfbdev tinyxserver

all: $(DISK)

userbins:
	$(MAKE) -C userspace

xfbdev tinyxserver:
	$(MAKE) -C userspace xfbdev

setup:
	rustup toolchain install nightly
	rustup +nightly component add llvm-tools-preview rust-src
	yay -S --needed qemu-system-x86 dosfstools mtools parted

$(BOOT_OBJ): src/arch/boot.s
	@mkdir -p $(@D)
	as --64 $< -o $@

$(TIMER_OBJ): src/arch/interrupts/timer.s
	@mkdir -p $(@D)
	as --64 $< -o $@

check: $(BOOT_OBJ) $(TIMER_OBJ) linker.ld
	RUSTFLAGS="$(RUSTFLAGS)" $(CARGO) check $(CARGO_FLAGS)

kernel: $(BOOT_OBJ) $(TIMER_OBJ) linker.ld
	touch src/main.rs
	RUSTFLAGS="$(RUSTFLAGS)" $(CARGO) build $(CARGO_FLAGS)

$(DISK): kernel userbins
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
	mmd -i $@@@$(KERNEL_OFFSET) ::tmp
	mcopy -i $@@@$(KERNEL_OFFSET) $(KERNEL) ::boot/kernel
	for bin in $(USER_BIN_DIR)/*; do \
		mcopy -i $@@@$(KERNEL_OFFSET) "$$bin" ::`basename "$$bin"` || exit 1; \
	done
	mmd -i $@@@$(KERNEL_OFFSET) ::usr
	mmd -i $@@@$(KERNEL_OFFSET) ::usr/share
	mmd -i $@@@$(KERNEL_OFFSET) ::usr/share/fonts
	mmd -i $@@@$(KERNEL_OFFSET) ::usr/share/fonts/X11
	mmd -i $@@@$(KERNEL_OFFSET) ::usr/share/fonts/X11/misc
	for f in userspace/fonts/*; do \
		mcopy -i $@@@$(KERNEL_OFFSET) "$$f" "::usr/share/fonts/X11/misc/`basename "$$f"`" || exit 1; \
	done
	

disk: $(DISK)

$(OVMF_VARS): $(OVMF_VARS_TEMPLATE)
	@mkdir -p $(OUT)
	cp $(OVMF_VARS_TEMPLATE) $@

QEMU_DISPLAY ?= $(shell [ -n "$$DISPLAY" ] || [ -n "$$WAYLAND_DISPLAY" ] && echo "sdl" || echo "none")
QEMU_KVM ?= $(shell test -w /dev/kvm && echo "-enable-kvm -cpu host")
QEMU_AUDIO ?= $(shell [ -n "$$DISPLAY" ] || [ -n "$$WAYLAND_DISPLAY" ] && echo "-audiodev sdl,id=snd0 -device AC97,audiodev=snd0 -machine q35,pcspk-audiodev=snd0" || echo "-audiodev none,id=snd0 -device AC97,audiodev=snd0")

run: $(DISK) $(OVMF_VARS)
	qemu-system-x86_64 \
		$(QEMU_AUDIO) \
		$(QEMU_KVM) \
		-drive if=pflash,format=raw,readonly=on,file=$(OVMF_CODE) \
		-drive if=pflash,format=raw,file=$(OVMF_VARS) \
		-drive format=raw,file=$(DISK) \
		-m 512 \
		-display $(QEMU_DISPLAY) \
		-serial stdio

symbols: kernel
	nm $(KERNEL) | grep -E ' (rust_entry|_start|long_mode_start|kernel_start|kernel_end)$$'

clean:
	cargo clean
	rm -rf $(OUT) target/boot.o target/timer.o
	$(MAKE) -C userspace clean