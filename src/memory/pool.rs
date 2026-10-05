use crate::paging::PAGE_SIZE;

pub const MAX_AREAS: usize = 64;

// Boot avoids plus one per device range. At 16 this overflowed past the 13th BAR
// and every later range became allocatable RAM.
pub const MAX_AVOID: usize = 3 + MAX_AREAS;
pub const MAX_FREE: usize = 8192;

// A half-open physical range, `[start, end)`. The only range type in the tree.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Area {
    pub start: u64,
    pub end: u64,
}

impl Area {
    pub const fn new(start: u64, end: u64) -> Self {
        Area { start, end }
    }

    // False for an inverted or empty range.
    pub fn is_valid(&self) -> bool {
        self.end > self.start
    }

    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.start && addr < self.end
    }

    pub fn overlaps(&self, other: &Area) -> bool {
        self.start < other.end && other.start < self.end
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FramePool {
    areas: [Area; MAX_AREAS],
    area_len: usize,
    area_idx: usize,
    next: u64,
    avoid: [Area; MAX_AVOID],
    avoid_len: usize,
    free: [u64; MAX_FREE],
    free_len: usize,
    live: usize,
    reused: usize,
}

impl FramePool {
    pub const fn new() -> Self {
        FramePool {
            areas: [Area::new(0, 0); MAX_AREAS],
            area_len: 0,
            area_idx: 0,
            next: 0,
            avoid: [Area::new(0, 0); MAX_AVOID],
            avoid_len: 0,
            free: [0; MAX_FREE],
            free_len: 0,
            live: 0,
            reused: 0,
        }
    }

    pub fn add_area(&mut self, area: Area) -> bool {
        if self.area_len >= MAX_AREAS {
            return false;
        }
        if area.end <= area.start {
            return false;
        }

        let index = self.area_len;

        self.areas[index] = area;
        self.area_len += 1;

        if index == self.area_idx {
            self.enter_area(index);
        }

        true
    }

    pub fn avoid(&mut self, area: Area) -> bool {
        if self.avoid_len >= MAX_AVOID {
            return false;
        }
        if area.end <= area.start {
            return false;
        }

        self.avoid[self.avoid_len] = area;
        self.avoid_len += 1;

        true
    }

    // Overlap, not containment: a page straddling a reserved edge is unusable too.
    fn blocked(&self, area: &Area) -> bool {
        self.avoid[..self.avoid_len]
            .iter()
            .any(|a| a.overlaps(area))
    }

    fn blocked_page(&self, addr: u64) -> bool {
        self.blocked(&Area::new(
            Self::page_floor(addr),
            Self::page_floor(addr) + PAGE_SIZE,
        ))
    }

    fn page_floor(addr: u64) -> u64 {
        addr & !(PAGE_SIZE - 1)
    }

    fn enter_area(&mut self, index: usize) {
        self.area_idx = index;

        let start = self.areas[index].start;

        self.next = (start + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
    }

    pub fn alloc(&mut self) -> Option<u64> {
        if self.free_len > 0 {
            self.free_len -= 1;
            let addr = self.free[self.free_len];

            self.live += 1;
            self.reused += 1;

            return Some(addr);
        }

        loop {
            if self.area_idx >= self.area_len {
                return None;
            }

            let area = self.areas[self.area_idx];

            if self.next + PAGE_SIZE <= area.end {
                let addr = self.next;
                self.next += PAGE_SIZE;

                if self.blocked_page(addr) {
                    continue;
                }

                self.live += 1;

                return Some(addr);
            }

            self.area_idx += 1;

            if self.area_idx >= self.area_len {
                return None;
            }

            self.enter_area(self.area_idx);
        }
    }

    // A bus master cannot follow a page table, so a scattered buffer's translation
    // does not describe the buffer. Scans forward only, so a run cannot overlap
    // frames already handed out.
    pub fn alloc_contiguous(&mut self, frames: usize) -> Option<u64> {
        if frames == 0 {
            return None;
        }

        let span = frames as u64 * PAGE_SIZE;

        let mut index = self.area_idx;
        let mut cursor = self.next;

        while index < self.area_len {
            let area = self.areas[index];

            if cursor < area.start {
                cursor = area.start;
            }

            cursor = Self::page_floor(cursor);

            while cursor + span <= area.end {
                let run = Area::new(cursor, cursor + span);

                // Free to reuse, but not inside a run: neighbours may since have gone.
                let on_free_list = self.free[..self.free_len]
                    .iter()
                    .any(|frame| run.contains(*frame));

                if !self.blocked(&run) && !on_free_list {
                    self.area_idx = index;
                    self.next = cursor + span;
                    self.live += frames;

                    return Some(cursor);
                }

                cursor += PAGE_SIZE;
            }

            index += 1;

            if index < self.area_len {
                cursor = self.areas[index].start;
            }
        }

        None
    }

    pub fn free(&mut self, addr: u64) -> bool {
        if addr % PAGE_SIZE != 0 {
            return false;
        }

        if !self.areas[..self.area_len].iter().any(|a| a.contains(addr)) {
            return false;
        }

        if self.blocked_page(addr) {
            return false;
        }

        if self.free[..self.free_len].contains(&addr) {
            return false;
        }

        if self.free_len >= MAX_FREE {
            return false;
        }

        self.free[self.free_len] = addr;
        self.free_len += 1;
        self.live -= 1;

        true
    }

    pub fn live(&self) -> usize {
        self.live
    }

    pub fn free_len(&self) -> usize {
        self.free_len
    }

    pub fn reused(&self) -> usize {
        self.reused
    }
}
