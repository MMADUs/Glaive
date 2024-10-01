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

// consumer config
#[derive(Debug, Deserialize, Serialize)]
pub struct Consumer {
    // the allowed consumer name
    name: String,
    // the list of allowed access control
    acl: Vec<String>,
}

impl Consumer {
    fn get_name(&self) -> &String {
        &self.name
    }
    fn get_acl(&self) -> &Vec<String> {
        &self.acl
    }
}