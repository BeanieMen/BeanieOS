use spin::Mutex;

#[derive(Clone, Copy)]
pub struct Region {
    pub start: u64,
    pub end: u64,
}

impl Region {
    pub const fn new(start: u64, end: u64) -> Self {
        Region { start, end }
    }

    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.start && addr < self.end
    }
}

const MAX_RESERVED: usize = 32;

pub struct Dma {
    reserved: Mutex<ReservedList>,
}

struct ReservedList {
    entries: [Region; MAX_RESERVED],
    count: usize,
}

impl ReservedList {
    const fn new() -> Self {
        ReservedList {
            entries: [Region::new(0, 0); MAX_RESERVED],
            count: 0,
        }
    }

    fn push(&mut self, region: Region) {
        if self.count >= MAX_RESERVED {
            return;
        }

        self.entries[self.count] = region;
        self.count += 1;
    }
}

impl Dma {
    pub const fn new() -> Self {
        Dma {
            reserved: Mutex::new(ReservedList::new()),
        }
    }

    pub fn reserve(&self, region: Region) {
        self.reserved.lock().push(region);
    }

    /// Feeds every claimed range to `visit` without allocating, so this runs
    /// before the heap exists.
    pub fn copy_reserved(&self, mut visit: impl FnMut(Region)) {
        let guard = self.reserved.lock();

        for region in &guard.entries[..guard.count] {
            visit(*region);
        }
    }
}

impl Default for Dma {
    fn default() -> Self {
        Self::new()
    }
}
