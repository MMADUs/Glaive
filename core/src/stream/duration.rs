use std::time::{Duration, Instant};
use std::task::Poll;
use tokio::io;

#[derive(Debug)]
pub struct AccumulatedDuration {
    total: Duration,
    last_start: Option<Instant>,
}

impl AccumulatedDuration {
    pub fn new() -> Self {
        AccumulatedDuration {
            total: Duration::ZERO,
            last_start: None,
        }
    }

    pub fn start(&mut self) {
        if self.last_start.is_none() {
            self.last_start = Some(Instant::now());
        }
    }

    pub fn stop(&mut self) {
        if let Some(start) = self.last_start.take() {
            self.total += start.elapsed();
        }
    }

    pub fn poll_write_time(&mut self, result: &Poll<io::Result<usize>>, buf_size: usize) {
        match result {
            Poll::Ready(Ok(n)) => {
                if *n == buf_size {
                    self.stop();
                } else {
                    // partial write
                    self.start();
                }
            }
            Poll::Ready(Err(_)) => {
                self.stop();
            }
            _ => self.start(),
        }
    }

    pub fn poll_time(&mut self, result: &Poll<io::Result<()>>) {
        match result {
            Poll::Ready(_) => {
                self.stop();
            }
            _ => self.start(),
        }
    }
}
