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
    pub fn prev(&mut self) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }
        if self.position > 0 {
            self.position -= 1;
        }
        Some(&self.entries[self.position])
    }

    #[allow(dead_code)]
    pub fn next(&mut self) -> Option<&str> {
        if self.position >= self.entries.len() {
            return None;
        }
        self.position += 1;
        if self.position >= self.entries.len() {
            None
        } else {
            Some(&self.entries[self.position])
        }
    }

    #[allow(dead_code)]
    pub fn all(&self) -> &[String] {
        &self.entries
    }
}
