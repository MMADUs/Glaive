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

use crate::proxy::RouterCtx;

// used to determine which cluster is selected
pub async fn select_cluster(
    prefix_map: &HashMap<String, usize>,
    original_uri: &str,
    session: &mut Session,
    ctx: &mut RouterCtx,
) -> bool {
    // create a uri to be modified
    let mut modified_uri = original_uri.to_string();

    // select the cluster address based on uri prefix
    let cluster_idx_option = prefix_map
        .iter()
        .find(|(prefix, _)| original_uri.starts_with(prefix.as_str()))
        .map(|(prefix, &idx)| {
            modified_uri = original_uri.replacen(prefix, "", 1);
            idx
        });

    println!("Original URI: {}", original_uri);
    println!("Modified URI: {}", modified_uri);

    // check if cluster address exist
    match cluster_idx_option {
        Some(idx) => {
            // if exist modify cluster address to the selected address
            ctx.cluster_address = idx;
        }
        None => {
            let header = ResponseHeader::build(404, None).unwrap();
            session.set_keepalive(None);
            session.write_response_header(Box::new(header), true).await.unwrap();
            return true
        }
    }

    // checks for empty modified uri
    if modified_uri.is_empty() {
        // if modified uri is empty then just redirect to "/"
        session.req_header_mut().set_uri("/".parse::<http::Uri>().unwrap());
        return false
    }

    // parse the modified uri to a valid http uri
    match modified_uri.parse::<http::Uri>() {
        Ok(new_uri) => {
            session.req_header_mut().set_uri(new_uri);
            false
        }
        Err(e) => {
            println!("URI parse error: {}", e);
            let header = ResponseHeader::build(400, None).unwrap();
            session.set_keepalive(None);
            session.write_response_header(Box::new(header), true).await.unwrap();
            true
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