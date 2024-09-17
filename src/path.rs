/**
 * Copyright (c) 2024-2025 ArcX, Inc.
 *
 * This file is part of ArcX Gateway
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
use pingora::proxy::{Session};

// router context
use crate::proxy::RouterCtx;

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

    // check if cluster address exist
    match cluster_idx_option {
        Some(idx) => {
            // if exist modify cluster address to the selected address
            println!("Cluster idx: {}", idx);
            ctx.cluster_address = idx;
        }
        None => {
            let header = ResponseHeader::build(404, None).unwrap();
            session.set_keepalive(None);
            session.write_response_header(Box::new(header), true).await.unwrap();
            return true
        }
    }

    if modified_uri.is_empty() {
        println!("uri is empty.");
        // if modified uri is empty then just redirect to "/"
        session.req_header_mut().set_uri("/".parse::<http::Uri>().unwrap());
        return false
    }

    // parse the modified uri to a valid http uri
    match modified_uri.parse::<http::Uri>() {
        Ok(new_uri) => {
            println!("New URI: {}", new_uri);
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