/**
 * Copyright (c) 2024-2025 Glaive, Inc.
 *
 * This file is part of Glaive Gateway
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */

use std::collections::HashMap;

use pingora::http::ResponseHeader;
use pingora::proxy::Session;

use serde::Serialize;

use crate::proxy::RouterCtx;
use crate::response::ResponseProvider;

pub struct ResolverProvider {
    pub response_provider: ResponseProvider,
}

impl ResolverProvider {
    pub fn new() -> Self {
        ResolverProvider {
            response_provider: ResponseProvider::new(),
        }
    }

    // used to determine which cluster is selected
    pub async fn select_cluster(
        &self,
        prefix_map: &HashMap<String, usize>,
        original_uri: &str,
        session: &mut Session,
        ctx: &mut RouterCtx,
    ) -> bool {
        // create a uri to be modified
        let mut modified_uri = original_uri.to_string();

        // validator for a valid http uri
        match original_uri.parse::<http::Uri>() {
            Ok(_) => (),
            Err(_) => {
                // mark as bad request when http uri is not valid
                let _ = &self
                    .response_provider
                    .error_response(session, 400, "Invalid path", None)
                    .await;
                return true;
            }
        }

        // manipulating uri string
        // Split the input path and collect the segments
        let segments: Vec<&str> = original_uri.split('/').filter(|s| !s.is_empty()).collect();
        // get and format the result
        let serialized_uri = match segments.get(0) {
            Some(base) => format!("/{}", base),
            None => "/".to_string(),
        };

        // select the cluster address based on uri prefix
        let cluster_idx_option = prefix_map
            .iter()
            .find(|(prefix, _)| {
                // validate if the original uri matches the one from list
                prefix.to_string() == serialized_uri
            })
            .map(|(prefix, &idx)| {
                // modify by removing cluster prefix to be service uri
                modified_uri = original_uri.replacen(prefix, "", 1);
                // return the cluster index
                idx
            });

        // check if cluster address exist
        match cluster_idx_option {
            Some(idx) => {
                // if exist modify cluster address to the selected address
                ctx.cluster_address = idx;
            }
            None => {
                // if cluster does not exist, respond with 404
                let _ = &self
                    .response_provider
                    .error_response(session, 404, "Path does not exist", None)
                    .await;
                return true;
            }
        }

        // checks for empty modified uri
        if modified_uri.is_empty() {
            // if modified uri is empty then just redirect to "/"
            session
                .req_header_mut()
                .set_uri("/".parse::<http::Uri>().unwrap());
            return false;
        }

        // parse the modified uri to a valid http uri
        match modified_uri.parse::<http::Uri>() {
            Ok(new_uri) => {
                session.req_header_mut().set_uri(new_uri);
                false
            }
            Err(_) => {
                let _ = &self
                    .response_provider
                    .error_response(session, 400, "Invalid path result", None)
                    .await;
                true
            }
        }
    }
}

#[cfg(test)]
mod path_mod {
    fn get_base_path(input: &str) -> Option<String> {
        // Split the input path and collect the segments
        let segments: Vec<&str> = input.split('/').filter(|s| !s.is_empty()).collect();

        // Return the base path if it exists
        if let Some(base) = segments.get(0) {
            return Some(format!("/{}", base));
        }
        None
    }

    #[test]
    fn path_test() {
        // let paths = vec!["/cluster1s", "/cluster1s/", "/cluster1s/any", "/cluster1s/any/any", "/cluster1s/any?data=1"];
        let paths = vec!["/"];

        for path in paths {
            if let Some(base_path) = get_base_path(path) {
                assert_eq!(base_path, "/");
            }
        }
    }
}
