use std::sync::Arc;

pub struct DisplayableVec {
    inner: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
}

impl DisplayableVec {
    pub fn new<T: ToString + Send + Sync + 'static>(vec: Vec<T>) -> Self {
        Self {
            inner: Arc::new(move || vec.iter().map(|x| x.to_string()).collect()),
        }
    }

    pub fn to_strings(&self) -> Vec<String> {
        (self.inner)()
    }
}
