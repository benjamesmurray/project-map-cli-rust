pub struct RetentionPolicy {
    days: u32,
}

impl RetentionPolicy {
    pub fn new(days: u32) -> Self {
        Self { days }
    }

    pub fn apply(&self) -> u32 {
        self.days
    }
}

pub fn flush_buffer() {}
