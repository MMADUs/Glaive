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

use crate::auth::AuthProvider;
use crate::limiter::LimiterProvider;
use crate::path::ResolverProvider;
use crate::request::RequestProvider;
use crate::response::ResponseProvider;

// the gateway is used for bootstrapping the entire gateway systems
pub struct Gateway {
    pub auth_provider: AuthProvider,
    pub limiter_provider: LimiterProvider,
    pub resolver_provider: ResolverProvider,
    pub request_provider: RequestProvider,
    pub response_provider: ResponseProvider,
}

impl Gateway {
    pub fn new() -> Self {
        Gateway {
            auth_provider: AuthProvider::new(),
            limiter_provider: LimiterProvider::new(),
            resolver_provider: ResolverProvider::new(),
            request_provider: RequestProvider::new(),
            response_provider: ResponseProvider::new(),
        }
    }
}
