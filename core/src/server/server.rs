use tokio::runtime::{Builder, Handle};
use tokio::signal::unix;

use crate::listener::listener::Listener;

pub struct Runtime {
    runtime: tokio::runtime::Runtime,
}

impl Runtime {
    // new runtime builder
    pub fn new(thread_name: &str, alloc_threads: usize) -> Self {
        let built_runtime = Builder::new_multi_thread()
            .enable_all()
            .worker_threads(alloc_threads)
            .thread_name(thread_name)
            .build()
            .unwrap();
        Runtime{
            runtime: built_runtime,
        }
    }
    // handle thread work
    pub fn handle_work(&self) -> &Handle {
        self.runtime.handle()
    }
}

enum ShutdownType {
    Graceful,
    Fast,
}

pub struct Server {
    services: Vec<Listener>,
}

impl Server {
    pub fn new() -> Self {
        Server{
            services: Vec::new(),
        }
    }

    pub fn add_service(&mut self, service: Listener) {
        self.services.push(service);
    }

    pub fn run_forever(&mut self) {
        let mut runtimes: Vec<Runtime> = Vec::new();
        while let Some(service) = self.services.pop() {
            // example threads, make it dynamic later
            let alloc_threads = 10;
            let runtime = Self::run_service(service, alloc_threads);
            runtimes.push(runtime);
        }
        // the main server runtime
        let main_runtime = Runtime::new("main-runtime", 1);
        let shutdown_type = main_runtime.handle_work().block_on(
            // this block forever until the entire program exit
            self.run_server(),
        );
        match shutdown_type {
            ShutdownType::Graceful => println!("shutdown gracefully"),
            ShutdownType::Fast => println!("shutdown fast"),
        }
    }

    fn run_service(service: Listener, alloc_threads: usize) -> Runtime {
        // run each listener service on top of the runtime
        let runtime = Runtime::new("runtime", alloc_threads);
        runtime.handle_work().spawn(async move {
            // run the async service here
            let _ = service.listen().await;
        });
        runtime
    }

    async fn run_server(&self) -> ShutdownType {
        // waiting for exit signal
        let mut graceful_upgrade_signal = unix::signal(unix::SignalKind::quit()).unwrap();
        let mut graceful_terminate_signal = unix::signal(unix::SignalKind::terminate()).unwrap();
        let mut fast_shutdown_signal = unix::signal(unix::SignalKind::interrupt()).unwrap();
        tokio::select! {
            _ = graceful_upgrade_signal.recv() => {
                println!("graceful upgrade signalled");
                ShutdownType::Graceful
            },
            _ = graceful_terminate_signal.recv() => {
                println!("graceful termintation signalled");
                ShutdownType::Graceful
            },
            _ = fast_shutdown_signal.recv() => {
                println!("fast shutdown signalled");
                ShutdownType::Fast
            }
        }
    }
}
