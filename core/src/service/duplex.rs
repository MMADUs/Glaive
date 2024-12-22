use tokio::sync::mpsc;
use futures::FutureExt;

use crate::stream::stream::Stream;
use crate::service::service::{Service, ServiceType};
use crate::session::downstream::Downstream;
use crate::session::upstream::Upstream;
use crate::session::task::Task;

const BUFFER_SIZE: usize = 32;

// handle request and io zero-copy bidirectionally
// with full duplex mechanism
impl<A: ServiceType> Service<A> {
    // pre process before perfoming io copy
    pub async fn handle_process(
        &self,
        client: Stream,
        server: Stream,
    ) -> tokio::io::Result<()> {
        // create new stream session for both
        let mut downstream = Downstream::new(client);
        let mut upstream = Upstream::new(server);
        // read request from client
        if let Err(e) = downstream.read_request().await {
            return Err(e)
        }
        // NOTE: we can use this mutable req header for modifications
        let mut_req_header = downstream.get_mut_request_headers();
        // get req header
        // TODO: figure out if we can prevent clone
        let req_header = downstream.get_request_headers().clone();
        // directly send to upstream
        // this way we dont necessarily handle the req header in duplex
        if let Err(e) = upstream.write_request_header(req_header).await {
            return Err(e)
        }
        // perform io copy
        self.copy_bidirectional(&mut downstream, &mut upstream).await
    }

    // the function associated to peform io copy bidirectionally
    pub async fn copy_bidirectional(
        &self,
        downstream: &mut Downstream,
        upstream: &mut Upstream,
    ) -> tokio::io::Result<()> {
        // split stream buffer into 2 task
        let (upstream_sender, upstream_reader) = mpsc::channel(BUFFER_SIZE);
        let (downstream_sender, downstream_reader) = mpsc::channel(BUFFER_SIZE);

        println!("performing io copy");
        // handle both concurrently
        let result = tokio::try_join!(
            Self::handle_downstream(downstream, upstream_sender, downstream_reader),
            Self::handle_upstream(upstream, downstream_sender, upstream_reader),
        );

        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }

    // the function associated for write & read to downstream
    async fn handle_downstream(
        downstream: &mut Downstream,
        upstream_sender: mpsc::Sender<Task>,
        mut downstream_reader: mpsc::Receiver<Task>,
    ) -> tokio::io::Result<()> {
        // process flag
        let mut request_done = false;
        let mut response_done = false;
        // duplex mode
        while !request_done || !response_done {
            // reserve channels to prevent failures during duplex
            let sender_permit = match upstream_sender.try_reserve() {
                Ok(permit) => permit,
                Err(e) => return Err(tokio::io::Error::new(tokio::io::ErrorKind::Other, e)),
            };

            tokio::select! {
                // the first process that executes concurrently
                // read the request from downstream and send the request task to upstream
                request_task = downstream.read_downstream_request(), if !request_done => {
                    match request_task {
                        Ok(task) => {
                            match task {
                                Task::Body(req_body, end_stream) => {
                                    println!("received data from client");
                                    // send task to upstream
                                    sender_permit.send(Task::Body(req_body, end_stream));
                                    // request finished determined by end of stream
                                    request_done = end_stream
                                },
                                // downstream reader only returns body, so ignore the rest.
                                _ => panic!("unexpected task on downstream read"),
                            }
                        }
                        Err(e) => {
                            let _ = upstream_sender.send(Task::Failed(e)).await;
                            return Ok(());
                        }
                    }
                }

                // the last process that executes concurrently
                // received response task from upstream and write it to downstream
                response_task = downstream_reader.recv(), if !response_done => {
                    if let Some(first_task) = response_task {
                        // list of response task
                        let mut tasks = Vec::with_capacity(BUFFER_SIZE);
                        tasks.push(first_task);
                        // get as many response task as possible
                        while let Some(resp_task) = downstream_reader.recv().now_or_never() {
                            if let Some(task) = resp_task {
                                tasks.push(task);
                            } else {
                                break;
                            }
                        }
                        // processed the response task at once
                        match downstream.write_downstream_response(tasks).await {
                            Ok(is_end) => {
                                // response finished determined by end of stream
                                response_done = is_end
                            },
                            Err(e) => return Err(e),
                        }
                    } else {
                        response_done = true
                    }
                }

                // this should not be reach at any point
                else => break,
            }
        }

        Ok(())
    }

    // function associated for write & read to upstream
    async fn handle_upstream(
        upstream: &mut Upstream,
        downstream_sender: mpsc::Sender<Task>,
        mut upstream_reader: mpsc::Receiver<Task>,
    ) -> tokio::io::Result<()> {
        // process flag
        let mut request_done = false;
        let mut response_done = false;
        // duplex mode
        while !request_done || !response_done {
            tokio::select! {
                // the third process that executes concurrently
                // read the response from upstream and send the response task to downstream
                response_task = upstream.read_upstream_response(), if !response_done => {
                    match response_task {
                        Ok(task) => {
                            println!("read response");
                            // response finished determined by end of stream
                            response_done = task.is_end();
                            // directly send task
                            let result = downstream_sender.send(task).await;
                            // handle task sending error & somehow parse from std result to tokio result
                            if result.is_err() && !upstream.is_request_upgrade() {
                                return result.map_err(|err| tokio::io::Error::new(tokio::io::ErrorKind::Other, err));
                            }
                        }
                        Err(e) => {
                            let _ = downstream_sender.send(Task::Failed(e)).await;
                            return Ok(());
                        }
                    }
                }

                // the second process that executes concurrently
                // received request task from downstream and write it to upstream
                request_task = upstream_reader.recv(), if !request_done => {
                    if let Some(task) = request_task {
                        match upstream.write_upstream_request(task).await {
                            Ok(is_end) => {
                                // request finished determined by end of stream
                                request_done = is_end
                            },
                            Err(e) => return Err(e),
                        }
                    } else {
                        request_done = true
                    }
                }

                // this should not be reach at any point
                else => break,
            }
        }

        Ok(())
    }
}
