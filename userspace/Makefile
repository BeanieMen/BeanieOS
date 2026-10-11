CC := gcc
CFLAGS := -Wall -Wextra -O2 -nostdlib -ffreestanding -fno-pie -no-pie -fno-stack-protector -Iinclude -m64
LDFLAGS := -nostdlib -static -T linker.ld

BUILD_DIR := bin
TARGET := $(BUILD_DIR)/init

SRCS := src/crt0.s src/init.c

all: $(TARGET)

$(TARGET): $(SRCS) linker.ld
	@mkdir -p $(BUILD_DIR)
	$(CC) $(CFLAGS) $(LDFLAGS) $(SRCS) -o $@

clean:
	rm -rf $(BUILD_DIR)

.PHONY: all clean