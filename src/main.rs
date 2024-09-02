use async_trait::async_trait;

use std::sync::Arc;
use pingora::lb::LoadBalancer;
use pingora::prelude::{background_service, HttpPeer, Opt, RoundRobin, TcpHealthCheck};
use pingora::proxy::{http_proxy_service, ProxyHttp, Session};
use pingora::server::Server;
use pingora::Result;

pub struct LB(
    Arc<LoadBalancer<RoundRobin>>
);

#[async_trait]
impl ProxyHttp for LB {
    type CTX = ();

    fn new_ctx(&self) -> () {
        ()
    }

    async fn upstream_peer(&self, _session: &mut Session, _ctx: &mut ()) -> Result<Box<HttpPeer>> {
        let upstream = self
            .0
            .select(b"", 256) // hash doesn't matter
            .unwrap();

        println!("upstream peer is: {:?}", upstream);

        let peer = Box::new(HttpPeer::new(upstream, true, "one.one.one.one".to_string()));
        Ok(peer)
    }

    // async fn upstream_request_filter(
    //     &self,
    //     _session: &mut Session,
    //     upstream_request: &mut RequestHeader,
    //     _ctx: &mut Self::CTX,
    // ) -> Result<()> {
    //     upstream_request.insert_header("Host", "host.docker.internal").unwrap();
    //     Ok(())
    // }
}

fn main() {
    let opt = Opt::parse_args();
    let mut my_server = Server::new(Some(opt)).unwrap();
    my_server.bootstrap();

    let mut upstreams = LoadBalancer::try_from_iter([
        "host.docker.internal:3000",
        "host.docker.internal:3001",
        "host.docker.internal:3002",
    ])
        .unwrap();

    let hc = TcpHealthCheck::new();
    upstreams.set_health_check(hc);
    upstreams.health_check_frequency = Some(std::time::Duration::from_secs(1));

    let background = background_service("health check", upstreams);
    let upstreams = background.task();

    let mut lb = http_proxy_service(&my_server.configuration, LB(upstreams));
    lb.add_tcp("0.0.0.0:6188");

    my_server.add_service(background);

    my_server.add_service(lb);
    my_server.run_forever();
}