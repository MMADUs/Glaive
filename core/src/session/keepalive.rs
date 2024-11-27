use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeepaliveStatus {
    Timeout(Duration),
    Infinite,
    Off,
}

pub struct ConnectionValue {
    pub keep_alive: bool,
    pub upgrade: bool,
    pub close: bool,
}

impl ConnectionValue {
    pub fn new() -> Self {
        ConnectionValue {
            keep_alive: false,
            upgrade: false,
            close: false,
        }
    }

    pub fn close(mut self) -> Self {
        self.close = true;
        self
    }

    pub fn upgrade(mut self) -> Self {
        self.upgrade = true;
        self
    }
    pub fn keep_alive(mut self) -> Self {
        self.keep_alive = true;
        self
    }
}
