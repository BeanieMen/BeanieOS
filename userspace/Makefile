CC := gcc
CFLAGS := -Wall -Wextra -O2 -nostdlib -ffreestanding -fno-pie -no-pie \
	-fno-stack-protector -Iinclude -m64
LDFLAGS := -nostdlib -static -T linker.ld

BUILD_DIR := bin

C_SRCS := $(sort $(wildcard src/*.c))
BINS := $(patsubst src/%.c,$(BUILD_DIR)/%,$(C_SRCS))

ASM_SRC := src/crt0.s

all: $(BINS) doom xfbdev

doom:
	@if [ -d doom/doomgeneric ]; then \
		$(MAKE) -C doom/doomgeneric; \
	fi

xfbdev:
	@if [ -d tinyxserver ]; then \
		$(MAKE) -C tinyxserver Xfbdev; \
		mkdir -p $(BUILD_DIR); \
		cp tinyxserver/Xfbdev $(BUILD_DIR)/Xfbdev; \
	fi

$(BUILD_DIR)/%: src/%.c $(ASM_SRC) linker.ld include/syscall.h
	@mkdir -p $(BUILD_DIR)
	$(CC) $(CFLAGS) $(LDFLAGS) $(ASM_SRC) $< -o $@

clean:
	rm -f $(BINS) $(BUILD_DIR)/doom $(BUILD_DIR)/Xfbdev
	@if [ -d doom/doomgeneric ]; then \
		$(MAKE) -C doom/doomgeneric clean; \
	fi
	@if [ -d tinyxserver ]; then \
		$(MAKE) -C tinyxserver clean; \
	fi

.PHONY: all clean doom xfbdev