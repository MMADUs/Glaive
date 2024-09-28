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

use serde::{Deserialize, Serialize};

use crate::config::{auth, consumer};

// route endpoint config
#[derive(Debug, Deserialize, Serialize)]
pub struct Route {
    // the route or endpoint name
    name: Option<String>,
    // the list of path for this route
    paths: Option<Vec<String>>,
    // the list of allowed methods in this route
    // by default or leaving empty, all method is allowed
    methods: Option<Vec<String>>,
    // the headers opt for insert and remove
    headers: Option<Headers>,
    // the specified auth strategy for this route
    auth: Option<auth::AuthType>,
    // the list of allowed consumers for this route
    consumers: Option<Vec<consumer::Consumer>>,
}

// headers config
#[derive(Debug, Deserialize, Serialize)]
struct Headers {
    // used for storing a list headers data to be inserted
    insert: Vec<InsertHeader>,
    // used for removing a list of headers
    remove: Vec<RemoveHeader>,
}

// insert header config
#[derive(Debug, Deserialize, Serialize)]
struct InsertHeader {
    // header key
    key: String,
    // header value
    value: String,
}

// remove header config
#[derive(Debug, Deserialize, Serialize)]
struct RemoveHeader {
    // header key
    key: String,
}