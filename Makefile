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
	mmd -i $@@@$(KERNEL_OFFSET) ::bin
	mmd -i $@@@$(KERNEL_OFFSET) ::sbin
	mmd -i $@@@$(KERNEL_OFFSET) ::etc
	mmd -i $@@@$(KERNEL_OFFSET) ::etc/apk
	mmd -i $@@@$(KERNEL_OFFSET) ::lib
	mmd -i $@@@$(KERNEL_OFFSET) ::lib/apk
	mmd -i $@@@$(KERNEL_OFFSET) ::lib/apk/db
	mmd -i $@@@$(KERNEL_OFFSET) ::var
	mmd -i $@@@$(KERNEL_OFFSET) ::var/cache
	mmd -i $@@@$(KERNEL_OFFSET) ::var/cache/apk
	mmd -i $@@@$(KERNEL_OFFSET) ::dev
	mmd -i $@@@$(KERNEL_OFFSET) ::proc
	mmd -i $@@@$(KERNEL_OFFSET) ::sys
	mmd -i $@@@$(KERNEL_OFFSET) ::root
	mmd -i $@@@$(KERNEL_OFFSET) ::home
	mmd -i $@@@$(KERNEL_OFFSET) ::tmp

	echo "nameserver 10.0.2.3" | mcopy -i $@@@$(KERNEL_OFFSET) - ::etc/resolv.conf
	printf "http://dl-cdn.alpinelinux.org/alpine/v3.20/main\nhttp://dl-cdn.alpinelinux.org/alpine/v3.20/community\n" | mcopy -i $@@@$(KERNEL_OFFSET) - ::etc/apk/repositories
	echo "root:x:0:0:root:/root:/bin/sh" | mcopy -i $@@@$(KERNEL_OFFSET) - ::etc/passwd
	echo "127.0.0.1 localhost" | mcopy -i $@@@$(KERNEL_OFFSET) - ::etc/hosts

	mcopy -i $@@@$(KERNEL_OFFSET) $(KERNEL) ::boot/kernel
	if [ -f /usr/lib/musl/lib/libc.so ]; then \
		mcopy -i $@@@$(KERNEL_OFFSET) /usr/lib/musl/lib/libc.so ::lib/ld-musl-x86_64.so.1 || true; \
		mcopy -i $@@@$(KERNEL_OFFSET) /usr/lib/musl/lib/libc.so ::lib/libc.musl-x86_64.so.1 || true; \
	fi
	for bin in $(USER_BIN_DIR)/*; do \
		mcopy -i $@@@$(KERNEL_OFFSET) "$$bin" "::bin/`basename "$$bin"`" || exit 1; \
	done
	mmd -i $@@@$(KERNEL_OFFSET) ::usr
	mmd -i $@@@$(KERNEL_OFFSET) ::usr/lib
	mmd -i $@@@$(KERNEL_OFFSET) ::usr/lib64
	mmd -i $@@@$(KERNEL_OFFSET) ::usr/include
	mmd -i $@@@$(KERNEL_OFFSET) ::usr/include/sys
	mmd -i $@@@$(KERNEL_OFFSET) ::usr/include/bits
	mmd -i $@@@$(KERNEL_OFFSET) ::usr/local
	mmd -i $@@@$(KERNEL_OFFSET) ::usr/local/lib
	mmd -i $@@@$(KERNEL_OFFSET) ::usr/local/lib/tcc
	mmd -i $@@@$(KERNEL_OFFSET) ::usr/local/lib/tcc/include
	mmd -i $@@@$(KERNEL_OFFSET) ::usr/share
	mmd -i $@@@$(KERNEL_OFFSET) ::usr/share/fonts
	mmd -i $@@@$(KERNEL_OFFSET) ::usr/share/fonts/X11
	mmd -i $@@@$(KERNEL_OFFSET) ::usr/share/fonts/X11/misc
	for f in userspace/fonts/*; do \
		mcopy -o -i $@@@$(KERNEL_OFFSET) "$$f" "::usr/share/fonts/X11/misc/`basename "$$f"`" || exit 1; \
	done
	if [ -f /usr/lib/musl/lib/libc.so ]; then \
		mcopy -o -i $@@@$(KERNEL_OFFSET) /usr/lib/musl/lib/libc.so ::lib/ld-musl-x86_64.so.1 || true; \
		mcopy -o -i $@@@$(KERNEL_OFFSET) /usr/lib/musl/lib/libc.so ::lib/libc.musl-x86_64.so.1 || true; \
		for f in /usr/lib/musl/lib/crt*.o /usr/lib/musl/lib/libc.a; do \
			mcopy -o -i $@@@$(KERNEL_OFFSET) "$$f" "::usr/lib/`basename "$$f"`" || true; \
			mcopy -o -i $@@@$(KERNEL_OFFSET) "$$f" "::usr/lib64/`basename "$$f"`" || true; \
			mcopy -o -i $@@@$(KERNEL_OFFSET) "$$f" "::lib/`basename "$$f"`" || true; \
		done; \
	fi
	if [ -d /usr/lib/musl/include ]; then \
		for h in /usr/lib/musl/include/*.h; do \
			mcopy -o -i $@@@$(KERNEL_OFFSET) "$$h" "::usr/include/`basename "$$h"`" || true; \
		done; \
		for h in /usr/lib/musl/include/sys/*.h; do \
			mcopy -o -i $@@@$(KERNEL_OFFSET) "$$h" "::usr/include/sys/`basename "$$h"`" || true; \
		done; \
		for h in /usr/lib/musl/include/bits/*.h; do \
			mcopy -o -i $@@@$(KERNEL_OFFSET) "$$h" "::usr/include/bits/`basename "$$h"`" || true; \
		done; \
	fi
	if [ -f /tmp/tinycc/tcc ]; then \
		mcopy -o -i $@@@$(KERNEL_OFFSET) /tmp/tinycc/tcc ::bin/tcc || true; \
		mcopy -o -i $@@@$(KERNEL_OFFSET) /tmp/tinycc/libtcc1.a ::usr/local/lib/tcc/libtcc1.a || true; \
		mcopy -o -i $@@@$(KERNEL_OFFSET) /tmp/tinycc/libtcc1.a ::usr/lib64/libtcc1.a || true; \
		mcopy -o -i $@@@$(KERNEL_OFFSET) /tmp/tinycc/libtcc1.a ::lib/libtcc1.a || true; \
		mcopy -o -i $@@@$(KERNEL_OFFSET) /tmp/tinycc/libtcc1.a ::usr/lib/libtcc1.a || true; \
		for h in /tmp/tinycc/include/*; do \
			mcopy -o -i $@@@$(KERNEL_OFFSET) "$$h" "::usr/local/lib/tcc/include/`basename "$$h"`" || true; \
		done; \
	fi
	if [ -f userspace/hello.c ]; then \
		mcopy -o -i $@@@$(KERNEL_OFFSET) userspace/hello.c ::hello.c || true; \
	fi
	mmd -i $@@@$(KERNEL_OFFSET) ::packages
	for pkg in userspace/packages/*; do \
		[ -f "$$pkg" ] && mcopy -o -i $@@@$(KERNEL_OFFSET) "$$pkg" "::packages/`basename "$$pkg"`" || true; \
	done


disk: $(DISK)

$(OVMF_VARS): $(OVMF_VARS_TEMPLATE)
	@mkdir -p $(OUT)
	cp $(OVMF_VARS_TEMPLATE) $@

QEMU_DISPLAY ?= $(shell [ -n "$$DISPLAY" ] || [ -n "$$WAYLAND_DISPLAY" ] && echo "sdl" || echo "none")
QEMU_KVM ?= $(shell test -w /dev/kvm && echo "-enable-kvm -cpu host")
QEMU_AUDIO ?= $(shell [ -n "$$DISPLAY" ] || [ -n "$$WAYLAND_DISPLAY" ] && echo "-audiodev sdl,id=snd0 -device AC97,audiodev=snd0 -machine q35,pcspk-audiodev=snd0" || echo "-audiodev none,id=snd0 -device AC97,audiodev=snd0")
QEMU_NET ?= -netdev user,id=net0,hostfwd=tcp::10022-:22 -device e1000,netdev=net0

run: $(DISK) $(OVMF_VARS)
	qemu-system-x86_64 \
		$(QEMU_AUDIO) \
		$(QEMU_NET) \
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
