#ifndef MALLOC_H
#define MALLOC_H

#include "syscall.h"

typedef unsigned long size_t;
#define NULL ((void *)0)

// 32-byte header (multiple of 16 bytes to guarantee 16-byte payload alignment)
struct block_header {
    size_t size;               // Payload size in bytes
    size_t is_free;            // 1 if available, 0 if allocated
    struct block_header *next; // Next block in heap linked list
    size_t _padding;           // Pad struct size to 32 bytes
};

#define BLOCK_HEADER_SIZE (sizeof(struct block_header))

static struct block_header *free_list_head = NULL;

static inline void *malloc(size_t size) {
    if (size == 0) {
        return NULL;
    }

    size = (size + 15) & ~15UL;

    struct block_header *curr = free_list_head;
    while (curr) {
        if (curr->is_free && curr->size >= size) {
            if (curr->size >= size + BLOCK_HEADER_SIZE + 16) {
                struct block_header *next_block = (struct block_header *)((char *)(curr + 1) + size);
                next_block->size = curr->size - size - BLOCK_HEADER_SIZE;
                next_block->is_free = 1;
                next_block->next = curr->next;
                next_block->_padding = 0;

                curr->next = next_block;
                curr->size = size;
            }
            curr->is_free = 0;
            return (void *)(curr + 1);
        }
        curr = curr->next;
    }

    size_t chunk_size = size + BLOCK_HEADER_SIZE;
    if (chunk_size < 65536) {
        chunk_size = 65536;
    }
    chunk_size = (chunk_size + 4095) & ~4095UL;

    void *old_brk = sys_brk(0);
    if (!old_brk) {
        return NULL;
    }

    void *new_brk = sys_brk((char *)old_brk + chunk_size);
    if (new_brk <= old_brk) {
        return NULL;
    }

    struct block_header *new_block = (struct block_header *)old_brk;
    new_block->size = chunk_size - BLOCK_HEADER_SIZE;
    new_block->is_free = 0;
    new_block->next = NULL;
    new_block->_padding = 0;

    // Split the freshly allocated chunk if it's much larger than requested
    if (new_block->size >= size + BLOCK_HEADER_SIZE + 16) {
        struct block_header *remainder = (struct block_header *)((char *)(new_block + 1) + size);
        remainder->size = new_block->size - size - BLOCK_HEADER_SIZE;
        remainder->is_free = 1;
        remainder->next = NULL;
        remainder->_padding = 0;

        new_block->next = remainder;
        new_block->size = size;
    }

    if (!free_list_head) {
        free_list_head = new_block;
    } else {
        struct block_header *last = free_list_head;
        while (last->next) {
            last = last->next;
        }
        last->next = new_block;
    }

    return (void *)(new_block + 1);
}

static inline void free(void *ptr) {
    if (!ptr) {
        return;
    }
    struct block_header *block = (struct block_header *)ptr - 1;
    block->is_free = 1;

    // Coalesce adjacent free blocks
    struct block_header *curr = free_list_head;
    while (curr && curr->next) {
        if (curr->is_free && curr->next->is_free) {
            curr->size += BLOCK_HEADER_SIZE + curr->next->size;
            curr->next = curr->next->next;
        } else {
            curr = curr->next;
        }
    }
}

static inline void *realloc(void *ptr, size_t new_size) {
    if (!ptr) {
        return malloc(new_size);
    }
    if (new_size == 0) {
        free(ptr);
        return NULL;
    }

    struct block_header *block = (struct block_header *)ptr - 1;
    if (block->size >= new_size) {
        return ptr;
    }

    void *new_ptr = malloc(new_size);
    if (!new_ptr) {
        return NULL;
    }

    size_t copy_len = block->size < new_size ? block->size : new_size;
    char *src = (char *)ptr;
    char *dst = (char *)new_ptr;
    for (size_t i = 0; i < copy_len; i++) {
        dst[i] = src[i];
    }

    free(ptr);
    return new_ptr;
}

static inline void *calloc(size_t nmemb, size_t size) {
    size_t total = nmemb * size;
    void *ptr = malloc(total);
    if (!ptr) {
        return NULL;
    }
    char *p = (char *)ptr;
    for (size_t i = 0; i < total; i++) {
        p[i] = 0;
    }
    return ptr;
}

#endif