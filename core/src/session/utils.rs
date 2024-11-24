use std::time::Duration;

use super::{request::RequestHeader, response::ResponseHeader};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeepaliveStatus {
    Timeout(Duration),
    Infinite,
    Off,
}

pub struct Utils;

impl Utils {
    /// check if header are a valid chunk encoding
    pub fn is_header_value_chunk_encoding(
        header_value: Option<&http::header::HeaderValue>,
    ) -> bool {
        match header_value {
            Some(value) => value.as_bytes().eq_ignore_ascii_case(b"chunked"),
            None => false,
        }
    }

    /// check if content length has a valid length
    pub fn get_content_length_value(
        header_value: Option<&http::header::HeaderValue>,
    ) -> Option<usize> {
        match header_value {
            Some(header_value) => {
                let value_bytes = header_value.as_bytes();
                match std::str::from_utf8(value_bytes) {
                    Ok(str_value) => match str_value.parse::<i64>() {
                        Ok(value_len) => {
                            if value_len >= 0 {
                                Some(value_len as usize)
                            } else {
                                println!("negative content length value: {:?}", value_len);
                                None
                            }
                        }
                        Err(_) => {
                            println!("invalid content length value: {:?}", str_value);
                            None
                        }
                    },
                    Err(_) => {
                        println!("invalid content length encoding");
                        None
                    }
                }
            }
            None => None,
        }
    }

    /// check if a request is a request upgrade
    pub fn is_request_upgrade(request: &RequestHeader) -> bool {
        let version = *request.get_version();
        let upgrade_header = request.get_header(http::header::UPGRADE);
        // qualifies upgrade request
        version == http::Version::HTTP_11 && upgrade_header.is_some()
    }

    /// check if a response is a response upgrade
    pub fn is_response_upgrade(response: &ResponseHeader) -> bool {
        let version = *response.get_version();
        let status = response.get_status_code().as_u16();
        // qualifies the upgrade response
        version == http::Version::HTTP_11 && status == 101
    }

    /// check if request is expect continue (Expect: 100 continue)
    pub fn is_request_expect_continue(request: &RequestHeader) -> bool {
        let version = *request.get_version();
        let expect = request.get_header(http::header::EXPECT);
        let valid = match expect {
            Some(header) => header.as_bytes().eq_ignore_ascii_case(b"100-continue"),
            None => false,
        };
        // qualifies the expect continue request
        version == http::Version::HTTP_11 && valid 
    }
}
