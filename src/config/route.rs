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

impl Route {
    pub fn get_name(&self) -> &Option<String> {
        &self.name
    }
    pub fn get_paths(&self) -> &Option<Vec<String>> {
        &self.paths
    }
    pub fn get_methods(&self) -> &Option<Vec<String>> {
        &self.methods
    }
    pub fn get_auth(&self) -> &Option<auth::AuthType> {
        &self.auth
    }
    pub fn get_consumers(&self) -> &Option<Vec<consumer::Consumer>> {
        &self.consumers
    }
}

// headers config
#[derive(Debug, Deserialize, Serialize)]
struct Headers {
    // used for storing a list headers data to be inserted
    insert: Vec<InsertHeader>,
    // used for removing a list of headers
    remove: Vec<RemoveHeader>,
}

impl Headers {
    pub fn to_be_inserted(&self) -> &Vec<InsertHeader> {
        &self.insert
    }
    pub fn to_be_removed(&self) -> &Vec<RemoveHeader> {
        &self.remove
    }
}

// insert header config
#[derive(Debug, Deserialize, Serialize)]
struct InsertHeader {
    // header key
    key: String,
    // header value
    value: String,
}

impl InsertHeader {
    pub fn insert_header(&self) {
        let key = self.key.clone();
        let value = self.value.clone();
    }
}

// remove header config
#[derive(Debug, Deserialize, Serialize)]
struct RemoveHeader {
    // header key
    key: String,
}

impl RemoveHeader {
    pub fn remove_header(&self) {
        let key = self.key.clone();
    }
}