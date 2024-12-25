use core::panic;

use futures::FutureExt;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::service::service::{Service, ServiceType};
use crate::session::downstream::Downstream;
use crate::session::task::Task;
use crate::session::upstream::Upstream;
use crate::stream::stream::Stream;

const BUFFER_SIZE: usize = 32;

// implementation for io copy in duplex mode
impl<A: ServiceType> Service<A> {
    // pre process before perfoming io copy
    pub async fn handle_process(
        &self,
        client: Stream,
        server: Stream,
    ) -> tokio::io::Result<Stream> {
        // create new stream session for both
        let mut downstream = Downstream::new(client);
        let mut upstream = Upstream::new(server);
        // read request from client
        let res = tokio::select! {
            biased;
            res = downstream.read_request() => { res }
            // we can add shutdown handler here
        };
        if let Err(e) = res {
            return Err(e);
        }
        // NOTE: we can use this mutable req header for modifications
        let mut_req_header = downstream.get_mut_request_headers();
        let test = mut_req_header.get_header("test");
        println!("{:?}", test);
        // get req header
        // TODO: figure out if we can prevent clone
        let req_header = downstream.get_request_headers().clone();
        // directly send to upstream
        // this way we dont necessarily handle the req header in duplex
        if let Err(e) = upstream.write_request_header(req_header).await {
            return Err(e);
        }
        // enable retry buffer
        downstream.enable_retry_buffer();
        // perform io copy
        let start = tokio::time::Instant::now();
        if let Err(e) = self
            .copy_bidirectional(&mut downstream, &mut upstream)
            .await
        {
            return Err(e);
        }
        let elapsed = start.elapsed();
        tracing::info!("io copy operations completed in {:?}", elapsed);
        // return stream to pool
        let stream = upstream.return_stream();
        Ok(stream)
    }

    // the function associated to peform io copy bidirectionally
    async fn copy_bidirectional(
        &self,
        downstream: &mut Downstream,
        upstream: &mut Upstream,
    ) -> tokio::io::Result<()> {
        // split stream buffer into 2 task
        let (upstream_sender, upstream_reader) = mpsc::channel(BUFFER_SIZE);
        let (downstream_sender, downstream_reader) = mpsc::channel(BUFFER_SIZE);
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
        // check retry buffer first
        if let Some(buffer) = downstream.get_retry_buffer() {
            // reserve channel
            let sender_permit = upstream_sender
                .reserve()
                .await
                .map_err(|e| tokio::io::Error::new(tokio::io::ErrorKind::Other, e))?;
            // send buffer
            sender_permit.send(Task::Body(Some(buffer), request_done));
        }
        // duplex mode
        while !request_done || !response_done {
            // log timer
            let timer = tokio::time::Instant::now();
            // reserve channel
            let sender_permit = upstream_sender
                .try_reserve()
                .map_err(|e| tokio::io::Error::new(tokio::io::ErrorKind::Other, e));
            // event selection
            tokio::select! {
                // the first process that executes concurrently
                // read the request from downstream and send the request task to upstream
                request_task = downstream.read_downstream_request(), if !request_done && sender_permit.is_ok() => {
                    let elapsed = timer.elapsed();
                    info!("read downstream request took: {:?}", elapsed);
                    match request_task {
                        Ok(task) => {
                            match task {
                                Task::Body(req_body, end_stream) => {
                                    // send task to upstream
                                    // safe to unwrap because we check is_ok()
                                    sender_permit.unwrap().send(Task::Body(req_body, end_stream));
                                    // request finished determined by end of stream
                                    request_done = end_stream;
                                },
                                Task::Done => {
                                    // directly mark request as done
                                    response_done = true;
                                },
                                // downstream reader only returns body, so ignore the rest.
                                _ => panic!("unexpected downstream read task"),
                            }
                        }
                        Err(e) => {
                            error!("error during downstream read: {}", e);
                            let _ = upstream_sender.send(Task::Failed(e)).await;
                            return Ok(());
                        }
                    }
                }

                // the last process that executes concurrently
                // received response task from upstream and write it to downstream
                response_task = downstream_reader.recv(), if !response_done => {
                    let elapsed = timer.elapsed();
                    info!("write downstream response took: {:?}", elapsed);
                    if let Some(first_task) = response_task {
                        // list of response task
                        let mut tasks = Vec::with_capacity(BUFFER_SIZE);
                        tasks.push(first_task);
                        // get as many response task as possible
                        while let Some(resp_task) = downstream_reader.recv().now_or_never() {
                            match resp_task {
                                Some(task) => tasks.push(task),
                                None => break,
                            }
                        }
                        // processed the accumulated response task at once
                        match downstream.write_downstream_response(tasks).await {
                            Ok(is_end) => {
                                // response finished determined by end of stream
                                response_done = is_end;
                                // try to force request done when response is successful
                                request_done = is_end;
                            },
                            Err(e) => return Err(e),
                        }
                    } else {
                        // if no task received, mark response as done
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
            // log timer
            let timer = tokio::time::Instant::now();
            // event selection
            tokio::select! {
                // the third process that executes concurrently
                // read the response from upstream and send the response task to downstream
                response_task = upstream.read_upstream_response(), if !response_done => {
                    let elapsed = timer.elapsed();
                    info!("read upstream response took: {:?}", elapsed);
                    match response_task {
                        Ok(task) => {
                            // response finished determined by end of stream
                            response_done = task.is_end();
                            // try to force request done when response is successful
                            request_done = task.is_end();
                            // directly send task
                            let result = downstream_sender.send(task).await;
                            // handle task sending error & somehow parse from std result to tokio result
                            if result.is_err() && !upstream.is_request_upgrade() {
                                return result.map_err(|err| tokio::io::Error::new(tokio::io::ErrorKind::Other, err));
                            }
                        }
                        Err(e) => {
                            error!("error during upstream read: {}", e);
                            let _ = downstream_sender.send(Task::Failed(e)).await;
                            return Ok(());
                        }
                    }
                }

                // the second process that executes concurrently
                // received request task from downstream and write it to upstream
                request_task = upstream_reader.recv(), if !request_done => {
                    let elapsed = timer.elapsed();
                    info!("write upstream request took: {:?}", elapsed);
                    if let Some(task) = request_task {
                        match upstream.write_upstream_request(task).await {
                            Ok(is_end) => {
                                // request finished determined by end of stream
                                request_done = is_end;
                            },
                            Err(e) => return Err(e),
                        }
                    } else {
                        // if no task revceived, mark request as done
                        request_done = true;
                    }
                }

                // this should not be reach at any point
                else => break,
            }
        }

        Ok(())
    }
}
