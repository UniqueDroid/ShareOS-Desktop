use std::{
    io::{self, Read},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
#[derive(Default)]
pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
}
impl Progress {
    pub fn values(&self) -> (u64, u64) {
        (
            self.done.load(Ordering::Relaxed),
            self.total.load(Ordering::Relaxed),
        )
    }
    pub fn total(&self, total: u64) {
        self.total.store(total, Ordering::Relaxed);
    }
    pub fn add(&self, bytes: usize) {
        self.done.fetch_add(bytes as u64, Ordering::Relaxed);
    }
}
pub struct Reader<R> {
    pub inner: R,
    pub progress: Arc<Progress>,
}
impl<R: Read> Read for Reader<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(bytes)?;
        self.progress.add(n);
        Ok(n)
    }
}
