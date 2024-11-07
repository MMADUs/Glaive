use core::add;

fn main() {
    println!("Hello, world!");
    println!("2 + 2 is: {}", add(2, 2));
    let mut server = core::server::server::Server::new();
    let service1 = core::listener::listener::Listener::new_tcp("127.0.0.1:8500");
    let service2 = core::listener::listener::Listener::new_tcp("127.0.0.1:8600");
    server.add_service(service1);
    server.add_service(service2);
    server.run_forever();
}
