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

use pingora::prelude::Session;

use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

use crate::cluster::ClusterMetadata;
use crate::def::{Jwt, Key};
use crate::proxy::{ProxyRouter, RouterCtx};
use crate::response::ResponseProvider;

// claim for jsonwebtoken
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    exp: usize,
    consumer: String,
}

// error response body
#[derive(Debug, Serialize, Deserialize)]
struct Forbidden {
    status_code: usize,
    message: String,
}

pub struct AuthProvider {
    response_provider: ResponseProvider,
}

impl AuthProvider {
    // get auth provider
    pub fn new() -> Self {
        AuthProvider {
            response_provider: ResponseProvider::new(),
        }
    }

    // basic key auth
    pub async fn basic_key(&self, key: &Key, session: &mut Session, _ctx: &mut RouterCtx) -> bool {
        let req_header = session.req_header();
        // get key from bearer headers
        if let Some(creds) = req_header
            .headers
            .get("Authorization")
            .map(|v| v.as_bytes())
        {
            // if key exist, parse key to str
            if let Ok(creds_str) = std::str::from_utf8(creds) {
                // clean key from headers
                let cleaned_key = creds_str.trim_start_matches("Bearer ").trim();
                // checks if key is allowed
                match key.allowed.contains(&cleaned_key.to_string()) {
                    true => false, // if key are contained, continue request
                    false => {
                        // return 403 because key is not provided/allowed
                        let _ = &self
                            .response_provider
                            .error_response(session, 403, "Invalid API Key", None)
                            .await;
                        true
                    }
                }
            } else {
                // return 502, because this error happen during string parse
                let _ = &self
                    .response_provider
                    .error_response(session, 502, "Unable to parse key", None)
                    .await;
                true
            }
        } else {
            // return 403 because key does not exist in request
            let _ = &self
                .response_provider
                .error_response(session, 403, "Key is required", None)
                .await;
            true
        }
    }

    // basic jwt auth
    // with authorization and acl
    pub async fn basic_jwt(
        &self,
        gateway: &ProxyRouter,
        cluster: &ClusterMetadata,
        jwt: &Jwt,
        session: &mut Session,
        _ctx: &mut RouterCtx,
    ) -> bool {
        let req_header = session.req_header();
        // get token from bearer headers
        if let Some(token) = req_header
            .headers
            .get("Authorization")
            .map(|v| v.as_bytes())
        {
            // parse token to str from utf8
            if let Ok(parsed_token) = std::str::from_utf8(token) {
                // remover the Bearer from the headers
                let cleaned_token = parsed_token.trim_start_matches("Bearer ").trim();
                // create the validator instance
                let validation = Validation::new(Algorithm::HS256);
                // get the payload as token_claim
                let token_claim = decode::<Claims>(
                    cleaned_token,
                    &DecodingKey::from_secret(jwt.secret.as_ref()),
                    &validation,
                );
                // checks if token is valid
                match token_claim {
                    Ok(claim) => {
                        // the authentication ends here
                        // the rest the code below will be the authorization process based on the defined consumers
                        // get consumers if consumers is provided
                        // if no consumers is provided, auth layer ends here
                        if let Some(consumers) = cluster.get_consumers() {
                            // checks if the consumer from token is allowed by the defined service consumer
                            let (client_consumer_name, allowed_acl) = if let Some(consumer) =
                                consumers.iter().find(|consumer| {
                                    // Check if consumer token exists in the allowed list
                                    let name = consumer.get_name();
                                    claim.claims.consumer == *name
                                }) {
                                // If the consumer is found, return its name and ACL
                                (consumer.get_name(), consumer.get_acl())
                            } else {
                                // If the consumer is not found, return a 403 response
                                let _ = &self
                                    .response_provider
                                    .error_response(session, 403, "Unauthorized consumer", None)
                                    .await;
                                return true; // Indicate that the error response was handled
                            };

                            // checks if the gateway consumer data is provided
                            if let Some(def_consumer) = &gateway.consumers {
                                // if its provided, trying to query the acl by consumer name that we got previously
                                let acl = if let Some(consumer) =
                                    def_consumer.iter().find(|consumer| {
                                        // Check if the consumer exists in the consumer gateway
                                        let name = consumer.get_name();
                                        client_consumer_name == name
                                    }) {
                                    // If the consumer is found, return the ACL
                                    consumer.get_acl()
                                } else {
                                    // If the consumer is not found, return 502 and stop execution
                                    let _ = &self
                                        .response_provider
                                        .error_response(session, 502, "Consumer not found", None)
                                        .await;
                                    return true;
                                }; // we should return 502, because we are using consumer, but they are not defined on the gateway

                                // checks if the acl we got from the consumer gateway config match the allowed acl in the service
                                let authorized = acl.iter().any(|acl_item| {
                                    // check if exist
                                    allowed_acl
                                        .iter()
                                        .any(|allowed_acl_item| allowed_acl_item == acl_item)
                                });
                                // checks if the client is allowed by the acl
                                if !authorized {
                                    let _ = &self
                                        .response_provider
                                        .error_response(
                                            session,
                                            403,
                                            "Unauthorized access control",
                                            None,
                                        )
                                        .await;
                                    true
                                } else {
                                    // when client is authorized, continue the request
                                    println!("consumer is authorized");
                                    false
                                }
                            } else {
                                // return 502 because consumers is not defined in the gateway config
                                let _ = &self
                                    .response_provider
                                    .error_response(session, 502, "Undefined consumer", None)
                                    .await;
                                true
                            }
                        } else {
                            // continue the request when service disables authorization
                            false
                        }
                    }
                    Err(error) => {
                        // return 403 due to invalid token
                        let message = format!("Invalid Token: {}", error);
                        let _ = &self
                            .response_provider
                            .error_response(session, 403, message.as_str(), None)
                            .await;
                        true
                    }
                }
            } else {
                // return 400 due to header parse error
                let _ = &self
                    .response_provider
                    .error_response(session, 400, "Unable to parse token", None)
                    .await;
                true
            }
        } else {
            // return 403 due to bearer does not exist in headers
            let _ = &self
                .response_provider
                .error_response(session, 403, "Token is required", None)
                .await;
            true
        }
    }

    // pub fn external() {
    //
    // }
}
