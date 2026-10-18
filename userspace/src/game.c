#include "syscall.h"

#define W 60
#define H 20
#define PADDLE_LEN 4

#define KEY_UP   0x80
#define KEY_DOWN 0x81

#define POLL_TIMEOUT 0

static char field[H][W + 1];

static int paddle_y;
static int ai_y;
static int ball_x, ball_y;
static int dir_x, dir_y;
static int score_me, score_ai;

static unsigned long strlen(const char *s)
{
    unsigned long n = 0;

    while (s[n]) {
        n++;
    }

    return n;
}

static void print(const char *s)
{
    sys_write(1, s, strlen(s));
}

static void print_number(int n)
{
    char buf[16];
    int i = 0;

    if (n < 0) {
        print("-");
        n = -n;
    }

    do {
        buf[i++] = '0' + (n % 10);
        n /= 10;
    } while (n > 0 && i < 15);

    while (i > 0) {
        char out[2];

        out[0] = buf[--i];
        out[1] = '\0';

        print(out);
    }
}

static void reset_ball(void)
{
    ball_x = W / 2;
    ball_y = H / 2;

    dir_x = 1;
    dir_y = 1;
}

static void build_field(void)
{
    int x, y;

    for (y = 0; y < H; y++) {
        for (x = 0; x < W; x++) {
            field[y][x] = ' ';
        }

        field[y][W] = '\0';
    }

    for (y = 0; y < PADDLE_LEN; y++) {
        field[paddle_y + y][1] = '|';
        field[ai_y + y][W - 2] = '|';
    }

    field[ball_y][ball_x] = 'O';
}


static void draw(void)
{
    int y;

    print("\033[H");

    print("============== BEANIEOS PONG ==============\r\n");

    print("You: ");
    print_number(score_me);

    print("   AI: ");
    print_number(score_ai);

    print("\r\n");
    print("+------------------------------------------------------------+\r\n");

    for (y = 0; y < H; y++) {
        print("|");
        print(field[y]);
        print("|\r\n");
    }

    print("+------------------------------------------------------------+\r\n");
    print("W/S or arrows: move   Q: quit\r\n");
}


static int read_key(void)
{
    char fds[8];
    unsigned char c = 0;
    long ready;
    long bytes;

    ready = sys_poll(fds, 1, POLL_TIMEOUT);

    if (ready <= 0) {
        return -1;
    }

    bytes = sys_read(0, &c, 1);

    if (bytes <= 0) {
        return -1;
    }

    return (int)c;
}


static void move_paddle(int key)
{
    if (key == 'w' || key == 'W' || key == KEY_UP) {
        paddle_y--;
    } else if (key == 's' || key == 'S' || key == KEY_DOWN) {
        paddle_y++;
    }

    if (paddle_y < 0) {
        paddle_y = 0;
    }

    if (paddle_y > H - PADDLE_LEN) {
        paddle_y = H - PADDLE_LEN;
    }
}

/* Paddle collision */

static int hits_paddle(int y, int paddle)
{
    return y >= paddle && y < paddle + PADDLE_LEN;
}

/* Simple AI */

static void move_ai(void)
{
    int target = ball_y - PADDLE_LEN / 2;

    if (ai_y < target) {
        ai_y++;
    } else if (ai_y > target) {
        ai_y--;
    }

    if (ai_y < 0) {
        ai_y = 0;
    }

    if (ai_y > H - PADDLE_LEN) {
        ai_y = H - PADDLE_LEN;
    }
}

/* Advance the game by one step */

static void step(void)
{
    int next_x = ball_x + dir_x;
    int next_y = ball_y + dir_y;

    /* Top and bottom walls */
    if (next_y < 0 || next_y >= H) {
        dir_y = -dir_y;
        next_y = ball_y + dir_y;
    }

    /* Player paddle on the left */
    if (dir_x < 0 && next_x <= 1) {
        if (hits_paddle(next_y, paddle_y)) {
            dir_x = 1;
            next_x = ball_x + dir_x;

            if (next_y < paddle_y + PADDLE_LEN / 2) {
                dir_y = -1;
            } else {
                dir_y = 1;
            }
        } else {
            score_ai++;
            reset_ball();
            return;
        }
    }

    /* AI paddle on the right */
    if (dir_x > 0 && next_x >= W - 2) {
        if (hits_paddle(next_y, ai_y)) {
            dir_x = -1;
            next_x = ball_x + dir_x;

            if (next_y < ai_y + PADDLE_LEN / 2) {
                dir_y = -1;
            } else {
                dir_y = 1;
            }
        } else {
            score_me++;
            reset_ball();
            return;
        }
    }

    ball_x += dir_x;
    ball_y = next_y;

    if (ball_x < 1) {
        ball_x = 1;
    }

    if (ball_x > W - 2) {
        ball_x = W - 2;
    }

    if (ball_y < 0) {
        ball_y = 0;
    }

    if (ball_y >= H) {
        ball_y = H - 1;
    }
}

/* Main */

int main(void)
{
    int key;

    paddle_y = H / 2 - PADDLE_LEN / 2;
    ai_y = H / 2 - PADDLE_LEN / 2;

    score_me = 0;
    score_ai = 0;

    reset_ball();

    print("\033[2J\033[H");
    print("Starting BeanieOS Pong...\r\n");

    for (;;) {
        key = read_key();

        if (key == 'q' || key == 'Q') {
            print("\033[2J\033[H");
            print("Thanks for playing Pong!\r\n");
            print("Final score: You ");

            print_number(score_me);
            print(" - AI ");
            print_number(score_ai);

            print("\r\n");
            return 0;
        }

        if (key >= 0) {
            move_paddle(key);
        }

        move_ai();
        step();
        build_field();
        draw();
        
        sys_sleep_ms(64); // 15 fps
        sys_yield();
    }

    return 0;
}
