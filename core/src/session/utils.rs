use super::{request::RequestHeader, response::ResponseHeader, keepalive::ConnectionValue};

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

    /// parse the connection header
    fn parse_connection_header(value: &[u8]) -> ConnectionValue {
        const KEEP_ALIVE: &str = "keep-alive";
        const CLOSE: &str = "close";
        const UPGRADE: &str = "upgrade";

        // fast path
        if value.eq_ignore_ascii_case(CLOSE.as_bytes()) {
            ConnectionValue::new().close()
        } else if value.eq_ignore_ascii_case(KEEP_ALIVE.as_bytes()) {
            ConnectionValue::new().keep_alive()
        } else if value.eq_ignore_ascii_case(UPGRADE.as_bytes()) {
            ConnectionValue::new().upgrade()
        } else {
            // slow path, parse the connection value
            let mut close = false;
            let mut upgrade = false;
            let value = std::str::from_utf8(value).unwrap_or("");

            for token in value
                .split(',')
                .map(|s| s.trim())
                .filter(|&x| !x.is_empty())
            {
                if token.eq_ignore_ascii_case(CLOSE) {
                    close = true;
                } else if token.eq_ignore_ascii_case(UPGRADE) {
                    upgrade = true;
                }
                if upgrade && close {
                    return ConnectionValue::new().upgrade().close();
                }
            }

            if close {
                ConnectionValue::new().close()
            } else if upgrade {
                ConnectionValue::new().upgrade()
            } else {
                ConnectionValue::new()
            }
        }
    }

    /// check if session keepalive by checking the connection header
    pub fn is_connection_keepalive(header_value: &http::header::HeaderValue) -> Option<bool> {
        let value = Self::parse_connection_header(header_value.as_bytes());
        if value.keep_alive {
            Some(true)
        } else if value.close {
            Some(false)
        } else {
            None
        }
    }
}
