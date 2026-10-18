#include "syscall.h"
#include "malloc.h"

static void test_print(const char *msg) {
    size_t len = 0;
    while (msg[len]) len++;
    sys_write(1, msg, len);
}

int main(void) {
    // 1. Basic malloc and alignment test
    char *p1 = (char *)malloc(64);
    if (!p1) {
        test_print("malloc(64) failed\n");
        return 1;
    }
    if (((unsigned long)p1 & 0xF) != 0) {
        test_print("p1 is not 16-byte aligned\n");
        return 1;
    }

    for (int i = 0; i < 63; i++) {
        p1[i] = 'A' + (i % 26);
    }
    p1[63] = '\0';

    // 2. Second allocation
    char *p2 = (char *)malloc(128);
    if (!p2 || p2 == p1) {
        test_print("malloc(128) failed\n");
        return 1;
    }

    // 3. Test realloc (expand p1 from 64 to 256 bytes)
    char *p3 = (char *)realloc(p1, 256);
    if (!p3) {
        test_print("realloc failed\n");
        return 1;
    }
    // Verify data was preserved after realloc
    if (p3[0] != 'A' || p3[1] != 'B') {
        test_print("realloc data corrupted\n");
        return 1;
    }

    // 4. Test calloc (all zeros)
    int *nums = (int *)calloc(16, sizeof(int));
    if (!nums) {
        test_print("calloc failed\n");
        return 1;
    }
    for (int i = 0; i < 16; i++) {
        if (nums[i] != 0) {
            test_print("calloc did not zero memory\n");
            return 1;
        }
    }

    // 5. Test free
    free(p2);
    free(p3);
    free(nums);

    // 6. Allocate again to verify reuse of freed block
    char *p4 = (char *)malloc(32);
    if (!p4) {
        test_print("reuse malloc failed\n");
        return 1;
    }
    free(p4);

    test_print("MALLOC OK\n");
    return 0;
}