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
        if let Err(e) = downstream.read_request().await {
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

        // perform io copy
        if let Err(e) = self
            .copy_bidirectional(&mut downstream, &mut upstream)
            .await
        {
            return Err(e);
        }
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

        info!("performing io copy");
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
                    info!("read downstream request");
                    match request_task {
                        Ok(task) => {
                            match task {
                                Task::Body(req_body, end_stream) => {
                                    // send task to upstream
                                    debug!("req body: {:?}", req_body);
                                    sender_permit.send(Task::Body(req_body, end_stream));
                                    // request finished determined by end of stream
                                    debug!("req body end: {:?}", end_stream);
                                    request_done = end_stream
                                },
                                Task::Done => {
                                    // directly mark request as done
                                    debug!("req body done");
                                    request_done = true
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
                    info!("write downstream response");
                    if let Some(first_task) = response_task {
                        // list of response task
                        let mut tasks = Vec::with_capacity(BUFFER_SIZE);
                        debug!("first task: {:?}", first_task);
                        tasks.push(first_task);
                        // get as many response task as possible
                        while let Some(resp_task) = downstream_reader.recv().now_or_never() {
                            if let Some(task) = resp_task {
                                debug!("vec task: {:?}", task);
                                tasks.push(task);
                            } else {
                                break;
                            }
                        }
                        // processed the accumulated response task at once
                        match downstream.write_downstream_response(tasks).await {
                            Ok(is_end) => {
                                // response finished determined by end of stream
                                debug!("is response end: {:?}", is_end);
                                response_done = is_end
                            },
                            Err(e) => return Err(e),
                        }
                    } else {
                        // if no task received, mark response as done
                        debug!("no req read task found");
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
                    info!("read upstream response");
                    match response_task {
                        Ok(task) => {
                            // response finished determined by end of stream
                            debug!("resp task end: {:?}", task.is_end());
                            response_done = task.is_end();
                            // directly send task
                            debug!("resp task: {:?}", task);
                            let result = downstream_sender.send(task).await;
                            // handle task sending error & somehow parse from std result to tokio result
                            if result.is_err() && !upstream.is_request_upgrade() {
                                error!("sender error");
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
                    info!("write upstream request");
                    if let Some(task) = request_task {
                        debug!("req task: {:?}", task);
                        match upstream.write_upstream_request(task).await {
                            Ok(is_end) => {
                                // request finished determined by end of stream
                                debug!("req task end: {:?}", is_end);
                                request_done = is_end
                            },
                            Err(e) => return Err(e),
                        }
                    } else {
                        // if no task revceived, mark request as done
                        debug!("no req write task found");
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
