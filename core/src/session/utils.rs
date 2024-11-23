use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeepaliveStatus {
    Timeout(Duration),
    Infinite,
    Off,
}

pub struct Utils;

impl Utils {
    pub fn is_header_value_chunk_encoding(
        header_value: Option<&http::header::HeaderValue>,
    ) -> bool {
        match header_value {
            Some(value) => value.as_bytes().eq_ignore_ascii_case(b"chunked"),
            None => false,
        }
    }

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
}
