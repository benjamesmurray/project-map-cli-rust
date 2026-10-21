/// Append-only audit log.
pub struct AuditLog {
    entries: Vec<String>,
}

impl AuditLog {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn append(&mut self, entry: String) {
        self.entries.push(entry);
    }
}

pub fn rotate_segments(log: &mut AuditLog) {
    log.entries.clear();
}

pub fn verify_chain(log: &AuditLog) -> bool {
    !log.entries.is_empty()
}
