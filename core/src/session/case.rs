use http::HeaderName;
use bytes::Bytes;
use http::header;

#[derive(Debug, Clone)]
pub struct CaseHeaderName(Bytes);

impl CaseHeaderName {
    pub fn new(name: String) -> Self {
        CaseHeaderName(name.into())
    }
}

impl CaseHeaderName {
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn from_slice(buf: &[u8]) -> Self {
        CaseHeaderName(Bytes::copy_from_slice(buf))
    }
}

/// A trait that converts into case-sensitive header names.
pub trait IntoCaseHeaderName {
    fn into_case_header_name(self) -> CaseHeaderName;
}

impl IntoCaseHeaderName for CaseHeaderName {
    fn into_case_header_name(self) -> CaseHeaderName {
        self
    }
}

impl IntoCaseHeaderName for String {
    fn into_case_header_name(self) -> CaseHeaderName {
        CaseHeaderName(self.into())
    }
}

impl IntoCaseHeaderName for &'static str {
    fn into_case_header_name(self) -> CaseHeaderName {
        CaseHeaderName(self.into())
    }
}

impl IntoCaseHeaderName for HeaderName {
    fn into_case_header_name(self) -> CaseHeaderName {
        CaseHeaderName(titled_header_name(&self))
    }
}

impl IntoCaseHeaderName for &HeaderName {
    fn into_case_header_name(self) -> CaseHeaderName {
        CaseHeaderName(titled_header_name(self))
    }
}

impl IntoCaseHeaderName for Bytes {
    fn into_case_header_name(self) -> CaseHeaderName {
        CaseHeaderName(self)
    }
}

fn titled_header_name(header_name: &HeaderName) -> Bytes {
    titled_header_name_str(header_name).map_or_else(
        || Bytes::copy_from_slice(header_name.as_str().as_bytes()),
        |s| Bytes::from_static(s.as_bytes()),
    )
}

pub fn titled_header_name_str(header_name: &HeaderName) -> Option<&'static str> {
    Some(match *header_name {
        header::AGE => "Age",
        header::CACHE_CONTROL => "Cache-Control",
        header::CONNECTION => "Connection",
        header::CONTENT_TYPE => "Content-Type",
        header::CONTENT_ENCODING => "Content-Encoding",
        header::CONTENT_LENGTH => "Content-Length",
        header::DATE => "Date",
        header::TRANSFER_ENCODING => "Transfer-Encoding",
        header::HOST => "Host",
        header::SERVER => "Server",
        header::SET_COOKIE => "Set-Cookie",
        // TODO: add more const header here to parse more
        _ => {
            return None;
        }
    })
}
