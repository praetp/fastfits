use crate::fits::FitsImage;

struct CacheEntry {
    index: usize,
    image: FitsImage,
    last_used: u64,
}

/// Simple LRU cache for preloaded FITS images, keyed by file-list index.
pub struct ImageCache {
    entries: Vec<CacheEntry>,
    capacity: usize,
    counter: u64,
}

impl ImageCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            capacity,
            counter: 0,
        }
    }

    /// Remove and return the cached image for `index`, if present.
    pub fn take(&mut self, index: usize) -> Option<FitsImage> {
        if let Some(pos) = self.entries.iter().position(|e| e.index == index) {
            Some(self.entries.swap_remove(pos).image)
        } else {
            None
        }
    }

    /// Insert an image into the cache, evicting the least-recently-used entry
    /// if at capacity.  If `index` is already present it is replaced.
    pub fn insert(&mut self, index: usize, image: FitsImage) {
        // Replace existing entry for this index.
        if let Some(pos) = self.entries.iter().position(|e| e.index == index) {
            self.counter += 1;
            self.entries[pos] = CacheEntry { index, image, last_used: self.counter };
            return;
        }
        // Evict LRU if full.
        if self.entries.len() >= self.capacity {
            let lru = self.entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(i, _)| i)
                .unwrap();
            self.entries.swap_remove(lru);
        }
        self.counter += 1;
        self.entries.push(CacheEntry { index, image, last_used: self.counter });
    }

    /// Check whether `index` is cached (without affecting LRU order).
    pub fn contains(&self, index: usize) -> bool {
        self.entries.iter().any(|e| e.index == index)
    }

    /// Drop all cached images.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.counter = 0;
    }
}
