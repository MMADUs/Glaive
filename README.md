![Glaive Diagram Image](./asset/glaive_logo.png)

# About Glaive

Glaive in an API Gateway written in Rust, this is a gateway service that will fulfill your needs, it provides a simple configuration and some built in plugins to work with.
We aim the system to work perfectly and providing robust performance, resource usage, and security within a very simple configuration.

> [!WARNING]
> This project is a work-in-progress.

# Architecture Overview

Glaive capable of handling a bunch of systems in the backdoor from the outer world, it is capable of handling a static web, services, LLMs & more.

![Glaive Diagram Image](./asset/glaive_diagram.png)

# Glaive key Features

This is currently the plan for future key features to be implemented.

- [x] Reverse Proxy
- [x] Load Balancer
- [ ] Load Balancer (Advanced)
- [ ] Serve Static Files
- [x] Upstream Healthcheck
- [x] YAML Configuration (DB-less Mode)
- [ ] DB-Mode with MongoDB
- [x] Dynamic Routing
- [x] Endpoint Configuration
- [x] Retry Mechanism
- [x] Request Timeout
- [ ] Redis Support
- [x] Rate Limiter
- [ ] Rate Limiter (Advanced)
- [ ] Request & Response Header Modification
- [ ] CORS Policy
- [ ] Circuit Breaking
- [ ] Request Size Limiter
- [x] Key Authentication
- [x] Basic Token Authentication
- [ ] Advanced Token Authentication
- [ ] Built in external Authentication
- [x] Authorization
- [x] Consumer ACL
- [x] IP Restriction
- [ ] OAuth 2.0 Authentication
- [x] Consul Discovery
- [ ] DNS Discovery
- [ ] K8S Discovery
- [ ] Advanced Logging
- [ ] Metrics with Prometheus
- [ ] API Documentation
- [ ] Vault Configuration
- [x] ENV Configuration
- [ ] TLS/SSL Termination
- [x] Caching Layer
- [ ] Caching Layer (Advanced)
- [ ] Open Telemetry
- [ ] gRPC Proxy
- [ ] gRPC Transformation
- [ ] Kafka Transformation
- [ ] Kafka Logging
- [ ] Async HTTP Logging
- [ ] Logstash Logging
- [ ] WebSocket Proxy
- [ ] HTTP1 & HTTP2 Support
- [ ] GraphQL Proxy
- [ ] Kubernetes ingress controller
- [ ] Zipkin Tracing
- [ ] Fault Tolerance