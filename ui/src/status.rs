use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct SharedStatus(Arc<Mutex<String>>);

impl SharedStatus {
    pub fn new(s: &str) -> Self {
        SharedStatus(Arc::new(Mutex::new(s.to_string())))
    }

    pub fn set(&self, s: &str) {
        if let Ok(mut g) = self.0.lock() {
            *g = s.to_string();
        }
    }

    pub fn get(&self) -> String {
        if let Ok(g) = self.0.lock() {
            return g.clone();
        }
        "<locked>".to_string()
    }
}
