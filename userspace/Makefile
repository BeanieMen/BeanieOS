CC := gcc
CFLAGS := -Wall -Wextra -O2 -nostdlib -ffreestanding -fno-pie -no-pie \
	-fno-stack-protector -Iinclude -m64
LDFLAGS := -nostdlib -static -T linker.ld

BUILD_DIR := bin

C_SRCS := $(sort $(wildcard src/*.c))
BINS := $(patsubst src/%.c,$(BUILD_DIR)/%,$(C_SRCS))

ASM_SRC := src/crt0.s

all: $(BINS)

$(BUILD_DIR)/%: src/%.c $(ASM_SRC) linker.ld include/syscall.h
	@mkdir -p $(BUILD_DIR)
	$(CC) $(CFLAGS) $(LDFLAGS) $(ASM_SRC) $< -o $@

clean:
	rm -rf $(BUILD_DIR)

.PHONY: all clean