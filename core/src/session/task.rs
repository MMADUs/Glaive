use super::response::ResponseHeader;

#[derive(Debug)]
pub enum Task {
    Header(ResponseHeader, bool),
    Body(Option<bytes::Bytes>, bool),
    Trailer(Option<Box<http::HeaderMap>>),
    Done,
    Failed(tokio::io::Error),
}

impl Task {
    pub fn is_end(&self) -> bool {
        match self {
            Task::Header(_, end) => *end,
            Task::Body(_, end) => *end,
            Task::Trailer(_) => true,
            Task::Done => true,
            Task::Failed(_) => true,
        }
    }
}
