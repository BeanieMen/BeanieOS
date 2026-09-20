use core::pin::Pin;
use core::task::{Context, Poll};

use crossbeam_queue::ArrayQueue;
use futures_util::stream::{Stream, StreamExt};
use futures_util::task::AtomicWaker;
use pc_keyboard::{DecodedKey, HandleControl, Keyboard, ScancodeSet1, layouts};
use spin::Once;

use crate::{print, println};

static SCANCODE_QUEUE: Once<ArrayQueue<u8>> = Once::new();
static WAKER: AtomicWaker = AtomicWaker::new();

pub struct ScancodeStream {
    _private: (),
}

impl ScancodeStream {
    pub fn new() -> Self {
        SCANCODE_QUEUE.call_once(|| ArrayQueue::new(100));
        Self { _private: () }
    }
}

impl Stream for ScancodeStream {
    type Item = u8;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<u8>> {
        let queue = SCANCODE_QUEUE
            .get()
            .expect("scancode queue not initialized");

        // Fast path.
        if let Some(scancode) = queue.pop() {
            return Poll::Ready(Some(scancode));
        }

        WAKER.register(cx.waker());
        match queue.pop() {
            Some(scancode) => {
                WAKER.take();
                Poll::Ready(Some(scancode))
            }
            None => Poll::Pending,
        }
    }
}

pub async fn print_keypresses() {
    let mut scancodes = ScancodeStream::new();
    let mut keyboard = Keyboard::new(
        ScancodeSet1::new(),
        layouts::Us104Key,
        HandleControl::Ignore,
    );

    while let Some(scancode) = scancodes.next().await {
        if let Ok(Some(key_event)) = keyboard.add_byte(scancode)
            && let Some(key) = keyboard.process_keyevent(key_event)
        {
            match key {
                DecodedKey::Unicode(character) => print!("{character}"),
                DecodedKey::RawKey(key) => print!("{key:?}"),
            }
        }
    }
}

pub(crate) fn add_scancode(scancode: u8) {
    if let Some(queue) = SCANCODE_QUEUE.get() {
        if queue.push(scancode).is_err() {
            println!("WARNING: scancode queue full; dropping keyboard input");
        } else {
            WAKER.wake();
        }
    } else {
        println!("WARNING: scancode queue uninitialized");
    }
}
