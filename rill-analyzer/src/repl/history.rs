pub struct History {
    entries: Vec<String>,
    position: usize,
}

impl History {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            position: 0,
        }
    }

    pub fn add(&mut self, entry: String) {
        self.entries.push(entry);
        self.position = self.entries.len();
    }

    #[allow(dead_code)]
    pub fn all(&self) -> &[String] {
        &self.entries
    }
}
